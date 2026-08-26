#!/usr/bin/env python3
"""Issue #55: does a common, genuinely specific/disqualifying color
value exhibit broad cross-brand distribution on real ESCI data? Used to
test (and reject, before writing any commerce-core code) whether a
cross-Brand-breadth axis could safely substitute for
ResidualPolicy::classify's existing cross-ProductType-breadth signal on
product-type-sparse catalogs -- see
docs/decisions/ISSUE55_RESIDUAL_BREADTH_SIGNAL_DECISION.md.

Usage: python3 scripts/research/i55_residual_breadth_signal_check.py
Requires: dataset_cache/esci_electronics/esci_electronics_products.jsonl
"""

import json
from collections import defaultdict

by_color_brands = defaultdict(set)
with open("dataset_cache/esci_electronics/esci_electronics_products.jsonl") as f:
    for line in f:
        p = json.loads(line)
        color = (p.get("color") or "").strip().lower()
        brand = (p.get("brand") or "").strip().lower()
        if color and brand:
            by_color_brands[color].add(brand)

rows = sorted(by_color_brands.items(), key=lambda kv: -len(kv[1]))
print(f"{len(rows)} distinct color values in electronics")
for color, brands in rows[:15]:
    print(f"  color={color!r:20s} distinct_brands={len(brands)}")
print("...")
# specific, plausibly-disqualifying colors
for c in ["black", "white", "blue", "red", "silver", "gold"]:
    if c in by_color_brands:
        print(f"  color={c!r:20s} distinct_brands={len(by_color_brands[c])}")
