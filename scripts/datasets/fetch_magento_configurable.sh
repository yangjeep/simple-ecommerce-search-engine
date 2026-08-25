#!/usr/bin/env bash
# Issue #55 H3: fetch Magento's official sample-data repository's real
# configurable-product (apparel size/color variant) CSV fixtures -- a
# genuinely different, real-world Product/Variant dataset, since neither
# WANDS nor ESCI has real variant structure (flagged as an external-validity
# gap in ISSUE47_DECISION.md and never closed).
#
# Pinned to a specific immutable commit, not `main`, for the same
# reproducibility reason `fetch_wands.sh` pins WANDS.
#
# Source: https://github.com/magento/magento2-sample-data (public, no auth,
# dual OSL-3.0/AFL-3.0 licensed sample data).
#
# Output: dataset_cache/magento_configurable/products_{men_tops,men_bottoms,
# women_tops,women_bottoms}.csv (gitignored -- this script is what's
# reproducible, not the downloaded bytes).
set -euo pipefail

PINNED_SHA="15d8538019b0c5ddefd349dec18c2b35f384afbb"
BASE="https://raw.githubusercontent.com/magento/magento2-sample-data/${PINNED_SHA}/app/code/Magento/ConfigurableSampleData/Test/Integration/_files/fixtures/ConfigurableProduct"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
OUT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)/dataset_cache/magento_configurable"
mkdir -p "$OUT_DIR"

for f in products_men_tops.csv products_men_bottoms.csv products_women_tops.csv products_women_bottoms.csv; do
  echo "Fetching $f..."
  curl -sS --max-time 60 -o "$OUT_DIR/$f" "$BASE/$f"
done

echo "Verifying checksums against scripts/datasets/magento_configurable_checksums.sha256..."
(cd "$OUT_DIR" && sha256sum -c "$SCRIPT_DIR/magento_configurable_checksums.sha256")

ls -la "$OUT_DIR"
