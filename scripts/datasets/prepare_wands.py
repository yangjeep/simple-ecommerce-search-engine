#!/usr/bin/env python3
"""Phase 6A (Issue #23): deterministically transform the raw WANDS
product.csv into catalog.jsonl -- the single, shared source both the
Rust ingestion (crates/phase6a-eval) and the Solr indexing script read,
so both systems see identical documents (the same discipline
scripts/round1/export_esci.py established for ESCI).

Real-data mapping decisions, grounded in scripts/datasets/profile_wands.py's
output (docs/research/artifacts/p6a_dataset_acquisition/manifest.json):

- `category hierarchy` (real column name has a space, not the underscore
  the WANDS README uses) is a "/"-delimited breadcrumb, depth 0-6 in this
  corpus. Each depth level 1-6 becomes its own field
  (category_depth_1..category_depth_6), valued as the FULL prefix path up
  to that depth (not the bare segment name) so two different subtrees
  that happen to share a segment name at the same depth never collide.
  This lets "subtree browse at any level" reuse a plain exact-match
  filter on the corresponding depth field -- no new query semantics.
- `product_class` is kept as an independent field (NOT derived from the
  hierarchy: only 28.3% of products have a hierarchy leaf that equals
  product_class). Maps to Commerce Core's `product_type`.
- The deepest available hierarchy segment becomes `category_leaf`, mapped
  to Commerce Core's dedicated `category` field (its own bitmap index,
  exercised here with real cardinality for the first time in this
  project -- ESCI always used a `CategoryId(0)` sentinel).
- `product_features` (a "|"-delimited `key:value` bag) is parsed into a
  dict; only `color`, `style`, `primarymaterial`, `material`, `shape` are
  carried through as first-class fields -- these are the only keys with
  broad-enough coverage (>=9% of the corpus) to serve as real facet
  workloads, per profile_wands.py's frequency analysis. No price,
  brand/manufacturer, or availability field exists in the source data at
  all (confirmed by grep against the raw file, not just the parsed
  keys) -- none is fabricated here.
- No parent-ASIN/variant-grouping equivalent exists; each row becomes
  exactly one product with exactly one variant, the same degenerate
  mapping round1_eval::catalog already uses for ESCI.
"""
import csv
import json
import sys
from pathlib import Path

IN_DIR = Path(__file__).resolve().parents[2] / "dataset_cache" / "wands"
OUT_DIR = IN_DIR
MAX_DEPTH = 6


def parse_features(raw):
    out = {}
    if not raw:
        return out
    for kv in raw.split("|"):
        if ":" not in kv:
            continue
        k, v = kv.split(":", 1)
        k = k.strip()
        if k:
            out[k] = v.strip()
    return out


def hierarchy_prefixes(raw):
    if not raw:
        return []
    parts = [p.strip() for p in raw.split(" / ") if p.strip()]
    return [" / ".join(parts[:d]) for d in range(1, len(parts) + 1)]


def main():
    csv.field_size_limit(sys.maxsize)
    out_path = OUT_DIR / "catalog.jsonl"
    n = 0
    with open(IN_DIR / "product.csv", newline="", encoding="utf-8") as f_in, open(
        out_path, "w", encoding="utf-8"
    ) as f_out:
        reader = csv.DictReader(f_in, delimiter="\t")
        for row in reader:
            prefixes = hierarchy_prefixes(row.get("category hierarchy", ""))
            features = parse_features(row.get("product_features", ""))
            record = {
                "id": row["product_id"],
                "title": row["product_name"],
                "description": row.get("product_description") or None,
                "product_class": row.get("product_class") or None,
                "category_leaf": prefixes[-1] if prefixes else None,
            }
            for d in range(1, MAX_DEPTH + 1):
                record[f"category_depth_{d}"] = prefixes[d - 1] if d <= len(prefixes) else None
            for key in ("color", "style", "primarymaterial", "material", "shape"):
                record[key] = features.get(key) or None
            for numeric_key in ("rating_count", "average_rating", "review_count"):
                raw_val = row.get(numeric_key)
                record[numeric_key] = float(raw_val) if raw_val else None
            f_out.write(json.dumps(record, ensure_ascii=False) + "\n")
            n += 1
    print(f"wrote {n} records to {out_path}")


if __name__ == "__main__":
    main()
