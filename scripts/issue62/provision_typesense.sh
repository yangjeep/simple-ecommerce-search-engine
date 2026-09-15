#!/usr/bin/env bash
# Issue #62 (Infra E2): provision a fresh Typesense container under the
# campaign's frozen resource limits, index a WANDS-format catalog into it,
# and measure the resulting on-disk index footprint.
#
# Companion to scripts/issue61/provision_solr.sh in spirit only -- Typesense's
# provisioning model is different enough (single required flags, an NDJSON
# bulk-import endpoint, no core precreate step) that this script does not try
# to share code with it, only the overall shape (fresh container -> wait for
# readiness -> create schema -> stream-index -> verify count -> measure size
# -> print a machine-parsed result line).
#
# Usage:
#   bash scripts/issue62/provision_typesense.sh <catalog_path> <expected_docs>
#
# Design notes:
#   * The catalog is streamed line-by-line by an embedded Python3 script --
#     never loaded fully into memory -- since E2's largest tier is ~5.2GB /
#     5,030,298 rows.
#   * Null-valued optional fields (category_depth_N, color, style,
#     primarymaterial, material, shape, rating_count, average_rating,
#     review_count) are OMITTED from the imported document entirely rather
#     than sent as JSON null. This mirrors scripts/issue62/index_wands_catalog.py's
#     choice for the Solr companion indexer and sidesteps any per-field-type
#     null-handling surprises in Typesense's schema -- every declared field
#     below is `"optional": true` to make this valid.
#   * `description`, `product_class` and `category_leaf` are ALSO declared
#     `"optional": true`, which the task's field-shape summary did not call
#     for on those three. This was a genuine blocker found by running the
#     real WANDS catalog: 6,008 / 42,994 rows have `description: null`,
#     2,852 have `product_class: null`, and 1,556 have `category_leaf: null`
#     (verified directly against dataset_cache/wands/catalog.jsonl). Typesense
#     rejects an import document that omits a non-optional declared field, so
#     without this change every row with one of those three nulls fails
#     import. `title` has zero nulls in the observed data and is left
#     required per the task spec.
#   * The container uses a NAMED DOCKER VOLUME (i62-typesense-data), not a
#     bind mount, so `docker exec ... du -sb /data` can measure the index
#     footprint directly from inside the container's own filesystem view.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
# shellcheck source=../../benchmarks/configs/issue62/container_limits.env
source "$REPO_ROOT/benchmarks/configs/issue62/container_limits.env"

# --- 0. validate args --------------------------------------------------
if [[ $# -ne 2 ]]; then
  echo "FATAL: usage: provision_typesense.sh <catalog_path> <expected_docs>" >&2
  exit 2
fi

CATALOG="$1"
EXPECTED_DOCS="$2"

if [[ ! -f "$CATALOG" ]]; then
  echo "FATAL: catalog not found: $CATALOG" >&2
  exit 2
fi

if ! [[ "$EXPECTED_DOCS" =~ ^[0-9]+$ ]] || [[ "$EXPECTED_DOCS" -le 0 ]]; then
  echo "FATAL: expected_docs must be a positive integer, got '$EXPECTED_DOCS'" >&2
  exit 2
fi

COLLECTION="i62_wands"
BASE_URL="http://localhost:${I62_TYPESENSE_PORT}"
DATA_VOLUME="i62-typesense-data"

# --- 1. fresh container + fresh volume ----------------------------------
echo "==> removing any existing container/volume from a previous run"
docker rm -f "$I62_TYPESENSE_CONTAINER" >/dev/null 2>&1 || true
docker volume rm "$DATA_VOLUME" >/dev/null 2>&1 || true

echo "==> starting fresh container $I62_TYPESENSE_CONTAINER"
docker run -d \
  --name "$I62_TYPESENSE_CONTAINER" \
  --cpus="$I62_CPUS" \
  --cpuset-cpus="$I62_CPUSET" \
  --memory="$I62_MEMORY" \
  --memory-swap="$I62_MEMORY_SWAP" \
  -p "${I62_TYPESENSE_PORT}:8108" \
  -v "${DATA_VOLUME}:/data" \
  "$I62_TYPESENSE_IMAGE" \
  --data-dir /data --api-key="$I62_TYPESENSE_API_KEY" --enable-cors >/dev/null

# --- 2. wait for readiness ----------------------------------------------
echo "==> waiting for typesense to report healthy (timeout ${I62_READINESS_TIMEOUT_SECONDS}s)"
READY=0
for ((i = 0; i < I62_READINESS_TIMEOUT_SECONDS; i += 2)); do
  if curl -sf "${BASE_URL}/health" 2>/dev/null | grep -q '"ok":true'; then
    READY=1
    break
  fi
  sleep 2
done

if [[ "$READY" -ne 1 ]]; then
  echo "FATAL: typesense did not become ready" >&2
  docker logs "$I62_TYPESENSE_CONTAINER" >&2
  exit 3
fi
echo "  typesense healthy"

# --- 3. create collection schema ----------------------------------------
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

CREATE_HTTP_CODE=$(curl -s -o /tmp/i62_typesense_create_resp.json -w '%{http_code}' \
  -X POST "${BASE_URL}/collections" \
  -H "X-TYPESENSE-API-KEY: ${I62_TYPESENSE_API_KEY}" \
  -H "Content-Type: application/json" \
  --data-binary "$SCHEMA_JSON")

if [[ "$CREATE_HTTP_CODE" != "201" ]]; then
  echo "FATAL: collection creation failed (HTTP $CREATE_HTTP_CODE): $(cat /tmp/i62_typesense_create_resp.json)" >&2
  rm -f /tmp/i62_typesense_create_resp.json
  exit 3
fi
rm -f /tmp/i62_typesense_create_resp.json
echo "  collection created"

# --- 4. stream-index the catalog (batched NDJSON bulk import) -----------
echo "==> indexing $CATALOG into $COLLECTION"
IMPORTED_DOCS=$(python3 - "$CATALOG" "$BASE_URL" "$COLLECTION" "$I62_TYPESENSE_API_KEY" <<'PYEOF'
import json
import sys

import requests

catalog_path, base_url, collection, api_key = sys.argv[1:5]

BATCH_SIZE = 5000

# Optional fields: omitted from the document entirely when null in the
# source row, matching the sibling Solr indexer's null-handling choice
# (see script header comment).
FIELDS = [
    "id",
    "title",
    "description",
    "product_class",
    "category_leaf",
    "category_depth_1",
    "category_depth_2",
    "category_depth_3",
    "category_depth_4",
    "category_depth_5",
    "category_depth_6",
    "color",
    "style",
    "primarymaterial",
    "material",
    "shape",
    "rating_count",
    "average_rating",
    "review_count",
]

import_url = f"{base_url}/collections/{collection}/documents/import?action=upsert"
headers = {
    "X-TYPESENSE-API-KEY": api_key,
    "Content-Type": "text/plain",
}


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
        print(
            f"FATAL: import error: expected {len(batch)} result lines, got {len(result_lines)}",
            file=sys.stderr,
        )
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

# --- 5. verify document count -------------------------------------------
echo "==> verifying document count via collection retrieval endpoint"
COLLECTION_JSON=$(curl -sf "${BASE_URL}/collections/${COLLECTION}" \
  -H "X-TYPESENSE-API-KEY: ${I62_TYPESENSE_API_KEY}")
ACTUAL_DOCS=$(python3 -c 'import json,sys; print(json.loads(sys.argv[1])["num_documents"])' "$COLLECTION_JSON")

if [[ "$ACTUAL_DOCS" != "$EXPECTED_DOCS" ]]; then
  echo "FATAL: doc count mismatch: got $ACTUAL_DOCS, expected $EXPECTED_DOCS" >&2
  exit 4
fi
echo "  doc count OK: $ACTUAL_DOCS"

# --- 6. measure on-disk index size ---------------------------------------
# Fresh volume with only this one collection, so the whole /data directory
# size is a fair proxy for index footprint.
echo "==> measuring on-disk index size"
DU_OUTPUT=$(docker exec "$I62_TYPESENSE_CONTAINER" du -sb /data)
INDEX_BYTES=$(echo "$DU_OUTPUT" | awk '{print $1}')

if ! [[ "$INDEX_BYTES" =~ ^[0-9]+$ ]]; then
  echo "FATAL: could not parse index size from 'du -sb /data' output: $DU_OUTPUT" >&2
  exit 3
fi

echo "PROVISION_OK container=${I62_TYPESENSE_CONTAINER} docs=${ACTUAL_DOCS} index_bytes=${INDEX_BYTES}"
