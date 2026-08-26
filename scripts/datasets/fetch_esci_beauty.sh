#!/usr/bin/env bash
# Issue #35 (docs/experiments/ISSUE35_ESCI_BEAUTY_PROTOCOL.md): fetch one
# shard of the real Amazon Shopping Queries Dataset ("ESCI", Apache-2.0)
# from the `tasksource/esci` parquet mirror on Hugging Face -- a third,
# independent download for a third, independent vertical slice.
#
# Pinned to the same immutable HF revision the electronics/automotive
# slices use, for the same reproducibility reason.
#
# Output: dataset_cache/esci_beauty/train0000.parquet (gitignored).
set -euo pipefail

PINNED_REVISION="45c948250c2116f1e535bac67b92501c695307a4"
URL="https://huggingface.co/datasets/tasksource/esci/resolve/${PINNED_REVISION}/default/train/0000.parquet"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
OUT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)/dataset_cache/esci_beauty"
mkdir -p "$OUT_DIR"

echo "Fetching train0000.parquet (~115 MB, one shard of the full ESCI train split)..."
curl -sS -L --max-time 300 -o "$OUT_DIR/train0000.parquet" "$URL"

echo "Verifying checksum against scripts/datasets/esci_checksums.sha256..."
(cd "$OUT_DIR" && sha256sum -c "$SCRIPT_DIR/esci_checksums.sha256" --ignore-missing)

ls -la "$OUT_DIR"
echo "Next: python3 scripts/datasets/filter_esci_beauty.py"
