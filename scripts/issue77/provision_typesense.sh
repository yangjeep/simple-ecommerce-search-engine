#!/usr/bin/env bash
# Issue #77 (Infra E3): provision a fresh Typesense container with TWO
# collections -- Dataset A (WANDS-scale) and Dataset B (the tiny
# deterministic fixture, correctness only). Reuses #62's proven schema/
# null-handling for Dataset A verbatim.
#
# Usage:
#   bash scripts/issue77/provision_typesense.sh <catalog_path> <expected_docs>
#
# On success, the LAST stdout line is:
#   PROVISION_OK container=i77-typesense docs=<count> index_bytes=<bytes>
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
# shellcheck source=../../benchmarks/configs/issue77/resource_envelope.env
source "$REPO_ROOT/benchmarks/configs/issue77/resource_envelope.env"

COLLECTION="i77_wands"
FIXTURE_COLLECTION="i77_fixture"
BASE_URL="http://localhost:${I77_TYPESENSE_PORT}"
DATA_VOLUME="i77-typesense-data"

if [[ $# -ne 2 ]]; then
  echo "FATAL: usage: $0 <catalog_path> <expected_docs>" >&2
  exit 2
fi
CATALOG="$1"
EXPECTED_DOCS="$2"
if [[ ! -f "$CATALOG" ]]; then
  echo "FATAL: catalog not found: $CATALOG" >&2
  exit 2
fi

echo "==> removing any existing container/volume from a previous run"
docker rm -f "$I77_TYPESENSE_CONTAINER" >/dev/null 2>&1 || true
docker volume rm "$DATA_VOLUME" >/dev/null 2>&1 || true

echo "==> starting fresh container $I77_TYPESENSE_CONTAINER"
docker run -d \
  --name "$I77_TYPESENSE_CONTAINER" \
  --cpus="$I77_CPUS" \
  --cpuset-cpus="$I77_CPUSET" \
  --memory="$I77_MEMORY" \
  --memory-swap="$I77_MEMORY_SWAP" \
  -p "${I77_TYPESENSE_PORT}:8108" \
  -v "${DATA_VOLUME}:/data" \
  "$I77_TYPESENSE_IMAGE" \
  --data-dir /data --api-key="$I77_TYPESENSE_API_KEY" --enable-cors >/dev/null

echo "==> waiting for typesense to report healthy (timeout ${I77_READINESS_TIMEOUT_SECONDS}s)"
READY=0
for ((i = 0; i < I77_READINESS_TIMEOUT_SECONDS; i += 2)); do
  if curl -sf "${BASE_URL}/health" 2>/dev/null | grep -q '"ok":true'; then
    READY=1
    break
  fi
  sleep 2
done
if [[ "$READY" -ne 1 ]]; then
  echo "FATAL: typesense did not become ready" >&2
  docker logs "$I77_TYPESENSE_CONTAINER" >&2
  exit 3
fi
echo "  typesense healthy"

echo "==> creating collection $COLLECTION"
SCHEMA_JSON=$(cat <<JSON
{
  "name": "${COLLECTION}",
  "fields": [
    {"name": "title", "type": "string"},
    {"name": "description", "type": "string", "optional": true},
    {"name": "product_class", "type": "string", "facet": true, "optional": true},
    {"name": "category_leaf", "type": "string", "facet": true, "optional": true},
    {"name": "category_depth_1", "type": "string", "facet": true, "optional": true},
    {"name": "category_depth_2", "type": "string", "facet": true, "optional": true},
    {"name": "category_depth_3", "type": "string", "facet": true, "optional": true},
    {"name": "category_depth_4", "type": "string", "facet": true, "optional": true},
    {"name": "category_depth_5", "type": "string", "facet": true, "optional": true},
    {"name": "category_depth_6", "type": "string", "facet": true, "optional": true},
    {"name": "color", "type": "string", "facet": true, "optional": true},
    {"name": "style", "type": "string", "facet": true, "optional": true},
    {"name": "primarymaterial", "type": "string", "facet": true, "optional": true},
    {"name": "material", "type": "string", "facet": true, "optional": true},
    {"name": "shape", "type": "string", "facet": true, "optional": true},
    {"name": "rating_count", "type": "float", "optional": true},
    {"name": "average_rating", "type": "float", "optional": true},
    {"name": "review_count", "type": "float", "optional": true}
  ]
}
JSON
)
CREATE_HTTP_CODE=$(curl -s -o /tmp/i77_typesense_create_resp.json -w '%{http_code}' \
  -X POST "${BASE_URL}/collections" \
  -H "X-TYPESENSE-API-KEY: ${I77_TYPESENSE_API_KEY}" \
  -H "Content-Type: application/json" \
  --data-binary "$SCHEMA_JSON")
if [[ "$CREATE_HTTP_CODE" != "201" ]]; then
  echo "FATAL: collection creation failed (HTTP $CREATE_HTTP_CODE): $(cat /tmp/i77_typesense_create_resp.json)" >&2
  rm -f /tmp/i77_typesense_create_resp.json
  exit 3
fi
rm -f /tmp/i77_typesense_create_resp.json
echo "  collection created"

echo "==> creating and loading fixture collection $FIXTURE_COLLECTION"
FIXTURE_SCHEMA='{"name":"i77_fixture","fields":[{"name":"product_id","type":"string","facet":true},{"name":"color","type":"string","facet":true},{"name":"size","type":"string","facet":true},{"name":"width","type":"string","facet":true},{"name":"available","type":"bool","facet":true}]}'
FIXTURE_CREATE_CODE=$(curl -s -o /tmp/i77_typesense_fixture_resp.json -w '%{http_code}' \
  -X POST "${BASE_URL}/collections" \
  -H "X-TYPESENSE-API-KEY: ${I77_TYPESENSE_API_KEY}" \
  -H "Content-Type: application/json" \
  --data-binary "$FIXTURE_SCHEMA")
if [[ "$FIXTURE_CREATE_CODE" != "201" ]]; then
  echo "FATAL: fixture collection creation failed (HTTP $FIXTURE_CREATE_CODE): $(cat /tmp/i77_typesense_fixture_resp.json)" >&2
  rm -f /tmp/i77_typesense_fixture_resp.json
  exit 3
fi
rm -f /tmp/i77_typesense_fixture_resp.json

FIXTURE_NDJSON="$(mktemp)"
trap 'rm -f "$FIXTURE_NDJSON"' EXIT
cat > "$FIXTURE_NDJSON" <<'NDJSON'
{"id":"A1","product_id":"A","color":"black","size":"8","width":"wide","available":true}
{"id":"A2","product_id":"A","color":"red","size":"9","width":"narrow","available":true}
{"id":"B1","product_id":"B","color":"black","size":"9","width":"wide","available":true}
{"id":"C1","product_id":"C","color":"black","size":"9","width":"narrow","available":false}
NDJSON
FIXTURE_IMPORT_RESP=$(curl -s -X POST "${BASE_URL}/collections/${FIXTURE_COLLECTION}/documents/import?action=upsert" \
  -H "X-TYPESENSE-API-KEY: ${I77_TYPESENSE_API_KEY}" \
  -H "Content-Type: text/plain" \
  --data-binary "@$FIXTURE_NDJSON")
if echo "$FIXTURE_IMPORT_RESP" | grep -q '"success":false'; then
  echo "FATAL: fixture import failed: $FIXTURE_IMPORT_RESP" >&2
  exit 5
fi
echo "  fixture collection loaded"

echo "==> indexing $CATALOG into $COLLECTION"
IMPORTED_DOCS=$(python3 - "$CATALOG" "$BASE_URL" "$COLLECTION" "$I77_TYPESENSE_API_KEY" <<'PYEOF'
import json
import sys

import requests

catalog_path, base_url, collection, api_key = sys.argv[1:5]
BATCH_SIZE = 5000
FIELDS = [
    "id", "title", "description", "product_class", "category_leaf",
    "category_depth_1", "category_depth_2", "category_depth_3",
    "category_depth_4", "category_depth_5", "category_depth_6",
    "color", "style", "primarymaterial", "material", "shape",
    "rating_count", "average_rating", "review_count",
]
import_url = f"{base_url}/collections/{collection}/documents/import?action=upsert"
headers = {"X-TYPESENSE-API-KEY": api_key, "Content-Type": "text/plain"}


def to_ts_doc(row: dict) -> dict:
    doc = {}
    for field in FIELDS:
        value = row.get(field)
        if value is None:
            continue
        if field == "id":
            value = str(value)
        doc[field] = value
    return doc


def flush(batch: list[str]) -> None:
    if not batch:
        return
    body = "\n".join(batch)
    resp = requests.post(import_url, headers=headers, data=body.encode("utf-8"), timeout=300)
    if resp.status_code != 200:
        print(f"FATAL: import batch HTTP {resp.status_code}: {resp.text[:2000]}", file=sys.stderr)
        sys.exit(5)
    result_lines = resp.text.strip("\n").split("\n")
    if len(result_lines) != len(batch):
        print(f"FATAL: import error: expected {len(batch)} result lines, got {len(result_lines)}", file=sys.stderr)
        sys.exit(5)
    for line in result_lines:
        result = json.loads(line)
        if not result.get("success", False):
            print(f"FATAL: import error: {result}", file=sys.stderr)
            sys.exit(5)


batch: list[str] = []
total = 0
with open(catalog_path, "r", encoding="utf-8") as f:
    for line in f:
        line = line.strip()
        if not line:
            continue
        row = json.loads(line)
        doc = to_ts_doc(row)
        batch.append(json.dumps(doc))
        total += 1
        if len(batch) >= BATCH_SIZE:
            flush(batch)
            batch = []

flush(batch)
print(total)
PYEOF
)
echo "  imported $IMPORTED_DOCS documents (rows read from catalog)"

echo "==> verifying document count"
COLLECTION_JSON=$(curl -sf "${BASE_URL}/collections/${COLLECTION}" \
  -H "X-TYPESENSE-API-KEY: ${I77_TYPESENSE_API_KEY}")
ACTUAL_DOCS=$(python3 -c 'import json,sys; print(json.loads(sys.argv[1])["num_documents"])' "$COLLECTION_JSON")
if [[ "$ACTUAL_DOCS" != "$EXPECTED_DOCS" ]]; then
  echo "FATAL: doc count mismatch: got $ACTUAL_DOCS, expected $EXPECTED_DOCS" >&2
  exit 4
fi
echo "  doc count OK: $ACTUAL_DOCS"

echo "==> measuring on-disk index size"
DU_OUTPUT=$(docker exec "$I77_TYPESENSE_CONTAINER" du -sb /data)
INDEX_BYTES=$(echo "$DU_OUTPUT" | awk '{print $1}')
if ! [[ "$INDEX_BYTES" =~ ^[0-9]+$ ]]; then
  echo "FATAL: could not parse index size from 'du -sb /data' output: $DU_OUTPUT" >&2
  exit 3
fi

echo "PROVISION_OK container=${I77_TYPESENSE_CONTAINER} docs=${ACTUAL_DOCS} index_bytes=${INDEX_BYTES}"
