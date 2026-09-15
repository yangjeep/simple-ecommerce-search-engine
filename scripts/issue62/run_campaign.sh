#!/usr/bin/env bash
# Issue #62 (Infra E2) campaign runner: loops (engine, tier, run) cells
# through target/release/i62_measure, sequentially (one engine/container at
# a time, matching the preregistered resource-isolation rule), writing each
# cell's raw JSON result to artifacts/issue62/results/.
#
# Usage: bash scripts/issue62/run_campaign.sh [engine ...]
#   With no args, runs all engines in ENGINES below. Tiers run smallest to
#   largest (100k -> 5m) so a real scale ceiling is hit only after the
#   smaller, cheaper tiers are already fully measured. 3 runs per cell,
#   fixed order, no reruns for "bad" numbers, no early stop on a single
#   cell's failure — failures are recorded and the loop continues to the
#   next cell.
set -uo pipefail  # not -e: a single cell's failure must not abort the loop

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
RESULTS_DIR="$REPO_ROOT/artifacts/issue62/results"
mkdir -p "$RESULTS_DIR"

# Overridable via I62_TIERS="100k 500k 1m" env var so a session can bound
# scope to what's realistically achievable (documented in the decision doc,
# not a silent protocol narrowing) without editing this file.
if [[ -n "${I62_TIERS:-}" ]]; then
  read -ra TIERS <<<"$I62_TIERS"
else
  TIERS=(100k 500k 1m 3m 5m)
fi
RUNS=(1 2 3)
DEFAULT_ENGINES=(native solr elasticsearch opensearch typesense meilisearch vespa havenask)
ENGINES=("${@:-${DEFAULT_ENGINES[@]}}")

BINARY="$REPO_ROOT/target/release/i62_measure"
if [[ ! -x "$BINARY" ]]; then
  echo "FATAL: $BINARY missing; run: cargo build -p issue62-eval --release" >&2
  exit 2
fi

total=0
ok=0
failed=0

for tier in "${TIERS[@]}"; do
  for engine in "${ENGINES[@]}"; do
    for run in "${RUNS[@]}"; do
      out="$RESULTS_DIR/${engine}_${tier}_run${run}.json"
      if [[ -f "$out" ]]; then
        echo "==> skip (already have result): $engine $tier run $run"
        continue
      fi
      total=$((total + 1))
      echo "==> [$engine/$tier/run$run] starting $(date -u +%FT%TZ)"
      if "$BINARY" --engine "$engine" --tier "$tier" --run "$run" \
           --repository-root "$REPO_ROOT" --out "$out"; then
        ok=$((ok + 1))
        echo "==> [$engine/$tier/run$run] OK"
      else
        failed=$((failed + 1))
        echo "==> [$engine/$tier/run$run] FAILED (see $out for status/reason)"
      fi
    done
  done
done

echo "==> campaign pass complete: total=$total ok=$ok failed=$failed"
