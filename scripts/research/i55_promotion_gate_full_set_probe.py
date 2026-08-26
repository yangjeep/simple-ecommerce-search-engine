#!/usr/bin/env python3
"""Issue #55 Priority 2: preregistered promotion-gate evidence test on the
FULL live product_type_hyponym_groups candidate set (149 groups, 317
broader/narrower pairs) -- see
docs/experiments/ISSUE55_PROMOTION_GATE_FULL_SET_PROTOCOL.md for the
preregistered method, ground truth, and thresholds. Do not change any
threshold in that document after reading this script's output.

Extends scripts/research/i55_promotion_evidence_probe.py (which tested
only top-level overlap on a hand-picked 2-group subset) with: the full
candidate set (loaded from the JSON export, not hand-transcribed), and
a second evidence source (category-path overlap at 2 levels).

Usage: python3 scripts/research/i55_promotion_gate_full_set_probe.py
Requires: dataset_cache/wands/product.csv,
docs/research/artifacts/i55_promotion_gate_full_set/candidate_set.json
(produced by crates/phase9-eval/src/bin/i55_hyponym_candidate_set_export.rs).
"""

import csv
import json
from collections import defaultdict

PRODUCT_CSV = "dataset_cache/wands/product.csv"
CANDIDATE_SET_JSON = "docs/research/artifacts/i55_promotion_gate_full_set/candidate_set.json"

# Established by prior, already-adversarially-reviewed checkpoints
# (ISSUE55_HYPONYM_LEAF_ONLY_DECISION.md's own re-audit after the
# leaf-only fix) -- not invented or re-derived here.
KNOWN_BAD = {("beds", "cat beds"), ("beds", "dog beds & mats")}


def top_level(category_hierarchy: str) -> str:
    return category_hierarchy.split("/")[0].strip().lower()


def level_2(category_hierarchy: str) -> str:
    parts = [p.strip().lower() for p in category_hierarchy.split("/")]
    if len(parts) < 2:
        return parts[0] if parts else ""
    return f"{parts[0]} / {parts[1]}"


def load_category_hierarchies_by_product_class(path: str) -> dict[str, list[str]]:
    by_class: dict[str, list[str]] = defaultdict(list)
    with open(path, encoding="utf-8") as f:
        for row in csv.DictReader(f, delimiter="\t"):
            product_class = (row.get("product_class") or "").strip().lower()
            category_hierarchy = (row.get("category hierarchy") or "").strip()
            if product_class and category_hierarchy:
                by_class[product_class].append(category_hierarchy)
    return by_class


def score(broad_rows, narrow_rows, extractor):
    if not broad_rows or not narrow_rows:
        return None  # UNRESOLVED (no evidence), excluded from the recall denominator
    broad_set = {extractor(c) for c in broad_rows}
    narrow_set = {extractor(c) for c in narrow_rows}
    return bool(broad_set & narrow_set)


def main() -> None:
    by_class = load_category_hierarchies_by_product_class(PRODUCT_CSV)
    with open(CANDIDATE_SET_JSON, encoding="utf-8") as f:
        candidate_set = json.load(f)

    total_pairs = sum(len(narrows) for narrows in candidate_set.values())
    print(f"=== Issue #55 Priority 2: full-set promotion-gate evidence test ({len(candidate_set)} groups, {total_pairs} pairs) ===\n")

    results = {"top_level": {"promoted": 0, "unresolved_no_evidence": 0, "unresolved_no_overlap": 0, "false_promotions": []},
               "level_2": {"promoted": 0, "unresolved_no_evidence": 0, "unresolved_no_overlap": 0, "false_promotions": []}}
    resolvable_non_known_bad = 0
    promoted_non_known_bad = {"top_level": 0, "level_2": 0}

    for broad, narrows in candidate_set.items():
        broad_rows = by_class.get(broad.lower(), [])
        for narrow in narrows:
            narrow_rows = by_class.get(narrow.lower(), [])
            is_known_bad = (broad, narrow) in KNOWN_BAD
            has_evidence = bool(broad_rows) and bool(narrow_rows)
            if has_evidence and not is_known_bad:
                resolvable_non_known_bad += 1

            for source, extractor in (("top_level", top_level), ("level_2", level_2)):
                verdict = score(broad_rows, narrow_rows, extractor)
                r = results[source]
                if verdict is None:
                    r["unresolved_no_evidence"] += 1
                elif verdict:
                    r["promoted"] += 1
                    if is_known_bad:
                        r["false_promotions"].append((broad, narrow))
                    else:
                        promoted_non_known_bad[source] += 1
                else:
                    r["unresolved_no_overlap"] += 1

    for source in ("top_level", "level_2"):
        r = results[source]
        print(f"--- evidence source: {source} ---")
        print(f"  promoted={r['promoted']} unresolved_no_overlap={r['unresolved_no_overlap']} unresolved_no_evidence={r['unresolved_no_evidence']}")
        print(f"  false_promotions={r['false_promotions']} (must be empty to pass the safety gate)")
        recall = promoted_non_known_bad[source] / resolvable_non_known_bad if resolvable_non_known_bad else 0.0
        print(f"  recall among resolvable non-known-bad candidates: {promoted_non_known_bad[source]}/{resolvable_non_known_bad} = {recall:.1%}")
        print(f"  safety gate: {'PASS' if not r['false_promotions'] else 'FAIL'}")
        print(f"  >=50% recall bar: {'PASS' if recall >= 0.5 else 'FAIL'}")
        print()

    print("--- comparative claim: does level_2 promote strictly more non-known-bad resolvable candidates than top_level? ---")
    print(f"  top_level={promoted_non_known_bad['top_level']} level_2={promoted_non_known_bad['level_2']}")
    print(
        "  "
        + (
            "level_2 promotes strictly more (added value confirmed)"
            if promoted_non_known_bad["level_2"] > promoted_non_known_bad["top_level"]
            else "level_2 does NOT promote strictly more (no added value over top_level at this scale)"
        )
    )


if __name__ == "__main__":
    main()
