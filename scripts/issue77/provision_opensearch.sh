#!/usr/bin/env bash
# Issue #77 (Infra E3): provision a fresh single-node OpenSearch
# container with TWO indices -- Dataset A (WANDS-scale, for PLP/facet/
# filter/sort performance) and Dataset B (the tiny deterministic
# multi-variant fixture, for correctness only). Reuses #62's proven
# mapping/bulk-indexer for Dataset A verbatim.
#
# Usage:
#   bash scripts/issue77/provision_elasticsearch.sh <catalog_path> <expected_docs>
#
# On success, the LAST stdout line is:
#   PROVISION_OK container=i77-elasticsearch docs=<count> index_bytes=<bytes>
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
# shellcheck source=../../benchmarks/configs/issue77/resource_envelope.env
source "$REPO_ROOT/benchmarks/configs/issue77/resource_envelope.env"

INDEX_NAME="i77_wands"
FIXTURE_INDEX="i77_fixture"
DATA_DIR="/usr/share/opensearch/data"

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

BASE="http://localhost:${I77_OPENSEARCH_PORT}"
BUILD_TIMEOUT="${I77_BUILD_TIMEOUT_SECONDS:-3600}"

echo "==> removing any existing container $I77_OPENSEARCH_CONTAINER"
docker rm -f "$I77_OPENSEARCH_CONTAINER" >/dev/null 2>&1 || true

echo "==> starting $I77_OPENSEARCH_CONTAINER ($I77_OPENSEARCH_IMAGE)"
docker run -d \
  --name "$I77_OPENSEARCH_CONTAINER" \
  --cpus="$I77_CPUS" \
  --cpuset-cpus="$I77_CPUSET" \
  --memory="$I77_MEMORY" \
  --memory-swap="$I77_MEMORY_SWAP" \
  -p "${I77_OPENSEARCH_PORT}:9200" \
  -e "discovery.type=single-node" \
  -e "DISABLE_SECURITY_PLUGIN=true" \
  -e "OPENSEARCH_JAVA_OPTS=-Xms${I77_JVM_HEAP} -Xmx${I77_JVM_HEAP}" \
  "$I77_OPENSEARCH_IMAGE" >/dev/null

echo "==> waiting for OpenSearch cluster health (timeout ${I77_READINESS_TIMEOUT_SECONDS}s)"
READY=""
STATUS=""
for ((i = 0; i < I77_READINESS_TIMEOUT_SECONDS; i += 2)); do
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
  echo "FATAL: OpenSearch did not become ready within ${I77_READINESS_TIMEOUT_SECONDS}s" >&2
  docker logs "$I77_OPENSEARCH_CONTAINER" >&2
  exit 3
fi
echo "  cluster health OK (status=$STATUS)"

echo "==> creating index $INDEX_NAME"
MAPPING_BODY=$(cat <<'JSON'
{
  "settings": { "number_of_shards": 1, "number_of_replicas": 0 },
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

echo "==> creating and loading fixture index $FIXTURE_INDEX"
FIXTURE_MAPPING='{"mappings":{"properties":{"product_id":{"type":"keyword"},"color":{"type":"keyword"},"size":{"type":"keyword"},"width":{"type":"keyword"},"available":{"type":"boolean"}}}}'
FIXTURE_CREATE_RESP="$(curl -s -X PUT "$BASE/$FIXTURE_INDEX" -H 'Content-Type: application/json' --data-binary "$FIXTURE_MAPPING")"
if ! echo "$FIXTURE_CREATE_RESP" | jq -e '.acknowledged == true' >/dev/null 2>&1; then
  echo "FATAL: fixture index creation failed: $FIXTURE_CREATE_RESP" >&2
  exit 5
fi

FIXTURE_NDJSON="$(mktemp)"
trap 'rm -f "$FIXTURE_NDJSON"' EXIT
cat > "$FIXTURE_NDJSON" <<'NDJSON'
{"index":{"_index":"i77_fixture","_id":"A1"}}
{"product_id":"A","color":"black","size":"8","width":"wide","available":true}
{"index":{"_index":"i77_fixture","_id":"A2"}}
{"product_id":"A","color":"red","size":"9","width":"narrow","available":true}
{"index":{"_index":"i77_fixture","_id":"B1"}}
{"product_id":"B","color":"black","size":"9","width":"wide","available":true}
{"index":{"_index":"i77_fixture","_id":"C1"}}
{"product_id":"C","color":"black","size":"9","width":"narrow","available":false}
NDJSON
FIXTURE_BULK_RESP="$(curl -s -X POST "$BASE/_bulk" -H 'Content-Type: application/x-ndjson' --data-binary "@$FIXTURE_NDJSON")"
if ! echo "$FIXTURE_BULK_RESP" | jq -e '.errors == false' >/dev/null 2>&1; then
  echo "FATAL: fixture bulk load failed: $FIXTURE_BULK_RESP" >&2
  exit 5
fi

FIXTURE_REFRESH_RESP="$(curl -s -X POST "$BASE/$FIXTURE_INDEX/_refresh")"
if ! echo "$FIXTURE_REFRESH_RESP" | jq -e '._shards.failed == 0' >/dev/null 2>&1; then
  echo "FATAL: fixture index refresh failed: $FIXTURE_REFRESH_RESP" >&2
  exit 5
fi
echo "  fixture index loaded"

echo "==> bulk indexing $CATALOG into $INDEX_NAME"
BULK_ERR="$(mktemp)"
trap 'rm -f "$BULK_ERR" "$FIXTURE_NDJSON"' EXIT
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
        bulk_url, data=body, method="POST",
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

echo "==> refreshing index"
curl -sf -X POST "$BASE/$INDEX_NAME/_refresh" >/dev/null

echo "==> force-merging to 1 segment (timeout ${BUILD_TIMEOUT}s)"
curl -sf --max-time "$BUILD_TIMEOUT" -X POST "$BASE/$INDEX_NAME/_forcemerge?max_num_segments=1" >/dev/null

COUNT_JSON="$(curl -sf "$BASE/$INDEX_NAME/_count")"
ACTUAL_DOCS="$(echo "$COUNT_JSON" | jq -r '.count')"
if [[ "$ACTUAL_DOCS" != "$EXPECTED_DOCS" ]]; then
  echo "FATAL: doc count mismatch: got $ACTUAL_DOCS, expected $EXPECTED_DOCS" >&2
  exit 4
fi
echo "  doc count verified: $ACTUAL_DOCS"

INDEX_BYTES="$(docker exec "$I77_OPENSEARCH_CONTAINER" du -sb "$DATA_DIR" | awk '{print $1}')"
if [[ -z "$INDEX_BYTES" ]]; then
  echo "FATAL: could not measure index size at $DATA_DIR" >&2
  exit 5
fi

echo "PROVISION_OK container=$I77_OPENSEARCH_CONTAINER docs=$ACTUAL_DOCS index_bytes=$INDEX_BYTES"
