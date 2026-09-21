#!/usr/bin/env bash
# Issue #77 (Infra E3): provision a fresh single-node Vespa container with
# TWO document types in one application package -- Dataset A (WANDS-scale,
# doctype "wands") and Dataset B (the tiny deterministic multi-variant
# fixture, doctype "fixture", correctness only). Reuses #62's proven
# application-package/deploy/feed machinery for Dataset A verbatim (same
# AVX2-image-substitution caveat, same convergence-polling deploy, same
# vespa-feed-client streaming feed).
#
# Usage:
#   bash scripts/issue77/provision_vespa.sh <catalog_path> <expected_docs>
#
# On success, the LAST stdout line is:
#   PROVISION_OK container=i77-vespa docs=<count> index_bytes=<bytes>
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
# shellcheck source=../../benchmarks/configs/issue77/resource_envelope.env
source "$REPO_ROOT/benchmarks/configs/issue77/resource_envelope.env"

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

CONFIG_URL="http://localhost:${I77_VESPA_CONFIG_PORT}"
QUERY_URL="http://localhost:${I77_VESPA_QUERY_PORT}"
NAMESPACE="i77"
CONTENT_CLUSTER="i77_content"
DOC_DIR_IN_CONTAINER="/opt/vespa/var/db/vespa/search/cluster.${CONTENT_CLUSTER}/n0/documents"

# --- pick an image the host CPU can actually execute (see #62 precedent) ---
EFFECTIVE_VESPA_IMAGE="$I77_VESPA_IMAGE"
if ! grep -qw avx2 /proc/cpuinfo 2>/dev/null; then
  case "$I77_VESPA_IMAGE" in
    vespaengine/vespa:*)
      TAG="${I77_VESPA_IMAGE#vespaengine/vespa:}"
      EFFECTIVE_VESPA_IMAGE="vespaengine/vespa-generic-intel-x86_64:${TAG}"
      echo "==> host CPU lacks AVX2; substituting $EFFECTIVE_VESPA_IMAGE for $I77_VESPA_IMAGE"
      ;;
    *)
      echo "==> host CPU lacks AVX2 but \$I77_VESPA_IMAGE doesn't match vespaengine/vespa:<tag>; using as-is" >&2
      ;;
  esac
fi
if ! docker image inspect "$EFFECTIVE_VESPA_IMAGE" >/dev/null 2>&1; then
  echo "==> pulling $EFFECTIVE_VESPA_IMAGE"
  docker pull "$EFFECTIVE_VESPA_IMAGE" >/dev/null
fi

echo "==> (re)starting container $I77_VESPA_CONTAINER"
docker rm -f "$I77_VESPA_CONTAINER" >/dev/null 2>&1 || true
docker run -d \
  --name "$I77_VESPA_CONTAINER" \
  --cpus="$I77_CPUS" \
  --cpuset-cpus="$I77_CPUSET" \
  --memory="$I77_MEMORY" \
  --memory-swap="$I77_MEMORY_SWAP" \
  -p "${I77_VESPA_CONFIG_PORT}:19071" \
  -p "${I77_VESPA_QUERY_PORT}:8080" \
  "$EFFECTIVE_VESPA_IMAGE" >/dev/null

echo "==> waiting for config server readiness (up to ${I77_READINESS_TIMEOUT_SECONDS}s)"
READY=0
for ((i = 0; i < I77_READINESS_TIMEOUT_SECONDS; i += 5)); do
  if curl -sf -m 3 "$CONFIG_URL/ApplicationStatus" >/dev/null 2>&1; then
    READY=1
    break
  fi
  sleep 5
done
if [[ "$READY" -ne 1 ]]; then
  echo "FATAL: vespa config server did not become ready" >&2
  docker logs "$I77_VESPA_CONTAINER" >&2
  exit 3
fi
echo "  config server ready"

APP_DIR="$(mktemp -d)"
trap 'rm -rf "$APP_DIR"' EXIT
mkdir -p "$APP_DIR/schemas"

cat > "$APP_DIR/services.xml" <<EOF
<?xml version="1.0" encoding="utf-8" ?>
<services version="1.0">
  <container id="i77_container" version="1.0">
    <search/>
    <document-api/>
  </container>
  <content id="${CONTENT_CLUSTER}" version="1.0">
    <redundancy>1</redundancy>
    <documents>
      <document type="wands" mode="index"/>
      <document type="fixture" mode="index"/>
    </documents>
    <nodes>
      <node distribution-key="0" hostalias="node1"/>
    </nodes>
  </content>
</services>
EOF

cat > "$APP_DIR/hosts.xml" <<'EOF'
<?xml version="1.0" encoding="utf-8" ?>
<hosts>
  <host name="localhost">
    <alias>node1</alias>
  </host>
</hosts>
EOF

cat > "$APP_DIR/schemas/wands.sd" <<'EOF'
schema wands {
  document wands {
    field id type string {
      indexing: summary | attribute
    }
    field title type string {
      indexing: summary | index
      index: enable-bm25
    }
    field description type string {
      indexing: summary | index
      index: enable-bm25
    }
    field product_class type string {
      indexing: summary | attribute
      attribute: fast-search
    }
    field category_leaf type string {
      indexing: summary | attribute
      attribute: fast-search
    }
    field category_depth_1 type string {
      indexing: summary | attribute
      attribute: fast-search
    }
    field category_depth_2 type string {
      indexing: summary | attribute
      attribute: fast-search
    }
    field category_depth_3 type string {
      indexing: summary | attribute
      attribute: fast-search
    }
    field category_depth_4 type string {
      indexing: summary | attribute
      attribute: fast-search
    }
    field category_depth_5 type string {
      indexing: summary | attribute
      attribute: fast-search
    }
    field category_depth_6 type string {
      indexing: summary | attribute
      attribute: fast-search
    }
    field color type string {
      indexing: summary | attribute
      attribute: fast-search
    }
    field style type string {
      indexing: summary | attribute
      attribute: fast-search
    }
    field primarymaterial type string {
      indexing: summary | attribute
      attribute: fast-search
    }
    field material type string {
      indexing: summary | attribute
      attribute: fast-search
    }
    field shape type string {
      indexing: summary | attribute
      attribute: fast-search
    }
    field rating_count type double {
      indexing: summary | attribute
    }
    field average_rating type double {
      indexing: summary | attribute
    }
    field review_count type double {
      indexing: summary | attribute
    }
  }
}
EOF

cat > "$APP_DIR/schemas/fixture.sd" <<'EOF'
schema fixture {
  document fixture {
    field product_id type string {
      indexing: summary | attribute
      attribute: fast-search
    }
    field color type string {
      indexing: summary | attribute
      attribute: fast-search
    }
    field size type string {
      indexing: summary | attribute
      attribute: fast-search
    }
    field width type string {
      indexing: summary | attribute
      attribute: fast-search
    }
    field available type bool {
      indexing: summary | attribute
    }
  }
}
EOF

APP_ZIP="$APP_DIR/app.zip"
(cd "$APP_DIR" && zip -rq "$APP_ZIP" services.xml hosts.xml schemas)

echo "==> deploying application package"
DEPLOY_RESPONSE="$(curl -sf -X POST \
  --header 'Content-Type: application/zip' \
  --data-binary @"$APP_ZIP" \
  "$CONFIG_URL/application/v2/tenant/default/prepareandactivate" 2>&1)" || {
  echo "FATAL: vespa application deploy failed: $DEPLOY_RESPONSE" >&2
  exit 6
}
if ! echo "$DEPLOY_RESPONSE" | python3 -c 'import json,sys; d=json.load(sys.stdin); sys.exit(0 if d.get("activated") else 1)' 2>/dev/null; then
  echo "FATAL: vespa application deploy failed: $DEPLOY_RESPONSE" >&2
  exit 6
fi
echo "  deploy accepted and activated"

CONVERGE_URL="$CONFIG_URL/application/v2/tenant/default/application/default/environment/prod/region/default/instance/default/serviceconverge"
echo "==> waiting for application convergence (up to ${I77_BUILD_TIMEOUT_SECONDS}s)"
CONVERGED=0
for ((i = 0; i < I77_BUILD_TIMEOUT_SECONDS; i += 5)); do
  RESP="$(curl -sf -m 5 "$CONVERGE_URL" 2>/dev/null || true)"
  if [[ -n "$RESP" ]]; then
    IS_CONVERGED="$(echo "$RESP" | python3 -c 'import json,sys
try:
    d = json.load(sys.stdin)
    print("1" if d.get("converged") else "0")
except Exception:
    print("0")' 2>/dev/null || echo 0)"
    if [[ "$IS_CONVERGED" == "1" ]]; then
      CONVERGED=1
      break
    fi
  fi
  sleep 5
done
if [[ "$CONVERGED" -ne 1 ]]; then
  echo "FATAL: vespa application deploy failed: convergence did not complete within ${I77_BUILD_TIMEOUT_SECONDS}s" >&2
  exit 6
fi
echo "  application converged"

echo "==> feeding fixture (Dataset B)"
FIXTURE_JSON="$APP_DIR/fixture_feed.json"
cat > "$FIXTURE_JSON" <<EOF
[
  {"put": "id:${NAMESPACE}:fixture::A1", "fields": {"product_id":"A","color":"black","size":"8","width":"wide","available":true}},
  {"put": "id:${NAMESPACE}:fixture::A2", "fields": {"product_id":"A","color":"red","size":"9","width":"narrow","available":true}},
  {"put": "id:${NAMESPACE}:fixture::B1", "fields": {"product_id":"B","color":"black","size":"9","width":"wide","available":true}},
  {"put": "id:${NAMESPACE}:fixture::C1", "fields": {"product_id":"C","color":"black","size":"9","width":"narrow","available":false}}
]
EOF
FIXTURE_FEED_LOG="$APP_DIR/fixture_feed.log"
docker cp "$FIXTURE_JSON" "$I77_VESPA_CONTAINER:/tmp/fixture_feed.json"
if ! docker exec "$I77_VESPA_CONTAINER" vespa-feed-client \
      --file /tmp/fixture_feed.json \
      --endpoint "http://localhost:8080" \
      --show-errors > "$FIXTURE_FEED_LOG" 2>&1; then
  echo "FATAL: vespa fixture feed failed" >&2
  tail -n 40 "$FIXTURE_FEED_LOG" >&2
  exit 5
fi
echo "  fixture loaded"

CONVERTER="$APP_DIR/wands_to_feed.py"
cat > "$CONVERTER" <<'PYEOF'
import json
import sys

FIELDS = [
    "id", "title", "description", "product_class", "category_leaf",
    "category_depth_1", "category_depth_2", "category_depth_3",
    "category_depth_4", "category_depth_5", "category_depth_6",
    "color", "style", "primarymaterial", "material", "shape",
    "rating_count", "average_rating", "review_count",
]

namespace, doctype, path = sys.argv[1], sys.argv[2], sys.argv[3]
out = sys.stdout
first = True
out.write("[\n")
with open(path, "r", encoding="utf-8") as f:
    for line in f:
        line = line.strip()
        if not line:
            continue
        rec = json.loads(line)
        doc_id = rec["id"]
        fields = {k: rec[k] for k in FIELDS if rec.get(k) is not None}
        op = {"put": f"id:{namespace}:{doctype}::{doc_id}", "fields": fields}
        if not first:
            out.write(",\n")
        first = False
        out.write(json.dumps(op))
out.write("\n]\n")
PYEOF

FEED_LOG="$APP_DIR/feed.log"
echo "==> feeding catalog: $CATALOG"
set +e
python3 "$CONVERTER" "$NAMESPACE" "wands" "$CATALOG" \
  | docker exec -i "$I77_VESPA_CONTAINER" vespa-feed-client \
      --stdin \
      --endpoint "http://localhost:8080" \
      --connections 8 \
      --show-errors \
      --benchmark \
  > "$FEED_LOG" 2>&1
FEED_EXIT=$?
set -e
if [[ "$FEED_EXIT" -ne 0 ]]; then
  echo "FATAL: vespa feed error: vespa-feed-client exited $FEED_EXIT" >&2
  tail -n 40 "$FEED_LOG" >&2
  exit 5
fi
FEED_ERRORS="$(python3 -c '
import json, sys
text = open(sys.argv[1]).read()
decoder = json.JSONDecoder()
objs = []
i = 0
n = len(text)
while i < n:
    while i < n and text[i] in " \t\r\n":
        i += 1
    if i >= n:
        break
    try:
        obj, end = decoder.raw_decode(text, i)
        objs.append(obj)
        i = end
    except ValueError:
        i += 1
if not objs:
    print("unknown")
else:
    print(objs[-1].get("feeder.error.count", "unknown"))
' "$FEED_LOG")"
if [[ "$FEED_ERRORS" != "0" ]]; then
  echo "FATAL: vespa feed error: feeder.error.count=$FEED_ERRORS (see $FEED_LOG)" >&2
  tail -n 40 "$FEED_LOG" >&2
  exit 5
fi
echo "  feed completed with 0 errors"

echo "==> triggering flush"
docker exec "$I77_VESPA_CONTAINER" vespa-proton-cmd --local triggerFlush >/dev/null 2>&1 || true
PREV_SIZE=-1
for _ in $(seq 1 30); do
  sleep 3
  CUR_SIZE="$(docker exec "$I77_VESPA_CONTAINER" du -sb "$DOC_DIR_IN_CONTAINER" 2>/dev/null | cut -f1 || echo -1)"
  if [[ "$CUR_SIZE" == "$PREV_SIZE" && "$CUR_SIZE" != "-1" ]]; then
    break
  fi
  PREV_SIZE="$CUR_SIZE"
done

SEARCH_RESPONSE="$(curl -sf -m 30 "$QUERY_URL/search/?yql=select%20*%20from%20wands%20where%20true&hits=0" 2>&1)" || {
  echo "FATAL: vespa doc-count query failed: $SEARCH_RESPONSE" >&2
  exit 5
}
NUM_FOUND="$(echo "$SEARCH_RESPONSE" | python3 -c 'import json,sys; print(json.load(sys.stdin)["root"]["fields"]["totalCount"])' 2>/dev/null || echo "")"
if [[ -z "$NUM_FOUND" ]]; then
  echo "FATAL: could not parse totalCount from: $SEARCH_RESPONSE" >&2
  exit 5
fi
echo "==> totalCount=$NUM_FOUND (expected $EXPECTED_DOCS)"
if [[ "$NUM_FOUND" != "$EXPECTED_DOCS" ]]; then
  echo "FATAL: doc count mismatch: got $NUM_FOUND, expected $EXPECTED_DOCS" >&2
  exit 4
fi

INDEX_BYTES="$(docker exec "$I77_VESPA_CONTAINER" du -sb "$DOC_DIR_IN_CONTAINER" 2>/dev/null | cut -f1 || echo "")"
if [[ -z "$INDEX_BYTES" ]]; then
  INDEX_BYTES="$(docker exec "$I77_VESPA_CONTAINER" sh -c \
    "find /opt/vespa/var/db/vespa/search -maxdepth 4 -type d -name documents | head -n1 | xargs du -sb 2>/dev/null | cut -f1" \
    2>/dev/null || echo "")"
fi
if [[ -z "$INDEX_BYTES" ]]; then
  echo "FATAL: could not measure on-disk index size" >&2
  exit 6
fi

echo "PROVISION_OK container=${I77_VESPA_CONTAINER} docs=${NUM_FOUND} index_bytes=${INDEX_BYTES}"
