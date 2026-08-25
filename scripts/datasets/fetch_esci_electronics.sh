#!/usr/bin/env bash
# Issue #35 (docs/experiments/ISSUE35_ESCI_ELECTRONICS_PROTOCOL.md): fetch
# one shard of the real Amazon Shopping Queries Dataset ("ESCI",
# Apache-2.0) from the `tasksource/esci` parquet mirror on Hugging Face
# -- this project's own precedent for using ESCI is
# docs/decisions/ROUND1_DECISION_TREE.md / PHASE2_DECISION.md.
#
# Pinned to a specific immutable HF revision (the `refs/convert/parquet`
# ref's target commit at the time this was written), not a moving branch,
# for the same reproducibility reason `fetch_wands.sh` pins a commit SHA
# rather than tracking `main`.
#
# Output: dataset_cache/esci_electronics/train0000.parquet (gitignored,
# ~115 MB -- this script + the checksum manifest are what's reproducible,
# not the downloaded bytes). Run filter_esci_electronics.py afterward to
# produce the actual catalog/query slice this project's Rust code reads.
set -euo pipefail

PINNED_REVISION="45c948250c2116f1e535bac67b92501c695307a4"
URL="https://huggingface.co/datasets/tasksource/esci/resolve/${PINNED_REVISION}/default/train/0000.parquet"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
OUT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)/dataset_cache/esci_electronics"
mkdir -p "$OUT_DIR"

echo "Fetching train0000.parquet (~115 MB, one shard of the full ESCI train split)..."
curl -sS -L --max-time 300 -o "$OUT_DIR/train0000.parquet" "$URL"

echo "Verifying checksum against scripts/datasets/esci_checksums.sha256..."
(cd "$OUT_DIR" && sha256sum -c "$SCRIPT_DIR/esci_checksums.sha256" --ignore-missing)

ls -la "$OUT_DIR"
echo "Next: python3 scripts/datasets/filter_esci_electronics.py"
