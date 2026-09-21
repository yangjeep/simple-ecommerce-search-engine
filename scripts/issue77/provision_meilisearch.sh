#!/usr/bin/env bash
# Issue #77 (Infra E3): provision a fresh Meilisearch container with TWO
# indexes -- Dataset A (WANDS-scale) and Dataset B (the tiny deterministic
# fixture, correctness only). Reuses #62's proven schema/indexer for
# Dataset A verbatim.
#
# Usage:
#   bash scripts/issue77/provision_meilisearch.sh <catalog_path> <expected_docs>
#
# On success, the LAST stdout line is:
#   PROVISION_OK container=i77-meilisearch docs=<count> index_bytes=<bytes>
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
# shellcheck source=../../benchmarks/configs/issue77/resource_envelope.env
source "$REPO_ROOT/benchmarks/configs/issue77/resource_envelope.env"

INDEX_UID="i77_wands"
FIXTURE_UID="i77_fixture"
BASE="http://localhost:${I77_MEILISEARCH_PORT}"
VOLUME="i77-meilisearch-data"

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

echo "==> removing any previous container/volume"
docker rm -f "$I77_MEILISEARCH_CONTAINER" >/dev/null 2>&1 || true
docker volume rm "$VOLUME" >/dev/null 2>&1 || true

echo "==> starting container $I77_MEILISEARCH_CONTAINER"
docker run -d \
  --name "$I77_MEILISEARCH_CONTAINER" \
  --cpus="$I77_CPUS" \
  --cpuset-cpus="$I77_CPUSET" \
  --memory="$I77_MEMORY" \
  --memory-swap="$I77_MEMORY_SWAP" \
  -p "${I77_MEILISEARCH_PORT}:7700" \
  -v "${VOLUME}:/meili_data" \
  -e MEILI_NO_ANALYTICS=true \
  -e MEILI_ENV=development \
  "$I77_MEILISEARCH_IMAGE" >/dev/null

echo "==> waiting for meilisearch to become ready (timeout ${I77_READINESS_TIMEOUT_SECONDS}s)"
READY=0
for (( i=0; i<I77_READINESS_TIMEOUT_SECONDS; i+=2 )); do
  STATUS="$(curl -sf "$BASE/health" 2>/dev/null | python3 -c 'import json,sys
try:
    print(json.load(sys.stdin).get("status",""))
except Exception:
    print("")' 2>/dev/null || true)"
  if [[ "$STATUS" == "available" ]]; then
    READY=1
    break
  fi
  sleep 2
done
if [[ "$READY" -ne 1 ]]; then
  echo "FATAL: meilisearch did not become ready" >&2
  docker logs "$I77_MEILISEARCH_CONTAINER" >&2
  exit 3
fi
echo "  ready"

WORKDIR="$(mktemp -d)"
trap 'rm -rf "$WORKDIR"' EXIT
POLL_HELPER="$WORKDIR/i77_poll_task.py"
cat > "$POLL_HELPER" <<'PYEOF'
import json, sys, time, urllib.request

def poll(base_url, task_uid, timeout=1800):
    start = time.time()
    url = f"{base_url}/tasks/{task_uid}"
    while True:
        with urllib.request.urlopen(url) as resp:
            data = json.loads(resp.read().decode("utf-8"))
        status = data.get("status")
        if status == "succeeded":
            return data
        if status == "failed":
            print(f"FATAL: task {task_uid} failed: {data.get('error')}", file=sys.stderr)
            sys.exit(1)
        if time.time() - start > timeout:
            print(f"FATAL: task {task_uid} timed out (last status={status})", file=sys.stderr)
            sys.exit(1)
        time.sleep(0.5)

if __name__ == "__main__":
    poll(sys.argv[1], sys.argv[2])
PYEOF

echo "==> creating index $INDEX_UID"
CREATE_RESP="$(curl -sf -X POST -H 'Content-Type: application/json' \
  --data-binary "{\"uid\": \"$INDEX_UID\", \"primaryKey\": \"id\"}" \
  "$BASE/indexes")"
CREATE_TASK_UID="$(echo "$CREATE_RESP" | python3 -c 'import json,sys; print(json.load(sys.stdin)["taskUid"])')"

echo "==> setting filterable/sortable attributes"
FILTER_RESP="$(curl -sf -X PUT -H 'Content-Type: application/json' \
  --data-binary '["product_class", "category_leaf", "category_depth_1", "category_depth_2", "category_depth_3", "category_depth_4", "category_depth_5", "category_depth_6", "color", "style", "primarymaterial", "material", "shape", "average_rating", "review_count", "rating_count"]' \
  "$BASE/indexes/$INDEX_UID/settings/filterable-attributes")"
FILTER_TASK_UID="$(echo "$FILTER_RESP" | python3 -c 'import json,sys; print(json.load(sys.stdin)["taskUid"])')"
SORT_RESP="$(curl -sf -X PUT -H 'Content-Type: application/json' \
  --data-binary '["average_rating", "review_count", "rating_count"]' \
  "$BASE/indexes/$INDEX_UID/settings/sortable-attributes")"
SORT_TASK_UID="$(echo "$SORT_RESP" | python3 -c 'import json,sys; print(json.load(sys.stdin)["taskUid"])')"

# Meilisearch caps reported/paginated totals at pagination.maxTotalHits
# (default 1000) -- discovered live when every filter/facet cell with a
# real match count above 1000 reported found=1000 instead of the true
# value. Raised well above the 1M tier so num_found is never artificially
# capped at any tier this experiment uses.
PAGINATION_RESP="$(curl -sf -X PATCH -H 'Content-Type: application/json' \
  --data-binary '{"maxTotalHits": 2000000}' \
  "$BASE/indexes/$INDEX_UID/settings/pagination")"
PAGINATION_TASK_UID="$(echo "$PAGINATION_RESP" | python3 -c 'import json,sys; print(json.load(sys.stdin)["taskUid"])')"

python3 "$POLL_HELPER" "$BASE" "$CREATE_TASK_UID"
python3 "$POLL_HELPER" "$BASE" "$FILTER_TASK_UID"
python3 "$POLL_HELPER" "$BASE" "$SORT_TASK_UID"
python3 "$POLL_HELPER" "$BASE" "$PAGINATION_TASK_UID"

echo "==> creating and loading fixture index $FIXTURE_UID"
FIXTURE_CREATE_RESP="$(curl -sf -X POST -H 'Content-Type: application/json' \
  --data-binary "{\"uid\": \"$FIXTURE_UID\", \"primaryKey\": \"id\"}" \
  "$BASE/indexes")"
FIXTURE_CREATE_TASK="$(echo "$FIXTURE_CREATE_RESP" | python3 -c 'import json,sys; print(json.load(sys.stdin)["taskUid"])')"
FIXTURE_FILTER_RESP="$(curl -sf -X PUT -H 'Content-Type: application/json' \
  --data-binary '["product_id", "color", "size", "width", "available"]' \
  "$BASE/indexes/$FIXTURE_UID/settings/filterable-attributes")"
FIXTURE_FILTER_TASK="$(echo "$FIXTURE_FILTER_RESP" | python3 -c 'import json,sys; print(json.load(sys.stdin)["taskUid"])')"
python3 "$POLL_HELPER" "$BASE" "$FIXTURE_CREATE_TASK"
python3 "$POLL_HELPER" "$BASE" "$FIXTURE_FILTER_TASK"

FIXTURE_DOCS='[
  {"id":"A1","product_id":"A","color":"black","size":"8","width":"wide","available":true},
  {"id":"A2","product_id":"A","color":"red","size":"9","width":"narrow","available":true},
  {"id":"B1","product_id":"B","color":"black","size":"9","width":"wide","available":true},
  {"id":"C1","product_id":"C","color":"black","size":"9","width":"narrow","available":false}
]'
FIXTURE_ADD_RESP="$(curl -sf -X POST -H 'Content-Type: application/json' \
  --data-binary "$FIXTURE_DOCS" \
  "$BASE/indexes/$FIXTURE_UID/documents")"
FIXTURE_ADD_TASK="$(echo "$FIXTURE_ADD_RESP" | python3 -c 'import json,sys; print(json.load(sys.stdin)["taskUid"])')"
python3 "$POLL_HELPER" "$BASE" "$FIXTURE_ADD_TASK"
echo "  fixture index loaded"

echo "==> indexing $CATALOG into $INDEX_UID"
INDEX_HELPER="$WORKDIR/i77_index_catalog.py"
cat > "$INDEX_HELPER" <<'PYEOF'
import json, sys, time, urllib.error, urllib.request

BATCH_SIZE = 8000

def poll_task(base_url, task_uid, timeout=1800):
    start = time.time()
    url = f"{base_url}/tasks/{task_uid}"
    while True:
        with urllib.request.urlopen(url) as resp:
            data = json.loads(resp.read().decode("utf-8"))
        status = data.get("status")
        if status == "succeeded":
            return data
        if status == "failed":
            print(f"FATAL: task {task_uid} failed: {data.get('error')}", file=sys.stderr)
            sys.exit(1)
        if time.time() - start > timeout:
            print(f"FATAL: task {task_uid} timed out (last status={status})", file=sys.stderr)
            sys.exit(1)
        time.sleep(0.5)

def post_batch(base_url, index_uid, batch):
    url = f"{base_url}/indexes/{index_uid}/documents"
    body = json.dumps(batch).encode("utf-8")
    req = urllib.request.Request(url, data=body, headers={"Content-Type": "application/json"}, method="POST")
    try:
        with urllib.request.urlopen(req) as resp:
            data = json.loads(resp.read().decode("utf-8"))
    except urllib.error.HTTPError as e:
        detail = e.read().decode("utf-8", errors="replace")
        print(f"FATAL: add-documents request failed: {e.code} {detail}", file=sys.stderr)
        sys.exit(1)
    return data["taskUid"]

def main():
    base_url, index_uid, catalog_path = sys.argv[1], sys.argv[2], sys.argv[3]
    batch, batch_num, total = [], 0, 0
    with open(catalog_path, "r", encoding="utf-8") as f:
        for line in f:
            line = line.strip()
            if not line:
                continue
            batch.append(json.loads(line))
            if len(batch) >= BATCH_SIZE:
                batch_num += 1
                task_uid = post_batch(base_url, index_uid, batch)
                poll_task(base_url, task_uid)
                total += len(batch)
                print(f"  batch {batch_num}: {total} docs indexed (task {task_uid} succeeded)")
                batch = []
    if batch:
        batch_num += 1
        task_uid = post_batch(base_url, index_uid, batch)
        poll_task(base_url, task_uid)
        total += len(batch)
        print(f"  batch {batch_num}: {total} docs indexed (task {task_uid} succeeded)")
    print(f"  total submitted: {total}")

if __name__ == "__main__":
    main()
PYEOF
python3 "$INDEX_HELPER" "$BASE" "$INDEX_UID" "$CATALOG"

sleep 2
echo "==> verifying document count"
STATS="$(curl -sf "$BASE/indexes/$INDEX_UID/stats")"
NUM_DOCS="$(echo "$STATS" | python3 -c 'import json,sys; print(json.load(sys.stdin)["numberOfDocuments"])')"
echo "  numberOfDocuments=$NUM_DOCS (expected $EXPECTED_DOCS)"
if [[ "$NUM_DOCS" != "$EXPECTED_DOCS" ]]; then
  echo "FATAL: doc count mismatch: got $NUM_DOCS, expected $EXPECTED_DOCS" >&2
  exit 4
fi

echo "==> measuring on-disk index size"
INDEX_BYTES="$(docker exec "$I77_MEILISEARCH_CONTAINER" du -sb /meili_data | cut -f1)"

echo "PROVISION_OK container=$I77_MEILISEARCH_CONTAINER docs=$NUM_DOCS index_bytes=$INDEX_BYTES"
