#!/usr/bin/env bash
# Round 1: fetch the real Amazon Shopping Queries Dataset (ESCI).
#
# The dataset's actual content lives behind Git LFS on
# amazon-science/esci-data. This session's anonymous git-read lane does
# not serve LFS objects, but GitHub's LFS media CDN
# (media.githubusercontent.com) does — see docs/experiments/ROUND1_LOG.md
# "Dataset and baseline acquisition" for the full acquisition story,
# including the paths that were tried and blocked first.
#
# Output: dataset_cache/{products,examples}.parquet (gitignored — this
# script is what's reproducible, not the ~1.1 GB of downloaded data).
set -euo pipefail

REF="main"
BASE="https://media.githubusercontent.com/media/amazon-science/esci-data/${REF}/shopping_queries_dataset"
OUT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)/dataset_cache"
mkdir -p "$OUT_DIR"

echo "Fetching shopping_queries_dataset_products.parquet (~1.1 GB)..."
curl -sS --max-time 1800 -o "$OUT_DIR/products.parquet" "$BASE/shopping_queries_dataset_products.parquet"

echo "Fetching shopping_queries_dataset_examples.parquet (~51 MB)..."
curl -sS --max-time 300 -o "$OUT_DIR/examples.parquet" "$BASE/shopping_queries_dataset_examples.parquet"

ls -la "$OUT_DIR"/*.parquet
