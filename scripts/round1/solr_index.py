#!/usr/bin/env python3
"""Round 1 R1-E04: bulk-index the real ESCI catalog into Apache Solr.

The same catalog.jsonl export used to build commerce-core's CatalogIndex
(dataset_cache/export/catalog.jsonl) is indexed here, so both systems see
identical documents. Schema (title/description/bullets: text_general,
standard analysis; brand/color: string, for exact filtering) was applied
via the Schema API before running this script — see
docs/experiments/ROUND1_LOG.md R1-E04 for the exact schema commands.
"""

import json
import sys
import time
from pathlib import Path

import requests

SOLR_URL = "http://localhost:8983/solr/commerce_bench"
CATALOG = Path(__file__).resolve().parents[2] / "dataset_cache" / "export" / "catalog.jsonl"
BATCH_SIZE = 5000


def to_solr_doc(record):
    doc = {"id": record["id"], "title": record["title"]}
    if record.get("description"):
        doc["description"] = record["description"]
    if record.get("bullets"):
        doc["bullets"] = record["bullets"]
    if record.get("brand"):
        doc["brand"] = record["brand"]
    if record.get("color"):
        doc["color"] = record["color"]
    return doc


def main():
    max_docs = int(sys.argv[1]) if len(sys.argv) > 1 else None
    session = requests.Session()
    batch = []
    total = 0
    t0 = time.time()
    with open(CATALOG) as f:
        for line in f:
            if max_docs and total >= max_docs:
                break
            record = json.loads(line)
            batch.append(to_solr_doc(record))
            if len(batch) >= BATCH_SIZE:
                resp = session.post(f"{SOLR_URL}/update/json/docs", json=batch, params={"commitWithin": 60000})
                resp.raise_for_status()
                total += len(batch)
                batch = []
                if total % 100_000 == 0:
                    print(f"  indexed {total} docs in {time.time() - t0:.1f}s")
    if batch:
        resp = session.post(f"{SOLR_URL}/update/json/docs", json=batch, params={"commitWithin": 60000})
        resp.raise_for_status()
        total += len(batch)

    print(f"submitted {total} docs in {time.time() - t0:.1f}s, committing...")
    t1 = time.time()
    resp = session.post(f"{SOLR_URL}/update", json={"commit": {}})
    resp.raise_for_status()
    print(f"commit took {time.time() - t1:.1f}s")
    print(f"total index build time: {time.time() - t0:.1f}s")

    status = session.get(f"{SOLR_URL}/select", params={"q": "*:*", "rows": 0}).json()
    print(f"numFound: {status['response']['numFound']}")


if __name__ == "__main__":
    main()
