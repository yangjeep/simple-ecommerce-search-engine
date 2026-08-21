# Phase 6D Experiment Log (Issue #21 Phase 6, extending 6A/6B/6C: ordinal-based facet counting for commerce-native)

## Why this phase exists

P6C-E01 (`docs/experiments/PHASE6C_LOG.md`) found that Lucene's own
dedicated, ordinal-based facet module (`SortedSetDocValuesFacetCounts`)
closes most of the facet crossover this project has repeatedly measured
against Solr (Phase 5, 6A, 6B, P6C-E00): it beats Solr in 5 of 7 real
checkpoints, trailing by only 1.11x-1.30x in the other 2 -- reversing the
naive-scan-based P6C-E00 finding that raw Lucene loses to Solr by
3.3x-4.0x. That result explicitly named the single highest-value
newly-enabled question: **could commerce-native's own facet counting
adopt the same ordinal-based technique, and would it close its own
crossover the same way?** This phase answers that question directly, by
implementing the technique in `commerce-core` itself rather than only in
a comparison harness.

## P6D-E00: ordinal/dictionary-based facet counting on `CatalogIndex` (hypothesis stated before implementation)

**Falsifiable hypothesis**: `facet_counts_by_scan` (`crates/commerce-core/
src/index/mod.rs`) is the naive per-candidate scan this project has
repeatedly found losing to Solr past a real cardinality threshold
(Phase 5's ESCI ~9,000-12,000 candidates, Phase 6A's WANDS ~2,072-2,175,
Phase 6B's scale-ladder confirmation, P6C-E00's Lucene-scan analog). Its
per-candidate cost is architecturally expensive for reasons independent
of the *counting* logic itself: for every surviving ordinal it (a)
re-derives `effective_attributes(product, variant)`, which clones the
product's entire `BTreeMap<String, AttributeValue>` and overlays variant
attributes (`domain/catalog.rs`), a fresh heap allocation per candidate;
(b) looks up the attribute by `&str` key in that fresh map; (c) clones
the resulting `String` value into a `BTreeMap<String, u64>` `entry()`/
`or_insert(0)` call, another potential allocation. **H (P6D-E00)**:
replacing this with an ordinal-encoded design -- a per-attribute
dictionary mapping each distinct value to a dense `u32` (built once, at
`CatalogIndex::build` time), plus a `Vec<u32>` column (one entry per
variant ordinal) recording each variant's value-ordinal -- will
materially reduce facet-counting latency, because counting becomes a
flat array increment per candidate with no per-candidate map clone,
string hash, or string clone. Falsifiable: the new method could show
similar or worse latency if array-bounds-checked indexing plus
`RoaringBitmap` iteration overhead turns out to dominate, or if the
one-time dictionary-build cost (deferred to index-build time, not
measured here) turns out to matter more than the per-query savings.

**Design**: added three new `CatalogIndex` fields
(`enum_dictionary: HashMap<String, Vec<String>>`,
`enum_value_ordinal: HashMap<String, HashMap<String, u32>>`,
`enum_columns: HashMap<String, Vec<u32>>`) and a new
`facet_counts_ordinal(&self, candidates: &RoaringBitmap, attribute: &str)
-> BTreeMap<String, u64>` method, built and populated entirely inside
`CatalogIndex::build`/`index_attributes`/`index_enum_value` (no other
file in `commerce-core` needed to change -- `CatalogIndex` is already
treated as build-once/immutable everywhere, so there is no incremental-
update path to worry about breaking). Deliberately built only from
`AttributeValue::Enum`, not `MultiEnum`, to exactly match
`facet_counts_by_scan`'s own existing semantics (which also only reads
`AttributeValue::Enum`, silently skipping `MultiEnum` -- a pre-existing
asymmetry in this codebase, not something this phase introduces).

**Correctness gate, checked before any timing claim was trusted**: a new
test, `facet_counts_ordinal_matches_facet_counts_by_scan_exactly`
(`crates/commerce-core/tests/physical_index.rs`), asserts exact
`BTreeMap<String, u64>` equality between `facet_counts_ordinal` and
`facet_counts_by_scan` across the full candidate set, a filtered subset,
the empty set, and a never-indexed attribute name (must return empty,
not panic) -- mirroring exactly the discipline the pre-existing
`facet_counts_by_scan_matches_facet_counts_exactly` test already applies
to the scan-vs-map comparison. All pass. Additionally, the real-data
benchmark below (P6D-E00's own measurement) cross-checks the new
method's top-50 output against Solr's own live response for every one of
the 7 real WANDS checkpoints, in all 3 repeated runs (21 checks total,
0 mismatches) -- the same correctness-before-speed pattern every prior
phase in this campaign has used.

**Benchmark**: extended `crates/phase6a-eval/src/bin/
p6a_e00_wands_vs_native_eval.rs`'s existing `color_facet_under_
depth1_crossover_sweep` (the same operation P6A-E00/P6B-E00/P6C-E01 all
measured) with a parallel `color_facet_ordinal_under_depth1_crossover_
sweep` row at the same 7 real `category_depth_1` checkpoints, reusing
the exact same live Solr measurement (`solr_ns`/`solr_facets`) captured
for the scan row rather than re-querying Solr a second time, so both
native strategies are compared against one identical Solr measurement in
the same run. Same `WARMUP=5`/`REPS=30` convention as every prior P6A/
P6B/P6C measurement; whole binary invoked 3 independent times.

## P6D-E00 result: CONFIRMED, and far more dramatically than P6C-E01's own Lucene-module finding

Raw data: `docs/research/artifacts/p6d_e00_ordinal_facet_run1/` (3 full
console logs and CSVs).

**Correctness**: 0 mismatches for `color_facet_ordinal_under_depth1=*`
across all 7 checkpoints in all 3 runs (21/21 exact top-50-facet matches
against the live Solr `wands_bench` core). The only `FACET MISMATCH`
lines present in any run are the same 2, already-documented, benign
`product_class_under_category` empty-string-bucket convention
differences this project's own binary has reported since P6A-E00 --
unrelated to this experiment's own facet field (`color`) or method.

**Three-way comparison (medians across 3 runs), color facet-scan under category filter**:

| Checkpoint | Candidates | Native scan p50 (ms) | Native ordinal p50 (ms) | Solr p50 (ms) | Ordinal vs. scan | Ordinal vs. Solr |
|---|---|---|---|---|---|---|
| Rugs | 2,002 | 1.268 | 0.023 | 1.113 | 55.1x faster | 48.4x faster |
| Storage & Organization | 2,175 | 1.563 | 0.040 | 1.132 | 39.1x faster | 28.3x faster |
| Lighting | 2,072 | 1.340 | 0.015 | 1.047 | 89.3x faster | 69.8x faster |
| Outdoor | 3,394 | 2.668 | 0.077 | 1.167 | 34.6x faster | 15.2x faster |
| Décor & Pillows | 4,612 | 4.672 | 0.199 | 1.191 | 23.5x faster | 6.0x faster |
| Home Improvement | 4,686 | 3.891 | 0.083 | 1.127 | 46.9x faster | 13.6x faster |
| Furniture | 16,039 | 18.159 | 0.238 | 1.233 | 76.3x faster | 5.2x faster |

**The ordinal-based method is faster than Solr at every single one of
the 7 real checkpoints -- by 5.2x to 69.8x, with no exceptions** (unlike
P6C-E01's Lucene-module finding, which still trailed Solr at the 2
largest checkpoints). It is also faster than commerce-native's own
existing `facet_counts_by_scan` at every checkpoint, by 23.5x-89.3x. This
is a substantially larger and more consistent win than P6C-E01 found for
Lucene's own facet module (which beat Solr by up to 3.0x in 5/7
checkpoints and still lost by 1.11x-1.30x in the remaining 2).

**Verified not a measurement artifact before trusting this**: (1) the
correctness gate above rules out the result being an accidentally-empty
return value (Solr's own facets are confirmed non-empty at every
checkpoint, so an early-return bug would have failed the exact-match
check, not silently produced a fast-but-wrong answer); (2) `time_reps`
returns each call's actual result and the caller uses it (for the
correctness check and `top_n`), so the Rust compiler cannot legally
dead-code-eliminate the measured work; (3) the scan-side baseline
numbers here (1.27-18.16ms) closely reproduce P6C-E01's own
independently-collected same-session native numbers (1.22-18.93ms) from
an entirely separate session, confirming the measurement harness itself
is stable and this run is not an outlier.

**Why the margin is so much larger than Lucene's own module's margin
over Solr**: `facet_counts_by_scan`'s per-candidate cost includes a full
`BTreeMap<String, AttributeValue>` clone (`effective_attributes`) on
every iteration -- a heap allocation-heavy operation Lucene's own naive
scan (P6C-E00's hand-rolled `facetScan()`, a plain `SortedDocValues`
ordinal lookup with no map-clone equivalent) never paid. commerce-
native's *naive* baseline was therefore carrying a larger, more easily
removable inefficiency than Lucene's naive baseline was, so removing it
via the ordinal design produced a proportionally larger win. This is a
disclosed, mechanistic explanation, not independently profiled (no
JFR/perf/valgrind run to confirm the BTreeMap-clone cost specifically).

**Named limitations**: only one facet field (`color`) was measured in
this same-session three-way comparison; `product_class` and other Enum
fields are architecturally identical but untested here. Only WANDS at
its natural 1x scale was tested -- the Phase 6B scale-ladder (2x-20x)
was not repeated for the ordinal method, so whether the margin holds,
narrows, or grows at larger candidate-set sizes is untested (though the
mechanism -- removing per-candidate allocation -- gives no reason to
expect it to narrow). The additional per-attribute memory cost of
`enum_dictionary`/`enum_value_ordinal`/`enum_columns` was not measured
with a dedicated RSS benchmark (Phase 7's own established methodology);
a rough estimate is a `Vec<u32>` column sized to the total variant count
per faceted attribute (~172 KB for WANDS' 42,994 variants at 2 faceted
attributes), plus a small dictionary/reverse-map proportional to
distinct-value count -- cheap relative to Phase 7's own measured
per-tenant memory costs, but not independently confirmed. `MultiEnum`
attributes are not supported by the ordinal path at all (by design, to
match `facet_counts_by_scan`'s existing scope) -- a real, disclosed
scope limitation, not a bug. No profiling confirms the specific
mechanism (BTreeMap-clone removal) named above. This result is measured
against Solr's `facet.field` (the same baseline every prior phase used),
not against Lucene's own facet module directly -- a genuine three-way
native-scan/native-ordinal/Lucene-module comparison was not run in this
pass (P6C-E01's Lucene numbers come from a separate binary and session;
referenced for context, not re-measured here).
