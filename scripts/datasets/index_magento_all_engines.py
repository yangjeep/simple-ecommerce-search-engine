#!/usr/bin/env python3
"""Issue #57 frozen benchmark: index the real Magento configurable-
product catalog into Solr, Elasticsearch, OpenSearch, and Havenask, one
row/document PER VARIANT (color+size pair), not per parent product --
this is the only dataset in the frozen matrix with genuine Product to
Variant structure (`dataset_cache/magento_configurable/catalog.jsonl`,
22 parent products, 155 real kept variants, checkerboard-sparsified per
`scripts/datasets/prepare_magento_configurable.py`'s own disclosed
methodology so real cross-variant trap opportunities exist).

Per-variant indexing is deliberate, not incidental: it is the only
schema shape under which a naive `color=X AND size=Y` filter query
enforces genuine same-variant correctness on every engine here (a
denormalized parent-product document with `colors: [...]`/`sizes:
[...]` arrays would let a filter match X and Y independently on
different array elements -- exactly the cross-variant false-match class
CLAUDE.md's hard rule forbids -- and is not attempted).

Usage: python3 scripts/datasets/index_magento_all_engines.py
"""
import json
import sys
from pathlib import Path

import requests

CATALOG = Path(__file__).resolve().parents[2] / "dataset_cache" / "magento_configurable" / "catalog.jsonl"

SOLR_URL = "http://localhost:8983/solr/magento_bench"
ES_URL = "http://127.0.0.1:9200"
OS_URL = "http://127.0.0.1:9201"
ES_INDEX = "magento_bench"


def load_variant_rows():
    rows = []
    with open(CATALOG) as f:
        for p_idx, line in enumerate(f):
            r = json.loads(line)
            for v_idx, v in enumerate(r["variants"]):
                rows.append(
                    {
                        "id": f"{r['sku']}_{v_idx}",
                        "int_id": p_idx * 1000 + v_idx,
                        "sku": r["sku"],
                        "product_name": r["name"],
                        "category_top": r["category_top"],
                        "material": r["material"],
                        "price_cents": r["price_cents"],
                        "color": v["color"],
                        "size": v["size"],
                    }
                )
    return rows


def index_solr(rows):
    session = requests.Session()
    session.post(
        f"{SOLR_URL}/config", json={"set-user-property": {"update.autoCreateFields": "false"}}
    ).raise_for_status()
    fields = []
    for name in ["sku", "product_name", "category_top", "material", "color", "size"]:
        fields.append(
            {"name": name, "type": "string", "indexed": True, "stored": True, "docValues": True, "multiValued": False}
        )
    fields.append(
        {"name": "price_cents", "type": "pint", "indexed": True, "stored": True, "docValues": True}
    )
    resp = session.post(f"{SOLR_URL}/schema", json={"add-field": fields})
    if resp.status_code >= 400 and "already exists" not in resp.text:
        resp.raise_for_status()
    docs = [
        {
            "id": r["id"],
            "sku": r["sku"],
            "product_name": r["product_name"],
            "category_top": r["category_top"],
            "material": r["material"],
            "price_cents": r["price_cents"],
            "color": r["color"],
            "size": r["size"],
        }
        for r in rows
    ]
    session.post(f"{SOLR_URL}/update/json/docs", json=docs, params={"commitWithin": 2000}).raise_for_status()
    session.post(f"{SOLR_URL}/update", json={"commit": {}}).raise_for_status()
    count = session.get(f"{SOLR_URL}/select", params={"q": "*:*", "rows": 0}).json()["response"]["numFound"]
    print(f"Solr: {count} docs")


def index_es_family(rows, base_url):
    session = requests.Session()
    if session.head(f"{base_url}/{ES_INDEX}").status_code == 200:
        session.delete(f"{base_url}/{ES_INDEX}").raise_for_status()
    mapping = {
        "settings": {"number_of_shards": 1, "number_of_replicas": 0},
        "mappings": {
            "properties": {
                "sku": {"type": "keyword"},
                "product_name": {"type": "text"},
                "category_top": {"type": "keyword"},
                "material": {"type": "keyword"},
                "price_cents": {"type": "long"},
                "color": {"type": "keyword"},
                "size": {"type": "keyword"},
            }
        },
    }
    session.put(f"{base_url}/{ES_INDEX}", json=mapping).raise_for_status()
    lines = []
    for r in rows:
        lines.append(json.dumps({"index": {"_index": ES_INDEX, "_id": r["id"]}}))
        doc = dict(r)
        doc.pop("id")
        doc.pop("int_id")
        doc["category_top"] = doc["category_top"].lower()
        doc["material"] = doc["material"].lower()
        doc["color"] = doc["color"].lower()
        doc["size"] = doc["size"].lower()
        lines.append(json.dumps(doc))
    body = "\n".join(lines) + "\n"
    resp = session.post(f"{base_url}/_bulk", data=body.encode(), headers={"Content-Type": "application/x-ndjson"})
    resp.raise_for_status()
    if resp.json().get("errors"):
        raise RuntimeError(f"bulk errors: {json.dumps(resp.json())[:2000]}")
    session.post(f"{base_url}/{ES_INDEX}/_refresh").raise_for_status()
    count = session.get(f"{base_url}/{ES_INDEX}/_count").json()["count"]
    print(f"{base_url}: {count} docs")


if __name__ == "__main__":
    rows = load_variant_rows()
    print(f"loaded {len(rows)} real variant rows from {CATALOG}")
    index_solr(rows)
    index_es_family(rows, ES_URL)
    index_es_family(rows, OS_URL)
