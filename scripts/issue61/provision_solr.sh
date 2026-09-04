#!/usr/bin/env bash
# Issue #61 (Infra E1): provision the frozen Solr baseline under the campaign's
# resource limits, index a dataset, and verify the corpus is the corpus prior
# evidence was measured on.
#
# Historical note, and the reason this script exists: every prior Solr
# checkpoint in this repository used a *local Java install*
# (`/home/user/solr_setup/solr-9.10.1`). Java is not installed on this host, so
# that route is gone. Docker is now the only path to a JVM engine here, which
# means the provisioning itself became reproducible infrastructure rather than
# undocumented local state.
#
# Competence bar (ISSUE61_PROTOCOL.md §2.2) -- a resource win against a
# crippled baseline is invalid, so this script must configure Solr the way a
# production ecommerce deployment would:
#   * structured attributes as `string` + docValues (exact match / facet / sort)
#   * lexical fields with the frozen native-compatible asymmetric analyzer
#   * explicit, recorded cache sizing rather than implicit defaults
#   * heap sized to leave the OS page cache for MMapDirectory
#   * forceMerge(1) on the read-only corpus
#
# Usage:
#   bash scripts/issue61/provision_solr.sh [wands|esci_electronics]
set -euo pipefail

DATASET="${1:-wands}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
# shellcheck source=../../benchmarks/configs/issue61/container_limits.env
source "$REPO_ROOT/benchmarks/configs/issue61/container_limits.env"

# Each dataset keeps its OWN indexer. This is deliberate, not duplication: the
# two corpora have genuinely different schemas (WANDS has `id` plus a
# category_depth_1..6 breadcrumb and product_class; ESCI has `product_id`,
# `brand`, `bullet_point` and carries no taxonomy at all). Reusing the existing
# per-dataset indexers also means E1 indexes each corpus exactly the way the
# prior checkpoints that produced this repository's published numbers did.
case "$DATASET" in
  wands)
    CORE="i61_wands"
    CATALOG="$REPO_ROOT/dataset_cache/wands/catalog.jsonl"
    EXPECTED_DOCS="$I61_WANDS_EXPECTED_DOCS"
    INDEXER=("$REPO_ROOT/scripts/datasets/solr_index_wands.py" "__CORE_URL__" "" "$CATALOG")
    LEXICAL_FIELDS=(title description)
    STRUCTURAL_FIELDS=(product_class category_leaf color style primarymaterial material)
    ;;
  esci_electronics)
    CORE="i61_esci_electronics"
    CATALOG="$REPO_ROOT/dataset_cache/esci_electronics/esci_electronics_products.jsonl"
    EXPECTED_DOCS="$I61_ESCI_ELECTRONICS_EXPECTED_DOCS"
    INDEXER=("$REPO_ROOT/scripts/datasets/solr_index_esci_electronics.py" "__CORE_URL__")
    LEXICAL_FIELDS=(title description bullet_point)
    STRUCTURAL_FIELDS=(brand color)
    ;;
  *)
    echo "FATAL: unknown dataset '$DATASET' (expected: wands | esci_electronics)" >&2
    exit 2
    ;;
esac

BASE="http://localhost:${I61_SOLR_PORT}/solr"
CORE_URL="$BASE/$CORE"

if [[ ! -f "$CATALOG" ]]; then
  echo "FATAL: catalog not found: $CATALOG" >&2
  echo "  run the dataset fetch/prepare scripts first (see ISSUE61_LOG.md step 1)" >&2
  exit 2
fi

# --- 1. verify the image is the digest the protocol froze ------------------
# A floating tag silently changing under us would invalidate every comparison
# made against it, so the digest is checked rather than trusted.
echo "==> verifying $I61_SOLR_IMAGE digest"
ACTUAL_DIGEST="$(docker image inspect "$I61_SOLR_IMAGE" --format '{{.Id}}' 2>/dev/null || true)"
if [[ -z "$ACTUAL_DIGEST" ]]; then
  echo "  image absent locally; pulling"
  docker pull "$I61_SOLR_IMAGE" >/dev/null
  ACTUAL_DIGEST="$(docker image inspect "$I61_SOLR_IMAGE" --format '{{.Id}}')"
fi
if [[ "$ACTUAL_DIGEST" != "$I61_SOLR_DIGEST" ]]; then
  echo "FATAL: image digest mismatch" >&2
  echo "  expected (frozen in container_limits.env): $I61_SOLR_DIGEST" >&2
  echo "  actual:                                    $ACTUAL_DIGEST" >&2
  exit 3
fi
echo "  digest OK: $ACTUAL_DIGEST"

# --- 2. start the container under the frozen limits ------------------------
echo "==> (re)starting container $I61_SOLR_CONTAINER"
docker rm -f "$I61_SOLR_CONTAINER" >/dev/null 2>&1 || true
docker run -d \
  --name "$I61_SOLR_CONTAINER" \
  --cpus="$I61_CPUS" \
  --cpuset-cpus="$I61_CPUSET" \
  --memory="$I61_MEMORY" \
  --memory-swap="$I61_MEMORY_SWAP" \
  -p "${I61_SOLR_PORT}:8983" \
  -e SOLR_HEAP="$I61_SOLR_HEAP" \
  "$I61_SOLR_IMAGE" \
  solr-precreate "$CORE" >/dev/null

echo "==> waiting for Solr to accept queries"
for _ in $(seq 1 90); do
  if curl -sf "$BASE/admin/cores?action=STATUS" >/dev/null 2>&1; then break; fi
  sleep 1
done
if ! curl -sf "$BASE/admin/cores?action=STATUS" >/dev/null 2>&1; then
  echo "FATAL: Solr did not become ready within 90s" >&2
  docker logs --tail 40 "$I61_SOLR_CONTAINER" >&2
  exit 4
fi
# solr-precreate finishes asynchronously relative to the admin endpoint.
for _ in $(seq 1 60); do
  if curl -sf "$CORE_URL/admin/ping" >/dev/null 2>&1; then break; fi
  sleep 1
done

# --- 3. explicit cache configuration (protocol §2.2, revised in Revision 2) --
# The three Solr caches are NOT equivalent for benchmarking, and treating them
# uniformly is a defect in either direction. Revision 2 sets them deliberately:
#
#   queryResultCache -> DISABLED (size 0).
#     This memoizes an entire (query, sort, filter) result list. The frozen
#     WANDS workload is 480 fixed queries and the protocol runs 3 warm-up
#     passes, so ANY cache larger than 480 entries would hold every result
#     before measurement began -- the "warm" measurement would then be a hash
#     lookup, not retrieval. The native engine has no whole-query result cache,
#     so this would not be a fair advantage, it would be a different experiment.
#
#   filterCache -> ENABLED and generously sized.
#     This caches filter-context bitsets. It is the closest Solr analogue to
#     the native engine's precomputed Roaring bitmaps, and it is how a
#     production Solr actually serves `fq`. Disabling it would be a straw man
#     in the opposite direction -- forcing Solr to rebuild structures native
#     gets for free. Hit/miss counters are published with the results.
#
#   documentCache -> ENABLED.
#     Both engines materialize result documents; caching that is symmetric.
#
# autowarmCount is 0 everywhere because warm state is defined by an explicit
# warm-up pass over the real workload (§9.1). An engine that autowarms while
# another does not is an unfair comparison.
echo "==> applying explicit cache configuration"
curl -sf -X POST -H 'Content-Type: application/json' \
  --data-binary '{
    "set-property": {
      "query.filterCache.size": 4096,
      "query.filterCache.initialSize": 4096,
      "query.filterCache.autowarmCount": 0,
      "query.queryResultCache.size": 0,
      "query.queryResultCache.initialSize": 0,
      "query.queryResultCache.autowarmCount": 0,
      "query.documentCache.size": 4096,
      "query.documentCache.initialSize": 4096
    }
  }' "$CORE_URL/config" >/dev/null

# --- 3b/4. index, install frozen field types, re-index ---------------------
# Protocol Revision 2 R2.2. The shared translator's historical output for an
# exact structured filter is a case-insensitive RegexpQuery (`field:/(?i)val/`),
# which makes Solr run an automaton over the term dictionary where a production
# deployment resolves a single term. Benchmarking against that is a straw man.
#
# `string_lc` is KeywordTokenizer + LowerCaseFilter: the whole value stays one
# token and is lowercased, so `product_class_lc:"dining chairs"` is an exact
# term lookup with the SAME case-insensitive semantics the regex had.
#
# The indexer must run BEFORE the companion fields are declared and AGAIN
# after. copyField only populates at index time and its source field must
# already exist, but Solr applies an `add-field` batch atomically -- so
# pre-creating a source field here makes the indexer's own batch fail, which
# silently leaves its other fields (title_sort) uncreated. Indexing twice
# around the schema change is the only ordering that satisfies both
# constraints without editing the historical indexer.
INDEXER=("${INDEXER[@]/__CORE_URL__/$CORE_URL}")

echo "==> indexing $DATASET from $CATALOG (pass 1: establishes base schema)"
python3 "${INDEXER[@]}"

echo "==> adding native-compatible lexical field type"
curl -sf -X POST -H 'Content-Type: application/json' --data-binary '{
  "add-field-type": {
    "name": "native_lexical",
    "class": "solr.TextField",
    "indexAnalyzer": {
      "tokenizer": {
        "class": "solr.PatternTokenizerFactory",
        "pattern": "[^\\p{L}\\p{N}]+"
      },
      "filters": [ { "class": "solr.LowerCaseFilterFactory" } ]
    },
    "queryAnalyzer": {
      "tokenizer": { "class": "solr.WhitespaceTokenizerFactory" },
      "filters": [ { "class": "solr.LowerCaseFilterFactory" } ]
    }
  }
}' "$CORE_URL/schema" >/dev/null

echo "==> replacing lexical fields: ${LEXICAL_FIELDS[*]}"
for f in "${LEXICAL_FIELDS[@]}"; do
  curl -sf -X POST -H 'Content-Type: application/json' --data-binary "{
    \"replace-field\": {\"name\":\"$f\",\"type\":\"native_lexical\",\"indexed\":true,\"stored\":true}
  }" "$CORE_URL/schema" >/dev/null
done

echo "==> adding lowercased companion fields: ${STRUCTURAL_FIELDS[*]}"
curl -sf -X POST -H 'Content-Type: application/json' --data-binary '{
  "add-field-type": {
    "name": "string_lc",
    "class": "solr.TextField",
    "omitNorms": true,
    "analyzer": {
      "tokenizer": { "class": "solr.KeywordTokenizerFactory" },
      "filters": [ { "class": "solr.LowerCaseFilterFactory" } ]
    }
  }
}' "$CORE_URL/schema" >/dev/null

if [[ "$DATASET" == "wands" ]]; then
  curl -sf -X POST -H 'Content-Type: application/json' --data-binary '{
    "add-field-type": {
      "name": "first_pipe_segment_lc",
      "class": "solr.TextField",
      "omitNorms": true,
      "analyzer": {
        "charFilters": [
          {
            "class": "solr.PatternReplaceCharFilterFactory",
            "pattern": "\\|.*$",
            "replacement": ""
          }
        ],
        "tokenizer": {"class": "solr.KeywordTokenizerFactory"},
        "filters": [ { "class": "solr.LowerCaseFilterFactory" } ]
      }
    }
  }' "$CORE_URL/schema" >/dev/null
fi

for f in "${STRUCTURAL_FIELDS[@]}"; do
  field_type="string_lc"
  if [[ "$DATASET" == "wands" && "$f" == "product_class" ]]; then
    field_type="first_pipe_segment_lc"
  fi
  curl -sf -X POST -H 'Content-Type: application/json' --data-binary "{
    \"add-field\": {\"name\":\"${f}_lc\",\"type\":\"$field_type\",\"indexed\":true,\"stored\":false,\"multiValued\":false}
  }" "$CORE_URL/schema" >/dev/null
  curl -sf -X POST -H 'Content-Type: application/json' --data-binary "{
    \"add-copy-field\": {\"source\":\"$f\",\"dest\":\"${f}_lc\"}
  }" "$CORE_URL/schema" >/dev/null
done

echo "==> re-indexing so copyField populates the companion fields"
python3 "${INDEXER[@]}"

# --- 5. forceMerge(1) on the read-only corpus -------------------------------
# Standard read-only-benchmark practice. This FAVOURS Lucene (one segment, no
# background merges during measurement); the direction is conservative against
# the native engine and is retained deliberately (protocol §2.2).
echo "==> forceMerge(1)"
curl -sf "$CORE_URL/update?optimize=true&maxSegments=1&waitSearcher=true" >/dev/null

# --- 6. corpus parity (protocol §13 stop condition) -------------------------
NUM_FOUND="$(curl -sf "$CORE_URL/select?q=*:*&rows=0&wt=json" \
  | python3 -c 'import json,sys; print(json.load(sys.stdin)["response"]["numFound"])')"
echo "==> numFound=$NUM_FOUND (expected $EXPECTED_DOCS)"
if [[ "$NUM_FOUND" != "$EXPECTED_DOCS" ]]; then
  echo "FATAL: corpus parity failed -- this is not the corpus prior evidence used" >&2
  exit 5
fi

# --- 7. exact schema/config contract gate ------------------------------------
echo "==> validating live Solr schema/config contract"
(
  cd "$REPO_ROOT/benchmarks/configs/issue61"
  sha256sum --check solr_frozen.sha256
)
cargo run --quiet --manifest-path "$REPO_ROOT/Cargo.toml" \
  -p issue61-eval --bin i61_solr_contract -- \
  "$DATASET" "$CORE_URL" "$REPO_ROOT/benchmarks/configs/issue61"

# --- 8. record what was actually provisioned --------------------------------
INDEX_BYTES="$(docker exec "$I61_SOLR_CONTAINER" \
  du -sb "/var/solr/data/$CORE/data/index" 2>/dev/null | cut -f1 || echo 0)"
echo "==> index_bytes=$INDEX_BYTES"
echo "==> container_id=$(docker inspect --format '{{.Id}}' "$I61_SOLR_CONTAINER")"
echo "PROVISION_OK core=$CORE docs=$NUM_FOUND index_bytes=$INDEX_BYTES"
