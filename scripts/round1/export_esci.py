#!/usr/bin/env python3
"""Round 1: export the ESCI parquet files into flat JSONL for Rust ingestion.

Requires: pip install pandas pyarrow (see dataset_cache/venv, gitignored).
Input: dataset_cache/{products,examples}.parquet (fetch_esci.sh).
Output: dataset_cache/export/{catalog,queries}.jsonl (gitignored — this
script plus the raw parquet is what's reproducible).

catalog.jsonl: one real US-locale product per line, deduplicated by
product_id. No price/category/product_type fields exist in the source
data at all (see docs/experiments/ROUND1_LOG.md R1-E01) — only title,
description, bullet points, brand, color.

queries.jsonl: one real US-locale *test*-split query-product judgment per
line (the dataset's own held-out evaluation split — nothing in this
project trains on it). label is one of E(xact)/S(ubstitute)/C(omplement)/
I(rrelevant), per Amazon's human-annotated ESCI relevance scheme.
"""

import json
from pathlib import Path

import pandas as pd

CACHE = Path(__file__).resolve().parents[2] / "dataset_cache"
EXPORT = CACHE / "export"
EXPORT.mkdir(parents=True, exist_ok=True)


def clean(value):
    if pd.isna(value):
        return None
    text = str(value).strip()
    return text if text else None


def export_catalog():
    products = pd.read_parquet(CACHE / "products.parquet")
    us = products[products["product_locale"] == "us"].drop_duplicates(subset=["product_id"])
    print(f"catalog: {len(us)} distinct us-locale products")
    with open(EXPORT / "catalog.jsonl", "w") as f:
        for row in us.itertuples(index=False):
            record = {
                "id": row.product_id,
                "title": clean(row.product_title) or "",
                "description": clean(row.product_description),
                "bullets": clean(row.product_bullet_point),
                "brand": clean(row.product_brand),
                "color": clean(row.product_color),
            }
            f.write(json.dumps(record, ensure_ascii=False) + "\n")


def export_queries():
    examples = pd.read_parquet(CACHE / "examples.parquet")
    us_test = examples[(examples["product_locale"] == "us") & (examples["split"] == "test")]
    print(f"queries: {len(us_test)} judged (query, product) pairs, "
          f"{us_test['query_id'].nunique()} distinct queries")
    with open(EXPORT / "queries.jsonl", "w") as f:
        for row in us_test.itertuples(index=False):
            record = {
                "query_id": int(row.query_id),
                "query": row.query,
                "product_id": row.product_id,
                "label": row.esci_label,
            }
            f.write(json.dumps(record, ensure_ascii=False) + "\n")


if __name__ == "__main__":
    export_catalog()
    export_queries()
