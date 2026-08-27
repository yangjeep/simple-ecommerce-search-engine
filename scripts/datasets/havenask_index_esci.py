#!/usr/bin/env python3
"""Issue #57 frozen benchmark: index one real ESCI vertical slice into
Havenask via its SQL/QRS `INSERT` endpoint. See
scripts/datasets/havenask_index_wands.py's doc comment for why this is
one-row-per-request (Havenask's SQL layer rejects multi-row `INSERT ...
VALUES`, confirmed live) driven concurrently.

`brand`/`color` are lower-cased at index time, matching
crates/comparator-eval/src/translate_havenask.rs and
es_family_index_esci.py's identical convention -- load-bearing here
since ESCI's `brand` field has real casing collisions.

Usage: python3 scripts/datasets/havenask_index_esci.py <vertical> [base_url] [table] [concurrency]
  e.g. python3 scripts/datasets/havenask_index_esci.py electronics http://172.17.0.2:45800 esci_electronics
"""
import json
import sys
import time
from concurrent.futures import ThreadPoolExecutor, as_completed
from pathlib import Path

import requests

VERTICAL = sys.argv[1]
BASE_URL = sys.argv[2] if len(sys.argv) > 2 else "http://172.17.0.2:45800"
TABLE = sys.argv[3] if len(sys.argv) > 3 else f"esci_{VERTICAL}"
CONCURRENCY = int(sys.argv[4]) if len(sys.argv) > 4 else 64
import os

_env_catalog = os.environ.get("ESCI_CATALOG_PATH")
if _env_catalog:
    CATALOG = Path(_env_catalog)
else:
    CATALOG = (
        Path(__file__).resolve().parents[2]
        / "dataset_cache"
        / f"esci_{VERTICAL}"
        / f"esci_{VERTICAL}_products.jsonl"
    )


def escape_sql(s):
    return s.replace("'", "''")


def to_insert_sql(record, int_id):
    # Havenask's PRIMARY_KEY64 index requires a numeric column (a STRING
    # primary key produced a real, reproducible "invalid table config"
    # schema-load error, confirmed live -- see
    # docs/experiments/FULL_MATRIX_PROTOCOL.md §3.1's ESCI addendum): the
    # real ASIN is kept as a separate STRING attribute column instead.
    cols = ["id", "asin", "title"]
    vals = [str(int_id), f"'{escape_sql(record['product_id'])}'", f"'{escape_sql(record['title'])}'"]
    for key in ("description", "bullet_point"):
        v = record.get(key)
        if v:
            cols.append(key)
            vals.append(f"'{escape_sql(v)}'")
    for key in ("brand", "color"):
        v = record.get(key)
        if v:
            cols.append(key)
            vals.append(f"'{escape_sql(v.lower())}'")
    return f"insert into {TABLE} ({','.join(cols)}) values ({','.join(vals)}) &&kvpair=databaseName:database;formatType:json"


def submit_one(session, sql, row_id):
    resp = session.post(f"{BASE_URL}/QrsService/searchSql", json={"assemblyQuery": sql}, timeout=30)
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
    print(f"loaded {len(records)} real ESCI {VERTICAL} records")

    session_local = requests.Session()
    adapter = requests.adapters.HTTPAdapter(pool_connections=CONCURRENCY, pool_maxsize=CONCURRENCY)
    session_local.mount("http://", adapter)

    t0 = time.time()
    failures = []
    with ThreadPoolExecutor(max_workers=CONCURRENCY) as ex:
        futures = {
            ex.submit(submit_one, session_local, to_insert_sql(r, i + 1), r["product_id"]): r["product_id"]
            for i, r in enumerate(records)
        }
        for fut in as_completed(futures):
            row_id, ok, err = fut.result()
            if not ok:
                failures.append((row_id, err))

    elapsed = time.time() - t0
    print(f"submitted {len(records)} rows in {elapsed:.1f}s ({len(records)/elapsed:.1f} rows/s)")
    print(f"failures: {len(failures)}")
    for row_id, err in failures[:20]:
        print(f"  FAILED id={row_id}: {err}")
    if failures:
        sys.exit(1)


if __name__ == "__main__":
    main()
