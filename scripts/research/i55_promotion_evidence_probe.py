#!/usr/bin/env python3
"""Issue #55 Priority 2 (semantic promotion architecture): a preliminary,
exploratory probe of ONE candidate evidence source -- top-level
category-hierarchy co-membership -- for distinguishing PROMOTE from
UNRESOLVED candidate ProductTypeAny hyponym relations.

NOT a preregistered, held-out-validated experiment. This script was
written to scope the problem (what does real WANDS category data even
look like for the currently-live candidate set?) before designing a
proper preregistered promotion-gate test. See
docs/experiments/ISSUE55_SEMANTIC_PROMOTION_LOG.md for the full
writeup, caveats, and named next steps -- do not treat this script's
output as a finished PROMOTE/REJECT decision rule.

Ground truth used (established by prior, already-adversarially-reviewed
checkpoints, not invented here):
  - docs/decisions/ISSUE55_HYPONYM_LEAF_ONLY_DECISION.md's own disclosed,
    still-unfixed residual risk: "beds" -> "cat beds" / "dog beds & mats"
    are confirmed cross-family false positives that survive the current
    (leaf-only) production mechanism.
  - crates/phase9-eval/src/bin/p9_e08_hyponym_group_false_family_audit.rs,
    rerun fresh against current (leaf-only) production immediately before
    this script was written, confirming the CURRENT live candidate groups
    are exactly {"beds": [12 narrower names, including the two known-bad
    ones], "recliners": [2 narrower names, both presumed-fine]} --
    everything else (hot tubs->saunas, candles->diffusers,
    bed accessories->shower curtains) that an EARLIER (pre-leaf-fix) audit
    flagged is confirmed GONE under current production and must not be
    used as ground truth for testing an evidence source against today's
    mechanism.

Usage: python3 scripts/research/i55_promotion_evidence_probe.py
Requires: dataset_cache/wands/product.csv (already fetched by
scripts/datasets/fetch_wands.sh in prior sessions).
"""

import csv
from collections import Counter, defaultdict

PRODUCT_CSV = "dataset_cache/wands/product.csv"

# The exact, currently-live candidate set (verified fresh via
# p9_e08_hyponym_group_false_family_audit immediately before this probe;
# do not edit without rerunning that audit first).
CURRENT_LIVE_CANDIDATES = {
    "beds": [
        "adjustable beds",
        "cat beds",
        "dog beds & mats",
        "daybeds & guest beds",
        "kids beds",
        "teen beds",
    ],
    "recliners": ["gray recliners"],
}
KNOWN_BAD = {("beds", "cat beds"), ("beds", "dog beds & mats")}


def top_level(category_hierarchy: str) -> str:
    return category_hierarchy.split("/")[0].strip().lower()


def load_category_hierarchies_by_product_class(path: str) -> dict[str, list[str]]:
    by_class: dict[str, list[str]] = defaultdict(list)
    with open(path, encoding="utf-8") as f:
        for row in csv.DictReader(f, delimiter="\t"):
            product_class = (row.get("product_class") or "").strip().lower()
            category_hierarchy = (row.get("category hierarchy") or "").strip()
            if product_class and category_hierarchy:
                by_class[product_class].append(category_hierarchy)
    return by_class


def main() -> None:
    by_class = load_category_hierarchies_by_product_class(PRODUCT_CSV)

    false_promotions = 0
    promoted = 0
    unresolved = 0
    for broad, narrows in CURRENT_LIVE_CANDIDATES.items():
        broad_tops = Counter(top_level(c) for c in by_class.get(broad, []))
        print(f"BROAD {broad!r} n={len(by_class.get(broad, []))} top-levels={broad_tops.most_common(3)}")
        for narrow in narrows:
            rows = by_class.get(narrow, [])
            tops = Counter(top_level(c) for c in rows)
            overlap = set(broad_tops) & set(tops)
            is_promote = bool(overlap) and bool(rows)
            verdict = "PROMOTE" if is_promote else "UNRESOLVED"
            is_known_bad = (broad, narrow) in KNOWN_BAD
            if is_promote:
                promoted += 1
                if is_known_bad:
                    false_promotions += 1
            else:
                unresolved += 1
            flag = " <-- KNOWN BAD (must stay UNRESOLVED, never PROMOTE)" if is_known_bad else ""
            print(
                f"   {broad!r:12s} -> {narrow!r:25s} n={len(rows):4d} "
                f"top-levels={tops.most_common(2)} verdict={verdict}{flag}"
            )

    print()
    print(f"promoted={promoted} unresolved={unresolved} false_promotions={false_promotions}")
    print(
        "Zero-false-promotion bar: "
        + ("PASS" if false_promotions == 0 else "FAIL")
        + f" ({false_promotions}/{len(KNOWN_BAD)} known-bad candidates were wrongly promoted)"
    )


if __name__ == "__main__":
    main()
