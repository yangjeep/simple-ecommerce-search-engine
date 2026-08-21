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

## P6D-E01: does the margin hold, narrow, or grow at Phase 6B's own controlled-stress scale ladder? (hypothesis stated before implementation)

**Falsifiable hypothesis**: P6D-E00 measured only WANDS' natural 1x
scale (2,002-16,039 candidates across the 7 real checkpoints) -- named
explicitly as an unresolved risk. Phase 6B built a controlled-stress
scale ladder (`scripts/datasets/replicate_wands_scale.py`, 2x/5x/10x/20x
catalog replication, holding facet cardinality and per-candidate
attribute complexity fixed while scaling only candidate-set size) to
test exactly this kind of question for the scan-based crossover, and the
replicated catalogs and matching Solr cores (`wands_bench_2x/_5x/_10x/
_20x`) already exist in this environment. **H (P6D-E01)**: the ordinal
method's advantage over Solr will hold (remain faster) across the full
1x-20x range, since the mechanism removed (a per-candidate attribute-map
clone) does not depend on catalog scale. Falsifiable: the margin could
narrow, disappear, or reverse at larger candidate counts if the ordinal
method's own per-candidate cost (array bounds-checked access,
`RoaringBitmap` iteration) turns out to scale worse than Solr's own
`facet.field` implementation does.

**Design**: extended `crates/phase6a-eval/src/bin/p6b_e00_scale_ladder.rs`
(Phase 6B's own scale-ladder binary) with the identical
`facet_counts_ordinal` row P6D-E00 added to `p6a_e00`, reusing the same
live Solr measurement for both native comparisons at each tier. Run once
per tier (matching this binary's own established one-run-per-tier
convention, `results.csv` append pattern -- not the 3-repeated-full-binary
convention P6A-E00/P6C/P6D-E00 used), across all 5 tiers: 1x (real
WANDS, 42,994 products), 2x, 5x, 10x, 20x (replicated, up to 859,880
products), against the matching Solr core for each tier.

**Correctness gate, checked before any timing claim**: every tier's own
built-in correctness check (`counts_match` between native and Solr
filter/facet counts) passed 17/17 rows at every one of the 5 tiers
(85/85 total), and 0 `FACET MISMATCH` lines (top-50-facet exact matches
against Solr) at any of the 35 checkpoint x tier combinations (7
checkpoints x 5 tiers) for either the scan or the ordinal method.

## P6D-E01 result: CONFIRMED across the entire 1x-20x range -- the ordinal method never loses to Solr, but the margin narrows (not grows) at the largest candidate counts

Raw data: `docs/research/artifacts/p6d_e01_scale_ladder_run1/` (5
console logs, one combined CSV).

**The ordinal method beats Solr at every one of the 35 checkpoint x tier
combinations tested, from WANDS' real 1x scale up through 20x
controlled-stress replication (candidate counts from 2,002 to
320,780) -- zero exceptions anywhere in the whole ladder.** Margins
range from 2.5x (Furniture, the largest checkpoint, at both the 10x and
20x tiers) up to 72.6x (Lighting, the smallest-cardinality checkpoint,
at the natural 1x scale).

**But the margin is not scale-invariant, and does not grow with scale --
it narrows, converging toward a floor around 2.5x-3x at the largest
candidate counts tested, not diverging further.** Representative
checkpoint (Furniture, the largest real category):

| Tier | Candidates | Native ordinal (ms) | Solr (ms) | Ordinal vs. Solr |
|---|---|---|---|---|
| 1x | 16,039 | 0.240 | 1.245 | 5.2x |
| 2x | 32,078 | 0.338 | 1.496 | 4.4x |
| 5x | 80,195 | 0.696 | 2.334 | 3.4x |
| 10x | 160,390 | 1.273 | 3.237 | 2.5x |
| 20x | 320,780 | 2.297 | 5.639 | 2.5x |

The same narrowing pattern holds at every one of the other 6
checkpoints too (e.g. Storage & Organization: 30.9x at 1x -> 32.3x at 2x
-> 24.1x at 5x -> 15.8x at 10x -> 8.8x at 20x). **Both methods' absolute
latency grows with candidate count** (Solr's own `facet.field` is not
scale-invariant either -- its own p50 at Furniture grows from 1.245ms at
1x to 5.639ms at 20x), but the ordinal method's growth rate is slightly
faster proportionally, so the ratio between them narrows as candidate
count grows, converging rather than diverging. This is a real,
disclosed nuance, not a reason to doubt the finding: the ordinal method
remains meaningfully faster than Solr at every scale tested, just by a
smaller multiple at the largest scales than at small-to-medium ones.

**The ordinal method's advantage over commerce-native's OWN scan method,
by contrast, grows sharply with scale, not narrows** -- from
20.6x-99.6x at the natural 1x scale up to 118.2x-327.0x at the 10x/20x
tiers. This is the mirror image of the Solr comparison and is
mechanistically consistent with the working hypothesis: the scan
method's per-candidate `BTreeMap` clone gets relatively more expensive
as candidate count grows (allocator pressure, cache effects), while the
ordinal method's flat-array access scales close to linearly -- so the
scan method's own cost grows faster than either the ordinal method's or
Solr's, explaining both directions of the trend at once (ordinal
narrows its lead over Solr somewhat, but widens its lead over the scan
method dramatically).

**This directly answers P6D-E00's own named unresolved risk #2**: the
margin does hold (never crosses over into a loss) across the entire
tested range, but it is not correct to assume the margin grows or stays
constant with scale -- it narrows at the largest candidate counts,
converging toward roughly 2.5x-3x rather than an ever-widening
advantage. Both the "does it hold" question and the more precise "how
does it change with scale" question are now answered with real evidence
rather than left as an open risk.

**Named limitations**: single-run-per-tier (matching this binary's own
established convention), not the 3-repeated-full-binary-invocation
convention other P6A/P6C/P6D-E00 measurements used -- run-to-run
variance at each tier is therefore not independently characterized here
(though the monotonic, checkpoint-consistent narrowing pattern across 5
independent tiers, each tier itself a fresh process invocation, is
itself a form of reproducibility evidence). Only `color` facet tested
(same scope as P6D-E00). The scale ladder replicates the real WANDS
catalog holding facet cardinality fixed (Phase 6B's own deliberate,
disclosed choice) -- a real, organically-larger catalog with
correspondingly more distinct facet values was not tested, so whether
the same narrowing pattern holds under organic (not just candidate-count)
growth is untested. 320,780 candidates (the largest tested) is still a
single-digit-million-scale candidate set, not the 10M+-candidate range
some real large catalogs might reach -- whether the narrowing trend
continues, plateaus, or reverses beyond this range is untested.

## P6D-E02: does the ordinal technique still help when the naive baseline never had the expensive part? (adversarial test, hypothesis stated before implementation)

**Why this test exists.** P6D-E00/E01 measured only the generic `Enum`
`color` attribute, where `facet_counts_by_scan`'s naive baseline paid a
genuinely expensive per-candidate cost: a full `BTreeMap<String,
AttributeValue>` clone via `effective_attributes`. The three dedicated
typed-ID facets (`brand`, `category`, `product_type`) have their own
`_by_scan` siblings that were **never that expensive** -- they read
`product.brand`/`product.category`/`product.product_type` directly via
an `O(1)` reference lookup (`lookup_product`), no attribute-map clone,
no string hashing (the key is already a `Copy` `u32` newtype). Before
extending the "ordinal counting wins" narrative to these fields too,
this experiment adversarially asks: does the ordinal technique still
help when the specific inefficiency it removes was never present in the
baseline, or could it actually be *slower*, given it has its own real
fixed cost (allocating and zeroing a `Vec<u64>` counter array sized to
the full attribute dictionary on every call) that the already-cheap
scan never paid?

**Falsifiable hypothesis (H, P6D-E02)**: `product_type_facet_counts_ordinal`
will show a **real crossover of its own** against
`product_type_facet_counts_by_scan` -- slower at small candidate counts
(where the fixed dictionary-sized array allocation dominates) and
faster only past some real candidate-count threshold (where per-candidate
savings, still real even without a clone to remove, start to amortize
that fixed cost). Falsifiable both ways: the ordinal method could
instead win uniformly (if array allocation is cheap enough to be
negligible even at n=2) or lose uniformly (if per-candidate savings
without a clone-removal are too small to ever amortize the fixed cost
at realistic candidate counts).

**Design**: added `brand_facet_counts_ordinal`/`category_facet_counts_ordinal`/
`product_type_facet_counts_ordinal` to `CatalogIndex`, using the
identical dictionary + flat-column technique as `facet_counts_ordinal`,
but simpler: every variant always has exactly one brand/category/
product_type (no "missing value" case), so each column is built with a
single in-order `push` per variant, no raw-then-finalize pass needed. A
shared `ordinal_for` free function (not a `&mut self` method, to keep
dictionary/reverse-map field borrows disjoint) does the get-or-assign
dictionary lookup for all three fields plus the existing `color` case.
Extended `p6a_e00_wands_vs_native_eval.rs`'s existing
`product_class_facet_under_category_filter` operation (product_type
faceting under a category-leaf filter, at 8 real leaf-category groups
spanning WANDS' natural tiny-to-large size distribution: n=2 to
n=1,103) with a parallel ordinal row, reusing the same live Solr
measurement. 3 independent full-binary runs.

**Correctness gate**: a new unit test
(`brand_category_product_type_facet_counts_ordinal_match_scan_exactly`)
asserts exact match against the three `_by_scan` methods across full/
filtered/empty inputs. The real-data benchmark's own mismatch check
(ordinal vs. Solr) reproduced the exact same 2 pre-existing, already-
documented benign `product_class` empty-string-bucket mismatches every
prior P6A-E00/P6C/P6D measurement has shown (byte-for-byte identical
output to the existing `_by_scan` method, confirmed by direct
comparison) -- not a new problem, and expected, since the ordinal method
is designed to exactly replicate `_by_scan`'s own output.

## P6D-E02 result: CONFIRMED -- a real crossover exists, and the earlier "ordinal always wins" framing needed this qualifier

Raw data: `docs/research/artifacts/p6d_e02_typed_facet_ordinal_run1/`
(3 console logs, 3 CSVs).

**Medians across 3 runs, product_type ("product_class") facet under a
category-leaf filter, at WANDS' natural leaf-category size distribution**:

| Candidates | Scan (ms) | Ordinal (ms) | Solr (ms) | Ordinal vs. scan | Ordinal vs. Solr |
|---|---|---|---|---|---|
| 2 | 0.00013 | 0.00070 | 0.659 | 0.19x (**5.2x slower**) | 944x faster |
| 2 | 0.00028 | 0.00129 | 0.699 | 0.21x (**4.8x slower**) | 542x faster |
| 2 | 0.00014 | 0.00073 | 0.524 | 0.19x (**5.2x slower**) | 716x faster |
| 13 | 0.00038 | 0.00080 | 0.664 | 0.47x (**2.1x slower**) | 830x faster |
| 13 | 0.00040 | 0.00075 | 0.571 | 0.53x (**1.9x slower**) | 766x faster |
| 13 | 0.00038 | 0.00091 | 0.520 | 0.41x (**2.4x slower**) | 574x faster |
| 121 | 0.00302 | 0.00129 | 0.564 | **2.34x faster** | 438x faster |
| 1,103 | 0.03008 | 0.00485 | 0.691 | **6.20x faster** | 142x faster |

**A real crossover exists, exactly as hypothesized: the ordinal method
is SLOWER than the existing scan method at small candidate counts
(n=2: 4.8x-5.2x slower; n=13: 1.9x-2.4x slower), and only becomes faster
somewhere between n=13 and n=121 (2.3x faster at n=121, 6.2x faster at
n=1,103).** This is the mechanistic confirmation the adversarial
hypothesis predicted: `product_type_facet_counts_ordinal`'s fixed cost
-- allocating and zeroing a `Vec<u64>` sized to the full `product_class`
dictionary (~860 distinct values in WANDS) on *every single call* --
genuinely dominates when the candidate set is tiny, since the already-
cheap scan (no clone, no string hash, just a `lookup_product` + typed
integer key `BTreeMap` insert) has no equivalent fixed cost to remove.
Past roughly n=100-1,000, per-candidate savings (still real: the scan's
own `BTreeMap<TypedId, u64>` insert has real `O(log n)` tree-rebalancing
cost per candidate that the ordinal method's flat-array increment
avoids) amortize the fixed cost and the ordinal method wins.

**This is a genuine, valuable qualifier to P6D-E00/E01's own framing, not
a contradiction of it.** Both findings are true and mechanistically
consistent: for the generic `Enum` `color` field, where the naive scan
paid an expensive per-candidate attribute-map clone, the ordinal method
won at every single candidate count tested (2,002-320,780) with no
crossover at all -- the fixed cost was trivial next to the clone-removal
savings even at the smallest tested scale. For the dedicated
`product_type` field, where the naive scan was already cheap, the
ordinal method has its own real, adversarially-confirmed crossover
against that already-cheap baseline. **The lesson: the ordinal
technique's win is not universal or free -- it trades a per-call fixed
cost (proportional to attribute dictionary size) for per-candidate
savings, and whether it wins depends on both the candidate count AND
how expensive the specific baseline being replaced already was.** A
production system wiring this in as a default (named as a future step
in `PHASE6D_DECISION.md`, not done in this codebase) would need this
crossover characterized per-field, or a candidate-count-adaptive choice
between strategies, not a blanket switch.

**Against Solr, the ordinal method still wins by a wide margin at every
candidate count tested (142x-944x)** -- but this specific comparison is
not very informative about facet-counting algorithms at these
particular candidate counts: Solr's own absolute cost here (0.52-0.70ms)
is consistent with being dominated by HTTP/network round-trip overhead
rather than real per-candidate facet-counting work, since candidate
counts this small (2-1,103) are trivial for any in-process method
regardless of algorithm. The meaningful vs.-Solr comparison for this
field, at candidate counts large enough to be informative (matching
color's 2,002-320,780 range), was not run in this pass -- named as a
limitation below.

**Named limitations**: only `product_type` (not `brand`/`category`)
measured in a real-data benchmark (all three have equivalent unit-test
correctness coverage, but only `product_type` had an existing Solr-
comparable operation in `p6a_e00` to extend). The candidate-count range
tested here (2-1,103) comes from WANDS' natural leaf-category size
distribution, not a deliberately-scaled sweep -- the exact crossover
point (somewhere between n=13 and n=121) is bracketed, not pinpointed.
A genuinely large-candidate-count product_type-vs-Solr comparison
(matching color's 2,002-320,780 range) was not run, so whether the
ordinal method's margin over Solr specifically narrows the way color's
did (P6D-E01) is untested for this field. Absolute timings at the
smallest candidate counts (sub-microsecond to low-microsecond) are close
to measurement-noise territory for a single sample, though the direction
was consistent across all 3 independent runs. The mechanism (dictionary-
size-proportional array allocation as the fixed cost) is inferred from
the code's own structure, not independently profiled.
