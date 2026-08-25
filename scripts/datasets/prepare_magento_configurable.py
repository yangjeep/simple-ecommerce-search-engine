#!/usr/bin/env python3
"""Issue #55 H3: expand Magento's configurable-product CSV fixtures (one
row per *parent* style, with `size`/`color` cells each holding a
newline-separated list of every option that style comes in) into one JSON
object per parent product, each carrying its concrete list of (size,
color) child variants -- the real Product/Variant shape
`commerce_core::domain::Product`/`Variant` models directly.

This is a deliberately different vertical/format from WANDS/ESCI: real
apparel configurable products with genuine parent/child SKU structure,
used to test H3 (same-variant conjunction correctness) against real
data, since neither WANDS nor ESCI has real variant structure.

**Disclosed sparsification, not fabrication.** Magento's fixture format
enumerates the *full cartesian product* of each parent's color list and
size list as its variant set -- every color pairs with every size. That
leaves zero genuine "cross-variant conjunction" trap opportunities within
a single product (the exact bug class H3/`variant_safety.rs` tests for:
querying color=A AND size=B must not match a product merely because some
variant has A and some other variant has B, unless one variant has both).
A deterministic checkerboard keeps roughly half of each product's
cartesian combinations -- `keep iff (color_index + size_index) is even`
-- which (a) is fully reproducible with no RNG, (b) guarantees every real
color and every real size value from the source CSV still appears on at
least one kept variant (no attribute value is invented or dropped), and
(c) creates real per-product holes, mirroring how real retail catalogs
routinely do not stock every color in every size. Only which
*combinations* are treated as in-stock SKUs is a disclosed modification;
every product name, attribute value, and category is untouched real data.

Reads dataset_cache/magento_configurable/products_{men_tops,men_bottoms,
women_tops,women_bottoms}.csv (fetch_magento_configurable.sh's output).

Output: dataset_cache/magento_configurable/catalog.jsonl, one JSON object
per parent product:
  {"sku": "...", "name": "...", "price_cents": N, "category": "...",
   "material": "...", "colors": [...], "sizes": [...],
   "variants": [{"color": "...", "size": "..."}, ...]}  # checkerboard subset
"""
import csv
import json
import sys
from pathlib import Path

SCRIPT_DIR = Path(__file__).resolve().parent
ROOT = SCRIPT_DIR.parent.parent
IN_DIR = ROOT / "dataset_cache" / "magento_configurable"
OUT_PATH = IN_DIR / "catalog.jsonl"

FILES = [
    "products_men_tops.csv",
    "products_men_bottoms.csv",
    "products_women_tops.csv",
    "products_women_bottoms.csv",
]


def split_multi(value):
    return [v.strip() for v in value.split("\n") if v.strip()]


def parse_price_cents(raw):
    raw = raw.strip()
    if not raw:
        return 0
    return round(float(raw) * 100)


def main():
    products = []
    for fname in FILES:
        path = IN_DIR / fname
        if not path.exists():
            print(f"missing {path}, run fetch_magento_configurable.sh first", file=sys.stderr)
            sys.exit(1)
        with open(path, newline="", encoding="utf-8") as f:
            reader = csv.DictReader(f)
            for row in reader:
                colors = split_multi(row["color"])
                sizes = split_multi(row["size"])
                if not colors or not sizes:
                    # disclosed, not silently dropped: a parent row with no
                    # color or size option is not a configurable product in
                    # the sense this experiment needs (no variant axis to
                    # test cross-variant conjunctions against).
                    print(
                        f"skipping {row['sku']} ({fname}): colors={colors} sizes={sizes}",
                        file=sys.stderr,
                    )
                    continue
                # Checkerboard sparsification only creates a genuine
                # cross-variant trap opportunity -- and only preserves every
                # attribute value's presence -- when both axes have >= 2
                # distinct values (with >= 2 colors, color indices 0 and 1
                # have opposite parity, so between them every size parity is
                # covered, and symmetrically for sizes >= 2; a single-color
                # or single-size product has no color x size trap to create
                # in the first place, so it keeps its full, real cartesian
                # set unmodified).
                if len(colors) >= 2 and len(sizes) >= 2:
                    variants = [
                        {"color": c, "size": s}
                        for size_idx, s in enumerate(sizes)
                        for color_idx, c in enumerate(colors)
                        if (color_idx + size_idx) % 2 == 0
                    ]
                else:
                    variants = [
                        {"color": c, "size": s} for s in sizes for c in colors
                    ]
                kept_colors = {v["color"] for v in variants}
                kept_sizes = {v["size"] for v in variants}
                assert kept_colors == set(colors), (
                    f"{row['sku']}: sparsification dropped a color entirely: "
                    f"{set(colors) - kept_colors}"
                )
                assert kept_sizes == set(sizes), (
                    f"{row['sku']}: sparsification dropped a size entirely: "
                    f"{set(sizes) - kept_sizes}"
                )
                category_path = split_multi(row["category"])
                products.append(
                    {
                        "sku": row["sku"],
                        "name": row["name"].strip(),
                        "source_file": fname,
                        "price_cents": parse_price_cents(row["price"]),
                        "category_top": category_path[0] if category_path else "",
                        "material": row.get("material", "").replace("\n", "; ").strip(),
                        "colors": colors,
                        "sizes": sizes,
                        "variants": variants,
                    }
                )

    with open(OUT_PATH, "w", encoding="utf-8") as out:
        for p in products:
            out.write(json.dumps(p, ensure_ascii=False) + "\n")

    total_variants = sum(len(p["variants"]) for p in products)
    print(f"wrote {len(products)} parent products, {total_variants} expanded variants to {OUT_PATH}")


if __name__ == "__main__":
    main()
