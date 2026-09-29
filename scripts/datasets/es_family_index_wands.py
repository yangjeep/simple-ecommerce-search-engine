#!/usr/bin/env python3
"""Issue #57 frozen benchmark: schema + bulk-index the real WANDS catalog
into an Elasticsearch- or OpenSearch-family engine (both share the same
REST mapping/bulk API surface at the subset used here).

Mirrors scripts/datasets/solr_index_wands.py's field selection and
fairness discipline exactly, so all three engines index the identical
real catalog with equivalent field capabilities (docValues-equivalent
keyword fields for every filter/sort/facet field used by the benchmark,
a dedicated non-analyzed sort field for title since `title` itself is
analyzed text -- same reason Solr gets a separate `title_sort` field).

Usage: python3 scripts/datasets/es_family_index_wands.py <base_url> <index_name> [catalog_path]
  e.g. python3 scripts/datasets/es_family_index_wands.py http://127.0.0.1:9200 wands_bench
       python3 scripts/datasets/es_family_index_wands.py http://127.0.0.1:9201 wands_bench
"""
import json
import sys
import time
from pathlib import Path

import requests

BASE_URL = sys.argv[1] if len(sys.argv) > 1 else "http://127.0.0.1:9200"
INDEX = sys.argv[2] if len(sys.argv) > 2 else "wands_bench"
CATALOG = (
    Path(sys.argv[3])
    if len(sys.argv) > 3
    else Path(__file__).resolve().parents[2] / "dataset_cache" / "wands" / "catalog.jsonl"
)
BATCH_SIZE = 2000

KEYWORD_FIELDS = [
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
]
NUMERIC_FIELDS = ["rating_count", "average_rating", "review_count"]


def build_mapping():
    props = {}
    for name in KEYWORD_FIELDS:
        props[name] = {"type": "keyword", "doc_values": True}
    for name in NUMERIC_FIELDS:
        props[name] = {"type": "double", "doc_values": True}
    props["title"] = {"type": "text"}
    props["title_sort"] = {"type": "keyword", "doc_values": True}
    props["description"] = {"type": "text"}
    return {
        "settings": {"number_of_shards": 1, "number_of_replicas": 0},
        "mappings": {"properties": props},
    }


def to_doc(record):
    # KEYWORD_FIELDS are lower-cased at index time to match
    # crates/comparator-eval/src/translate_es.rs's translator, which
    # lower-cases the *query* side of every term/terms clause -- both
    # sides must agree for this to be the case-insensitive-whole-match
    # equivalent of Solr's regex fq that translate_es.rs's doc comment
    # claims, not a silent correctness gap. `title`/`title_sort` are
    # deliberately left at original case: Solr's own `title_sort` field
    # is likewise unnormalized, so case-sensitive ASCII sort order stays
    # comparable across engines.
    doc = {"title": record["title"], "title_sort": record["title"]}
    for key in KEYWORD_FIELDS:
        if record.get(key):
            doc[key] = str(record[key]).lower()
    for key in NUMERIC_FIELDS:
        if record.get(key) is not None:
            doc[key] = record[key]
    if record.get("description"):
        doc["description"] = record["description"]
    return doc


def main():
    session = requests.Session()

    resp = session.head(f"{BASE_URL}/{INDEX}")
    if resp.status_code == 200:
        session.delete(f"{BASE_URL}/{INDEX}").raise_for_status()
    resp = session.put(f"{BASE_URL}/{INDEX}", json=build_mapping())
    resp.raise_for_status()

    batch_lines = []
    total = 0
    t0 = time.time()
    with open(CATALOG) as f:
        for line in f:
            record = json.loads(line)
            doc = to_doc(record)
            batch_lines.append(json.dumps({"index": {"_index": INDEX, "_id": record["id"]}}))
            batch_lines.append(json.dumps(doc))
            if len(batch_lines) >= BATCH_SIZE * 2:
                body = "\n".join(batch_lines) + "\n"
                resp = session.post(
                    f"{BASE_URL}/_bulk",
                    data=body.encode("utf-8"),
                    headers={"Content-Type": "application/x-ndjson"},
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
            f"{BASE_URL}/_bulk",
            data=body.encode("utf-8"),
            headers={"Content-Type": "application/x-ndjson"},
        )
        resp.raise_for_status()
        result = resp.json()
        if result.get("errors"):
            raise RuntimeError(f"bulk index errors: {json.dumps(result)[:2000]}")
        total += len(batch_lines) // 2

    print(f"submitted {total} docs in {time.time() - t0:.1f}s, refreshing...")
    t1 = time.time()
    resp = session.post(f"{BASE_URL}/{INDEX}/_refresh")
    resp.raise_for_status()
    print(f"refresh took {time.time() - t1:.1f}s")
    print(f"total index build time: {time.time() - t0:.1f}s")

    status = session.get(f"{BASE_URL}/{INDEX}/_count").json()
    print(f"count: {status['count']}")


if __name__ == "__main__":
    main()
