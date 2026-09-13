#!/usr/bin/env bash
# Issue #61/#73 Revision 11 tracked live launcher (docs/experiments/ISSUE61_PROTOCOL.md §25.1).
#
# Exact frozen invocation: bash scripts/run-benchmarks/i61_campaign_live.sh <run1|rerun1|rerun2>
# No other positional arguments, flags, environment overrides, or resume mode are accepted.
set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "FATAL: usage: i61_campaign_live.sh <run1|rerun1|rerun2>" >&2
  exit 2
fi
CYCLE="$1"
case "$CYCLE" in
  run1|rerun1|rerun2) ;;
  *)
    echo "FATAL: invalid cycle '$CYCLE'; expected run1, rerun1, or rerun2" >&2
    exit 2
    ;;
esac

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

if [[ -L "$REPO_ROOT" ]]; then
  echo "FATAL: repository root must not be a symlink: $REPO_ROOT" >&2
  exit 2
fi

# shellcheck source=../../benchmarks/configs/issue61/container_limits.env
source "$REPO_ROOT/benchmarks/configs/issue61/container_limits.env"

REQUIRED_FILES=(
  "benchmarks/configs/issue61/container_limits.env"
  "benchmarks/configs/issue61/solr_esci_electronics_config.json"
  "benchmarks/configs/issue61/solr_esci_electronics_schema.json"
  "benchmarks/configs/issue61/solr_wands_config.json"
  "benchmarks/configs/issue61/solr_wands_schema.json"
  "benchmarks/workloads/i61_esci_electronics.jsonl"
  "benchmarks/workloads/i61_wands_480.jsonl"
  "scripts/issue61/provision_solr.sh"
  "target/release/i61_analyze"
  "target/release/i61_bench"
  "target/release/i61_campaign"
  "target/release/i61_native_server"
)
for relative in "${REQUIRED_FILES[@]}"; do
  if [[ ! -f "$REPO_ROOT/$relative" ]]; then
    echo "FATAL: required file missing: $REPO_ROOT/$relative" >&2
    exit 2
  fi
done

if ! command -v docker >/dev/null 2>&1; then
  echo "FATAL: docker is required for live execution" >&2
  exit 2
fi

exec "$REPO_ROOT/target/release/i61_campaign" --execute --repository-root "$REPO_ROOT" --cycle "$CYCLE"
