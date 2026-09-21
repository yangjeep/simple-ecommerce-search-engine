#!/usr/bin/env bash
# Issue #77 (Infra E3): primary-scale (500k) measurement, 3 independent runs
# from clean state per engine, per the standing repetition rule. Sequential,
# one container at a time (frozen envelope's isolation contract). Each
# i77_measure invocation always exits 0 and writes a result JSON with its own
# status field (Ok/CorrectnessFail/HarnessFailure/InvalidEnvironmental/
# EngineExcluded) -- a single engine's failure must not halt the rest of the
# matrix, so this script does not use `set -e` around the measurement loop.
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
BIN="$REPO_ROOT/target/release/i77_measure"
OUT_DIR="$REPO_ROOT/artifacts/issue77/results"
mkdir -p "$OUT_DIR"

ENGINES=(native solr elasticsearch opensearch typesense meilisearch vespa)
CONTAINERS=(i77-native i77-solr i77-elasticsearch i77-opensearch i77-typesense i77-meilisearch i77-vespa)

for idx in "${!ENGINES[@]}"; do
  engine="${ENGINES[$idx]}"
  container="${CONTAINERS[$idx]}"
  for run in 1 2 3; do
    out="$OUT_DIR/${engine}_500k_run${run}.json"
    echo "=== $(date -u +%FT%TZ) starting engine=$engine run=$run ==="
    docker rm -f "$container" >/dev/null 2>&1 || true
    rm -f "$out"
    "$BIN" --engine "$engine" --tier 500k --run "$run" --repository-root "$REPO_ROOT" --out "$out"
    echo "=== $(date -u +%FT%TZ) finished engine=$engine run=$run exit=$? ==="
  done
  docker rm -f "$container" >/dev/null 2>&1 || true
done

echo "ALL_500K_RUNS_DONE"
