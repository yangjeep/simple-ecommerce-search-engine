#!/usr/bin/env bash
# Issue #77 (Infra E3): provision a fresh Solr 9.10.1 container with TWO
# cores -- Dataset A (WANDS-scale, for PLP/facet/filter/sort performance)
# and Dataset B (the tiny deterministic multi-variant fixture, for
# correctness only). Reuses #62's proven schema/indexing approach for
# Dataset A verbatim (same field mapping, same streaming indexer) rather
# than re-deriving it.
#
# Usage:
#   bash scripts/issue77/provision_solr.sh <catalog_path> <expected_docs>
#
# On success, the LAST line of stdout is exactly:
#   PROVISION_OK container=i77-solr docs=<numFound> index_bytes=<bytes>
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
# shellcheck source=../../benchmarks/configs/issue77/resource_envelope.env
source "$REPO_ROOT/benchmarks/configs/issue77/resource_envelope.env"

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

CORE="i77_wands"
FIXTURE_CORE="i77_fixture"
BASE="http://localhost:${I77_SOLR_PORT}/solr"
CORE_URL="$BASE/$CORE"
FIXTURE_URL="$BASE/$FIXTURE_CORE"

if [[ "${I77_RUNTIME:-docker}" == "scope" ]]; then
  # Issue #79: Docker-free equivalent envelope (scripts/issue79/scope_runtime.sh).
  # Same Solr version (9.10.1, official tgz) and the same heap; the solr:9.10.1
  # image's `solr-precreate` step is replaced by `bin/solr create -c` against
  # the same _default configset once the node is up (below).
  # shellcheck source=../issue79/scope_runtime.sh
  source "$REPO_ROOT/scripts/issue79/scope_runtime.sh"
  SOLR_DIST="$E3B_ENGINE_ROOT/solr-9.10.1"
  SOLR_HOME_DIR="$E3B_DATA_ROOT/solr"
  export JAVA_HOME="$E3B_ENGINE_ROOT/jdk"
  echo "==> removing any prior $I77_SOLR_CONTAINER scope/solr home"
  e3b_scope_stop "$I77_SOLR_CONTAINER"
  rm -rf "$SOLR_HOME_DIR"
  mkdir -p "$SOLR_HOME_DIR"
  cp "$SOLR_DIST/server/solr/solr.xml" "$SOLR_HOME_DIR/"
  echo "==> starting scope $I77_SOLR_CONTAINER"
  e3b_scope_start "$I77_SOLR_CONTAINER" "$E3B_DATA_ROOT/solr.log" \
    env JAVA_HOME="$JAVA_HOME" SOLR_JAVA_MEM="-Xms${I77_JVM_HEAP} -Xmx${I77_JVM_HEAP}" \
    SOLR_LOGS_DIR="$E3B_DATA_ROOT/solr-logs" SOLR_PID_DIR="$E3B_DATA_ROOT" \
    "$SOLR_DIST/bin/solr" start -f -p "$I77_SOLR_PORT" --solr-home "$SOLR_HOME_DIR"
  for _ in $(seq 1 "$I77_READINESS_TIMEOUT_SECONDS"); do
    if curl -sf "http://localhost:${I77_SOLR_PORT}/solr/admin/info/system" >/dev/null 2>&1; then
      break
    fi
    sleep 1
  done
  "$SOLR_DIST/bin/solr" create -c "$CORE" --solr-url "http://localhost:${I77_SOLR_PORT}" >/dev/null
else
  echo "==> removing any prior $I77_SOLR_CONTAINER container"
  docker rm -f "$I77_SOLR_CONTAINER" >/dev/null 2>&1 || true

  echo "==> starting container $I77_SOLR_CONTAINER (cores=$CORE,$FIXTURE_CORE)"
  docker run -d \
    --name "$I77_SOLR_CONTAINER" \
    --cpus="$I77_CPUS" \
    --cpuset-cpus="$I77_CPUSET" \
    --memory="$I77_MEMORY" \
    --memory-swap="$I77_MEMORY_SWAP" \
    -p "${I77_SOLR_PORT}:8983" \
    -e SOLR_JAVA_MEM="-Xms${I77_JVM_HEAP} -Xmx${I77_JVM_HEAP}" \
    "$I77_SOLR_IMAGE" \
    solr-precreate "$CORE" >/dev/null
fi

echo "==> waiting for Solr to become ready (timeout ${I77_READINESS_TIMEOUT_SECONDS}s)"
READY=0
for _ in $(seq 1 "$I77_READINESS_TIMEOUT_SECONDS"); do
  if curl -sf "$CORE_URL/admin/ping" >/dev/null 2>&1; then
    READY=1
    break
  fi
  sleep 1
done
if [[ "$READY" -ne 1 ]]; then
  echo "FATAL: solr did not become ready" >&2
  if [[ "${I77_RUNTIME:-docker}" == "scope" ]]; then
    cat "$E3B_DATA_ROOT/solr.log" >&2
  else
    docker logs "$I77_SOLR_CONTAINER" >&2
  fi
  exit 3
fi
echo "  solr is ready"

echo "==> creating fixture core $FIXTURE_CORE"
if [[ "${I77_RUNTIME:-docker}" == "scope" ]]; then
  "$SOLR_DIST/bin/solr" create -c "$FIXTURE_CORE" --solr-url "http://localhost:${I77_SOLR_PORT}" >/dev/null
else
  docker exec "$I77_SOLR_CONTAINER" solr create_core -c "$FIXTURE_CORE" >/dev/null
fi

# --- Dataset A schema (verbatim from #62's proven mapping) ------------------
echo "==> defining Dataset A schema on $CORE"
add_field() {
  local core_url="$1" name="$2" type="$3"
  curl -sf -X POST -H 'Content-Type: application/json' --data-binary \
    "{\"add-field\": {\"name\":\"$name\",\"type\":\"$type\",\"indexed\":true,\"stored\":true,\"multiValued\":false}}" \
    "$core_url/schema" >/dev/null
}
add_field "$CORE_URL" title text_general
add_field "$CORE_URL" description text_general
add_field "$CORE_URL" product_class string
add_field "$CORE_URL" category_leaf string
add_field "$CORE_URL" category_depth_1 string
add_field "$CORE_URL" category_depth_2 string
add_field "$CORE_URL" category_depth_3 string
add_field "$CORE_URL" category_depth_4 string
add_field "$CORE_URL" category_depth_5 string
add_field "$CORE_URL" category_depth_6 string
add_field "$CORE_URL" color string
add_field "$CORE_URL" style string
add_field "$CORE_URL" primarymaterial string
add_field "$CORE_URL" material string
add_field "$CORE_URL" shape string
add_field "$CORE_URL" rating_count pfloat
add_field "$CORE_URL" average_rating pfloat
add_field "$CORE_URL" review_count pfloat
echo "  Dataset A schema defined"

# --- Dataset B schema + load (tiny, inline, no streaming needed) -----------
echo "==> defining Dataset B (fixture) schema on $FIXTURE_CORE"
add_field "$FIXTURE_URL" product_id string
add_field "$FIXTURE_URL" color string
add_field "$FIXTURE_URL" size string
add_field "$FIXTURE_URL" width string
add_field "$FIXTURE_URL" available boolean
echo "==> loading Dataset B fixture (4 docs)"
curl -sf -X POST -H 'Content-Type: application/json' "$FIXTURE_URL/update?commit=true" --data-binary '[
  {"id":"A1","product_id":"A","color":"black","size":"8","width":"wide","available":true},
  {"id":"A2","product_id":"A","color":"red","size":"9","width":"narrow","available":true},
  {"id":"B1","product_id":"B","color":"black","size":"9","width":"wide","available":true},
  {"id":"C1","product_id":"C","color":"black","size":"9","width":"narrow","available":false}
]' >/dev/null
echo "  Dataset B fixture loaded"

# --- Dataset A bulk index (streamed, reusing #62's indexer verbatim) -------
echo "==> indexing $CATALOG into $CORE (streamed, batched)"
python3 "$REPO_ROOT/scripts/issue62/index_wands_catalog.py" "$CATALOG" "$CORE_URL"

echo "==> committing"
curl -sf "$CORE_URL/update?commit=true&waitSearcher=true" >/dev/null
echo "==> forceMerge(1)"
curl -sf "$CORE_URL/update?optimize=true&maxSegments=1&waitSearcher=true" >/dev/null

NUM_FOUND="$(curl -sf "$CORE_URL/select?q=*:*&rows=0&wt=json" \
  | python3 -c 'import json,sys; print(json.load(sys.stdin)["response"]["numFound"])')"
echo "==> numFound=$NUM_FOUND (expected $EXPECTED_DOCS)"
if [[ "$NUM_FOUND" != "$EXPECTED_DOCS" ]]; then
  echo "FATAL: doc count mismatch: got $NUM_FOUND, expected $EXPECTED_DOCS" >&2
  exit 4
fi

if [[ "${I77_RUNTIME:-docker}" == "scope" ]]; then
  INDEX_BYTES="$(du -sb "$SOLR_HOME_DIR/$CORE/data/index" 2>/dev/null | cut -f1 || true)"
else
  INDEX_BYTES="$(docker exec "$I77_SOLR_CONTAINER" \
    du -sb "/var/solr/data/$CORE/data/index" 2>/dev/null | cut -f1 || true)"
fi
if [[ -z "$INDEX_BYTES" ]]; then
  echo "FATAL: could not measure on-disk index size" >&2
  exit 5
fi
echo "==> index_bytes=$INDEX_BYTES"

echo "PROVISION_OK container=$I77_SOLR_CONTAINER docs=$NUM_FOUND index_bytes=$INDEX_BYTES"
