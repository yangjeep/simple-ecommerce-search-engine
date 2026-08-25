#!/usr/bin/env python3
"""Issue #35 (docs/experiments/ISSUE35_ESCI_ELECTRONICS_PROTOCOL.md):
schema + bulk-index the real ESCI electronics-vertical slice into Apache
Solr, mirroring scripts/datasets/solr_index_wands.py's own pattern (same
docValues-on-filter-fields discipline) for a fair, independent baseline.

Reads the same dataset_cache/esci_electronics/esci_electronics_products.jsonl
the Rust ingestion (crates/esci-eval) reads, so both systems see
identical documents. Deliberately no product_type/category field --
ESCI carries none, and this checkpoint's own methodology constraint
(docs/experiments/ISSUE35_ESCI_ELECTRONICS_PROTOCOL.md) is to inject no
hand-authored vertical ontology on either side of the comparison.

Usage: python3 scripts/datasets/solr_index_esci_electronics.py [core_url]
"""
import json
import sys
import time
from pathlib import Path

import requests

SOLR_URL = sys.argv[1] if len(sys.argv) > 1 else "http://localhost:8983/solr/esci_electronics_bench"
CATALOG = Path(__file__).resolve().parents[2] / "dataset_cache" / "esci_electronics" / "esci_electronics_products.jsonl"
BATCH_SIZE = 500

STRING_DOCVALUES_FIELDS = ["brand", "color"]


def setup_schema(session):
    session.post(
        f"{SOLR_URL}/config",
        json={"set-user-property": {"update.autoCreateFields": "false"}},
    ).raise_for_status()

    fields = []
    for name in STRING_DOCVALUES_FIELDS:
        fields.append(
            {
                "name": name,
                "type": "string",
                "indexed": True,
                "stored": True,
                "docValues": True,
                "multiValued": False,
            }
        )
    fields.append({"name": "title", "type": "text_general", "indexed": True, "stored": True})
    fields.append({"name": "description", "type": "text_general", "indexed": True, "stored": True})
    fields.append({"name": "bullet_point", "type": "text_general", "indexed": True, "stored": True})

    resp = session.post(f"{SOLR_URL}/schema", json={"add-field": fields})
    if resp.status_code >= 400 and "already exists" not in resp.text:
        resp.raise_for_status()


def to_solr_doc(record):
    doc = {"id": record["product_id"], "title": record["title"]}
    if record.get("description"):
        doc["description"] = record["description"]
    if record.get("bullet_point"):
        doc["bullet_point"] = record["bullet_point"]
    if record.get("brand"):
        doc["brand"] = record["brand"]
    if record.get("color"):
        doc["color"] = record["color"]
    return doc


def main():
    session = requests.Session()
    setup_schema(session)

    batch = []
    total = 0
    t0 = time.time()
    with open(CATALOG) as f:
        for line in f:
            record = json.loads(line)
            batch.append(to_solr_doc(record))
            if len(batch) >= BATCH_SIZE:
                resp = session.post(
                    f"{SOLR_URL}/update/json/docs", json=batch, params={"commitWithin": 10000}
                )
                resp.raise_for_status()
                total += len(batch)
                batch = []
    if batch:
        resp = session.post(
            f"{SOLR_URL}/update/json/docs", json=batch, params={"commitWithin": 10000}
        )
        resp.raise_for_status()
        total += len(batch)

    print(f"submitted {total} docs in {time.time() - t0:.1f}s, committing...")
    resp = session.post(f"{SOLR_URL}/update", json={"commit": {}})
    resp.raise_for_status()

    status = session.get(f"{SOLR_URL}/select", params={"q": "*:*", "rows": 0}).json()
    print(f"numFound: {status['response']['numFound']}")


if __name__ == "__main__":
    main()
