#!/usr/bin/env python3
"""Issue #79 (Infra E3b) analysis: aggregates raw run JSONs into the
preregistered tables (#79 sections 8-9). Pure aggregation -- no threshold
or rule here is chosen from the data except tau_F / rho_S, and those only
from calibration cells, by the preregistered procedure.

Usage:
  analyze_e3b.py calibration        # crossover existence + tau_F / rho_S
  analyze_e3b.py report             # all tables (after headline + memory)
"""

import glob
import json
import math
import os
import statistics
import sys

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
RES = os.path.join(ROOT, "artifacts", "issue79", "results")
N = 515928
TOP_K = 48
FACET_CELLS = [
    "facet_low_cardinality_style",
    "facet_medium_cardinality_primarymaterial",
    "facet_high_cardinality_color",
    "facet_disjunctive_multi_dim",
]
SORT_HEADLINE = [
    "numeric_range_sort",
    "sh2_color_white_review_count_desc",
    "sh3_depth3_review_count_desc",
]


def load(phase):
    runs = []
    for path in sorted(glob.glob(os.path.join(RES, phase, "*.json"))):
        with open(path) as f:
            runs.append(json.load(f))
    return runs


def med(values):
    values = [v for v in values if v is not None]
    return statistics.median(values) if values else None


def cv(values):
    values = [v for v in values if v is not None]
    if len(values) < 2:
        return None
    return statistics.stdev(values) / statistics.mean(values)


def mode_key(cell):
    if cell.get("facet_mode") is None:
        return "n0_binary"
    return f"{cell['facet_mode']}:{cell['sort_mode']}"


def native_table(runs):
    """{(cell, mode): {metric: [per-run values]}}"""
    table = {}
    for run in runs:
        assert run["status"] == "ok", (run["label"], run["run"], run["status"])
        for cell in run["cells"]:
            key = (cell["cell"], mode_key(cell))
            entry = table.setdefault(key, {"runs": []})
            entry["runs"].append(cell)
    return table


def metric(entry, name):
    return [c.get(name) for c in entry["runs"]]


def per_facet_observations(entry):
    """Per (run) list of facet_diag, and per-run facets_us (whole phase)."""
    return [(c["diag"]["facet_diag"], c["mean_facets_us"]) for c in entry["runs"]]


def calibration():
    table = native_table(load("calibration"))
    facet_obs = []  # (cell, x=|C|/V, f1_us, f2_us)
    sort_obs = []  # (cell, y=|C|^2/(k N), s1_us, s2_us)
    for (cell, mode), entry in sorted(table.items()):
        pass
    cells = sorted({cell for cell, _ in table})
    print("## Calibration phase-time medians (in-process us, median of 3 runs)\n")
    print("| cell | family | |C| | V | x=|C|/V or y | legacy | path A | path B | A/B |")
    print("|---|---|---|---|---|---|---|---|---|")
    for cell in cells:
        if ("%s" % cell, "ordinal:legacy") in table:
            f1 = table[(cell, "ordinal:legacy")]
            f2 = table[(cell, "bitmap:legacy")]
            n0 = table[(cell, "legacy:legacy")]
            diag = f1["runs"][0]["diag"]["facet_diag"]
            assert len(diag) == 1, "calibration facet cells are single-facet"
            c, v = diag[0]["candidates"], diag[0]["cardinality"]
            a = med(metric(f1, "mean_facets_us"))
            b = med(metric(f2, "mean_facets_us"))
            leg = med(metric(n0, "mean_facets_us"))
            x = c / v
            facet_obs.append((cell, x, a, b))
            print(f"| {cell} | facet | {c} | {v} | {x:.3g} | {leg:.0f} | {a:.0f} | {b:.0f} | {a/b:.3g} |")
        else:
            s1 = table[(cell, "legacy:topk")]
            s2 = table[(cell, "legacy:presorted")]
            n0 = table[(cell, "legacy:legacy")]
            c = s1["runs"][0]["diag"]["num_candidates"]
            y = c * c / (TOP_K * N)
            a = med(metric(s1, "mean_sort_us"))
            b = med(metric(s2, "mean_sort_us"))
            leg = med(metric(n0, "mean_sort_us"))
            sort_obs.append((cell, y, a, b))
            print(f"| {cell} | sort | {c} | - | {y:.3g} | {leg:.0f} | {a:.0f} | {b:.0f} | {a/b:.3g} |")
    print()
    tau = fit("facet", facet_obs, "ordinal (A)", "bitmap (B)")
    rho = fit("sort", sort_obs, "candidate top-K (A)", "presorted (B)")
    print(json.dumps({"tau_f": tau, "rho_s": rho}))


def fit(name, obs, a_name, b_name):
    """Preregistered: crossover iff some cell has A <= 0.8 B and some has
    B <= 0.8 A. Threshold t: B (bitmap / presorted) iff feature >= t.
    Candidates: 0, inf, and geometric midpoints of consecutive distinct
    features; minimize summed chosen-path time; ties -> smaller t."""
    a_wins = [o for o in obs if o[2] <= 0.8 * o[3]]
    b_wins = [o for o in obs if o[3] <= 0.8 * o[2]]
    crossover = bool(a_wins) and bool(b_wins)
    print(f"### {name}: {a_name} wins >=25% on {[o[0] for o in a_wins]}; "
          f"{b_name} wins >=25% on {[o[0] for o in b_wins]} -> crossover={crossover}")
    features = sorted({o[1] for o in obs})
    candidates = [0.0]
    for lo, hi in zip(features, features[1:]):
        candidates.append(math.sqrt(lo * hi) if lo > 0 else hi / 2)
    candidates.append(math.inf)
    best = None
    for t in candidates:
        total = sum(o[3] if o[1] >= t else o[2] for o in obs)
        if best is None or total < best[1] - 1e-9:
            best = (t, total)
    total_a = sum(o[2] for o in obs)
    total_b = sum(o[3] for o in obs)
    print(f"    summed calibration time: all-A={total_a:.0f}us all-B={total_b:.0f}us "
          f"best-threshold={best[0]} -> {best[1]:.0f}us")
    if not crossover:
        winner = "A" if total_a <= total_b else "B"
        print(f"    NO CROSSOVER: hybrid not built; FINAL single path = {winner} "
              f"({a_name if winner == 'A' else b_name})\n")
        return None
    print(f"    CROSSOVER: fixed threshold = {best[0]}\n")
    return best[0]


def fastest_competitor(cell):
    best = None
    for path in glob.glob(os.path.join(RES, "competitor", "*.json")):
        pass
    by_engine = {}
    for run in load("competitor"):
        assert run["status"] == "ok" and run["correctness_all_passed"], run["engine"]
        for c in run["workload_cells"]:
            if c["name"] == cell:
                by_engine.setdefault(run["engine"], []).append(c["mean_cpu_usec_per_query"])
    meds = {e: med(v) for e, v in by_engine.items()}
    engine = min(meds, key=meds.get)
    return engine, meds[engine], meds


def report():
    head = native_table(load("headline"))
    n0 = native_table(load("n0"))
    print("## Headline cells: CPU/query us (median of 3 runs; CV) and P95 ms\n")
    for cell in FACET_CELLS + SORT_HEADLINE:
        modes = sorted({m for c, m in head if c == cell})
        n0e = n0.get((cell, "n0_binary"))
        print(f"### {cell}")
        try:
            engine, comp, allc = fastest_competitor(cell)
            print(f"fastest same-host competitor: {engine} {comp:.0f}us; all: "
                  + ", ".join(f"{e}={v:.0f}" for e, v in sorted(allc.items())))
        except (ValueError, AssertionError):
            engine, comp = None, None
            print("no competitor data (native-only cell)")
        print("| variant | cpu us | CV | p50 ms | p95 ms | cand us | facets us | sort us | server total us | inspected | path | N0/variant | vs fastest |")
        print("|---|---|---|---|---|---|---|---|---|---|---|---|---|")
        rows = [("N0 (unchanged #77 binary)", n0e)] + [(m, head[(cell, m)]) for m in modes]
        n0cpu = med(metric(n0e, "cpu_usec_per_query")) if n0e else None
        for label, e in rows:
            if e is None:
                continue
            cpu = med(metric(e, "cpu_usec_per_query"))
            diag = e["runs"][0].get("diag") or {}
            paths = ",".join(d["path"] for d in diag.get("facet_diag", [])) or diag.get("sort_path", "-")
            fus = med(metric(e, "mean_facets_us"))
            sus = med(metric(e, "mean_sort_us"))
            cus = med(metric(e, "mean_candidates_us"))
            tus = med(metric(e, "mean_total_us"))
            fmt = lambda v: "-" if v is None else f"{v:.0f}"
            print(f"| {label} | {cpu:.0f} | {cv(metric(e, 'cpu_usec_per_query')) or 0:.3f} | "
                  f"{med(metric(e, 'p50_ms')):.2f} | {med(metric(e, 'p95_ms')):.2f} | "
                  f"{fmt(cus)} | {fmt(fus)} | {fmt(sus)} | {fmt(tus)} | "
                  f"{diag.get('ids_inspected', '-')} | {paths} | "
                  f"{(n0cpu / cpu) if n0cpu else float('nan'):.2f}x | "
                  f"{(cpu / comp) if comp else float('nan'):.3g} |")
        print()


def classify(ratio, n0_over_final):
    if ratio is None:
        return "NOT MEASURED (no competitor)"
    if ratio <= 0.75:
        return "STRONG (cell-level: >=25% faster)"
    if ratio <= 1.25:
        return "RECOVERED / PARITY"
    if n0_over_final >= 2:
        return "PARTIAL RECOVERY"
    return "NOT RECOVERED"


def verdict():
    head = native_table(load("headline"))
    n0 = native_table(load("n0"))
    final = "hybrid:hybrid"
    print("## Preregistered classification (CPU/query, run medians; FINAL = hybrid:hybrid)\n")
    print("| cell | N0 us | FINAL us | FINAL CV | fastest competitor | its us | r = FINAL/fastest | N0/FINAL | N0/fastest (gap before) | P95 FINAL ms | P95 fastest ms | class |")
    print("|---|---|---|---|---|---|---|---|---|---|---|---|")
    out = {}
    for cell in FACET_CELLS + SORT_HEADLINE:
        n0cpu = med(metric(n0[(cell, "n0_binary")], "cpu_usec_per_query"))
        fe = head[(cell, final)]
        fcpu = med(metric(fe, "cpu_usec_per_query"))
        fp95 = med(metric(fe, "p95_ms"))
        try:
            engine, comp, _ = fastest_competitor(cell)
            cp95 = med([c["p95_ms"] for r in load("competitor") if r["engine"] == engine
                        for c in r["workload_cells"] if c["name"] == cell])
        except ValueError:
            engine, comp, cp95 = None, None, None
        ratio = fcpu / comp if comp else None
        cls = classify(ratio, n0cpu / fcpu)
        out[cell] = (ratio, n0cpu / fcpu, cls)
        print(f"| {cell} | {n0cpu:.0f} | {fcpu:.0f} | {cv(metric(fe, 'cpu_usec_per_query')) or 0:.3f} | "
              f"{engine or '-'} | {'-' if comp is None else f'{comp:.0f}'} | "
              f"{'-' if ratio is None else f'{ratio:.3f}'} | {n0cpu / fcpu:.1f}x | "
              f"{'-' if comp is None else f'{n0cpu / comp:.1f}x'} | {fp95:.2f} | "
              f"{'-' if cp95 is None else f'{cp95:.2f}'} | {cls} |")
    worst = max(FACET_CELLS[:3], key=lambda c: out[c][0])
    print(f"\nSub-dimension (a) single-facet full-catalog: worst cell = {worst} -> {out[worst][2]}")
    print(f"Sub-dimension (b) disjunctive: {out['facet_disjunctive_multi_dim'][2]}")
    strong = out["facet_disjunctive_multi_dim"][0] <= 0.75 and any(out[c][0] <= 0.75 for c in FACET_CELLS[:3])
    print(f"Overall facet STRONG condition met: {strong}")
    print(f"Sort (SH1 numeric_range_sort): {out['numeric_range_sort'][2]}")


def memory():
    runs = load("memory")
    by = {}
    for r in runs:
        assert r["status"] == "ok", r["label"]
        by.setdefault(r["label"], []).append(r)
    n0runs = load("n0")
    print("## Memory / build accounting (median of 3 launches; memory.current after load, 2 s settle)\n")
    print("| configuration | RSS after load MiB | peak after load MiB | launch->ready s | index build ms | sort columns bytes | presence bytes | columns build ms | presence build ms |")
    print("|---|---|---|---|---|---|---|---|---|")
    mib = lambda b: b / 1048576
    print(f"| N0 binary (#77, from N0 runs) | {mib(med([r['rss_after_load_bytes'] for r in n0runs])):.1f} | "
          f"{mib(med([r['peak_after_load_bytes'] for r in n0runs])):.1f} | "
          f"{med([r['launch_to_ready_ms'] for r in n0runs]) / 1000:.1f} | - | - | - | - | - |")
    base = None
    for label in ["mem_base", "mem_facet_only", "mem_sort_columns_only", "mem_presence_only", "mem_combined"]:
        rs = by[label]
        rss = med([r["rss_after_load_bytes"] for r in rs])
        if label == "mem_base":
            base = rss
        ready = [r["server_ready"] for r in rs]
        print(f"| {label[4:]} | {mib(rss):.1f} (delta vs base {mib(rss - base):+.1f}) | "
              f"{mib(med([r['peak_after_load_bytes'] for r in rs])):.1f} | "
              f"{med([r['launch_to_ready_ms'] for r in rs]) / 1000:.1f} | "
              f"{med([x['index_build_ms'] for x in ready]):.0f} | {ready[0]['sort_columns_bytes']} | "
              f"{ready[0]['presence_bytes']} | {med([x['sort_columns_build_ms'] for x in ready]):.1f} | "
              f"{med([x['presence_build_ms'] for x in ready]):.1f} |")
    print(f"\nindex_approx_bytes (on-heap estimate) = {by['mem_base'][0]['server_ready']['index_approx_bytes']}, "
          f"ordinal_facet_approx_bytes = {by['mem_base'][0]['server_ready']['ordinal_facet_approx_bytes']}, N = {N}")


if __name__ == "__main__":
    {"calibration": calibration, "report": report, "verdict": verdict, "memory": memory}[sys.argv[1]]()
