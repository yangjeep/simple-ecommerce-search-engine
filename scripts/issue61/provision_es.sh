#!/usr/bin/env bash
# Issue #61 (Infra E1) §2.4 Elasticsearch time-box.
#
# Elasticsearch is a SECONDARY, gate-optional arm. This script exists to answer
# one binary question honestly rather than assert a deferral without trying:
#
#   Can Elasticsearch run on THIS host, under the SAME frozen resource limits
#   as Solr, and accept a competent commerce mapping over the real corpus?
#
# Answering it separates two very different reasons for excluding ES:
#   * the infrastructure cannot support it here (host/limits/image), versus
#   * the infrastructure is fine and the blocker is adapter scope.
# Only the second is a legitimate DEFERRED-TO-PRE-E2; the first would be an
# environment finding that also affects #57.
#
# Mapping competence bar mirrors the Solr side (protocol §2.2):
#   * structured commerce attributes as `keyword` (exact, doc_values by
#     default) -- never `text`;
#   * a lowercased companion produced by `copy_to` into a `keyword` field with
#     a lowercase `normalizer`, the ES analogue of Solr's copyField into a
#     KeywordTokenizer+LowerCaseFilter field, so exact filters resolve a term
#     rather than running a regex automaton;
#   * lexical fields as analyzed `text`;
#   * one shard, zero replicas, refresh disabled during bulk load, then an
#     explicit refresh and forcemerge(1) on the read-only corpus.
#
# Usage: bash scripts/issue61/provision_es.sh [wands]
set -euo pipefail

DATASET="${1:-wands}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
# shellcheck source=../../benchmarks/configs/issue61/container_limits.env
source "$REPO_ROOT/benchmarks/configs/issue61/container_limits.env"

if [[ "$DATASET" != "wands" ]]; then
  echo "FATAL: the ES time-box covers wands only (dataset='$DATASET')" >&2
  exit 2
fi
INDEX="i61_wands"
CATALOG="$REPO_ROOT/dataset_cache/wands/catalog.jsonl"
BASE="http://localhost:${I61_ES_PORT}"
EXPECTED_DOCS="$I61_WANDS_EXPECTED_DOCS"

[[ -f "$CATALOG" ]] || { echo "FATAL: catalog missing: $CATALOG" >&2; exit 2; }

echo "==> (re)starting $I61_ES_CONTAINER under the frozen limits"
docker rm -f "$I61_ES_CONTAINER" >/dev/null 2>&1 || true
docker run -d \
  --name "$I61_ES_CONTAINER" \
  --cpus="$I61_CPUS" \
  --cpuset-cpus="$I61_CPUSET" \
  --memory="$I61_MEMORY" \
  --memory-swap="$I61_MEMORY_SWAP" \
  -p "${I61_ES_PORT}:9200" \
  -e discovery.type=single-node \
  -e xpack.security.enabled=false \
  -e xpack.security.http.ssl.enabled=false \
  -e ES_JAVA_OPTS="-Xms${I61_ES_HEAP} -Xmx${I61_ES_HEAP}" \
  "$I61_ES_IMAGE" >/dev/null

echo "==> waiting for the cluster to accept requests (max 180s)"
READY=0
for _ in $(seq 1 180); do
  if curl -sf "$BASE/_cluster/health" >/dev/null 2>&1; then READY=1; break; fi
  sleep 1
done
if [[ "$READY" -ne 1 ]]; then
  echo "ES_TIMEBOX_RESULT=INFRA_FAILED reason=cluster_never_ready" >&2
  docker logs --tail 30 "$I61_ES_CONTAINER" 2>&1 | tail -30 >&2
  exit 4
fi
echo "  health: $(curl -sf "$BASE/_cluster/health" | python3 -c 'import json,sys;d=json.load(sys.stdin);print(d["status"],d["number_of_nodes"],"nodes")')"

echo "==> creating index with a competent commerce mapping"
curl -sf -X DELETE "$BASE/$INDEX" >/dev/null 2>&1 || true
curl -sf -X PUT -H 'Content-Type: application/json' --data-binary '{
  "settings": {
    "number_of_shards": 1,
    "number_of_replicas": 0,
    "refresh_interval": "-1",
    "analysis": {
      "normalizer": {
        "i61_lowercase": { "type": "custom", "filter": ["lowercase"] }
      }
    }
  },
  "mappings": {
    "properties": {
      "title":         { "type": "text" },
      "description":   { "type": "text" },
      "product_class": { "type": "keyword", "copy_to": "product_class_lc" },
      "category_leaf": { "type": "keyword", "copy_to": "category_leaf_lc" },
      "product_class_lc": { "type": "keyword", "normalizer": "i61_lowercase" },
      "category_leaf_lc": { "type": "keyword", "normalizer": "i61_lowercase" },
      "color":           { "type": "keyword", "copy_to": "color_lc" },
      "color_lc":        { "type": "keyword", "normalizer": "i61_lowercase" },
      "style":           { "type": "keyword" },
      "material":        { "type": "keyword" },
      "primarymaterial": { "type": "keyword" },
      "shape":           { "type": "keyword" },
      "average_rating":  { "type": "double" },
      "rating_count":    { "type": "double" },
      "review_count":    { "type": "double" }
    }
  }
}' "$BASE/$INDEX" >/dev/null

echo "==> bulk indexing $DATASET"
python3 - "$CATALOG" "$BASE/$INDEX" <<'PY'
import json, sys, urllib.request

catalog, index_url = sys.argv[1], sys.argv[2]
KEEP = {"title","description","product_class","category_leaf","color","style",
        "material","primarymaterial","shape","average_rating","rating_count","review_count"}
BATCH = 2000

def flush(lines, n):
    if not lines:
        return n
    req = urllib.request.Request(
        index_url + "/_bulk", data="".join(lines).encode(),
        headers={"Content-Type": "application/x-ndjson"}, method="POST")
    body = json.load(urllib.request.urlopen(req))
    if body.get("errors"):
        first = next(i for i in body["items"] if "error" in i.get("index", {}))
        raise SystemExit(f"bulk error: {first}")
    return n + len(lines) // 2

lines, total = [], 0
with open(catalog) as f:
    for line in f:
        rec = json.loads(line)
        doc = {k: v for k, v in rec.items() if k in KEEP and v not in (None, "")}
        lines.append(json.dumps({"index": {"_id": rec["id"]}}) + "\n")
        lines.append(json.dumps(doc) + "\n")
        if len(lines) >= BATCH * 2:
            total = flush(lines, total); lines = []
total = flush(lines, total)
print(f"  bulk submitted {total} docs")
PY

echo "==> refresh + forcemerge(1)"
curl -sf -X POST "$BASE/$INDEX/_refresh" >/dev/null
curl -sf -X POST "$BASE/$INDEX/_forcemerge?max_num_segments=1" >/dev/null

COUNT="$(curl -sf "$BASE/$INDEX/_count" | python3 -c 'import json,sys;print(json.load(sys.stdin)["count"])')"
echo "==> count=$COUNT (expected $EXPECTED_DOCS)"
if [[ "$COUNT" != "$EXPECTED_DOCS" ]]; then
  echo "ES_TIMEBOX_RESULT=INFRA_FAILED reason=corpus_parity count=$COUNT" >&2
  exit 5
fi

# Prove the lowercased companion actually resolves an exact term filter -- the
# same semantics-preservation check the Solr side had to pass before its
# numbers could be trusted.
HITS="$(curl -sf -X POST -H 'Content-Type: application/json' --data-binary '{
  "size": 0, "query": { "bool": { "filter": [ { "term": { "product_class_lc": "beds" } } ] } }
}' "$BASE/$INDEX/_search" | python3 -c 'import json,sys;print(json.load(sys.stdin)["hits"]["total"]["value"])')"
echo "==> exact-term filter product_class_lc=beds -> $HITS hits"

STORE="$(curl -sf "$BASE/_cat/indices/$INDEX?bytes=b&h=store.size" | tr -d ' \n')"
echo "==> store_size_bytes=$STORE"
echo "ES_TIMEBOX_RESULT=INFRA_OK docs=$COUNT beds_hits=$HITS store_bytes=$STORE"
