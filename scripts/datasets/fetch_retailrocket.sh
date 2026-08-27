#!/usr/bin/env bash
# Issue #55 Phase B (docs/decisions/README.md: dataset recovery inventory,
# `docs/experiments/ISSUE57_DATASET_RECOVERY_LOG.md`): the Retailrocket
# recommender-system dataset -- real anonymized shopper behavior events
# (view/addtocart/transaction) and item-property changes over time, a
# schema dimension (behavior/relevance-adjacent evidence, noisy anonymized
# marketplace property values) no other dataset in this project has.
#
# Historically recorded BLOCKED (`kaggle.com` returned a 403 CONNECT-
# tunnel failure -- an organization-policy network block, not a Kaggle
# auth requirement). Retried under Issue #55 Phase B's "network access is
# now materially more open" premise and confirmed genuinely reachable:
# the dataset downloads anonymously via Kaggle's public API endpoint, no
# API key/session required.
#
# Source: https://www.kaggle.com/datasets/retailrocket/ecommerce-dataset
# License: CC BY-NC-SA 4.0 (NonCommercial -- research/benchmark use only,
# never redistribute commercially).
#
# Caveat, unlike this project's GitHub-hosted fetch scripts: Kaggle does
# not expose an immutable, content-pinned URL the way a pinned git commit
# does. The checksum manifest below is the actual hash of the bytes
# fetched on the retrieval date recorded in the recovery log; if a rerun
# produces a different hash, the dataset owner has updated it upstream --
# treat that as a new dataset revision, not a fetch bug.
#
# Output: dataset_cache/retailrocket/retailrocket.zip (gitignored -- this
# script + the checksum manifest are what's reproducible).
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
OUT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)/dataset_cache/retailrocket"
mkdir -p "$OUT_DIR"

echo "Fetching retailrocket.zip (~305 MB: category_tree.csv, events.csv [2,756,101 rows], item_properties_part1/2.csv)..."
curl -sS --max-time 300 -L -o "$OUT_DIR/retailrocket.zip" \
  "https://www.kaggle.com/api/v1/datasets/download/retailrocket/ecommerce-dataset"

echo "Verifying checksum against scripts/datasets/retailrocket_checksums.sha256..."
(cd "$OUT_DIR" && sha256sum -c "$SCRIPT_DIR/retailrocket_checksums.sha256")

ls -la "$OUT_DIR"
