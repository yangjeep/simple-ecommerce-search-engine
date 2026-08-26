#!/usr/bin/env python3
"""Issue #35 (docs/experiments/ISSUE35_ESCI_BEAUTY_PROTOCOL.md): filter
one real ESCI train parquet shard (fetched by fetch_esci_beauty.sh) to
a real beauty/personal-care-vertical slice -- a third, materially
different commerce vertical, completing Issue #35's own "at least
three materially different verticals" Workstream D goal.

Keyword list and filtering/capping logic are fixed *before* inspecting
any downstream metric, mirroring filter_esci_electronics.py and
filter_esci_automotive.py exactly.

Requires: pip install pyarrow

Output: dataset_cache/esci_beauty/{esci_beauty_products,esci_beauty_queries}.jsonl
(gitignored).
"""
import json
import re
import sys
from pathlib import Path

import pyarrow.parquet as pq

KEYWORDS = [
    "shampoo", "conditioner", "face moisturizer", "lip balm", "mascara",
    "eyeliner", "foundation makeup", "nail polish", "hair dryer",
    "flat iron", "body lotion", "sunscreen", "deodorant", "perfume",
    "essential oil", "makeup brush", "hair straightener",
    "facial cleanser", "eye cream", "beard oil",
]
PATTERN = re.compile("|".join(re.escape(k) for k in KEYWORDS), re.IGNORECASE)

MAX_PRODUCTS = 4000
MAX_QUERIES = 600

SCRIPT_DIR = Path(__file__).resolve().parent
DATA_DIR = SCRIPT_DIR.parents[1] / "dataset_cache" / "esci_beauty"


def main():
    pf = pq.ParquetFile(DATA_DIR / "train0000.parquet")
    print(f"total row groups: {pf.num_row_groups}, total rows: {pf.metadata.num_rows}", file=sys.stderr)

    products = {}
    judgments = {}

    columns = [
        "query", "product_id", "product_locale", "esci_label",
        "product_title", "product_description", "product_bullet_point",
        "product_brand", "product_color",
    ]
    for rg in range(pf.num_row_groups):
        table = pf.read_row_group(rg, columns=columns)
        for row in table.to_pylist():
            if row["product_locale"] != "us":
                continue
            title = row.get("product_title") or ""
            desc = row.get("product_description") or ""
            bullet = row.get("product_bullet_point") or ""
            if not PATTERN.search(f"{title} {desc} {bullet}"):
                continue
            pid = row["product_id"]
            if pid not in products and len(products) < MAX_PRODUCTS:
                products[pid] = {
                    "product_id": pid,
                    "title": title,
                    "description": desc,
                    "bullet_point": bullet,
                    "brand": row.get("product_brand") or "",
                    "color": row.get("product_color") or "",
                }
            if pid in products:
                q = row["query"]
                if q not in judgments and len(judgments) >= MAX_QUERIES:
                    continue
                judgments.setdefault(q, []).append(
                    {"product_id": pid, "label": row["esci_label"]}
                )
        if len(products) >= MAX_PRODUCTS and len(judgments) >= MAX_QUERIES:
            break

    with open(DATA_DIR / "esci_beauty_products.jsonl", "w") as f:
        for p in products.values():
            f.write(json.dumps(p) + "\n")

    with open(DATA_DIR / "esci_beauty_queries.jsonl", "w") as f:
        for q, js in judgments.items():
            f.write(json.dumps({"query": q, "judgments": js}) + "\n")

    print(f"FINAL: {len(products)} products, {len(judgments)} queries", file=sys.stderr)
    label_counts = {}
    positive_queries = 0
    for js in judgments.values():
        if any(j["label"] != "Irrelevant" for j in js):
            positive_queries += 1
        for j in js:
            label_counts[j["label"]] = label_counts.get(j["label"], 0) + 1
    print(f"label distribution: {label_counts}", file=sys.stderr)
    print(
        f"queries with >=1 non-Irrelevant judgment: {positive_queries}/{len(judgments)}",
        file=sys.stderr,
    )


if __name__ == "__main__":
    main()
