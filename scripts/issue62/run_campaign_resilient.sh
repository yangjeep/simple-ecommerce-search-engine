#!/usr/bin/env bash
# Wraps run_campaign.sh with automatic restart-on-death, since this shared
# host has been observed (2026-09-14, #62 campaign) to intermittently kill
# background processes under transient external memory pressure unrelated
# to this experiment. run_campaign.sh's own skip-if-already-have-result
# logic makes restarting always safe/idempotent — no cell is silently
# re-run or double-counted, and any container left running by an abrupt
# kill is removed before the next attempt.
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
MAX_ATTEMPTS=30
ATTEMPT=0

while [[ $ATTEMPT -lt $MAX_ATTEMPTS ]]; do
  ATTEMPT=$((ATTEMPT + 1))
  echo "==> campaign attempt $ATTEMPT/$MAX_ATTEMPTS starting $(date -u +%FT%TZ)"
  bash "$SCRIPT_DIR/run_campaign.sh" "$@"
  status=$?
  if [[ $status -eq 0 ]]; then
    echo "==> campaign attempt $ATTEMPT completed normally (exit 0) — full pass done"
    break
  fi
  echo "==> campaign attempt $ATTEMPT exited $status — cleaning up and retrying after a pause"
  for name in i62-native i62-solr i62-elasticsearch i62-opensearch i62-typesense i62-meilisearch i62-vespa i62-havenask; do
    docker rm -f "$name" >/dev/null 2>&1 || true
  done
  sleep 30
done

echo "==> resilient runner stopping after $ATTEMPT attempt(s)"
