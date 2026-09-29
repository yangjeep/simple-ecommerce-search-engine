#!/usr/bin/env python3
"""Issue #57 frozen benchmark: schema + bulk-index one real ESCI vertical
slice (electronics/automotive/beauty) into an Elasticsearch- or
OpenSearch-family engine. Mirrors
scripts/datasets/solr_index_esci_electronics.py's field selection (no
product_type/category field -- ESCI carries none, and Issue #35's own
methodology constraint was to inject no hand-authored vertical ontology
on either side of that comparison; this benchmark preserves that
constraint rather than adding one now).

`brand`/`color` are lower-cased at index time, matching
crates/comparator-eval/src/translate_es.rs's case-insensitive strategy --
ESCI's `brand` field has real casing collisions (confirmed by Issue #35's
own comparator-hardening audit), unlike WANDS, so this is load-bearing
here, not a defensive no-op.

Usage: python3 scripts/datasets/es_family_index_esci.py <vertical> <base_url> <index_name>
  e.g. python3 scripts/datasets/es_family_index_esci.py electronics http://127.0.0.1:9200 esci_electronics_bench
"""
import json
import sys
import time
from pathlib import Path

import requests

VERTICAL = sys.argv[1]
BASE_URL = sys.argv[2] if len(sys.argv) > 2 else "http://127.0.0.1:9200"
INDEX = sys.argv[3] if len(sys.argv) > 3 else f"esci_{VERTICAL}_bench"
CATALOG = (
    Path(__file__).resolve().parents[2]
    / "dataset_cache"
    / f"esci_{VERTICAL}"
    / f"esci_{VERTICAL}_products.jsonl"
)
BATCH_SIZE = 500


def build_mapping():
    props = {
        "brand": {"type": "keyword", "doc_values": True},
        "color": {"type": "keyword", "doc_values": True},
        "title": {"type": "text"},
        "description": {"type": "text"},
        "bullet_point": {"type": "text"},
    }
    return {
        "settings": {"number_of_shards": 1, "number_of_replicas": 0},
        "mappings": {"properties": props},
    }


def to_doc(record):
    doc = {"title": record["title"]}
    if record.get("description"):
        doc["description"] = record["description"]
    if record.get("bullet_point"):
        doc["bullet_point"] = record["bullet_point"]
    if record.get("brand"):
        doc["brand"] = record["brand"].lower()
    if record.get("color"):
        doc["color"] = record["color"].lower()
    return doc


def main():
    session = requests.Session()
    if session.head(f"{BASE_URL}/{INDEX}").status_code == 200:
        session.delete(f"{BASE_URL}/{INDEX}").raise_for_status()
    session.put(f"{BASE_URL}/{INDEX}", json=build_mapping()).raise_for_status()

    batch_lines = []
    total = 0
    t0 = time.time()
    with open(CATALOG) as f:
        for line in f:
            record = json.loads(line)
            doc = to_doc(record)
            batch_lines.append(json.dumps({"index": {"_index": INDEX, "_id": record["product_id"]}}))
            batch_lines.append(json.dumps(doc))
            if len(batch_lines) >= BATCH_SIZE * 2:
                body = "\n".join(batch_lines) + "\n"
                resp = session.post(
                    f"{BASE_URL}/_bulk", data=body.encode("utf-8"), headers={"Content-Type": "application/x-ndjson"}
                )
                resp.raise_for_status()
                result = resp.json()
                if result.get("errors"):
                    raise RuntimeError(f"bulk index errors: {json.dumps(result)[:2000]}")
                total += len(batch_lines) // 2
                batch_lines = []
    if batch_lines:
        body = "\n".join(batch_lines) + "\n"
        resp = session.post(
            f"{BASE_URL}/_bulk", data=body.encode("utf-8"), headers={"Content-Type": "application/x-ndjson"}
        )
        resp.raise_for_status()
        result = resp.json()
        if result.get("errors"):
            raise RuntimeError(f"bulk index errors: {json.dumps(result)[:2000]}")
        total += len(batch_lines) // 2

    session.post(f"{BASE_URL}/{INDEX}/_refresh").raise_for_status()
    print(f"submitted {total} docs in {time.time() - t0:.1f}s")
    status = session.get(f"{BASE_URL}/{INDEX}/_count").json()
    print(f"count: {status['count']}")


if __name__ == "__main__":
    main()
