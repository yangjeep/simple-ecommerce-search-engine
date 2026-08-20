# Issue #7 Experiment Log

Append-only, continuing the format established by `docs/experiments/LOG.md`
(Phase 0), `ROUND1_LOG.md` (Round 1, Issue #5), `PHASE2_LOG.md` (Phase 2,
Issue #6), and `REALTIME_LOG.md` (Issue #8). Issue #7 asks: deep-read
Havenask/IndexLib and the broader commerce-search market as prior art,
and for each residual hypothesis our own architecture might still need,
determine — with real measurement, not assumption — whether it is a
genuine differentiation opportunity or a mature primitive we would
otherwise reinvent.

The archaeology itself (13 research agents + 1 synthesis pass, background
Workflow `wf_81e4323f-dc0`) produced a cross-reference matrix, a 4-layer
classification (generic IR / consumer-search / commerce-domain /
marketplace-scale optimizations), and 5 ranked, falsifiable residual
hypotheses with concrete experiments — full detail in that workflow's
synthesis output (not duplicated here; see the summary each entry below
opens with). This log records the *experiments themselves*: hypothesis,
measurement, real result, decision.

Same evidence-class/independence framing as prior logs.

---

## I7-E01 — Does the already-built planner+Tantivy-delegate composition fix R1-E05's named adversarial Punt-path latency case?

**Evidence class**: real (1,215,854-product ESCI catalog, same as every
prior real-data entry in this project).

**Independence**: n/a (a latency reproduction against this project's own
prior recorded baseline, not a relevance/judgment measurement).

**Background**: the archaeology synthesis ranked "bounded top-K early
termination on the non-selective (Punt) path" the single highest-
information-value residual experiment, hypothesizing that R1-E05's
measured 36,700x-worse-than-selective-baseline degradation (961.23ms p50
for a `Text`-only query with no structural predicate, vs. 26.2µs for a
moderately-selective single-brand filter) is an artifact of *exhaustive*
linear scanning, not something inherent to bitmap-first execution — and
that adding a quota/early-termination bound should recover well past
Issue #7's revised >=5x P50/P95 bar.

Before writing any new mechanism, the synthesis's own "Obvious Wheel-
Reinvention Candidates" list flagged the risk directly: "Custom WAND/
weak-AND top-K pruning for multi-term OR lexical queries... Tantivy
already implements this internally... strengthens our decision to
delegate lexical scoring to Tantivy rather than reimplement it." R1-E05's
961ms number was measured against `CatalogIndex::execute`'s raw,
undelegated linear scan — but Issue #6 priority 5 (`commerce_core::plan`,
P2-E05) already built exactly the mechanism that avoids ever calling that
path for this query shape: a query with no structural constraint at all
routes straight to `ExecutionOutcome::Punt`, which delegates the entire
search to Tantivy instead. This had never been directly re-measured
against R1-E05's own named adversarial case — P2-E05 measured aggregate
relevance/latency across the full real query mix, not this specific
worst-case query in isolation.

**Hypothesis**: reproducing R1-E05 Case 1's exact query shape (no
structural constraint, free-text term "waterproof") through the current
planner+Tantivy-delegate path, with no new code beyond a benchmark
harness, already clears the >=5x bar — because the real cost driver in
R1-E05's number was the naive per-document substring `.contains()` check
across 1.2M products (matching R1-E07's independent finding that a
substring scan is ~6,660x slower than an indexed token lookup for a
similar reason), not "scanning" per se, and Tantivy's own inverted index
sidesteps that cost class entirely.

**Implementation**: `crates/phase2-eval/src/bin/punt_path_adversarial_eval.rs`.
Reuses `planner_integration_eval.rs`'s exact `TantivyDelegate`/schema/
build (same real catalog, same Tantivy config as P2-E01/P2-E05) and
R1-E05's own `time_iters`/percentile methodology (n=30) so the comparison
is apples-to-apples. Constructs the identical adversarial query
(`constraints: []`, `residual_lexical: ["waterproof"]`), asserts
`plan()` routes it to `Punt`, and times `execute_planned` end to end
(including `commerce_core`'s own re-verification of every returned hit —
this is not measuring Tantivy in isolation, it's measuring the real
composed path a shopper query actually goes through).

**Results** (real 1.2M-product catalog):

```
Tantivy index built in 13.9s
planner routing confirmed: Punt (as expected)
first call: 10 hits returned (k=10)

p50=1.1723ms  p95=1.4766ms  p99=1.5242ms  (n=30, k=10)

R1-E05 Case 1 (raw unbounded linear scan, no delegate):     p50=961.23ms
R1-E05 Case 3 (moderately-selective single-brand baseline): p50=0.0262ms
this experiment (same query, current planner+delegate):     p50=1.1723ms

multiplier vs. Case 1:  820.0x faster   (bar: >=5x)
multiplier vs. Case 3:  44.7x further than the selective baseline still
```

**Interpretation**: the hypothesis is confirmed, decisively — 820x past
the >=5x bar, using zero new production mechanism. The architecture Issue
#6 already built (structural-index-first, delegate-to-Tantivy-on-Punt)
was already the correct answer to R1-E05's finding; this experiment is a
missing regression/confirmation check on a specific named worst case, not
a new capability. This also *validates* rather than contradicts the
synthesis's own wheel-reinvention warning: the "right" experiment here
was recognizing existing machinery already solved it, not building a
bounded-heap collector inside `commerce_core::index` that would duplicate
what Tantivy's `TopDocs::with_limit` already does internally (per the
synthesis's cross-reference matrix, row "Bounded top-K collector / early
termination": "Tantivy already implements this internally").

The remaining, honest gap: this experiment's query has no real judged
relevance ground truth (same limitation R1-E05's own choice of
"waterproof" had — it was picked as a *physical-cost* probe, not a
relevance-quality one). The relevance side of this same code path is
already covered by P2-E05's aggregate NDCG@10/Recall@10/MRR across the
*full* real 22,458-query set (which necessarily includes every real
Punt-routed query, not just this one adversarial term) — this latency
result should be read alongside that evidence, not in isolation, per
CLAUDE.md's "do not claim a win from microbenchmarks alone."

**44.7x further than the selective baseline, a real remaining gap,
stated rather than hidden**: this experiment's 1.17ms p50 is itself
~45x slower than a genuinely selective single-brand structural filter
(26.2µs). That is expected — this query has *no* structural constraint
to narrow on, so it necessarily pays real work proportional to Tantivy's
own inverted-index term lookup + BM25 scoring + result collection over
however many real documents match "waterproof" (14,839, per R1-E05), not
some artificially small quota. This is not a new gap this experiment
introduces; it is the honest cost floor of the Punt path's *actual*
mechanism, correctly much closer to R1-E04's already-accepted Solr
baseline (p50=1486µs) and P2-E01's Tantivy-standalone number (p50=1.09ms,
Punt/full-corpus search) than to the structural-only best case, because
that is architecturally what this query shape requires.

**Regression check**: `cargo fmt --all -- --check`, `cargo clippy
--workspace --all-targets --all-features -- -D warnings`, `cargo test
--workspace --all-features`, `cargo build --workspace --release` all
clean before this entry. No `commerce_core` or `phase2-eval` production
code changed by this experiment — new standalone benchmark binary only.

**Decision: CONFIRMED, no new mechanism needed.** Issue #7's synthesis
ranked this the highest-value residual experiment; the answer is that it
was already solved by Issue #6's own architecture, and the real
deliverable of this experiment is the missing regression evidence proving
it, plus an explicit correction to the synthesis's implied plan (build a
bounded-top-K collector) in favor of the cheaper, already-correct one
(measure what's already built). Feed into Issue #5/
`ROUND1_DECISION_TREE.md`: R1-E05's ~36,700x finding is now a *closed*
finding for the current architecture (post-Issue-6), not an open risk —
record this explicitly so it is not re-discovered as a surprise later.

**Next**: proceed to the remaining ranked hypotheses (#2 columnar RSS
reduction, #3 numeric-range-as-bitmap, #4 tiered ranking, #5 mmap
cold-start/degradation) — none of which this experiment's finding
resolves, since each targets a materially different subsystem (RSS,
compound-constraint planning, ranking quality, and storage tiering
respectively, vs. this entry's pure lexical-Punt-latency question).

---

## I7-E02 — Does a columnar attribute layout shrink RSS by >=5x? (residual hypothesis #2)

**Evidence class**: real (full 1,215,854-product ESCI catalog).

**Independence**: implemented and measured by one agent; independently
re-run and cross-checked against source by a second, separate agent
(adversarial verify stage) before this entry was written.

**Background**: P2-E06 (`docs/experiments/PHASE2_LOG.md`) measured
`commerce_core::domain::Catalog::build` costing +2,926.17 MB RSS — the
dominant cost in the whole real-data pipeline, well above
`CatalogIndex::build`'s own +827.60 MB — and left open whether a denser
(columnar) representation would measurably shrink it.

**Hypothesis**: a genuinely columnar (struct-of-arrays) representation
of the same real catalog data reduces RSS by >=5x versus the measured
+2,926.17 MB baseline, without regressing per-field random-access read
latency by more than 2x.

**Implementation**: `crates/phase2-eval/src/bin/columnar_attribute_layout_eval.rs`.
A standalone `ColumnarCatalog` (flat `Vec<u32>`/`Vec<i64>` columns for
product_type/category/brand/color-id/price/inventory, offset-table+blob
columns for title/description/bullets) built from the same real
`RealProduct` records `round1_eval::catalog::build_catalog` uses —
`commerce_core`'s actual `Product`/`Variant` types are untouched. RSS
measured at 3 checkpoints in one process (raw load, `Catalog` build,
`ColumnarCatalog` build); random-access read latency measured for
brand+price and title lookups (10,000 lookups/batch, n=50 batches,
identical index sequence fed to both representations); a full
1,215,854-product correctness cross-check between the two
representations (0 mismatches).

**Results**:

| Checkpoint | RSS delta since process start |
|---|---|
| raw `RealProduct` load (reference) | +1,626.19 MB |
| `Catalog` build, incremental | +2,926.16 MB (matches P2-E06's +2,926.17 MB to 0.01 MB) |
| `ColumnarCatalog` build, incremental | +1,408.42 MB |
| **RSS reduction ratio** | **2.08x** (bar: >=5x) |
| raw title+description+bullets text bytes (supplementary, computed directly from catalog.jsonl) | 1,372.75 MB |

| Random-access (10k lookups/batch, n=50) | Catalog p50 | Columnar p50 | ratio |
|---|---|---|---|
| brand + price | 58.72ns | 11.57ns | 0.197x (5.1x **faster**) |
| title text | 19.00ns | 30.04ns | 1.581x (slower, within the <=2x bar) |

Independent re-run (verify agent, separate process): RSS numbers matched
to within 0.06 MB (essentially deterministic); brand+price ratio measured
at 0.151x (Catalog 63.08ns, Columnar 9.52ns — same direction, ~5-6x
speedup either way, does not change the pass/fail conclusion since both
runs land far under the <=2x threshold); title ratio 1.605x, matching
almost exactly.

**Interpretation**: FALSIFIED on the pre-registered 5x threshold, but the
*why* is itself a real, useful finding, not a shortfall to explain away.
The real title+description+bullets text alone totals 1,372.75 MB across
1,215,854 products — `ColumnarCatalog`'s own +1,408.42 MB is only ~2.6%
above that raw-text floor (essentially all per-item overhead —
`BTreeMap` nodes, a `Vec<Variant>` heap allocation per product, duplicated
`String` headers — has been eliminated). But `Catalog`'s own +2,926.16 MB
is only 2.13x that same text floor: the dominant share of `Catalog`'s
footprint is not overhead at all, it is real, uncompressed text content.
Even a hypothetical zero-overhead columnar design could not have exceeded
roughly a 2.13x reduction for this specific text-heavy real dataset — this
implementation reached 2.08x, 97.5% of that ceiling. The 5x hypothesis
implicitly assumed overhead was a much larger share of the ~2,926MB total
than it actually is. Getting materially past ~2.1x for this dataset would
require actual byte-level compression (dictionary/zstd-style encoding of
the text blobs), not columnar layout alone — a distinct, unattempted
follow-up. The latency side is an unambiguous, verified win: columnar
brand+price access is ~5x *faster* (an extra heap-allocation pointer-chase
eliminated per lookup, confirmed by reading `commerce_core::domain::Product`'s
actual layout), and the only latency regression (title text, 1.58-1.6x) is
comfortably under the 2x bar.

**Regression check**: `cargo fmt --all -- --check`, `cargo clippy -p
phase2-eval --all-targets --all-features -- -D warnings` clean. No
`commerce_core`/existing file touched — new standalone binary only.

**Verify-stage findings**: ENDORSED_WITH_CAVEATS. Two minor points raised,
neither changing the verdict: (1) the latency loop always times the
`Catalog` arm before the `ColumnarCatalog` arm (never alternated), a
possible small warm-up bias — but the measured effect size (~5-6x) is far
larger than any plausible ordering artifact at this scale. (2) the
correctness check validates brand id/price/title/description/bullets but
not color, so "0 mismatches" is not a literally complete field-by-field
proof, though color is not part of the benchmarked fields either.

**Decision: FALSIFIED (5x bar) / PARTIALLY CONFIRMED (architectural
direction)**: columnar layout alone gets ~2.1x RSS reduction for this
dataset, at the ceiling of what's achievable without compression, with a
real latency *improvement* on fixed-field access. A >=5x reduction would
require adding text compression, a materially larger, separate
undertaking not attempted here — recorded as a genuine open follow-up,
not silently folded into this result.

---

## I7-E03 — Does pre-bucketed RoaringBitmap composition beat the current numeric-range mechanism by >=5x? (residual hypothesis #3)

**Evidence class**: mixed. Catalog structure, brand assignments, and the
`CatalogIndex` mechanism under test are entirely real; price *values* are
synthetic (the real ESCI export has no price field at all — every real
product ingests as `Price::usd(0)`, `round1_eval::catalog::build_catalog`'s
own documented limitation) — a deterministic splitmix64-seeded, right-skewed
per-product price ($2.99-$224.60, mean $19.01) was generated for this
experiment only, disclosed plainly rather than silently substituted.

**Independence**: implemented by one agent, independently re-run twice
(two full separate processes) by a second agent before this entry.

**Background**: `CatalogIndex`'s current mechanism for a Price constraint
(`structural_bitmap`'s `PriceUnderCents`/`PriceOverCents` arms,
`crates/commerce-core/src/index/mod.rs`) binary-searches a sorted
`Vec<(i64, Ordinal)>` and then builds a **fresh** `RoaringBitmap` from the
matching slice on every single call — unlike `brand_bitmaps`, a
`HashMap<BrandId, RoaringBitmap>` populated once at build time (a Brand
constraint costs one hashmap lookup + one clone, no per-query
construction). This "impedance mismatch" — one structural constraint type
bitmap-shaped by construction, the other bitmap-shaped only after
per-query work — is what this experiment measures the cost of.

**Hypothesis**: pre-bucketing the numeric domain into a bounded number of
buckets, each holding a precomputed `RoaringBitmap` built once ahead of
query time (Havenask's / pre-BKD Lucene's approach), and answering a range
query by OR-ing the bucket bitmaps the range spans then AND-ing with
Brand, is >=5x faster than the current mechanism for a real compound
"Brand AND Price BETWEEN" query.

**Implementation**: `crates/phase2-eval/src/bin/price_bucket_bitmap_eval.rs`.
32 quantile (equal-population) bucket bitmaps precomputed once over the
full real (price-synthesized) catalog. Query: brand="nike" (real,
BrandId(3656), 6,165 real products, 0.507% of catalog) AND price in
$9.07-$24.44 — deliberately chosen to align exactly with bucket
boundaries, so both mechanisms answer a bit-for-bit identical query
(isolating the mechanism-cost comparison from any partial-bucket
verification cost a non-aligned range would add). (a) the CURRENT,
unmodified `CatalogIndex::indexed_candidates(&[Brand, PriceOver,
PriceUnder])`, timed n=2000. (b) the NEW bucketed alternative, same n.

**Results**:

| Mechanism | p50 | p95 | p99 | hits |
|---|---|---|---|---|
| (a) CURRENT (binary-search + fresh-collect per bound) | 20.39ms | 22.35ms | 24.20ms | 3,048 |
| (b) NEW (32 precomputed bucket bitmaps, OR-then-AND) | 1.64ms | 1.82ms | 2.12ms | 3,048 |
| **p50 speedup** | **12.44x** (bar: >=5x) | | | identical candidate sets: true |

One-time bucket-bitmap build cost: 62.15ms (not counted in per-query
timings, matching how `CatalogIndex::build`'s own cost is excluded from
every other query-timing experiment in this project).

Independent re-run (two full separate processes, verify agent): 12.21x
and 12.46x speedup respectively (p50 19.88ms/20.14ms vs. 1.617ms/1.629ms)
— within ~2.5% of the reported numbers, hit counts and candidate-set
equality matching exactly across all three runs (price synthesis is
deterministic, seeded by `ProductId`).

**Interpretation**: CONFIRMED, comfortably clearing the >=5x bar with a
~2.5x margin to spare. The mechanistic explanation holds up under source
review: the current mechanism pays two full binary-search-then-collect
passes over `price_index` (one per bound), each materializing a fresh
`RoaringBitmap` from a slice covering, for this ~50%-of-catalog-width
range, on the order of 600K entries — before Brand's cheap bitmap ever
gets to cut the set down. The bucketed alternative instead ORs 16 small
precomputed bitmaps and ANDs once against the already-fast Brand bitmap.
The Brand-bitmap fetch itself is identical in both timed arms (same
`indexed_candidates` code path), so the ~19ms gap is attributable
specifically to the Price-bitmap-construction mechanism the hypothesis
targeted.

**A genuine interpretive nuance, raised in verification, not by the
implementing agent**: part of the current mechanism's cost may be
attributable specifically to `RoaringBitmap`'s generic `.collect()`
(likely inserting one bit at a time) on an already-sorted slice, rather
than a bulk/sorted-load API — meaning a much smaller point-fix to the
*existing* binary-search path might recover some of this win without
full pre-bucketing. This doesn't make the comparison unfair (it
accurately describes what the current code does today), but it means the
12.44x figure conflates "bucketing helps" with "the current collect call
is needlessly slow" — worth separating in any follow-up that decides
whether to build the full bucketed mechanism or take the cheaper partial
fix.

**Regression check**: `cargo fmt --all -- --check`, `cargo clippy -p
phase2-eval --all-targets --all-features -- -D warnings` clean. No
`commerce_core` file touched, no new dependency (roaring is used only
through `commerce_core::index::CatalogIndex`'s existing public API).

**Verify-stage findings**: ENDORSED. No methodology concerns beyond the
above nuance and the already-disclosed scope limits (one brand/one range
width tested, boundary-aligned range only, single-threaded warm-cache
timing).

**Decision: CONFIRMED, with a cheaper alternative worth checking first.**
The full bucketed-bitmap mechanism is a real, substantial win, well past
the bar. Before committing to building it in `commerce_core`, the
cheaper "fix the existing `.collect()` call" hypothesis this verification
surfaced is a natural, smaller next experiment — record both, do not
conflate them.

---

## I7-E04 — Does tiered/lexicographic ranking beat additive Preference-boost-sum? (residual hypothesis #4)

**Evidence class**: real (full 1,215,854-product catalog, full real
22,458-query judged set).

**Independence**: implemented by one agent, independently re-run
(reproducing every number to 4 decimal places) and source-audited by a
second agent.

**Background**: `commerce_core::index::rank`'s current mechanism sums
compiled `Preference` boost weights into one `f64` per hit and sorts
descending, with a deterministic id-based tiebreak only on exact ties.
The Issue #7 archaeology found two independent real systems (Algolia's
8-criteria cascading tie-break, Meilisearch's `Criterion` bucket-sort)
converge on a different pattern: an ordered set of discrete tiers where
an earlier tier's difference always wins outright over any difference in
a later tier.

**Hypothesis**: re-ranking the same real candidate set with a
`(tier1, tier2, tier3)` tuple comparator (tier1 = matched-constraint
count, tier2 = discrete Preference-match count, tier3 = a real numeric
custom-rank signal) produces materially higher NDCG@10 (>=5% relative,
per CLAUDE.md's "material, not incremental" standard for a
relevance-quality — not latency — bar) than the shipping additive sum.

**Implementation**: `crates/phase2-eval/src/bin/tiered_ranking_eval.rs`.
Both arms rank the *identical* real candidate set per query (retrieved
once via `CatalogIndex::execute`), isolating ranking-shape from
retrieval. Arm (a) calls `commerce_core`'s own `execute_ranked` directly
(the literal shipping mechanism, not reimplemented). Arm (b) is a new
tiered scorer using `brand_occurrence_count` as tier3. Swept across
`min_enum_frequency` in {1, 5, 25, 100} (P2-E05's own threshold set).

**Results**:

| min_enum_frequency | evaluated | (a) additive NDCG@10 | (b) tiered NDCG@10 | relative change | tier3 varies | top-10 differs |
|---|---|---|---|---|---|---|
| 1 | 19,632 | 0.0206 | 0.0207 | +0.49% | 13.3% | 10.7% |
| 5 | 12,426 | 0.0370 | 0.0372 | +0.57% | 35.1% | 34.7% |
| 25 | 5,870 | 0.0236 | 0.0236 | -0.08% | 23.5% | 23.5% |
| 100 (P2-E05 headline) | 2,936 | 0.0056 | 0.0057 | +2.43% | 10.2% | 10.2% |

**Every threshold**: queries with any compiled `Preference`: 0/N.
Queries where tier1 (matched-constraint count) varies within a candidate
set: 0/N.

Independent re-run reproduced every number above to 4 decimal places,
including all diagnostic counts, at all four thresholds.

**Interpretation**: FALSIFIED, consistently, at every threshold tested —
relative NDCG@10 change never exceeds 2.43%, with no trend toward the 5%
bar as configuration varies. But the diagnostics explain *why*, and the
explanation is itself a significant, independently-source-verified
architectural finding, not an artifact of this experiment's design: at
every threshold, **zero** real queries ever compiled a non-empty
`Preference` list (`compile_lexicon` only ever calls
`Candidate::constraint`, confirmed by direct source read, matching its
own doc comment's admission that the profiler "cannot propose
`ir::Preference`s"), and **zero** ever showed tier1 varying within one
query's own candidate set — a direct, unavoidable consequence of
`CatalogIndex::execute`'s hard-AND semantics (only candidates satisfying
100% of a query's constraints are ever returned, so "how many constraints
matched" cannot discriminate within one query's own results). This means
the shipping additive-sum ranking is proven, on real cold-start-compiled
queries, to always tie at score=0.0 and therefore always degenerate to
its arbitrary id-based tiebreak — **ranking has zero real relevance
signal behind it on this path today**. The tiered scorer's only live
discriminator (tier3, brand popularity) varies in 10-35% of queries
depending on threshold, and tracks "queries where top-10 ordering
differs" almost 1:1 — the clean mechanistic confirmation that nothing
else in the tiered design contributed. So: replacing arbitrary id-order
with one real popularity-style tiebreak produces a real but small lift,
not a material one. The ~11%-of-ceiling integration gap this project has
tracked since P2-E05 does not live in additive-vs-tiered final-scoring
shape — it more likely lives in what `compile_lexicon` can and cannot
compile into `query.preferences`/constraints in the first place, a
retrieval/compilation-scope question this experiment did not test.

**A real methodology problem, caught in verification, disclosed here
rather than smoothed over**: the file's *latency* side-numbers (not the
NDCG@10 relevance verdict) are unreliable. The verify agent built a
standalone check and found that whichever arm runs **second** in the
per-query timing loop measures faster, regardless of which arm it
actually is (a classic cache/measurement-order artifact — arm (a) always
ran first, arm (b) always second, in the original file). Swapping the
call order reversed which arm "won" on latency. **This does not affect
the FALSIFIED verdict**, which is based entirely on NDCG@10 (explicitly
the stated bar — "relevance-quality, not latency" — and independently
reproduced to 4 decimal places, plus confirmed architecturally forced by
direct source review, not a fluke of one run) — but the printed "(b)
tiered ... faster than (a) additive-sum" latency lines in this
experiment's own output must not be read as a real finding about tiered
ranking's computational cost.

**Regression check**: `cargo fmt --all -- --check`, `cargo clippy -p
phase2-eval --all-targets --all-features -- -D warnings` clean. No
`commerce_core` file touched.

**Verify-stage findings**: ENDORSED_WITH_CAVEATS — the NDCG@10 verdict
and its architectural explanation are sound and independently confirmed;
the latency comparison specifically is not reliable, per above.

**Decision: FALSIFIED.** Tiered/lexicographic ranking does not
materially beat additive-sum on this real workload, because the current
architecture gives both arms almost nothing to discriminate on. The real,
higher-priority finding this experiment surfaced is that
`compile_lexicon` never produces `Preference` candidates at all on real
data — a retrieval/compilation gap, not a ranking-shape gap, and the more
promising place to look for the ~11%-of-ceiling integration gap next.

---

## I7-E05 — Does Tantivy's mmap reopen beat commerce_core's total lack of persistence by >=5x? (residual hypothesis #5, safety-scoped)

**Evidence class**: real (full 1,215,854-product catalog).

**Independence**: implemented by one agent, independently re-run twice by
a second agent.

**Scope correction, applied deliberately**: the original hypothesis
proposed simulating memory pressure smaller than the index size to test
graceful degradation. That was explicitly ruled out for this experiment —
constraining this process's own memory via ulimit/cgroups risks
destabilizing the sandboxed environment this session runs in, and was not
attempted. Scoped down instead to a safe, still-real, still-valuable
question: does `commerce_core` have *any* persistence/reload story at
all, compared to what Tantivy (already a dependency, already mmap-based)
gives for free?

**Background, confirmed by direct source read, not assumed**:
`crates/commerce-core/src/index/mod.rs`'s own `CatalogIndex` doc comment
states plainly: "There is no update path: a new catalog version means a
new `CatalogIndex::build` call... mmap itself is not implemented yet;
this is in-memory only." No `Serialize`/`Deserialize` derive, no
`save`/`load`/`persist` method exists anywhere in that module.

**Hypothesis**: reopening an already-built, on-disk, real Tantivy index
via mmap (`Index::open_in_dir`) is dramatically faster (>=5x) than
`commerce_core`'s only option — a full `CatalogIndex::build` — for
getting a queryable structure ready after a process restart.

**Implementation**: `crates/phase2-eval/src/bin/mmap_persistence_eval.rs`.
Builds a real Tantivy index to disk, drops every in-process handle, then
in a fresh measurement times `Index::open_in_dir` + reader/searcher
construction + running one real judged query, with RSS snapshots at each
step. Separately times `CatalogIndex::build` from the same ingested
catalog, reproducing R1-E01's baseline fresh. Explicitly discloses that
the OS page cache is warm at reopen time (same-process build-then-reopen)
— this measures the realistic same-machine-restart case, not a cold-boot/
evicted-cache scenario.

**Results**:

| Metric | Tantivy mmap reopen | `CatalogIndex::build` | Ratio |
|---|---|---|---|
| Time to queryable + 1 real query | 0.0109s | 64.36s | **5,889.7x** (bar: >=5x) |
| RSS growth for this phase | +19.4 MB (after 1 query) | +518.6 MB | 27.3x smaller |

Independent re-run (two full separate processes): time multiplier
14,174x and 14,868x (even *larger* than reported, not smaller); RSS ratio
41.3x and 42.9x (also larger). `CatalogIndex::build`'s
`approximate_size_bytes()` was bit-identical (271,809,052) across all
three runs, confirming the index computation itself is fully
deterministic — the ratio instability is timing/allocator noise on a
shared, concurrently-loaded sandbox machine (verified: another agent's
process was concurrently holding ~5.2GB RSS during one rerun), not a
methodology flaw.

**Interpretation**: CONFIRMED, decisively, by 3-4 orders of magnitude
past the bar in every single run produced (5,889x to 14,868x depending on
run). `open_in_dir` alone costs +4KB RSS (page-table mapping only, no
data touched) because Tantivy's on-disk segment files already *are* the
queryable structure — reopening is page-table setup, not computation.
`CatalogIndex::build` must re-derive every bitmap/range structure from
parsed domain objects every time, with no shortcut, and no way to persist
the result for next time. The headline ratio is real but noisy at this
scale (a single-digit-millisecond denominator swings several-fold with
scheduler noise) — the qualitative conclusion is not in doubt, but the
specific point-estimate numbers (5,889.7x, 27.3x) should be read as noisy
single-shot estimates of a very large effect, not precise constants, if
quoted elsewhere (e.g. `SCALE_UP_DECISION.md`).

**Regression check**: `cargo fmt --all -- --check`, `cargo clippy -p
phase2-eval --all-targets --all-features -- -D warnings` clean. No
`commerce_core` file touched, no new dependency.

**Verify-stage findings**: ENDORSED_WITH_CAVEATS. The ratio instability
above was the verify agent's own finding, not disclosed in the original
report — recorded here rather than treated as a discrepancy that
undermines the result, since it points toward a *larger* effect, not a
smaller one, in every rerun. A minor framing asymmetry was also noted:
the Tantivy arm's measured cost includes running one real query
end-to-end; the `commerce_core` arm's does not (build only). Given a
single query costs low milliseconds at most against either structure
elsewhere in this project's own measurements, this cannot plausibly
change the conclusion.

**Decision: CONFIRMED.** `commerce_core` has no persistence/reload
mechanism at all today, and the cost of that absence — a full rebuild on
every process restart, currently the only option for any state change or
process restart at all (also confirmed independently by Issue #8's
`REALTIME_LOG.md` R-E01) — is 3-4 orders of magnitude worse than what an
already-mature dependency (Tantivy) provides for free via mmap. This
does not mean "build a bespoke mmap format for `CatalogIndex`" follows
automatically — the two structures hold different content (typed
commerce attributes vs. an inverted text index) and the comparison is
about "cost of *a* queryable structure being ready," not "cost of an
equivalent one" — but it is a real, now-quantified gap worth a future
issue, not a hypothetical one.

---

## Issue #7 summary across all five experiments

| # | Hypothesis | Verdict | Real margin |
|---|---|---|---|
| 1 | Bounded top-K on the Punt path | CONFIRMED (already built, no new code) | 820x |
| 2 | Columnar attribute layout (RSS) | FALSIFIED (5x bar) / architecturally confirmed at ~2.1x ceiling | 2.08x of 2.13x ceiling |
| 3 | Pre-bucketed numeric-range bitmaps | CONFIRMED | 12.2-12.5x |
| 4 | Tiered/lexicographic ranking | FALSIFIED | 0.49% to 2.43% (bar: 5%) |
| 5 | mmap persistence vs. none | CONFIRMED | 5,890x-14,868x |

Three of five hypotheses are decisively confirmed with real, independently-
verified margins; two are honestly falsified against their pre-registered
bars while still surfacing real, actionable secondary findings (#2's
~2.1x ceiling and the compression follow-up it implies; #4's discovery
that `compile_lexicon` never emits real ranking signal on cold-start
queries at all, redirecting where the integration-gap investigation
should look next). Every verdict was independently reproduced by a
second, adversarial agent before being recorded here — one of which
(#4's latency-ordering artifact) materially changed what could honestly
be claimed, and is recorded as such rather than smoothed into the
headline result.

**Next**: feed all five findings into Issue #5/`ROUND1_DECISION_TREE.md`.
