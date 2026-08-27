#!/usr/bin/env python3
"""Issue #57 frozen benchmark: index the real Magento per-variant rows
(see scripts/datasets/index_magento_all_engines.py's doc comment for why
per-variant, not per-parent-product) into Havenask via SQL INSERT.

Usage: python3 scripts/datasets/havenask_index_magento.py [base_url] [table]
"""
import json
import os
import sys
from pathlib import Path

import requests

BASE_URL = sys.argv[1] if len(sys.argv) > 1 else "http://127.0.0.1:45800"
TABLE = sys.argv[2] if len(sys.argv) > 2 else "magento"
# Overridable since this script is typically `docker cp`'d into the
# Havenask container and run there, alongside a copy of the catalog
# file, rather than run against the repo checkout directly (matching
# havenask_index_esci.py's identical ESCI_CATALOG_PATH convention).
_env_catalog = os.environ.get("MAGENTO_CATALOG_PATH")
if _env_catalog:
    CATALOG = Path(_env_catalog)
else:
    CATALOG = (
        Path(__file__).resolve().parents[2]
        / "dataset_cache"
        / "magento_configurable"
        / "catalog.jsonl"
    )


def escape_sql(s):
    return s.replace("'", "''")


def main():
    rows = []
    with open(CATALOG) as f:
        for p_idx, line in enumerate(f):
            r = json.loads(line)
            for v_idx, v in enumerate(r["variants"]):
                rows.append(
                    {
                        "id": p_idx * 1000 + v_idx,
                        "sku": r["sku"],
                        "product_name": r["name"],
                        "category_top": r["category_top"].lower(),
                        "material": r["material"].lower(),
                        "price_cents": r["price_cents"],
                        "color": v["color"].lower(),
                        "size": v["size"].lower(),
                    }
                )
    print(f"loaded {len(rows)} real variant rows")

    session = requests.Session()
    failures = []
    for row in rows:
        cols = ["id", "sku", "product_name", "category_top", "material", "price_cents", "color", "size"]
        vals = [
            str(row["id"]),
            f"'{escape_sql(row['sku'])}'",
            f"'{escape_sql(row['product_name'])}'",
            f"'{escape_sql(row['category_top'])}'",
            f"'{escape_sql(row['material'])}'",
            str(row["price_cents"]),
            f"'{escape_sql(row['color'])}'",
            f"'{escape_sql(row['size'])}'",
        ]
        sql = f"insert into {TABLE} ({','.join(cols)}) values ({','.join(vals)}) &&kvpair=databaseName:database;formatType:json"
        resp = session.post(f"{BASE_URL}/QrsService/searchSql", json={"assemblyQuery": sql}, timeout=30)
        resp.raise_for_status()
        error_info = resp.json().get("error_info", "")
        if "ERROR_NONE" not in error_info:
            failures.append((row["id"], error_info))

    print(f"submitted {len(rows)} rows, failures: {len(failures)}")
    for row_id, err in failures[:10]:
        print(f"  FAILED id={row_id}: {err}")
    if failures:
        sys.exit(1)


if __name__ == "__main__":
    main()
