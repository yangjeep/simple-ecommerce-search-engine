#!/usr/bin/env bash
# Phase 6A (Issue #23, WANDS substitute -- see PHASE6A_DECISION.md for why
# Amazon Reviews 2023 was substituted): fetch the WANDS (Wayfair
# ANnotation Dataset) product/query/label CSVs.
#
# Pinned to a specific immutable commit, not the `main` branch, so this
# script always fetches exactly the same bytes: as of this writing `main`
# and this pin are the same commit (the repo has had no updates since
# 2022-01-18), but pinning removes any dependence on that staying true.
#
# Source: https://github.com/wayfair/WANDS (public, no auth, no Git LFS).
#
# Output: dataset_cache/wands/{product,query,label}.csv (gitignored --
# this script + the checksum manifest are what's reproducible, not the
# ~91 MB of downloaded data).
set -euo pipefail

PINNED_SHA="3b74dcf4ba29ab8ff3e6a50b5b09fc627cb882b5"
BASE="https://raw.githubusercontent.com/wayfair/WANDS/${PINNED_SHA}/dataset"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
OUT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)/dataset_cache/wands"
mkdir -p "$OUT_DIR"

echo "Fetching product.csv (~91 MB, 42,994 products)..."
curl -sS --max-time 300 -o "$OUT_DIR/product.csv" "$BASE/product.csv"

echo "Fetching query.csv (~20 KB, 480 queries)..."
curl -sS --max-time 60 -o "$OUT_DIR/query.csv" "$BASE/query.csv"

echo "Fetching label.csv (~5.7 MB, 233,448 relevance judgments)..."
curl -sS --max-time 120 -o "$OUT_DIR/label.csv" "$BASE/label.csv"

echo "Verifying checksums against scripts/datasets/wands_checksums.sha256..."
(cd "$OUT_DIR" && sha256sum -c "$SCRIPT_DIR/wands_checksums.sha256")

ls -la "$OUT_DIR"
