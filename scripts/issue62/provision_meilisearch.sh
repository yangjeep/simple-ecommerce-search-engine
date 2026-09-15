#!/usr/bin/env bash
# Issue #62 (Infra E2): provision a fresh Meilisearch container, index a
# WANDS-format product catalog into it, measure the resulting on-disk index
# size, and report the result.
#
# Usage:
#   bash scripts/issue62/provision_meilisearch.sh <catalog_path> <expected_docs>
#
# On success, the LAST line printed to stdout is exactly:
#   PROVISION_OK container=<container_name> docs=<count> index_bytes=<bytes>
# Another program parses that line, so nothing may print after it and its
# format must not change.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
# shellcheck source=../../benchmarks/configs/issue62/container_limits.env
source "$REPO_ROOT/benchmarks/configs/issue62/container_limits.env"

# --- 0. validate arguments ---------------------------------------------------
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

if ! [[ "$EXPECTED_DOCS" =~ ^[1-9][0-9]*$ ]]; then
  echo "FATAL: expected_docs must be a positive integer, got '$EXPECTED_DOCS'" >&2
  exit 2
fi

INDEX_UID="i62_wands"
BASE="http://localhost:${I62_MEILISEARCH_PORT}"
VOLUME="i62-meilisearch-data"

# --- 1. tear down any previous run, start genuinely fresh -------------------
echo "==> removing any previous container/volume"
docker rm -f "$I62_MEILISEARCH_CONTAINER" >/dev/null 2>&1 || true
docker volume rm "$VOLUME" >/dev/null 2>&1 || true

# --- 2. launch fresh container ------------------------------------------------
# MEILI_ENV=development disables the production master-key requirement so
# unauthenticated write requests are accepted -- appropriate for this local,
# non-production benchmark where credential-provisioning overhead would
# distort a pure indexing-footprint measurement.
echo "==> starting container $I62_MEILISEARCH_CONTAINER"
docker run -d \
  --name "$I62_MEILISEARCH_CONTAINER" \
  --cpus="$I62_CPUS" \
  --cpuset-cpus="$I62_CPUSET" \
  --memory="$I62_MEMORY" \
  --memory-swap="$I62_MEMORY_SWAP" \
  -p "${I62_MEILISEARCH_PORT}:7700" \
  -v "${VOLUME}:/meili_data" \
  -e MEILI_NO_ANALYTICS=true \
  -e MEILI_ENV=development \
  "$I62_MEILISEARCH_IMAGE" >/dev/null

# --- 3. wait for readiness ---------------------------------------------------
echo "==> waiting for meilisearch to become ready (timeout ${I62_READINESS_TIMEOUT_SECONDS}s)"
READY=0
for (( i=0; i<I62_READINESS_TIMEOUT_SECONDS; i+=2 )); do
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
  docker logs "$I62_MEILISEARCH_CONTAINER" >&2
  exit 3
fi
echo "  ready"

# --- 3b. verify unauthenticated writes actually work -------------------------
# Confirms the MEILI_ENV=development assumption before committing to it for
# the whole run; if a real deployment ever needs a master key instead, this
# check fails loudly here rather than 40 minutes into a 5M-doc index.
PROBE_STATUS="$(curl -s -o /dev/null -w '%{http_code}' -X GET "$BASE/indexes" || true)"
if [[ "$PROBE_STATUS" != "200" ]]; then
  echo "FATAL: meilisearch is not accepting unauthenticated requests (GET /indexes returned $PROBE_STATUS)" >&2
  docker logs "$I62_MEILISEARCH_CONTAINER" >&2
  exit 3
fi

# --- 4. create index ---------------------------------------------------------
echo "==> creating index $INDEX_UID"
CREATE_RESP="$(curl -sf -X POST -H 'Content-Type: application/json' \
  --data-binary "{\"uid\": \"$INDEX_UID\", \"primaryKey\": \"id\"}" \
  "$BASE/indexes")"
CREATE_TASK_UID="$(echo "$CREATE_RESP" | python3 -c 'import json,sys; print(json.load(sys.stdin)["taskUid"])')"

# --- 5. filterable attributes (settings before documents) --------------------
echo "==> setting filterable attributes"
FILTER_RESP="$(curl -sf -X PUT -H 'Content-Type: application/json' \
  --data-binary '["product_class", "category_leaf", "category_depth_1", "category_depth_2", "category_depth_3", "category_depth_4", "category_depth_5", "category_depth_6", "color", "style", "primarymaterial", "material", "shape"]' \
  "$BASE/indexes/$INDEX_UID/settings/filterable-attributes")"
FILTER_TASK_UID="$(echo "$FILTER_RESP" | python3 -c 'import json,sys; print(json.load(sys.stdin)["taskUid"])')"

# Poll the index-creation and settings tasks to completion before indexing.
# Helper python is written to a private temp dir (never into the repo tree)
# and cleaned up on exit regardless of how the script terminates.
WORKDIR="$(mktemp -d)"
trap 'rm -rf "$WORKDIR"' EXIT

POLL_HELPER="$WORKDIR/i62_poll_task.py"
cat > "$POLL_HELPER" <<'PYEOF'
import json
import sys
import time
import urllib.request

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
    base_url = sys.argv[1]
    task_uid = sys.argv[2]
    poll(base_url, task_uid)
PYEOF

python3 "$POLL_HELPER" "$BASE" "$CREATE_TASK_UID"
python3 "$POLL_HELPER" "$BASE" "$FILTER_TASK_UID"

# --- 6. bulk index the catalog, streamed, batched, task-polled ---------------
echo "==> indexing $CATALOG into $INDEX_UID"
INDEX_HELPER="$WORKDIR/i62_index_catalog.py"
cat > "$INDEX_HELPER" <<'PYEOF'
import json
import sys
import time
import urllib.error
import urllib.request

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
    req = urllib.request.Request(
        url, data=body, headers={"Content-Type": "application/json"}, method="POST"
    )
    try:
        with urllib.request.urlopen(req) as resp:
            data = json.loads(resp.read().decode("utf-8"))
    except urllib.error.HTTPError as e:
        detail = e.read().decode("utf-8", errors="replace")
        print(f"FATAL: add-documents request failed: {e.code} {detail}", file=sys.stderr)
        sys.exit(1)
    return data["taskUid"]


def main():
    base_url = sys.argv[1]
    index_uid = sys.argv[2]
    catalog_path = sys.argv[3]

    batch = []
    batch_num = 0
    total = 0
    with open(catalog_path, "r", encoding="utf-8") as f:
        for line in f:
            line = line.strip()
            if not line:
                continue
            doc = json.loads(line)
            batch.append(doc)
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

# --- 7. brief settle + re-check index stats before trusting the count -------
sleep 2

# --- 8. verify document count -------------------------------------------------
echo "==> verifying document count"
STATS="$(curl -sf "$BASE/indexes/$INDEX_UID/stats")"
NUM_DOCS="$(echo "$STATS" | python3 -c 'import json,sys; print(json.load(sys.stdin)["numberOfDocuments"])')"
echo "  numberOfDocuments=$NUM_DOCS (expected $EXPECTED_DOCS)"

if [[ "$NUM_DOCS" != "$EXPECTED_DOCS" ]]; then
  echo "FATAL: doc count mismatch: got $NUM_DOCS, expected $EXPECTED_DOCS" >&2
  exit 4
fi

# --- 9. measure on-disk index size -------------------------------------------
echo "==> measuring on-disk index size"
INDEX_BYTES="$(docker exec "$I62_MEILISEARCH_CONTAINER" du -sb /meili_data | cut -f1)"

echo "PROVISION_OK container=$I62_MEILISEARCH_CONTAINER docs=$NUM_DOCS index_bytes=$INDEX_BYTES"
