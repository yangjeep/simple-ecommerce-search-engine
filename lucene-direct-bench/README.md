# lucene-direct-bench

A standalone Java/Maven module, not a Rust crate — the first
non-Solr, non-native engine implementation this project has built for
a benchmark. See `PHASE6C_DECISION.md` and
`docs/experiments/PHASE6C_LOG.md` for why this exists: every prior
cross-engine comparison in this project (Phase 2 through 6B) measured
commerce-native against Solr, and only Solr. This isolates raw Apache
Lucene's own retrieval/faceting cost from Solr's HTTP/schema/facet-
wrapper layer, using Maven Central (reachable in this environment, unlike
every packaged distribution of Elasticsearch, OpenSearch, or Havenask).

Facet counting is measured two ways: a hand-rolled, per-candidate
DocValues scan (P6C-E00, `facetScan()`), and Lucene's own dedicated
`lucene-facet` module (P6C-E01, `facetModuleCount()`, using
`SortedSetDocValuesFacetField`/`SortedSetDocValuesFacetCounts`) — an
adversarial self-check of P6C-E00's own finding that substantially
revised it (Solr beats the naive scan, but loses to Lucene's own
specialized module in most checkpoints). See `PHASE6C_LOG.md`'s
P6C-E01 section for the full result.

## Why Java, not Rust

Lucene is an embedded library with no HTTP interface (unlike Solr,
which every other cross-engine binary in this repo reaches via
`ureq`/HTTP), and no mature, widely-used Rust binding exists. A small
standalone Java program is the direct equivalent of how Solr itself is
invoked — a separate process reached via its own protocol — not
embedded in `commerce-core`.

## Build and run

```bash
cd lucene-direct-bench
mvn -q package
java -jar target/lucene-direct-bench-jar-with-dependencies.jar \
  ../dataset_cache/wands/catalog.jsonl \
  http://localhost:8983/solr/wands_bench
```

Requires the real WANDS catalog (`scripts/datasets/fetch_wands.sh` +
`prepare_wands.py`) and a running Solr instance with the `wands_bench`
core built (`scripts/datasets/solr_index_wands.py`) for the live
correctness cross-check — the benchmark refuses to trust any timing
until every filter/range count matches Solr's own live count exactly.

## Why `NIOFSDirectory`, not `MMapDirectory`

Lucene's recommended default (`FSDirectory.open()`, which auto-selects
`MMapDirectory` on capable platforms) throws `LinkageError:
MemorySegmentIndexInputProvider is missing in Lucene JAR file` when run
from this module's Maven-Assembly-produced uber-jar — the assembly
plugin does not preserve Lucene's multi-release-JAR structure its
Panama/mmap implementation needs. `NIOFSDirectory` is a standard,
fully-supported, non-mmap Lucene backend that sidesteps this cleanly; a
real, disclosed methodology choice (see `PHASE6C_DECISION.md`'s
"Unresolved risks"), not a silent workaround.
