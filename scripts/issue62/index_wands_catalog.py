#!/usr/bin/env python3
"""Stream-index a WANDS-format catalog JSONL file into a Solr core.

Issue #62 (Infra E2). Companion to provision_solr.sh -- deliberately reads
the input file line-by-line rather than loading it into memory, since the
largest E2 catalog tiers are multi-gigabyte (up to ~5.2GB / 5,030,298 rows).

Usage:
    index_wands_catalog.py <catalog_path> <core_url>

<core_url> is the base Solr core URL, e.g. http://localhost:8984/solr/i62_wands
"""
from __future__ import annotations

import json
import sys

import requests

BATCH_SIZE = 5000

# WANDS catalog fields this script maps into the i62_wands schema. `id` is
# Solr's implicit unique key and is passed through unchanged.
FIELDS = [
    "id",
    "title",
    "description",
    "product_class",
    "category_leaf",
    "category_depth_1",
    "category_depth_2",
    "category_depth_3",
    "category_depth_4",
    "category_depth_5",
    "category_depth_6",
    "color",
    "style",
    "primarymaterial",
    "material",
    "shape",
    "rating_count",
    "average_rating",
    "review_count",
]


def to_solr_doc(row: dict) -> dict:
    """Map one WANDS catalog row to a Solr doc, dropping null-valued fields.

    Solr's JSON update handler is fine with missing fields; dropping nulls
    outright (rather than sending them as JSON null) is the cleanest way to
    avoid any per-field-type null handling surprises.
    """
    doc = {}
    for field in FIELDS:
        value = row.get(field)
        if value is not None:
            doc[field] = value
    return doc


def post_batch(session: requests.Session, update_url: str, batch: list[dict]) -> None:
    if not batch:
        return
    resp = session.post(
        update_url,
        headers={"Content-Type": "application/json"},
        data=json.dumps(batch),
        timeout=300,
    )
    resp.raise_for_status()


def main() -> int:
    if len(sys.argv) != 3:
        print("usage: index_wands_catalog.py <catalog_path> <core_url>", file=sys.stderr)
        return 2

    catalog_path = sys.argv[1]
    core_url = sys.argv[2].rstrip("/")
    update_url = f"{core_url}/update/json/docs?commit=false"

    session = requests.Session()
    batch: list[dict] = []
    total = 0

    with open(catalog_path, "r", encoding="utf-8") as f:
        for line in f:
            line = line.strip()
            if not line:
                continue
            row = json.loads(line)
            batch.append(to_solr_doc(row))
            if len(batch) >= BATCH_SIZE:
                post_batch(session, update_url, batch)
                total += len(batch)
                batch = []
                print(f"  indexed {total} docs", file=sys.stderr)

    if batch:
        post_batch(session, update_url, batch)
        total += len(batch)

    print(f"  indexed {total} docs total", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
