#!/usr/bin/env python3
"""Phase 6A (Issue #23): schema + bulk-index the real WANDS catalog into
Apache Solr, reusing Phase 5's fair-baseline discipline (docValues on
every field used for filter/sort/facet, a dedicated sortable title field
since Solr cannot sort on a tokenized text_general field).

Reads the same dataset_cache/wands/catalog.jsonl the Rust ingestion
(crates/phase6a-eval) reads, so both systems see identical documents.
Phase 6B's scale-ladder tiers pass a replicated controlled-stress
catalog (dataset_cache/wands_scale/catalog_{K}x.jsonl, produced by
scripts/datasets/replicate_wands_scale.py) as the third argument
instead -- same schema, same to_solr_doc mapping, just a different
source file.

Usage: python3 scripts/datasets/solr_index_wands.py [core_url] [max_docs] [catalog_path]
"""
import json
import sys
import time
from pathlib import Path

import requests

SOLR_URL = sys.argv[1] if len(sys.argv) > 1 else "http://localhost:8983/solr/wands_bench"
CATALOG = (
    Path(sys.argv[3])
    if len(sys.argv) > 3
    else Path(__file__).resolve().parents[2] / "dataset_cache" / "wands" / "catalog.jsonl"
)
BATCH_SIZE = 2000

STRING_DOCVALUES_FIELDS = [
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
NUMERIC_DOCVALUES_FIELDS = ["rating_count", "average_rating", "review_count"]


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
    for name in NUMERIC_DOCVALUES_FIELDS:
        fields.append(
            {
                "name": name,
                "type": "pdouble",
                "indexed": True,
                "stored": True,
                "docValues": True,
                "multiValued": False,
            }
        )
    fields.append(
        {
            "name": "title",
            "type": "text_general",
            "indexed": True,
            "stored": True,
        }
    )
    fields.append(
        {
            "name": "title_sort",
            "type": "string",
            "indexed": True,
            "stored": False,
            "docValues": True,
        }
    )
    fields.append(
        {
            "name": "description",
            "type": "text_general",
            "indexed": True,
            "stored": True,
        }
    )

    resp = session.post(f"{SOLR_URL}/schema", json={"add-field": fields})
    if resp.status_code >= 400 and "already exists" not in resp.text:
        resp.raise_for_status()

    # Unlike add-field, Solr's add-copy-field is NOT idempotent: re-posting
    # the same copy-field rule against a core that already has it does not
    # error, it silently appends a SECOND identical rule. Two rules copying
    # the same single-valued source into a non-multiValued destination
    # then makes every future update fail with "Multiple values
    # encountered for non multiValued copy field title_sort" -- a real bug
    # first hit when Phase 6B re-ran this script against an
    # already-initialized scale-ladder core (Phase 6A only ever indexed
    # each core once, so this path was never exercised before). Guard by
    # checking the existing copy-field list first.
    existing = session.get(f"{SOLR_URL}/schema/copyfields").json().get("copyFields", [])
    already_present = any(
        cf.get("source") == "title" and cf.get("dest") == "title_sort" for cf in existing
    )
    if not already_present:
        resp = session.post(
            f"{SOLR_URL}/schema",
            json={
                "add-copy-field": {"source": "title", "dest": "title_sort"},
            },
        )
        if (
            resp.status_code >= 400
            and "already exists" not in resp.text
            and "duplicate" not in resp.text.lower()
        ):
            resp.raise_for_status()


def to_solr_doc(record):
    doc = {"id": record["id"], "title": record["title"]}
    for key in STRING_DOCVALUES_FIELDS:
        if record.get(key):
            doc[key] = record[key]
    for key in NUMERIC_DOCVALUES_FIELDS:
        if record.get(key) is not None:
            doc[key] = record[key]
    if record.get("description"):
        doc["description"] = record["description"]
    return doc


def main():
    max_docs = int(sys.argv[2]) if len(sys.argv) > 2 and sys.argv[2] else None
    session = requests.Session()
    setup_schema(session)

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
                resp = session.post(
                    f"{SOLR_URL}/update/json/docs", json=batch, params={"commitWithin": 30000}
                )
                resp.raise_for_status()
                total += len(batch)
                batch = []
    if batch:
        resp = session.post(
            f"{SOLR_URL}/update/json/docs", json=batch, params={"commitWithin": 30000}
        )
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
