#!/usr/bin/env python3
"""Issue #55 Priority 2: preregistered ancestor-breadcrumb-structure
promotion evidence test -- see
docs/experiments/ISSUE55_PROMOTION_GATE_ANCESTOR_STRUCTURE_PROTOCOL.md
for the preregistered method, ground truth, and thresholds. Do not
change any threshold in that document after reading this script's
output.

Extends scripts/research/i55_promotion_gate_full_set_probe.py: a
"/"-containing name is itself already a real category-hierarchy path
(WANDS ingestion's own documented null-product_class fallback,
crates/phase6a-eval/src/catalog.rs's effective_product_class), so it is
used as direct evidence, additive to the existing product_class lookup,
not a replacement for it.

Usage: python3 scripts/research/i55_promotion_gate_ancestor_structure_probe.py
"""

import csv
import json
from collections import defaultdict

PRODUCT_CSV = "dataset_cache/wands/product.csv"
CANDIDATE_SET_JSON = "docs/research/artifacts/i55_promotion_gate_full_set/candidate_set.json"

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


def paths_for(name: str, by_class: dict[str, list[str]]) -> list[str]:
    direct = [name] if "/" in name else []
    looked_up = by_class.get(name.lower(), [])
    return direct + looked_up


def score(broad_paths, narrow_paths, extractor):
    if not broad_paths or not narrow_paths:
        return None
    broad_set = {extractor(c) for c in broad_paths}
    narrow_set = {extractor(c) for c in narrow_paths}
    return bool(broad_set & narrow_set)


def main() -> None:
    by_class = load_category_hierarchies_by_product_class(PRODUCT_CSV)
    with open(CANDIDATE_SET_JSON, encoding="utf-8") as f:
        candidate_set = json.load(f)

    total_pairs = sum(len(narrows) for narrows in candidate_set.values())
    print(f"=== Issue #55 Priority 2: ancestor-breadcrumb-structure promotion evidence ({len(candidate_set)} groups, {total_pairs} pairs) ===\n")

    results = {"top_level": {"promoted": 0, "unresolved_no_overlap": 0, "unresolved_no_evidence": 0, "false_promotions": []},
               "level_2": {"promoted": 0, "unresolved_no_overlap": 0, "unresolved_no_evidence": 0, "false_promotions": []}}
    resolvable_non_known_bad = 0
    resolvable_only_via_ancestor_structure = 0
    promoted_non_known_bad = {"top_level": 0, "level_2": 0}

    for broad, narrows in candidate_set.items():
        broad_paths = paths_for(broad, by_class)
        broad_paths_lookup_only = by_class.get(broad.lower(), [])
        for narrow in narrows:
            narrow_paths = paths_for(narrow, by_class)
            narrow_paths_lookup_only = by_class.get(narrow.lower(), [])
            is_known_bad = (broad, narrow) in KNOWN_BAD
            has_evidence = bool(broad_paths) and bool(narrow_paths)
            had_evidence_before = bool(broad_paths_lookup_only) and bool(narrow_paths_lookup_only)
            if has_evidence and not is_known_bad:
                resolvable_non_known_bad += 1
                if not had_evidence_before:
                    resolvable_only_via_ancestor_structure += 1

            for source, extractor in (("top_level", top_level), ("level_2", level_2)):
                verdict = score(broad_paths, narrow_paths, extractor)
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
        print(f"--- evidence source: {source} (ancestor-structure + product_class lookup) ---")
        print(f"  promoted={r['promoted']} unresolved_no_overlap={r['unresolved_no_overlap']} unresolved_no_evidence={r['unresolved_no_evidence']}")
        print(f"  false_promotions={r['false_promotions']} (must be empty to pass the safety gate)")
        recall = promoted_non_known_bad[source] / resolvable_non_known_bad if resolvable_non_known_bad else 0.0
        print(f"  recall among resolvable non-known-bad candidates: {promoted_non_known_bad[source]}/{resolvable_non_known_bad} = {recall:.1%}")
        print(f"  safety gate: {'PASS' if not r['false_promotions'] else 'FAIL'}")
        print(f"  >=50% recall bar: {'PASS' if recall >= 0.5 else 'FAIL'}")
        print()

    print(f"resolvable non-known-bad pairs newly resolvable due to ancestor-structure evidence specifically: {resolvable_only_via_ancestor_structure}/{resolvable_non_known_bad}")


if __name__ == "__main__":
    main()
