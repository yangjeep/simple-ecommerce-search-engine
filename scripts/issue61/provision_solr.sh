#!/usr/bin/env bash
# Issue #61 (Infra E1): provision the frozen Solr baseline under the campaign's
# resource limits, index a dataset, and verify the corpus is the corpus prior
# evidence was measured on.
#
# Historical note, and the reason this script exists: every prior Solr
# checkpoint in this repository used a *local Java install*
# (`/home/user/solr_setup/solr-9.10.1`). Java is not installed on this host, so
# that route is gone. Docker is now the only path to a JVM engine here, which
# means the provisioning itself became reproducible infrastructure rather than
# undocumented local state.
#
# Competence bar (ISSUE61_PROTOCOL.md §2.2) -- a resource win against a
# crippled baseline is invalid, so this script must configure Solr the way a
# production ecommerce deployment would:
#   * structured attributes as `string` + docValues (exact match / facet / sort)
#   * lexical fields as analyzed `text_general`
#   * explicit, recorded cache sizing rather than implicit defaults
#   * heap sized to leave the OS page cache for MMapDirectory
#   * forceMerge(1) on the read-only corpus
#
# Usage:
#   bash scripts/issue61/provision_solr.sh [wands|esci_electronics]
set -euo pipefail

DATASET="${1:-wands}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
# shellcheck source=../../benchmarks/configs/issue61/container_limits.env
source "$REPO_ROOT/benchmarks/configs/issue61/container_limits.env"

case "$DATASET" in
  wands)
    CORE="i61_wands"
    CATALOG="$REPO_ROOT/dataset_cache/wands/catalog.jsonl"
    EXPECTED_DOCS="$I61_WANDS_EXPECTED_DOCS"
    ;;
  esci_electronics)
    CORE="i61_esci_electronics"
    CATALOG="$REPO_ROOT/dataset_cache/esci_electronics/esci_electronics_products.jsonl"
    EXPECTED_DOCS="$I61_ESCI_ELECTRONICS_EXPECTED_DOCS"
    ;;
  *)
    echo "FATAL: unknown dataset '$DATASET' (expected: wands | esci_electronics)" >&2
    exit 2
    ;;
esac

BASE="http://localhost:${I61_SOLR_PORT}/solr"
CORE_URL="$BASE/$CORE"

if [[ ! -f "$CATALOG" ]]; then
  echo "FATAL: catalog not found: $CATALOG" >&2
  echo "  run the dataset fetch/prepare scripts first (see ISSUE61_LOG.md step 1)" >&2
  exit 2
fi

# --- 1. verify the image is the digest the protocol froze ------------------
# A floating tag silently changing under us would invalidate every comparison
# made against it, so the digest is checked rather than trusted.
echo "==> verifying $I61_SOLR_IMAGE digest"
ACTUAL_DIGEST="$(docker image inspect "$I61_SOLR_IMAGE" --format '{{.Id}}' 2>/dev/null || true)"
if [[ -z "$ACTUAL_DIGEST" ]]; then
  echo "  image absent locally; pulling"
  docker pull "$I61_SOLR_IMAGE" >/dev/null
  ACTUAL_DIGEST="$(docker image inspect "$I61_SOLR_IMAGE" --format '{{.Id}}')"
fi
if [[ "$ACTUAL_DIGEST" != "$I61_SOLR_DIGEST" ]]; then
  echo "FATAL: image digest mismatch" >&2
  echo "  expected (frozen in container_limits.env): $I61_SOLR_DIGEST" >&2
  echo "  actual:                                    $ACTUAL_DIGEST" >&2
  exit 3
fi
echo "  digest OK: $ACTUAL_DIGEST"

# --- 2. start the container under the frozen limits ------------------------
echo "==> (re)starting container $I61_SOLR_CONTAINER"
docker rm -f "$I61_SOLR_CONTAINER" >/dev/null 2>&1 || true
docker run -d \
  --name "$I61_SOLR_CONTAINER" \
  --cpus="$I61_CPUS" \
  --cpuset-cpus="$I61_CPUSET" \
  --memory="$I61_MEMORY" \
  --memory-swap="$I61_MEMORY_SWAP" \
  -p "${I61_SOLR_PORT}:8983" \
  -e SOLR_HEAP="$I61_SOLR_HEAP" \
  "$I61_SOLR_IMAGE" \
  solr-precreate "$CORE" >/dev/null

echo "==> waiting for Solr to accept queries"
for _ in $(seq 1 90); do
  if curl -sf "$BASE/admin/cores?action=STATUS" >/dev/null 2>&1; then break; fi
  sleep 1
done
if ! curl -sf "$BASE/admin/cores?action=STATUS" >/dev/null 2>&1; then
  echo "FATAL: Solr did not become ready within 90s" >&2
  docker logs --tail 40 "$I61_SOLR_CONTAINER" >&2
  exit 4
fi
# solr-precreate finishes asynchronously relative to the admin endpoint.
for _ in $(seq 1 60); do
  if curl -sf "$CORE_URL/admin/ping" >/dev/null 2>&1; then break; fi
  sleep 1
done

# --- 3. explicit cache sizing (protocol §2.2: recorded, not implicit) ------
# Deliberately generous for a 43k-doc corpus so the baseline is not
# cache-starved. autowarmCount is 0 because the protocol defines warm state by
# an explicit warm-up pass over the real workload (§9.1), not by autowarming --
# an engine that autowarms while another does not is an unfair comparison.
echo "==> applying explicit cache configuration"
curl -sf -X POST -H 'Content-Type: application/json' \
  --data-binary '{
    "set-property": {
      "query.filterCache.size": 4096,
      "query.filterCache.initialSize": 4096,
      "query.filterCache.autowarmCount": 0,
      "query.queryResultCache.size": 4096,
      "query.queryResultCache.initialSize": 4096,
      "query.queryResultCache.autowarmCount": 0,
      "query.documentCache.size": 4096,
      "query.documentCache.initialSize": 4096
    }
  }' "$CORE_URL/config" >/dev/null

# --- 4. index ---------------------------------------------------------------
echo "==> indexing $DATASET from $CATALOG"
python3 "$REPO_ROOT/scripts/datasets/solr_index_wands.py" "$CORE_URL" "" "$CATALOG"

# --- 5. forceMerge(1) on the read-only corpus -------------------------------
# Standard read-only-benchmark practice. This FAVOURS Lucene (one segment, no
# background merges during measurement); the direction is conservative against
# the native engine and is retained deliberately (protocol §2.2).
echo "==> forceMerge(1)"
curl -sf "$CORE_URL/update?optimize=true&maxSegments=1&waitSearcher=true" >/dev/null

# --- 6. corpus parity (protocol §13 stop condition) -------------------------
NUM_FOUND="$(curl -sf "$CORE_URL/select?q=*:*&rows=0&wt=json" \
  | python3 -c 'import json,sys; print(json.load(sys.stdin)["response"]["numFound"])')"
echo "==> numFound=$NUM_FOUND (expected $EXPECTED_DOCS)"
if [[ "$NUM_FOUND" != "$EXPECTED_DOCS" ]]; then
  echo "FATAL: corpus parity failed -- this is not the corpus prior evidence used" >&2
  exit 5
fi

# --- 7. record what was actually provisioned --------------------------------
INDEX_BYTES="$(docker exec "$I61_SOLR_CONTAINER" \
  du -sb "/var/solr/data/$CORE/data/index" 2>/dev/null | cut -f1 || echo 0)"
echo "==> index_bytes=$INDEX_BYTES"
echo "==> container_id=$(docker inspect --format '{{.Id}}' "$I61_SOLR_CONTAINER")"
echo "PROVISION_OK core=$CORE docs=$NUM_FOUND index_bytes=$INDEX_BYTES"
