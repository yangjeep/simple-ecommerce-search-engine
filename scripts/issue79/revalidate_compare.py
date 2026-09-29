#!/usr/bin/env python3
"""Issue #79 post-integration revalidation: compare FINAL on the final
integrated main against #79's held-out headline FINAL.

Checks (declared before the revalidation ran; see ISSUE79_LOG.md):
  1. every run status ok and #77's cross-variant fixture passed;
  2. per cell, facets/docs fingerprints identical to the headline FINAL
     (output identity) and identical planner paths;
  3. CPU/query median ratio vs the headline FINAL median. PASS band
     [0.60, 1.45] = #79's measured host-drift envelope (section 11b);
     outside it the cell is flagged REGRESSION-SUSPECT, never silently
     accepted.

Usage: revalidate_compare.py <revalidation dir> [<headline dir>]
"""
import json
import statistics
import sys
from pathlib import Path

CELLS = [
    ("FH1", "facet_low_cardinality_style"),
    ("FH2", "facet_medium_cardinality_primarymaterial"),
    ("FH3", "facet_high_cardinality_color"),
    ("FH4", "facet_disjunctive_multi_dim"),
    ("SH1", "numeric_range_sort"),
]
BAND = (0.60, 1.45)


def final_rows(path):
    data = json.loads(Path(path).read_text())
    rows = {
        c["cell"]: c
        for c in data["cells"]
        if c.get("facet_mode") == "hybrid" and c.get("sort_mode") == "hybrid"
    }
    return data, rows


def signature(row):
    diag = row.get("diag") or {}
    return (
        row["facets_fingerprint"],
        row["docs_fingerprint"],
        row["num_found"],
        diag.get("sort_path"),
        tuple(f["path"] for f in diag.get("facet_diag", [])),
    )


def main():
    reval_dir = Path(sys.argv[1])
    headline_dir = Path(sys.argv[2] if len(sys.argv) > 2 else "artifacts/issue79/results/headline")
    reval = [final_rows(p) for p in sorted(reval_dir.glob("final_500k_run*.json"))]
    head = [final_rows(p) for p in sorted(headline_dir.glob("headline_500k_run*.json"))]
    problems = []
    for data, _ in reval:
        if data["status"] != "ok" or not data["fixture_correctness_all_passed"]:
            problems.append(f"run {data['run']}: status={data['status']} fixture={data['fixture_correctness_all_passed']}")
    report = {"git_sha": sorted({d["git_sha"] for d, _ in reval}), "cells": []}
    print("| cell | revalidation CPU µs (runs) | median | headline FINAL median | ratio | outputs identical | paths identical | verdict |")
    print("|---|---|---|---|---|---|---|---|")
    for label, cell in CELLS:
        reval_cpu = [rows[cell]["cpu_usec_per_query"] for _, rows in reval]
        head_cpu = [rows[cell]["cpu_usec_per_query"] for _, rows in head]
        head_sig = {signature(rows[cell]) for _, rows in head}
        reval_sig = {signature(rows[cell]) for _, rows in reval}
        outputs_same = {s[:3] for s in reval_sig} == {s[:3] for s in head_sig} and len(head_sig) == 1
        paths_same = {s[3:] for s in reval_sig} == {s[3:] for s in head_sig}
        med, head_med = statistics.median(reval_cpu), statistics.median(head_cpu)
        ratio = med / head_med
        ok = outputs_same and paths_same and BAND[0] <= ratio <= BAND[1]
        verdict = "PASS" if ok else ("REGRESSION-SUSPECT" if outputs_same and paths_same else "FAIL")
        if not ok:
            problems.append(f"{label}: {verdict} ratio={ratio:.3f} outputs={outputs_same} paths={paths_same}")
        runs = " / ".join(f"{v:,.0f}" for v in reval_cpu)
        print(f"| {label} | {runs} | {med:,.0f} | {head_med:,.0f} | {ratio:.3f} | {outputs_same} | {paths_same} | {verdict} |")
        report["cells"].append({
            "label": label, "cell": cell, "revalidation_cpu_usec": reval_cpu,
            "headline_cpu_usec": head_cpu, "ratio_of_medians": ratio,
            "outputs_identical": outputs_same, "paths_identical": paths_same, "verdict": verdict,
        })
    report["problems"] = problems
    report["overall"] = "PASS" if not problems else "FAIL"
    (reval_dir / "revalidation_report.json").write_text(json.dumps(report, indent=2) + "\n")
    print(f"\noverall: {report['overall']}")
    for problem in problems:
        print(f"  - {problem}")
    return 0 if not problems else 1


if __name__ == "__main__":
    sys.exit(main())
