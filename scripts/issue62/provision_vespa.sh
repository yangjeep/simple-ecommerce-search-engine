#!/usr/bin/env bash
# Issue #62 (Infra E2): provision a fresh single-node Vespa container, deploy
# an application package (schema) for a WANDS-format product catalog, feed
# the catalog into it, measure the resulting on-disk index size, report the
# result.
#
# Vespa's deployment model differs from every other engine in this experiment
# (single-container Solr/ES/OpenSearch/Typesense/Meilisearch): Vespa requires
# an "application package" (services.xml + a schema .sd file) to be deployed
# to a config server before any document can be fed or any content node
# exists. This script builds that package from scratch on every run (a temp
# dir, zipped, POSTed to the config server's deploy API) rather than assuming
# a pre-existing package, matching the plain self-hosted "quick start"
# pattern documented at https://docs.vespa.ai/en/vespa-quick-start.html,
# adapted to run non-interactively under this experiment's container-limits
# contract.
#
# CPU-support caveat (discovered empirically while writing this script, not
# assumed): the frozen $I62_VESPA_IMAGE (vespaengine/vespa:latest) is built
# assuming a Haswell-or-later x86_64 baseline (AVX2 present). On a host whose
# CPU lacks AVX2 (this dev host reports a bare "QEMU Virtual CPU" with no
# avx/avx2/bmi flags at all), every Vespa binary in that image dies with
# SIGILL ("Problem running program ... => died with signal: illegal
# instruction") and the config server never starts -- this is a genuine,
# documented hardware/virtualization constraint
# (https://docs.vespa.ai/en/operations/self-managed/cpu-support.html), not a
# script bug. Vespa itself ships a slower, less-tested but fully supported
# counterpart image for exactly this situation:
# vespaengine/vespa-generic-intel-x86_64. This script detects AVX2 support on
# the host and transparently substitutes that image (same tag) when AVX2 is
# absent, so the script works unmodified on both capable and incapable hosts
# without editing the frozen container_limits.env. Verified end-to-end on
# this host with the generic image against the real 42,994-doc WANDS catalog.
#
# Usage:
#   bash scripts/issue62/provision_vespa.sh <catalog_path> <expected_docs>
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
# shellcheck source=../../benchmarks/configs/issue62/container_limits.env
source "$REPO_ROOT/benchmarks/configs/issue62/container_limits.env"

# --- 0. arg validation -------------------------------------------------------
if [[ $# -ne 2 ]]; then
  echo "FATAL: usage: provision_vespa.sh <catalog_path> <expected_docs>" >&2
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

CONFIG_URL="http://localhost:${I62_VESPA_CONFIG_PORT}"
QUERY_URL="http://localhost:${I62_VESPA_QUERY_PORT}"
DOCTYPE="wands"
NAMESPACE="i62"
CONTENT_CLUSTER="i62_content"
DOC_DIR_IN_CONTAINER="/opt/vespa/var/db/vespa/search/cluster.${CONTENT_CLUSTER}/n0/documents"

# --- 1. pick an image the host CPU can actually execute ---------------------
# See header comment. Detected once, on the host, not inside a container.
EFFECTIVE_VESPA_IMAGE="$I62_VESPA_IMAGE"
if ! grep -qw avx2 /proc/cpuinfo 2>/dev/null; then
  case "$I62_VESPA_IMAGE" in
    vespaengine/vespa:*)
      TAG="${I62_VESPA_IMAGE#vespaengine/vespa:}"
      EFFECTIVE_VESPA_IMAGE="vespaengine/vespa-generic-intel-x86_64:${TAG}"
      echo "==> host CPU lacks AVX2; substituting $EFFECTIVE_VESPA_IMAGE for $I62_VESPA_IMAGE" \
        "(see https://docs.vespa.ai/en/operations/self-managed/cpu-support.html)"
      ;;
    *)
      echo "==> host CPU lacks AVX2 but \$I62_VESPA_IMAGE ('$I62_VESPA_IMAGE') doesn't match the" \
        "expected vespaengine/vespa:<tag> shape; using it as-is and hoping for the best" >&2
      ;;
  esac
fi
if ! docker image inspect "$EFFECTIVE_VESPA_IMAGE" >/dev/null 2>&1; then
  echo "==> pulling $EFFECTIVE_VESPA_IMAGE"
  docker pull "$EFFECTIVE_VESPA_IMAGE" >/dev/null
fi

# --- 2. fresh container -------------------------------------------------------
echo "==> (re)starting container $I62_VESPA_CONTAINER"
docker rm -f "$I62_VESPA_CONTAINER" >/dev/null 2>&1 || true
docker run -d \
  --name "$I62_VESPA_CONTAINER" \
  --cpus="$I62_CPUS" \
  --cpuset-cpus="$I62_CPUSET" \
  --memory="$I62_MEMORY" \
  --memory-swap="$I62_MEMORY_SWAP" \
  -p "${I62_VESPA_CONFIG_PORT}:19071" \
  -p "${I62_VESPA_QUERY_PORT}:8080" \
  "$EFFECTIVE_VESPA_IMAGE" >/dev/null

# --- 3. wait for the config server to come up --------------------------------
echo "==> waiting for config server readiness (up to ${I62_READINESS_TIMEOUT_SECONDS}s)"
READY=0
for ((i = 0; i < I62_READINESS_TIMEOUT_SECONDS; i += 5)); do
  if curl -sf -m 3 "$CONFIG_URL/ApplicationStatus" >/dev/null 2>&1; then
    READY=1
    break
  fi
  sleep 5
done
if [[ "$READY" -ne 1 ]]; then
  echo "FATAL: vespa config server did not become ready" >&2
  docker logs "$I62_VESPA_CONTAINER" >&2
  exit 3
fi
echo "  config server ready"

# --- 4. build the application package ----------------------------------------
# services.xml: one container cluster (search + document-api) and one content
# cluster (redundancy 1, single node) -- the standard self-hosted single-node
# quick-start topology, not a multi-node production cluster.
APP_DIR="$(mktemp -d)"
trap 'rm -rf "$APP_DIR"' EXIT
mkdir -p "$APP_DIR/schemas"

cat > "$APP_DIR/services.xml" <<EOF
<?xml version="1.0" encoding="utf-8" ?>
<services version="1.0">
  <container id="i62_container" version="1.0">
    <search/>
    <document-api/>
  </container>
  <content id="${CONTENT_CLUSTER}" version="1.0">
    <redundancy>1</redundancy>
    <documents>
      <document type="${DOCTYPE}" mode="index"/>
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

# Field mapping (per task spec): id stays a plain attribute (Vespa documents
# are always addressed by their id:<namespace>:<doctype>::<id> document ID;
# we also keep it as a regular field so it is retrievable/queryable like the
# other engines' `id` field). title/description are indexed text with bm25
# enabled (lexical search). product_class/category_*/color/style/
# primarymaterial/material/shape are attribute-only strings (facetable /
# filterable, matching how the other engines in this experiment treat these
# as keyword/facet fields -- no free-text tokenization needed on them).
# rating_count/average_rating/review_count are attribute doubles.
cat > "$APP_DIR/schemas/${DOCTYPE}.sd" <<EOF
schema ${DOCTYPE} {
  document ${DOCTYPE} {
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
    }
    field category_leaf type string {
      indexing: summary | attribute
    }
    field category_depth_1 type string {
      indexing: summary | attribute
    }
    field category_depth_2 type string {
      indexing: summary | attribute
    }
    field category_depth_3 type string {
      indexing: summary | attribute
    }
    field category_depth_4 type string {
      indexing: summary | attribute
    }
    field category_depth_5 type string {
      indexing: summary | attribute
    }
    field category_depth_6 type string {
      indexing: summary | attribute
    }
    field color type string {
      indexing: summary | attribute
    }
    field style type string {
      indexing: summary | attribute
    }
    field primarymaterial type string {
      indexing: summary | attribute
    }
    field material type string {
      indexing: summary | attribute
    }
    field shape type string {
      indexing: summary | attribute
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

APP_ZIP="$APP_DIR/app.zip"
(cd "$APP_DIR" && zip -rq "$APP_ZIP" services.xml hosts.xml schemas)

# --- 5. deploy: POST the package, then wait for cluster convergence ---------
# Verified real endpoint/method for this image's Vespa version (8.404.14):
# POST /application/v2/tenant/<tenant>/prepareandactivate with the zipped
# package, application/zip content type. The 'default' tenant already exists
# on a fresh single-node quick-start container (no explicit tenant-create
# call needed -- confirmed via GET /application/v2/tenant/default).
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

# Deploy activation is not the same as the cluster actually running the new
# generation -- Vespa converges the config server and every content/search
# node asynchronously. Poll the real convergence endpoint until it reports
# converged:true before feeding anything.
CONVERGE_URL="$CONFIG_URL/application/v2/tenant/default/application/default/environment/prod/region/default/instance/default/serviceconverge"
echo "==> waiting for application convergence (up to ${I62_BUILD_TIMEOUT_SECONDS}s)"
CONVERGED=0
for ((i = 0; i < I62_BUILD_TIMEOUT_SECONDS; i += 5)); do
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
  echo "FATAL: vespa application deploy failed: convergence did not complete within ${I62_BUILD_TIMEOUT_SECONDS}s" >&2
  exit 6
fi
echo "  application converged"

# --- 6. feed the catalog -----------------------------------------------------
# vespa-feed-client (present in the image at /opt/vespa/bin/vespa-feed-client,
# confirmed via --help) is Vespa's documented bulk-feed tool. It streams a
# top-level JSON array of {"put": "id:<ns>:<doctype>::<id>", "fields": {...}}
# operations from stdin (--stdin) without requiring the whole array to be
# materialized by the client first. We generate that array on the fly with a
# small streaming Python converter (one line of the source JSONL in memory at
# a time, one converted record written to stdout at a time) piped directly
# into `docker exec -i ... vespa-feed-client --stdin`, so neither the shell
# nor Python ever holds the full catalog (up to ~5.2GB / 5M docs) in memory,
# and no multi-GB intermediate file is written to disk.
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
python3 "$CONVERTER" "$NAMESPACE" "$DOCTYPE" "$CATALOG" \
  | docker exec -i "$I62_VESPA_CONTAINER" vespa-feed-client \
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
# vespa-feed-client --benchmark prints one or more concatenated top-level
# JSON stats objects (periodic progress + a final summary), each containing
# its own nested objects (e.g. "http.response.code.counts": {"200": N}).
# A brace-matching regex without nesting support grabs the innermost object
# instead of the last *top-level* one, which silently loses top-level keys
# like feeder.error.count. Use json.JSONDecoder.raw_decode to walk only
# top-level objects and keep the last one.
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

# Force the (mostly in-memory) index to disk before measuring size, the same
# way the Solr script's forceMerge(1) makes its measurement reflect installed
# state rather than a transient write-ahead-log-heavy snapshot. Verified
# empirically: pre-flush the WANDS content-node directory reports ~29MB
# (mostly transaction log); after triggerFlush + a short settle it reports a
# stable ~54MB of actual flushed index/attribute/summary files.
echo "==> triggering flush"
docker exec "$I62_VESPA_CONTAINER" vespa-proton-cmd --local triggerFlush >/dev/null 2>&1 || true
# Poll until the on-disk size stabilizes (two consecutive equal readings) or
# a bounded number of checks elapse.
PREV_SIZE=-1
for _ in $(seq 1 30); do
  sleep 3
  CUR_SIZE="$(docker exec "$I62_VESPA_CONTAINER" du -sb "$DOC_DIR_IN_CONTAINER" 2>/dev/null | cut -f1 || echo -1)"
  if [[ "$CUR_SIZE" == "$PREV_SIZE" && "$CUR_SIZE" != "-1" ]]; then
    break
  fi
  PREV_SIZE="$CUR_SIZE"
done

# --- 7. verify doc count -----------------------------------------------------
SEARCH_RESPONSE="$(curl -sf -m 30 "$QUERY_URL/search/?yql=select%20*%20from%20${DOCTYPE}%20where%20true&hits=0" 2>&1)" || {
  echo "FATAL: vespa feed error: doc-count query failed: $SEARCH_RESPONSE" >&2
  exit 5
}
NUM_FOUND="$(echo "$SEARCH_RESPONSE" | python3 -c 'import json,sys; print(json.load(sys.stdin)["root"]["fields"]["totalCount"])' 2>/dev/null || echo "")"
if [[ -z "$NUM_FOUND" ]]; then
  echo "FATAL: vespa feed error: could not parse totalCount from: $SEARCH_RESPONSE" >&2
  exit 5
fi
echo "==> totalCount=$NUM_FOUND (expected $EXPECTED_DOCS)"
if [[ "$NUM_FOUND" != "$EXPECTED_DOCS" ]]; then
  echo "FATAL: doc count mismatch: got $NUM_FOUND, expected $EXPECTED_DOCS" >&2
  exit 4
fi

# --- 8. measure on-disk index size -------------------------------------------
INDEX_BYTES="$(docker exec "$I62_VESPA_CONTAINER" du -sb "$DOC_DIR_IN_CONTAINER" 2>/dev/null | cut -f1 || echo "")"
if [[ -z "$INDEX_BYTES" ]]; then
  # Fallback: the fixed path is derived from our own services.xml (content
  # cluster id + node distribution-key), so it should always exist, but fall
  # back to a search in case a future Vespa version renames the layout.
  INDEX_BYTES="$(docker exec "$I62_VESPA_CONTAINER" sh -c \
    "find /opt/vespa/var/db/vespa/search -maxdepth 4 -type d -name documents | head -n1 | xargs du -sb 2>/dev/null | cut -f1" \
    2>/dev/null || echo "")"
fi
if [[ -z "$INDEX_BYTES" ]]; then
  echo "FATAL: could not measure on-disk index size" >&2
  exit 6
fi
echo "==> index_bytes=$INDEX_BYTES"

echo "PROVISION_OK container=${I62_VESPA_CONTAINER} docs=${NUM_FOUND} index_bytes=${INDEX_BYTES}"
