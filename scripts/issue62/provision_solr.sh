#!/usr/bin/env bash
# Issue #62 (Infra E2): provision a fresh Solr 9.10.1 container, index a
# WANDS-format catalog (at whatever scale tier is passed in), measure the
# resulting on-disk index size, and report it.
#
# This is a NEW script, independent of scripts/issue61/provision_solr.sh
# (a frozen, protected script for a different, already-closed experiment
# with a WANDS-specific equivalence-audit schema). #62 only measures
# physical footprint / SKU-density scaling, so the schema here is a plain,
# "reasonable production-typical" field mapping -- no copyFields, no
# lowercase companion fields, no cache tuning, no forceMerge-fairness
# reasoning beyond producing a stable fully-merged on-disk size.
#
# Usage:
#   bash scripts/issue62/provision_solr.sh <catalog_path> <expected_docs>
#
# On success, the LAST line of stdout is exactly:
#   PROVISION_OK container=i62-solr docs=<numFound> index_bytes=<bytes>
# Another program parses that line -- do not print anything after it.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
# shellcheck source=../../benchmarks/configs/issue62/container_limits.env
source "$REPO_ROOT/benchmarks/configs/issue62/container_limits.env"

# --- 0. argument validation -------------------------------------------------
if [[ $# -ne 2 ]]; then
  echo "FATAL: usage: provision_solr.sh <catalog_path> <expected_docs>" >&2
  exit 2
fi

CATALOG="$1"
EXPECTED_DOCS="$2"

if [[ ! -f "$CATALOG" ]]; then
  echo "FATAL: catalog not found or not a regular file: $CATALOG" >&2
  exit 2
fi

if ! [[ "$EXPECTED_DOCS" =~ ^[1-9][0-9]*$ ]]; then
  echo "FATAL: expected_docs must be a positive integer, got: $EXPECTED_DOCS" >&2
  exit 2
fi

CORE="i62_wands"
BASE="http://localhost:${I62_SOLR_PORT}/solr"
CORE_URL="$BASE/$CORE"

# --- 1. clean slate ----------------------------------------------------------
echo "==> removing any prior $I62_SOLR_CONTAINER container"
docker rm -f "$I62_SOLR_CONTAINER" >/dev/null 2>&1 || true

# --- 2. launch Solr fresh, under the frozen E2 resource limits --------------
echo "==> starting container $I62_SOLR_CONTAINER (core=$CORE)"
docker run -d \
  --name "$I62_SOLR_CONTAINER" \
  --cpus="$I62_CPUS" \
  --cpuset-cpus="$I62_CPUSET" \
  --memory="$I62_MEMORY" \
  --memory-swap="$I62_MEMORY_SWAP" \
  -p "${I62_SOLR_PORT}:8983" \
  -e SOLR_JAVA_MEM="-Xms${I62_JVM_HEAP} -Xmx${I62_JVM_HEAP}" \
  "$I62_SOLR_IMAGE" \
  solr-precreate "$CORE" >/dev/null

# --- 3. wait for readiness ---------------------------------------------------
echo "==> waiting for Solr to become ready (timeout ${I62_READINESS_TIMEOUT_SECONDS}s)"
READY=0
for _ in $(seq 1 "$I62_READINESS_TIMEOUT_SECONDS"); do
  if curl -sf "$CORE_URL/admin/ping" >/dev/null 2>&1; then
    READY=1
    break
  fi
  sleep 1
done
if [[ "$READY" -ne 1 ]]; then
  echo "FATAL: solr did not become ready" >&2
  docker logs "$I62_SOLR_CONTAINER" >&2
  exit 3
fi
echo "  solr is ready"

# --- 4. define schema via the Schema API ------------------------------------
# `id` is Solr's implicit unique-key field (string) and already exists in the
# _default configset -- nothing to do for it. Everything else is added
# explicitly. This is a plain, production-typical mapping: text fields for
# free text, string fields for structured/facetable attributes, pfloat for
# numeric ratings. No copyFields, no lowercase companion fields -- #62
# measures footprint, not relevance or equivalence.
echo "==> defining schema on $CORE"

add_field() {
  local name="$1" type="$2"
  curl -sf -X POST -H 'Content-Type: application/json' --data-binary \
    "{\"add-field\": {\"name\":\"$name\",\"type\":\"$type\",\"indexed\":true,\"stored\":true,\"multiValued\":false}}" \
    "$CORE_URL/schema" >/dev/null
}

add_field title text_general
add_field description text_general
add_field product_class string
add_field category_leaf string
add_field category_depth_1 string
add_field category_depth_2 string
add_field category_depth_3 string
add_field category_depth_4 string
add_field category_depth_5 string
add_field category_depth_6 string
add_field color string
add_field style string
add_field primarymaterial string
add_field material string
add_field shape string
add_field rating_count pfloat
add_field average_rating pfloat
add_field review_count pfloat

echo "  schema defined"

# --- 5. bulk index (streamed, never load the whole file into memory) -------
echo "==> indexing $CATALOG into $CORE (streamed, batched)"
python3 "$SCRIPT_DIR/index_wands_catalog.py" "$CATALOG" "$CORE_URL"

# --- 6. final commit + forceMerge to a single segment -----------------------
# Mirrors #61's forceMerge(1) step: a stable, fully-merged on-disk size
# rather than mid-flight segment-count noise.
echo "==> committing"
curl -sf "$CORE_URL/update?commit=true&waitSearcher=true" >/dev/null

echo "==> forceMerge(1)"
curl -sf "$CORE_URL/update?optimize=true&maxSegments=1&waitSearcher=true" >/dev/null

# --- 7. verify document count ------------------------------------------------
NUM_FOUND="$(curl -sf "$CORE_URL/select?q=*:*&rows=0&wt=json" \
  | python3 -c 'import json,sys; print(json.load(sys.stdin)["response"]["numFound"])')"
echo "==> numFound=$NUM_FOUND (expected $EXPECTED_DOCS)"
if [[ "$NUM_FOUND" != "$EXPECTED_DOCS" ]]; then
  echo "FATAL: doc count mismatch: got $NUM_FOUND, expected $EXPECTED_DOCS" >&2
  exit 4
fi

# --- 8. measure on-disk index size ------------------------------------------
INDEX_BYTES="$(docker exec "$I62_SOLR_CONTAINER" \
  du -sb "/var/solr/data/$CORE/data/index" 2>/dev/null | cut -f1 || true)"
if [[ -z "$INDEX_BYTES" ]]; then
  echo "FATAL: could not measure on-disk index size" >&2
  exit 5
fi
echo "==> index_bytes=$INDEX_BYTES"

echo "PROVISION_OK container=$I62_SOLR_CONTAINER docs=$NUM_FOUND index_bytes=$INDEX_BYTES"
