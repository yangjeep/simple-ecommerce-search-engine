#!/usr/bin/env bash
# Issue #62 (Infra E2): provision a frozen, single-node Elasticsearch
# container under the E2 resource limits, stream-bulk-index a WANDS-format
# catalog, and verify the corpus indexed cleanly (exact doc count match)
# before reporting on-disk index footprint.
#
# This script measures physical footprint (disk bytes / RAM behavior) at
# increasing catalog scale (100k-5M rows). It is deliberately NOT a
# relevance-tuning or query-latency script: the mapping below is a
# reasonable production-typical ecommerce mapping, not a tuned one, and
# security/TLS are disabled because this is a local single-tenant benchmark
# container, not a production deployment -- enabling auth would add
# credential-provisioning overhead that has nothing to do with the thing
# being measured (index footprint).
#
# This script is intentionally self-contained (its Python bulk indexer is
# inlined below, not shared with provision_opensearch.sh) so the two engines'
# provisioning scripts can be modified independently.
#
# Usage:
#   bash scripts/issue62/provision_elasticsearch.sh <catalog_path> <expected_docs>
#
# On success, prints (as the LAST stdout line, parsed by another program):
#   PROVISION_OK container=<container-name> docs=<count> index_bytes=<bytes>
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
# shellcheck source=../../benchmarks/configs/issue62/container_limits.env
source "$REPO_ROOT/benchmarks/configs/issue62/container_limits.env"

INDEX_NAME="i62_wands"
DATA_DIR="/usr/share/elasticsearch/data"

# --- 0. argument validation -------------------------------------------------
if [[ $# -ne 2 ]]; then
  echo "FATAL: usage: $0 <catalog_path> <expected_docs>" >&2
  exit 2
fi

CATALOG="$1"
EXPECTED_DOCS="$2"

if [[ ! -f "$CATALOG" ]]; then
  echo "FATAL: catalog file not found: $CATALOG" >&2
  exit 2
fi

if ! [[ "$EXPECTED_DOCS" =~ ^[1-9][0-9]*$ ]]; then
  echo "FATAL: expected_docs must be a positive integer, got: $EXPECTED_DOCS" >&2
  exit 2
fi

BASE="http://localhost:${I62_ES_PORT}"
BUILD_TIMEOUT="${I62_BUILD_TIMEOUT_SECONDS:-3600}"

# --- 1. fresh container ------------------------------------------------------
echo "==> removing any existing container $I62_ES_CONTAINER"
docker rm -f "$I62_ES_CONTAINER" >/dev/null 2>&1 || true

echo "==> starting $I62_ES_CONTAINER ($I62_ES_IMAGE)"
docker run -d \
  --name "$I62_ES_CONTAINER" \
  --cpus="$I62_CPUS" \
  --cpuset-cpus="$I62_CPUSET" \
  --memory="$I62_MEMORY" \
  --memory-swap="$I62_MEMORY_SWAP" \
  -p "${I62_ES_PORT}:9200" \
  -e "discovery.type=single-node" \
  -e "xpack.security.enabled=false" \
  -e "ES_JAVA_OPTS=-Xms${I62_JVM_HEAP} -Xmx${I62_JVM_HEAP}" \
  "$I62_ES_IMAGE" >/dev/null

# --- 2. wait for readiness ---------------------------------------------------
echo "==> waiting for Elasticsearch cluster health (timeout ${I62_READINESS_TIMEOUT_SECONDS}s)"
READY=""
STATUS=""
for ((i = 0; i < I62_READINESS_TIMEOUT_SECONDS; i += 2)); do
  HEALTH_JSON="$(curl -sf "$BASE/_cluster/health" 2>/dev/null || true)"
  if [[ -n "$HEALTH_JSON" ]]; then
    STATUS="$(echo "$HEALTH_JSON" | jq -r '.status // empty' 2>/dev/null || true)"
    if [[ "$STATUS" == "yellow" || "$STATUS" == "green" ]]; then
      READY="1"
      break
    fi
  fi
  sleep 2
done

if [[ -z "$READY" ]]; then
  echo "FATAL: Elasticsearch did not become ready within ${I62_READINESS_TIMEOUT_SECONDS}s" >&2
  docker logs "$I62_ES_CONTAINER" >&2
  exit 3
fi
echo "  cluster health OK (status=$STATUS)"

# --- 3. create index with explicit mapping -----------------------------------
echo "==> creating index $INDEX_NAME"
MAPPING_BODY=$(cat <<'JSON'
{
  "settings": {
    "number_of_shards": 1,
    "number_of_replicas": 0
  },
  "mappings": {
    "properties": {
      "id": { "type": "keyword" },
      "title": { "type": "text" },
      "description": { "type": "text" },
      "product_class": { "type": "keyword" },
      "category_leaf": { "type": "keyword" },
      "category_depth_1": { "type": "keyword" },
      "category_depth_2": { "type": "keyword" },
      "category_depth_3": { "type": "keyword" },
      "category_depth_4": { "type": "keyword" },
      "category_depth_5": { "type": "keyword" },
      "category_depth_6": { "type": "keyword" },
      "color": { "type": "keyword" },
      "style": { "type": "keyword" },
      "primarymaterial": { "type": "keyword" },
      "material": { "type": "keyword" },
      "shape": { "type": "keyword" },
      "rating_count": { "type": "float" },
      "average_rating": { "type": "float" },
      "review_count": { "type": "float" }
    }
  }
}
JSON
)

CREATE_RESP="$(curl -sf -X PUT "$BASE/$INDEX_NAME" -H 'Content-Type: application/json' --data-binary "$MAPPING_BODY")"
if ! echo "$CREATE_RESP" | jq -e '.acknowledged == true' >/dev/null 2>&1; then
  echo "FATAL: index creation failed: $CREATE_RESP" >&2
  exit 5
fi
echo "  index created"

# --- 4. stream-bulk-index the catalog ----------------------------------------
# Read the source catalog line-by-line (never loading the whole file into
# memory -- the largest tier is ~5.2GB) and POST ~5000-doc _bulk batches.
echo "==> bulk indexing $CATALOG into $INDEX_NAME"
BULK_ERR="$(mktemp)"
trap 'rm -f "$BULK_ERR"' EXIT

set +e
python3 - "$CATALOG" "$BASE" "$INDEX_NAME" 5000 <<'PYEOF' 2>"$BULK_ERR"
import json
import sys
import urllib.error
import urllib.request

catalog_path, base_url, index_name, batch_size = sys.argv[1], sys.argv[2], sys.argv[3], int(sys.argv[4])
bulk_url = f"{base_url}/_bulk"

def send_batch(lines):
    if not lines:
        return 0
    body = ("\n".join(lines) + "\n").encode("utf-8")
    req = urllib.request.Request(
        bulk_url,
        data=body,
        method="POST",
        headers={"Content-Type": "application/x-ndjson"},
    )
    try:
        with urllib.request.urlopen(req) as resp:
            payload = json.loads(resp.read().decode("utf-8"))
    except urllib.error.HTTPError as e:
        sys.stderr.write(f"bulk request HTTP error: {e.code} {e.read().decode('utf-8', 'replace')}\n")
        sys.exit(1)

    if payload.get("errors"):
        for item in payload.get("items", []):
            action = item.get("index") or item.get("create") or {}
            if action.get("error"):
                sys.stderr.write(f"item error: {json.dumps(action['error'])}\n")
                sys.exit(1)
        sys.stderr.write("bulk response reported errors but no item error was found\n")
        sys.exit(1)

    return len(payload.get("items", []))


total = 0
batch_lines = []
batch_count = 0
with open(catalog_path, "r", encoding="utf-8") as f:
    for line in f:
        line = line.strip()
        if not line:
            continue
        doc = json.loads(line)
        doc_id = doc["id"]
        action = {"index": {"_index": index_name, "_id": doc_id}}
        batch_lines.append(json.dumps(action))
        batch_lines.append(json.dumps(doc))
        batch_count += 1

        if batch_count >= batch_size:
            total += send_batch(batch_lines)
            print(f"  indexed {total} docs so far")
            batch_lines = []
            batch_count = 0

total += send_batch(batch_lines)
print(f"  indexed {total} docs total")
PYEOF
PY_STATUS=$?
set -e

if [[ $PY_STATUS -ne 0 ]]; then
  echo "FATAL: bulk indexing error: $(tail -n 5 "$BULK_ERR")" >&2
  exit 5
fi

# --- 5. refresh + forcemerge -------------------------------------------------
echo "==> refreshing index"
curl -sf -X POST "$BASE/$INDEX_NAME/_refresh" >/dev/null

echo "==> force-merging to 1 segment (may take a while at scale, timeout ${BUILD_TIMEOUT}s)"
curl -sf --max-time "$BUILD_TIMEOUT" -X POST "$BASE/$INDEX_NAME/_forcemerge?max_num_segments=1" >/dev/null

# --- 6. verify document count ------------------------------------------------
COUNT_JSON="$(curl -sf "$BASE/$INDEX_NAME/_count")"
ACTUAL_DOCS="$(echo "$COUNT_JSON" | jq -r '.count')"
if [[ "$ACTUAL_DOCS" != "$EXPECTED_DOCS" ]]; then
  echo "FATAL: doc count mismatch: got $ACTUAL_DOCS, expected $EXPECTED_DOCS" >&2
  exit 4
fi
echo "  doc count verified: $ACTUAL_DOCS"

# --- 7. measure on-disk index size -------------------------------------------
echo "==> measuring on-disk index size"
INDEX_BYTES="$(docker exec "$I62_ES_CONTAINER" du -sb "$DATA_DIR" | awk '{print $1}')"
if [[ -z "$INDEX_BYTES" ]]; then
  echo "FATAL: could not measure index size at $DATA_DIR" >&2
  exit 5
fi

echo "PROVISION_OK container=$I62_ES_CONTAINER docs=$ACTUAL_DOCS index_bytes=$INDEX_BYTES"
