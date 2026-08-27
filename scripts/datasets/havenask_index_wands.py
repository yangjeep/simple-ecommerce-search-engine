#!/usr/bin/env python3
"""Issue #57 frozen benchmark: index the real WANDS catalog into Havenask
via its SQL/QRS `INSERT` endpoint (Havenask's SQL layer rejects
multi-row `INSERT ... VALUES (...), (...)` --
`IQUAN_EC_UNSUPPORTED_TABLE_MODIFY: unsupported table modify: insert
multi rows`, confirmed live this session -- so each row is one request;
concurrency is used to keep wall-clock reasonable, matching how a real
caller would drive this same realtime-ingest API under load rather than
serially).

STRING attribute fields are lower-cased at index time, matching
`crates/comparator-eval/src/translate_havenask.rs`'s case-insensitive
strategy (see that module's doc comment for why this is a faithful
equivalent of Solr's case-insensitive regex, not a weaker
approximation).

Usage: python3 scripts/datasets/havenask_index_wands.py [base_url] [table] [catalog_path] [concurrency]
  e.g. python3 scripts/datasets/havenask_index_wands.py http://127.0.0.1:45800 wands
"""
import json
import sys
import time
from concurrent.futures import ThreadPoolExecutor, as_completed
from pathlib import Path

import requests

BASE_URL = sys.argv[1] if len(sys.argv) > 1 else "http://127.0.0.1:45800"
TABLE = sys.argv[2] if len(sys.argv) > 2 else "wands"
CATALOG = (
    Path(sys.argv[3])
    if len(sys.argv) > 3
    else Path(__file__).resolve().parents[2] / "dataset_cache" / "wands" / "catalog.jsonl"
)
CONCURRENCY = int(sys.argv[4]) if len(sys.argv) > 4 else 32

STRING_FIELDS = [
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


def escape_sql(s):
    return s.replace("'", "''")


def to_insert_sql(record):
    cols = ["id", "title", "description"]
    vals = [str(int(record["id"])), f"'{escape_sql(record['title'])}'"]
    desc = record.get("description") or ""
    vals.append(f"'{escape_sql(desc)}'")
    for key in STRING_FIELDS:
        v = record.get(key)
        if v:
            cols.append(key)
            vals.append(f"'{escape_sql(str(v).lower())}'")
    for key in NUMERIC_FIELDS:
        v = record.get(key)
        if v is not None:
            cols.append(key)
            vals.append(str(v))
    sql = f"insert into {TABLE} ({','.join(cols)}) values ({','.join(vals)}) &&kvpair=databaseName:database;formatType:json"
    return sql


def submit_one(session, sql, row_id):
    resp = session.post(
        f"{BASE_URL}/QrsService/searchSql",
        json={"assemblyQuery": sql},
        timeout=30,
    )
    resp.raise_for_status()
    data = resp.json()
    error_info = data.get("error_info", "")
    if "ERROR_NONE" not in error_info:
        return (row_id, False, error_info)
    return (row_id, True, None)


def main():
    records = []
    with open(CATALOG) as f:
        for line in f:
            records.append(json.loads(line))
    print(f"loaded {len(records)} real WANDS records")

    session_local = requests.Session()
    adapter = requests.adapters.HTTPAdapter(pool_connections=CONCURRENCY, pool_maxsize=CONCURRENCY)
    session_local.mount("http://", adapter)

    t0 = time.time()
    failures = []
    completed = 0
    with ThreadPoolExecutor(max_workers=CONCURRENCY) as ex:
        futures = {
            ex.submit(submit_one, session_local, to_insert_sql(r), r["id"]): r["id"]
            for r in records
        }
        for fut in as_completed(futures):
            row_id, ok, err = fut.result()
            completed += 1
            if not ok:
                failures.append((row_id, err))
            if completed % 5000 == 0:
                elapsed = time.time() - t0
                print(f"  {completed}/{len(records)} in {elapsed:.1f}s ({completed/elapsed:.1f} rows/s)")

    elapsed = time.time() - t0
    print(f"submitted {len(records)} rows in {elapsed:.1f}s ({len(records)/elapsed:.1f} rows/s)")
    print(f"failures: {len(failures)}")
    for row_id, err in failures[:20]:
        print(f"  FAILED id={row_id}: {err}")
    if failures:
        sys.exit(1)


if __name__ == "__main__":
    main()
