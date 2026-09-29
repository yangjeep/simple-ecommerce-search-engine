#!/usr/bin/env python3
"""Issue #63 (Infra E3, amendment 1) analysis.

  analyze_i63.py adopt    # section 3.4 adoption rule over micro/ + gate/ -> adoption.json/.md
  analyze_i63.py confirm  # Part A tables over confirm/ -> confirm_report.json/.md
  analyze_i63.py micro    # B1/B2/C tables over micro/ -> micro_report.md
  analyze_i63.py report   # primitive table + FH3/FH4 residual attribution -> report.json/.md

Rules implemented here are the preregistered ones (GitHub #63, amendment 1
and clarification C1). Nothing is tuned after results: thresholds are
module constants below.
"""
import json
import os
import statistics
import sys
from collections import defaultdict
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
RESULTS = Path(os.environ.get("I63_RESULTS", ROOT / "artifacts/issue63/results"))

# Section 3.4 adoption thresholds.
ADOPT_MIN_FH_REDUCTION = 0.25
ADOPT_MAX_REGRESSION = 1.10
ADOPT_TIE = 0.05
# Section 2 classification (#60 materiality bar).
MATERIAL = 0.75
PARITY = 1.25

FH = [
    ("FH1", "facet_low_cardinality_style"),
    ("FH2", "facet_medium_cardinality_primarymaterial"),
    ("FH3", "facet_high_cardinality_color"),
    ("FH4", "facet_disjunctive_multi_dim"),
]
SECONDARY = [
    ("SH1", "numeric_range_sort"),
    ("FD1", "filter_depth_1"),
    ("FD3", "filter_depth_3"),
    ("FD5", "filter_depth_5"),
]
CANDIDATES = ["p0r", "p1", "p2", "p2b"]


def median(values):
    return statistics.median(values) if values else None


def cv(values):
    if len(values) < 2:
        return None
    mean = statistics.mean(values)
    return statistics.pstdev(values) / mean if mean else None


def classify(r):
    if r is None:
        return "N/A"
    if r <= MATERIAL:
        return "MATERIAL FACET ADVANTAGE"
    if r <= PARITY:
        return "PARITY"
    return "FACET DISADVANTAGE"


def load_micro(tier):
    runs = []
    for path in sorted((RESULTS / "micro").glob(f"{tier}_run*.json")):
        runs.append(json.loads(path.read_text()))
    return runs


def pipeline_cpu(runs):
    """{(cell, cand): [cpu_ns_per_op per run]} for B1 pipeline points."""
    out = defaultdict(list)
    for run in runs:
        for p in run["points"]:
            if p["part"] == "b1" and p["name"] == "pipeline":
                cell, cand = p["variant"].rsplit(":", 1)
                out[(cell, cand)].append(p["sample"]["cpu_ns_per_op"])
    return out


def adopt():
    runs = load_micro("500k")
    if len(runs) < 3:
        sys.exit(f"adopt: need 3 500k micro runs, found {len(runs)}")
    runs_100k = load_micro("100k")
    cpu = pipeline_cpu(runs)
    cells = sorted({cell for cell, _ in cpu})
    gate = {}
    for tier in ("100k", "500k"):
        g = json.loads((RESULTS / "gate" / f"gate_{tier}.json").read_text())
        gate[tier] = g
    failures_by_cand = defaultdict(int)
    checks_by_cand = defaultdict(int)
    for tier, g in gate.items():
        for c in g["checks"]:
            if c["is_baseline"]:
                continue
            cand = c.get("cand_mode", "p0")
            checks_by_cand[cand] += 1
            if not c["candidate_components_ok"]:
                failures_by_cand[cand] += 1
    micro_failures = defaultdict(int)
    for run in runs + runs_100k:
        for c in run["correctness"]:
            if not c["ok"]:
                # B1 checks are per candidate mode ("<cell>:<cand>"); any B2/C
                # (or representation-identity) failure blocks every candidate.
                is_mode = c["part"] == "b1" and ":" in c["name"]
                micro_failures[c["name"].rsplit(":", 1)[-1] if is_mode else "*"] += 1

    persistent = {}
    for p in runs[0]["points"]:
        if p["part"] == "b1" and p["name"] == "construct":
            persistent[p["variant"]] = p["extra"]["persistent_bytes"]
    p1_bytes = persistent.get("p1", 0)
    # P2/P2b borrow the P1 bitmap for candidate top-K over match-all, so the
    # server keeps it: their persistent memory is P1's.
    persistent_effective = {"p0r": 0, "p1": p1_bytes, "p2": p1_bytes, "p2b": p1_bytes}

    table = []
    passing = []
    for cand in CANDIDATES:
        row = {"cand": cand, "cells": {}}
        best_fh = None
        worst_ratio = 0.0
        worst_cell = None
        for cell in cells:
            base, alt = median(cpu[(cell, "p0")]), median(cpu[(cell, cand)])
            if base is None or alt is None:
                continue
            ratio = alt / base
            row["cells"][cell] = {"p0_ns": base, "cand_ns": alt, "ratio": ratio}
            if ratio > worst_ratio:
                worst_ratio, worst_cell = ratio, cell
            if cell in dict((c, l) for l, c in FH):
                reduction = 1 - ratio
                best_fh = reduction if best_fh is None else max(best_fh, reduction)
        correctness_ok = (
            checks_by_cand[cand] > 0 and failures_by_cand[cand] == 0 and micro_failures[cand] == 0
            and micro_failures["*"] == 0
        )
        fh34 = sum(
            row["cells"][c]["p0_ns"] - row["cells"][c]["cand_ns"]
            for _, c in FH[2:]
            if c in row["cells"]
        )
        row.update({
            "gate_checks": checks_by_cand[cand],
            "gate_failures": failures_by_cand[cand],
            "micro_failures": micro_failures[cand],
            "criterion1_correct": correctness_ok,
            "best_fh_reduction": best_fh,
            "criterion2_fh_reduction_ge_25pct": best_fh is not None and best_fh >= ADOPT_MIN_FH_REDUCTION,
            "worst_ratio": worst_ratio,
            "worst_cell": worst_cell,
            "criterion3_no_regression_gt_10pct": worst_ratio <= ADOPT_MAX_REGRESSION,
            "fh3_fh4_reduction_ns": fh34,
            "persistent_bytes": persistent_effective[cand],
        })
        row["passes"] = (row["criterion1_correct"] and row["criterion2_fh_reduction_ge_25pct"]
                         and row["criterion3_no_regression_gt_10pct"])
        if row["passes"]:
            passing.append(row)
        table.append(row)

    chosen = None
    note = "no candidate passed section 3.4; N+ is not run (3-arm square)"
    if passing:
        passing.sort(key=lambda r: -r["fh3_fh4_reduction_ns"])
        best = passing[0]
        tied = [r for r in passing
                if best["fh3_fh4_reduction_ns"] - r["fh3_fh4_reduction_ns"]
                <= ADOPT_TIE * abs(best["fh3_fh4_reduction_ns"])]
        smallest = min(r["persistent_bytes"] for r in tied)
        # Tie-break by persistent memory; if memory is also equal, the
        # larger FH3+FH4 reduction (the first rule) stands.
        chosen = next(r for r in tied if r["persistent_bytes"] == smallest)
        note = (f"{len(passing)} candidate(s) passed; largest FH3+FH4 reduction "
                f"{best['cand']}; within-5% tie set {[r['cand'] for r in tied]}; chosen "
                f"{chosen['cand']} (smallest persistent memory {smallest} B, then larger reduction)")
    result = {
        "rule": "amendment 1 section 3.4 + clarification C1",
        "thresholds": {"min_fh_reduction": ADOPT_MIN_FH_REDUCTION,
                       "max_regression_ratio": ADOPT_MAX_REGRESSION, "tie": ADOPT_TIE},
        "runs": [r["run"] for r in runs],
        "git_sha": sorted({r["git_sha"] for r in runs}),
        "candidates": table,
        "n_plus": chosen["cand"] if chosen else "none",
        "note": note,
        "facet_loop_candidates": "none implemented as a server mode this round; "
                                 "u32 counters are characterized in B2 only",
    }
    (RESULTS / "adoption.json").write_text(json.dumps(result, indent=2) + "\n")
    lines = ["# Issue #63 section 3.4 adoption (500k, median of 3 runs, in-process CPU/op)", "",
             "| cand | correct (gate checks / failures) | best FH reduction | worst ratio (cell) | FH3+FH4 reduction µs | persistent B | passes |",
             "|---|---|---|---|---|---|---|"]
    for r in table:
        lines.append(
            f"| {r['cand']} | {r['criterion1_correct']} ({r['gate_checks']}/{r['gate_failures']}) | "
            f"{(r['best_fh_reduction'] or 0):.1%} | {r['worst_ratio']:.3f} ({r['worst_cell']}) | "
            f"{r['fh3_fh4_reduction_ns'] / 1e3:,.0f} | {r['persistent_bytes']:,} | {r['passes']} |")
    lines += ["", f"**N+ = `{result['n_plus']}`** — {note}", ""]
    lines += ["Per-cell ratios (cand ÷ P0):", "", "| cell | " + " | ".join(CANDIDATES) + " | P0 µs |",
              "|---|" + "---|" * (len(CANDIDATES) + 1)]
    for cell in cells:
        ratios = [f"{table[i]['cells'].get(cell, {}).get('ratio', float('nan')):.3f}" for i in range(len(CANDIDATES))]
        p0 = table[0]["cells"].get(cell, {}).get("p0_ns", float("nan")) / 1e3
        lines.append(f"| {cell} | " + " | ".join(ratios) + f" | {p0:,.0f} |")
    (RESULTS / "adoption.md").write_text("\n".join(lines) + "\n")
    print("\n".join(lines))


def load_confirm():
    runs = {}
    for run_dir in sorted((RESULTS / "confirm").glob("run*")):
        run = int(run_dir.name[3:])
        entry = {"dir": run_dir}
        native = run_dir / "native_500k.json"
        if native.exists():
            entry["native"] = json.loads(native.read_text())
        for engine in ("meilisearch", "solr"):
            path = run_dir / f"{engine}_500k.json"
            if path.exists():
                entry[engine] = json.loads(path.read_text())
        eq = run_dir / "equivalence.json"
        entry["equivalence"] = json.loads(eq.read_text()) if eq.exists() else None
        runs[run] = entry
    return runs


def confirm():
    runs = load_confirm()
    ambient_path = RESULTS / "confirm/ambient.jsonl"
    ambient = [json.loads(l) for l in ambient_path.read_text().splitlines()] if ambient_path.exists() else []
    n_plus = None
    rows = defaultdict(lambda: defaultdict(dict))  # cell -> arm -> run -> metrics
    equivalent = defaultdict(dict)  # (arm, cell) -> run -> bool
    status = []
    for run, entry in runs.items():
        native = entry.get("native")
        if native:
            status.append((run, "native", native["status"], native["fixture_correctness_all_passed"]))
            for c in native["cells"]:
                arm = "native" if not c.get("cand_mode") else "native+"
                if c.get("cand_mode"):
                    n_plus = c["cand_mode"]
                rows[c["cell"]][arm][run] = {
                    "cpu": c["cpu_usec_per_query"], "p50": c["p50_ms"], "p95": c["p95_ms"],
                    "p99": c["p99_ms"], "wall": c["mean_wall_ms"], "requests": 1.0,
                    "n": c["measured_queries"], "status": c["status"],
                }
        for engine in ("meilisearch", "solr"):
            d = entry.get(engine)
            if not d:
                continue
            status.append((run, engine, d["status"], d["correctness_all_passed"]))
            for c in d["workload_cells"]:
                rows[c["name"]][engine][run] = {
                    "cpu": c["mean_cpu_usec_per_query"], "p50": c["p50_ms"], "p95": c["p95_ms"],
                    "p99": c["p99_ms"], "wall": c["mean_wall_ms"],
                    "requests": c["mean_backend_requests"], "n": 200, "status": "ok",
                }
        eq = entry.get("equivalence") or {"verdicts": []}
        for v in eq["verdicts"]:
            engine = v["engine"]
            if engine.startswith("native:"):
                arm = "native+" if engine.count("-") >= 2 else "native"
            else:
                arm = engine
            equivalent[(arm, v["cell"])][run] = v["equivalent"]

    def arm_stats(cell, arm):
        per_run = rows[cell][arm]
        cpus = [m["cpu"] for m in per_run.values() if m["cpu"] is not None]
        eq_runs = equivalent.get((arm, cell), {})
        return {
            "runs": {r: m["cpu"] for r, m in sorted(per_run.items())},
            "median": median(cpus), "min": min(cpus) if cpus else None,
            "max": max(cpus) if cpus else None, "cv": cv(cpus),
            "p50": median([m["p50"] for m in per_run.values()]),
            "p95": median([m["p95"] for m in per_run.values()]),
            "p99": median([m["p99"] for m in per_run.values()]),
            "wall": median([m["wall"] for m in per_run.values()]),
            "requests": median([m["requests"] for m in per_run.values()]),
            "equivalent_all_runs": bool(eq_runs) and all(eq_runs.values()) and len(eq_runs) == len(per_run),
            "equivalence_by_run": eq_runs,
        }

    report = {"n_plus": n_plus, "status": status, "cells": {}, "ambient": ambient}
    for label, cell in FH + SECONDARY:
        entry = {arm: arm_stats(cell, arm) for arm in ("native", "native+", "meilisearch", "solr") if rows[cell][arm]}
        competitors = {a: s for a, s in entry.items() if a in ("meilisearch", "solr")
                       and s["equivalent_all_runs"] and s["median"] is not None}
        fastest = min(competitors, key=lambda a: competitors[a]["median"]) if competitors else None
        for arm in ("native", "native+"):
            if arm not in entry or fastest is None:
                continue
            s = entry[arm]
            r = s["median"] / competitors[fastest]["median"]
            per_run = []
            for run, value in s["runs"].items():
                comp = competitors[fastest]["runs"].get(run)
                if value is not None and comp:
                    per_run.append(value / comp)
            classes = {classify(x) for x in per_run}
            s["r"] = r
            s["class"] = classify(r)
            s["r_by_run"] = per_run
            s["robust"] = "robust" if len(per_run) == 3 and classes == {s["class"]} else "median-only"
        report["cells"][label] = {"cell": cell, "fastest": fastest, "arms": entry,
                                  "not_equivalent": [a for a, s in entry.items() if not s["equivalent_all_runs"]]}
    (RESULTS / "confirm_report.json").write_text(json.dumps(report, indent=2, default=str) + "\n")

    fmt = lambda v: "—" if v is None else f"{v:,.0f}"
    lines = ["# Issue #63 Part A — same-window equal-work confirmation (500k, CPU/query µs)", "",
             f"N+ = `{n_plus or 'none'}`", "",
             "| cell | native FINAL (runs) | N+ | meili | solr | fastest | native r | class | robust? | N+ r | N+ class |",
             "|---|---|---|---|---|---|---|---|---|---|---|"]
    for label, _ in FH + SECONDARY:
        c = report["cells"][label]
        a = c["arms"]
        get = lambda arm: a.get(arm, {})
        runs_str = " / ".join(fmt(v) for v in get("native").get("runs", {}).values())
        lines.append(
            f"| {label} | {fmt(get('native').get('median'))} ({runs_str}) | {fmt(get('native+').get('median'))} | "
            f"{fmt(get('meilisearch').get('median'))}{'' if get('meilisearch').get('equivalent_all_runs', True) else ' ⚠NEW'} | "
            f"{fmt(get('solr').get('median'))}{'' if get('solr').get('equivalent_all_runs', True) else ' ⚠NEW'} | "
            f"{c['fastest']} | {get('native').get('r', float('nan')):.3f} | {get('native').get('class', 'N/A')} | "
            f"{get('native').get('robust', '—')} | {get('native+').get('r', float('nan')):.3f} | {get('native+').get('class', '—')} |")
    lines += ["", "⚠NEW = NOT_EQUIVALENT_WORK in at least one run (excluded from `fastest`).", "",
              "Latency and variance (medians over runs):", "",
              "| cell | arm | P50 ms | P95 ms | P99 ms | mean wall ms | backend requests | CV | min | max |",
              "|---|---|---|---|---|---|---|---|---|---|"]
    for label, _ in FH + SECONDARY:
        for arm, s in report["cells"][label]["arms"].items():
            lines.append(
                f"| {label} | {arm} | {s['p50']:.2f} | {s['p95']:.2f} | {s['p99']:.2f} | {s['wall']:.2f} | "
                f"{s['requests']:.0f} | {(s['cv'] or 0):.3f} | {fmt(s['min'])} | {fmt(s['max'])} |")
    lines += ["", "Run status:", ""] + [f"- run {r} {e}: status={s} correctness={c}" for r, e, s, c in status]
    if ambient:
        lines += ["", "Ambient (host probe CPU ms, before → after each arm):", ""]
        by = defaultdict(dict)
        for a in ambient:
            probe = a.get("host_probe") or {}
            by[(a["run"], a["arm"])][a["when"]] = (a["utc"], probe.get("cpu_ns", 0) / 1e6, a.get("loadavg"))
        for (run, arm), w in sorted(by.items()):
            b, e = w.get("before"), w.get("after")
            lines.append(f"- run {run} {arm}: {b[0] if b else '?'} → {e[0] if e else '?'}; probe "
                         f"{b[1] if b else float('nan'):.0f} → {e[1] if e else float('nan'):.0f} ms; "
                         f"load {b[2] if b else '?'} → {e[2] if e else '?'}")
    (RESULTS / "confirm_report.md").write_text("\n".join(lines) + "\n")
    print("\n".join(lines))


def micro():
    lines = ["# Issue #63 microbenchmarks (median of run medians; in-process thread CPU)", ""]
    for tier in ("100k", "500k"):
        runs = load_micro(tier)
        if not runs:
            continue
        agg = defaultdict(list)
        meta = {}
        for run in runs:
            for p in run["points"]:
                key = (p["part"], p["name"], p["variant"])
                agg[key].append(p["sample"]["cpu_ns_per_op"])
                meta[key] = p
        lines += [f"## Tier {tier} ({len(runs)} runs, {runs[0]['ordinals']:,} ordinals)", ""]
        lines += ["### B1 construction", "", "| rep | CPU µs/op (runs) | allocs/op | bytes/op | bytes touched est. | persistent B |",
                  "|---|---|---|---|---|---|"]
        for (part, name, variant), values in sorted(agg.items()):
            if part == "b1" and name == "construct":
                p = meta[(part, name, variant)]
                lines.append(f"| {variant} | {median(values) / 1e3:,.2f} ({' / '.join(f'{v / 1e3:,.1f}' for v in values)}) | "
                             f"{p['sample']['allocations_per_op']:.1f} | {p['sample']['allocated_bytes_per_op']:,.0f} | "
                             f"{p['bytes_touched_estimate']:,} | {p['extra']['persistent_bytes']:,} |")
        lines += ["", "### B1 pipeline (FH cells)", "", "| cell | cand | CPU µs/op | allocs/op | phases µs cand/facets/sort | paths |", "|---|---|---|---|---|---|"]
        for (part, name, variant), values in sorted(agg.items()):
            if part == "b1" and name == "pipeline" and variant.split(":")[0] in dict((c, l) for l, c in FH):
                p = meta[(part, name, variant)]
                ph = p["extra"]["phase_us"]
                lines.append(f"| {variant.split(':')[0]} | {variant.split(':')[1]} | {median(values) / 1e3:,.0f} | "
                             f"{p['sample']['allocations_per_op']:.0f} | {ph['candidates']:,.0f} / {ph['facets']:,.0f} / {ph['sort']:,.0f} | "
                             f"{','.join(p['extra']['facet_paths'])} {p['extra']['sort_path']} |")
        lines += ["", "### C primitives", "", "| primitive | variant | CPU ns/op | allocs/op | bytes/op | bytes touched est. | result |", "|---|---|---|---|---|---|---|"]
        for (part, name, variant), values in sorted(agg.items()):
            if part == "c":
                p = meta[(part, name, variant)]
                lines.append(f"| {name} | {variant} | {median(values):,.0f} | {p['sample']['allocations_per_op']:.1f} | "
                             f"{p['sample']['allocated_bytes_per_op']:,.0f} | {p['bytes_touched_estimate']:,} | {p['result_cardinality']:,} |")
        if tier == "500k":
            lines += ["", "### B2 facet counting (ns per candidate unless noted)", "",
                      "| set | attr | V | |C| | ordinal | bitmap (ns/value) | d1 iter | d2 +gather | d3 +count u64 | d3 u32 | d5 materialize (ns/value) | dense ord | dense bitmap-len (ns/value) |",
                      "|---|---|---|---|---|---|---|---|---|---|---|---|---|"]
            b2 = defaultdict(dict)
            d1 = {}
            for (part, name, variant), values in agg.items():
                if part != "b2":
                    continue
                p = meta[(part, name, variant)]
                if variant == "d1_iterate":
                    d1[name] = median(values) / max(p["work_items"], 1)
                    continue
                b2[name][variant] = (median(values), p)
            for name in sorted(b2):
                set_label, attr = name.split("|")
                v = b2[name]
                any_p = next(iter(v.values()))[1]
                c = any_p["extra"]["candidates"]
                vv = any_p["extra"]["cardinality_v"]
                per_c = lambda k: f"{v[k][0] / max(c, 1):.2f}" if k in v else "—"
                per_v = lambda k: f"{v[k][0] / max(vv, 1):.1f}" if k in v else "—"
                lines.append(f"| {set_label} | {attr} | {vv} | {c:,} | {per_c('ordinal')} | {per_v('bitmap')} | "
                             f"{d1.get(set_label, float('nan')):.2f} | {per_c('d2_iterate_gather')} | {per_c('d3_count_u64')} | "
                             f"{per_c('d3_count_u32')} | {per_v('d5_materialize')} | {per_c('dense_ordinal_all')} | {per_v('dense_bitmap_len_all')} |")
        lines.append("")
    (RESULTS / "micro_report.md").write_text("\n".join(lines) + "\n")
    print("\n".join(lines))


def micro_medians(tier):
    """{(part, name, variant): (median cpu ns/op, point)} over runs."""
    agg = defaultdict(list)
    meta = {}
    for run in load_micro(tier):
        for p in run["points"]:
            key = (p["part"], p["name"], p["variant"])
            agg[key].append(p["sample"]["cpu_ns_per_op"])
            meta[key] = p
    return {k: (median(v), meta[k]) for k, v in agg.items()}


# Section 4: primitives with a same-window Part A service cell.
PRIMITIVES = [
    # (label, C point (name, variant), Part A cell label or None, note)
    ("exact lookup (per lookup)", ("exact_lookup", "variant_id_to_ordinal_and_record"), None, "per-op / 1000"),
    ("single bitmap filter: color=white", ("single_bitmap_color_white", "indexed_candidates"), "FD1", ""),
    ("single bitmap filter: color=white (borrowed)", ("single_bitmap_color_white", "borrowed_len"), None, ""),
    ("single bitmap filter: category Accent Chairs", ("single_bitmap_category_accent_chairs", "indexed_candidates"), None, ""),
    ("3-way bitmap conjunction", ("conjunction_3way", "indexed_candidates"), "FD3", ""),
    ("5-way conjunction (4 enums + rating range)", ("conjunction_5way", "indexed_candidates"), "FD5",
     "service cell FD5 = #77 filter_depth_5 = the 4 enum filters only"),
    ("numeric range, broad (rating >= 4)", ("numeric_range_broad_rating_gte_4", "indexed_candidates"), "SH1",
     "service cell SH1 = range + bounded sort"),
    ("numeric range, narrow (review_count >= p99)", ("numeric_range_narrow_review_count_p99", "indexed_candidates"), None, ""),
    ("same-variant conjunction (synthetic, 100k-derived)", ("same_variant_conjunction", "indexed_candidates"), None, "100k tier only"),
    ("lexical residual: wood+bed", ("lexical_and:wood+bed", "lexical_and_candidates"), None, "reference only"),
    ("lexical residual: outdoor+dining+table", ("lexical_and:outdoor+dining+table", "lexical_and_candidates"), None, "reference only"),
]


def report():
    confirm_report = json.loads((RESULTS / "confirm_report.json").read_text())
    cells = confirm_report["cells"]
    m500 = micro_medians("500k")
    m100 = micro_medians("100k")
    lines = ["# Issue #63 report tables", ""]

    # Native service floor: the lightest Part A native FINAL cell.
    floor_label, floor = min(
        ((label, c["arms"]["native"]["median"]) for label, c in cells.items() if "native" in c["arms"]),
        key=lambda x: x[1])
    lines += [f"Native per-request service floor = {floor:,.0f} µs CPU/query ({floor_label}, Part A native FINAL median).", ""]

    # Primitive table.
    lines += ["## Primitive table (500k unless noted; in-process thread CPU, median of 3 runs)", "",
              "| primitive | CPU/op | allocs/op | bytes alloc/op | bytes touched (est.) | result | 100k→500k | same-window r (cell) | class |",
              "|---|---|---|---|---|---|---|---|---|"]
    rows = []
    for label, (name, variant), service, note in PRIMITIVES:
        key = ("c", name, variant)
        src = m500 if key in m500 else m100
        if key not in src:
            continue
        ns, p = src[key]
        per = 1000 if name == "exact_lookup" else 1
        ns /= per
        small = m100.get(key)
        scaling = f"{(ns * per) / small[0]:.2f}x" if small and src is m500 and small[0] else "—"
        r = None
        if service and service in cells and "native" in cells[service]["arms"]:
            r = cells[service]["arms"]["native"].get("r")
        if label.startswith("lexical"):
            cls = "DELEGATE (reference; #57)"
        elif r is not None:
            cls = "KEEP" if r <= MATERIAL else ("REFINE" if r <= PARITY else "DELEGATE")
        else:
            cls = "KEEP-provisional" if ns / 1e3 <= 0.10 * floor else "REFINE"
        rows.append({"primitive": label, "cpu_ns_per_op": ns, "allocs": p["sample"]["allocations_per_op"] / per,
                     "alloc_bytes": p["sample"]["allocated_bytes_per_op"] / per,
                     "bytes_touched": p["bytes_touched_estimate"] / per, "result": p["result_cardinality"],
                     "scaling": scaling, "r": r, "service_cell": service, "class": cls, "note": note,
                     "tier": "500k" if src is m500 else "100k"})
        cpu = f"{ns:,.0f} ns" if ns < 1e4 else f"{ns / 1e3:,.1f} µs"
        rstr = f"{r:.3f} ({service})" if r is not None else "—"
        lines.append(f"| {label}{' [100k]' if src is m100 else ''} | {cpu} | {p['sample']['allocations_per_op'] / per:.2f} | "
                     f"{p['sample']['allocated_bytes_per_op'] / per:,.0f} | {p['bytes_touched_estimate'] / per:,.0f} | "
                     f"{p['result_cardinality']:,} | {scaling} | {rstr} | {cls} |")
    notes = [f"- {r['primitive']}: {r['note']}" for r in rows if r["note"]]
    lines += ["", *notes, ""]

    # Residual attribution for FH3/FH4.
    def pipe(cell, cand="p0"):
        v = m500.get(("b1", "pipeline", f"{cell}:{cand}"))
        return v[0] / 1e3 if v else None

    def b2(name, variant):
        v = m500.get(("b2", name, variant))
        return v[0] / 1e3 if v else None

    construct = m500[("b1", "construct", "p0")][0] / 1e3
    lines += ["## Residual attribution (µs; service = Part A native FINAL median CPU/query; components from 500k microbenchmarks)", ""]
    attribution = {}
    for label, cell in (("FH3", "facet_high_cardinality_color"), ("FH4", "facet_disjunctive_multi_dim")):
        service = cells[label]["arms"]["native"]["median"]
        in_process = pipe(cell)
        parts = {}
        if label == "FH3":
            color = "full|color"
            parts["match-all construction (P0)"] = construct
            d1 = m500[("b2", "full", "d1_iterate")][0] / 1e3
            d2, d3, ordinal = b2(color, "d2_iterate_gather"), b2(color, "d3_count_u64"), b2(color, "ordinal")
            parts["facet counting: bitmap iteration"] = d1
            parts["facet counting: column gather"] = d2 - d1
            parts["facet counting: counter increments"] = d3 - d2
            parts["facet counting: output materialization"] = ordinal - d3
        else:
            black = "real:facet_disjunctive_multi_dim"
            parts["match-all construction (P0, color self-exclusion)"] = construct
            parts["color facet over full catalog (ordinal)"] = b2("full|color", "ordinal")
            parts["4 facets over the color=black set (ordinal)"] = sum(
                b2(f"{black}|{a}", "ordinal") or 0 for a in ("style", "primarymaterial", "material", "shape"))
        modelled = sum(parts.values())
        parts["other in-process (candidates for the base filter, assembly, maps)"] = in_process - modelled
        parts["outside the in-process pipeline (HTTP, JSON, kernel, launch-state)"] = service - in_process
        attribution[label] = {"service_us": service, "in_process_us": in_process, "parts": parts}
        lines += [f"### {label}: service {service:,.0f} µs; in-process pipeline {in_process:,.0f} µs", "",
                  "| component | µs | % of service |", "|---|---|---|"]
        for k, v in parts.items():
            lines.append(f"| {k} | {v:,.0f} | {100 * v / service:.1f}% |")
        lines.append("")
    lines += ["Components are separate microbenchmark medians (a model), not a profile of one request; "
              "'other in-process' absorbs their interaction and noise. Service CPU and microbenchmarks "
              "come from different processes and windows.", ""]
    (RESULTS / "report.json").write_text(json.dumps({"floor_us": floor, "floor_cell": floor_label,
                                                      "primitives": rows, "attribution": attribution}, indent=2) + "\n")
    (RESULTS / "report.md").write_text("\n".join(lines) + "\n")
    print("\n".join(lines))


if __name__ == "__main__":
    command = sys.argv[1] if len(sys.argv) > 1 else ""
    {"adopt": adopt, "confirm": confirm, "micro": micro, "report": report}.get(
        command, lambda: sys.exit(__doc__))()
