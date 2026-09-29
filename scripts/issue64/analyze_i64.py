#!/usr/bin/env python3
"""Issue #64 (Infra E4, amendment 1) analysis -> confirm_report.{json,md}.

Implements amendment section 5 as preregistered: per-cell r = native median
CPU/query / fastest EQUIVALENT competitor median; #60 classes (<=0.75
MATERIAL, <=1.25 PARITY, else DISADVANTAGE); robust = all three per-run
paired ratios in the same class; verdict over the realistic grid R (S1-S5
cells, k>=1, plus the disjunctive/multi-select cells) as KEEP /
KEEP-REGIONAL / PARITY / REFINE. Also facet-only CPU (cell - same-scope
k=0), incremental CPU per added facet (least-squares slope over the ladder),
the breakpoint table and cores per 1,000 QPS. A missing or non-equivalent
dump makes that (engine, cell) NOT_EQUIVALENT_WORK; it never counts.
"""
import json
import os
import re
import statistics
import subprocess
import sys
from collections import defaultdict
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
RESULTS = Path(os.environ.get("I64_RESULTS", ROOT / "artifacts/issue64/results"))
MATERIAL, PARITY = 0.75, 1.25
ENGINES = ("native", "meilisearch", "solr")
COMPETITORS = ("meilisearch", "solr")
SCOPE_SIZE = {"s0": 515928, "s1": 192468, "s2": 55344, "s3": 14472, "s4": 3024, "s5": 600}


def classify(r):
    if r is None:
        return "N/A"
    return "MATERIAL" if r <= MATERIAL else ("PARITY" if r <= PARITY else "DISADVANTAGE")


def parse(name):
    m = re.fullmatch(r"i64_(s\d)_(k(\d+)|color|k5_style1|k5_style2)", name)
    scope, tail, k = m.group(1), m.group(2), m.group(3)
    kind = "baseline" if k == "0" else ("ladder" if k else ("color" if tail == "color" else tail.split("_")[1]))
    return {"scope": scope, "k": int(k) if k else None, "kind": kind,
            "realistic": scope != "s0" and kind != "baseline"}


def load():
    runs = {}
    for run_dir in sorted((RESULTS / "confirm").glob("run*")):
        run = int(run_dir.name[3:])
        entry = {}
        for engine in ENGINES:
            raw = run_dir / f"{engine}_500k.json"
            eq = run_dir / f"equivalence_{engine}.json"
            entry[engine] = {
                "raw": json.loads(raw.read_text()) if raw.exists() else None,
                "eq": json.loads(eq.read_text()) if eq.exists() else None,
            }
        runs[run] = entry
    return runs


def main():
    runs = load()
    names = subprocess.run([sys.executable, str(ROOT / "scripts/issue64/cell_names.py")],
                           check=True, capture_output=True, text=True).stdout.strip().split(",")
    per = defaultdict(lambda: defaultdict(dict))  # cell -> engine -> run -> metrics
    equivalent = defaultdict(dict)  # (engine, cell) -> run -> bool
    status = []
    rss = defaultdict(list)
    for run, entry in runs.items():
        for engine in ENGINES:
            raw, eq = entry[engine]["raw"], entry[engine]["eq"]
            if raw is None:
                status.append((run, engine, "MISSING", False))
                continue
            if engine == "native":
                status.append((run, engine, raw["status"], raw["fixture_correctness_all_passed"]))
                rss[engine].append(raw.get("rss_after_serving_bytes") or raw.get("rss_after_load_bytes"))
                for c in raw["cells"]:
                    per[c["cell"]][engine][run] = {"cpu": c["cpu_usec_per_query"], "p50": c["p50_ms"], "p95": c["p95_ms"],
                                                   "p99": c["p99_ms"], "wall": c["mean_wall_ms"], "requests": 1.0,
                                                   "facets_us": c.get("mean_facets_us")}
            else:
                status.append((run, engine, raw["status"], raw["correctness_all_passed"]))
                rss[engine].append(raw.get("rss_during_serving_bytes"))
                for c in raw["workload_cells"]:
                    per[c["name"]][engine][run] = {"cpu": c["mean_cpu_usec_per_query"], "p50": c["p50_ms"], "p95": c["p95_ms"],
                                                   "p99": c["p99_ms"], "wall": c["mean_wall_ms"],
                                                   "requests": c["mean_backend_requests"]}
            verdicts = {v["cell"]: v["equivalent"] for v in (eq or {}).get("verdicts", [])}
            for name in names:
                equivalent[(engine, name)][run] = bool(verdicts.get(name, False)) and name in per and run in per[name][engine]

    med = statistics.median
    cells = {}
    for name in names:
        meta = parse(name)
        arms = {}
        for engine in ENGINES:
            values = {r: m["cpu"] for r, m in per[name][engine].items() if m["cpu"] is not None}
            eq_runs = equivalent[(engine, name)]
            arms[engine] = {
                "runs": dict(sorted(values.items())),
                "median": med(values.values()) if values else None,
                "min": min(values.values()) if values else None,
                "max": max(values.values()) if values else None,
                "cv": (statistics.pstdev(values.values()) / statistics.mean(values.values())) if len(values) > 1 else None,
                "p50": med([m["p50"] for m in per[name][engine].values()]) if values else None,
                "p95": med([m["p95"] for m in per[name][engine].values()]) if values else None,
                "p99": med([m["p99"] for m in per[name][engine].values()]) if values else None,
                "wall": med([m["wall"] for m in per[name][engine].values()]) if values else None,
                "requests": med([m["requests"] for m in per[name][engine].values()]) if values else None,
                "equivalent_all_runs": len(eq_runs) == len(runs) and all(eq_runs.values()),
            }
        eligible = {e: arms[e] for e in COMPETITORS if arms[e]["equivalent_all_runs"] and arms[e]["median"]}
        fastest = min(eligible, key=lambda e: eligible[e]["median"]) if eligible else None
        entry = {**meta, "arms": arms, "fastest": fastest,
                 "not_equivalent": [e for e in ENGINES if not arms[e]["equivalent_all_runs"]]}
        if fastest and arms["native"]["equivalent_all_runs"] and meta["kind"] != "baseline":
            r = arms["native"]["median"] / eligible[fastest]["median"]
            per_run = [arms["native"]["runs"][x] / eligible[fastest]["runs"][x]
                       for x in arms["native"]["runs"] if x in eligible[fastest]["runs"]]
            entry.update({"r": r, "class": classify(r), "r_by_run": per_run,
                          "robust": "robust" if len(per_run) == len(runs) and {classify(x) for x in per_run} == {classify(r)} else "median-only"})
        else:
            entry.update({"r": None, "class": "NOT_EQUIVALENT_WORK" if meta["kind"] != "baseline" else "baseline"})
        cells[name] = entry

    grid = [c for c in cells.values() if c["realistic"]]
    # Section 5: NOT_EQUIVALENT_WORK cells are excluded from the ratio (and
    # so from the verdict) and reported separately.
    classes = [c["class"] for c in grid if c["class"] != "NOT_EQUIVALENT_WORK"]
    excluded = [c for c in grid if c["class"] == "NOT_EQUIVALENT_WORK"]
    if not classes:
        verdict = "NO CLASSIFIABLE CELL"
    elif any(c == "DISADVANTAGE" for c in classes):
        verdict = "REFINE"
    elif all(c == "MATERIAL" for c in classes):
        verdict = "KEEP"
    elif any(c == "MATERIAL" for c in classes):
        verdict = "KEEP-REGIONAL"
    else:
        verdict = "PARITY"

    # Facet-only CPU and incremental cost per added facet.
    curves = {}
    for scope in SCOPE_SIZE:
        base = f"i64_{scope}_k0"
        ladder = sorted((c for n, c in cells.items() if c["scope"] == scope and c["kind"] == "ladder"), key=lambda c: c["k"])
        curves[scope] = {}
        for engine in ENGINES:
            b = cells[base]["arms"][engine]["median"]
            points = [(c["k"], c["arms"][engine]["median"]) for c in ladder if c["arms"][engine]["median"] is not None]
            slope = intercept = None
            if len(points) >= 2:
                xs, ys = zip(*points)
                mx, my = statistics.mean(xs), statistics.mean(ys)
                slope = sum((x - mx) * (y - my) for x, y in points) / sum((x - mx) ** 2 for x in xs)
                intercept = my - slope * mx
            curves[scope][engine] = {"k0": b, "points": points, "slope_us_per_facet": slope, "intercept_us": intercept,
                                     "facet_only": {k: (y - b) if b is not None else None for k, y in points}}

    report = {"verdict": verdict, "grid_size": len(grid), "grid_classes": {c: classes.count(c) for c in set(classes)},
              "grid_not_equivalent": [c for c in cells if cells[c] in excluded],
              "status": status, "cells": cells, "curves": curves,
              "rss_bytes_median": {e: med([x for x in v if x]) if any(v) else None for e, v in rss.items()}}
    (RESULTS / "confirm_report.json").write_text(json.dumps(report, indent=2, default=str) + "\n")

    f = lambda v: "—" if v is None else f"{v:,.0f}"
    lines = [f"# Issue #64 — facet economics (500k, CPU/query µs, median of {len(runs)} runs)", "",
             f"**Verdict over the realistic grid R ({len(grid)} cells): {verdict}** — classes {report['grid_classes']}; "
             f"NOT_EQUIVALENT_WORK (excluded): {len(excluded)}", "",
             "| cell | scope size | k | native | meili | solr | fastest | r | class | robust | native cores/1k QPS | fastest cores/1k QPS |",
             "|---|---|---|---|---|---|---|---|---|---|---|---|"]
    for name in names:
        c = cells[name]
        a = c["arms"]
        fastest_cpu = a[c["fastest"]]["median"] if c["fastest"] else None
        mark = lambda e: "" if a[e]["equivalent_all_runs"] else " ⚠NEW"
        lines.append(
            f"| {name}{'' if c['realistic'] else ' (ref)'} | {SCOPE_SIZE[c['scope']]:,} | {c['k'] if c['k'] is not None else c['kind']} | "
            f"{f(a['native']['median'])}{mark('native')} | {f(a['meilisearch']['median'])}{mark('meilisearch')} | "
            f"{f(a['solr']['median'])}{mark('solr')} | {c['fastest'] or '—'} | "
            f"{'—' if c['r'] is None else format(c['r'], '.3f')} | {c['class']} | {c.get('robust', '—')} | "
            f"{'—' if a['native']['median'] is None else format(a['native']['median'] / 1e3, '.2f')} | "
            f"{'—' if fastest_cpu is None else format(fastest_cpu / 1e3, '.2f')} |")
    lines += ["", "⚠NEW = NOT_EQUIVALENT_WORK (missing or non-equivalent dump) in at least one run.", "",
              "## Cost curves: incremental CPU per added facet (least-squares slope over the ladder, µs/facet) and k=0 baseline", "",
              "| scope | size | native k0 | native slope | meili k0 | meili slope | solr k0 | solr slope |", "|---|---|---|---|---|---|---|---|"]
    for scope, eng in curves.items():
        lines.append(f"| {scope} | {SCOPE_SIZE[scope]:,} | " + " | ".join(
            f"{f(eng[e]['k0'])} | {f(eng[e]['slope_us_per_facet'])}" for e in ENGINES) + " |")
    lines += ["", "## Breakpoints (class by scope × k, ladder cells)", "", "| scope | " + " | ".join(f"k={k}" for k in (1, 3, 5, 8, 9, 10, 11)) + " | color |",
              "|---|" + "---|" * 8]
    for scope in SCOPE_SIZE:
        row = []
        for k in (1, 3, 5, 8, 9, 10, 11):
            c = cells.get(f"i64_{scope}_k{k}")
            row.append("" if c is None else f"{c['class'][:4]} {c['r']:.2f}" if c["r"] is not None else c["class"])
        cc = cells[f"i64_{scope}_color"]
        row.append(f"{cc['class'][:4]} {cc['r']:.2f}" if cc["r"] is not None else cc["class"])
        lines.append(f"| {scope} ({SCOPE_SIZE[scope]:,}) | " + " | ".join(row) + " |")
    lines += ["", "## Latency (P50 / P95 / P99 ms, medians over runs) and backend requests", "",
              "| cell | native | meili | solr |", "|---|---|---|---|"]
    for name in names:
        a = cells[name]["arms"]
        g = lambda e: "—" if a[e]["p50"] is None else f"{a[e]['p50']:.1f} / {a[e]['p95']:.1f} / {a[e]['p99']:.1f} ({a[e]['requests']:.0f} req)"
        lines.append(f"| {name} | {g('native')} | {g('meilisearch')} | {g('solr')} |")
    lines += ["", "RSS (median over runs, bytes): " + ", ".join(f"{e} {f(v)}" for e, v in report["rss_bytes_median"].items()), "",
              "Run status:", ""] + [f"- run {r} {e}: status={s} correctness={c}" for r, e, s, c in status]
    (RESULTS / "confirm_report.md").write_text("\n".join(lines) + "\n")
    print("\n".join(lines))


if __name__ == "__main__":
    sys.exit(main())
