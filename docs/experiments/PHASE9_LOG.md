# Phase 9 Experiment Log — Issue #34: evidence closure, integrated system, final falsification

## Governing context

Issue #34 asks whether the commerce-native architecture, evaluated as an
integrated system rather than isolated primitives, is real. Section A of
that epic names the concrete first re-measurement: Phase 2's own
"what would be built next if scaling up" list (`PHASE2_DECISION.md`)
identified three concrete gaps in its STOP-leaning whole-engine verdict:

1. A bitmap/doc-id-set-based `Hybrid` delegate-restriction mechanism,
   replacing the reference `TantivyDelegate`'s per-query `TermSetQuery`
   (found to undermine `Hybrid`'s own narrow-first-is-cheap advantage).
2. A default ranking signal for FastPath, since `compile_lexicon` never
   emits a real `Preference` on any real catalog tested so far
   (`docs/experiments/ISSUE7_LOG.md` I7-E04), leaving `execute_ranked`
   degenerating to an arbitrary `(product_id, variant_id)` tie-break.
3. A real catalog with genuine multi-entity structural data, since the
   ESCI catalog Phase 2 measured on has only one real structural entity
   (Brand) — WANDS (already acquired, validated in Phase 6A/6B/7) has
   real `Category`/`ProductType` structure too.

The user selected this exact combination as the highest-information-value
first increment for Issue #34: fix both disclosed defects, then re-run
Phase 2's P1-D/P1-E-style physical-advantage-by-query-class +
traffic-weighted-economics measurement on WANDS against a fresh Solr
baseline.

**A correction to the premise, disclosed rather than smoothed over**:
WANDS does **not** have real price data — `dataset_cache/wands/product.csv`
has no price column at all, and `crates/phase6a-eval/src/catalog.rs`'s own
ingestion already uses a `Price::usd(0)` sentinel for every product
(confirmed by direct source read). So WANDS's genuine structural-entity
improvement over ESCI is `Category` + `ProductType` (both real, populated,
already used in Phase 6A/6B/7), not "Category/ProductType/Price" as a
prior summary in this session assumed. `range_plus_structural` (price-range
queries) will not populate meaningfully on WANDS either, for a real reason
(no data), not an implementation gap — this experiment will not fabricate
price data to manufacture that class.

## P9-E00: fix disclosed defect #1 — FastPath's missing ranking signal

**Hypothesis**: `commerce_core::index::rank::execute_ranked` can be given
a real, non-arbitrary default ranking signal for the (currently universal)
case where `query.preferences` is empty, without disturbing any existing
tested behavior for the (currently unreached in production, but tested)
case where a real `Preference` does exist.

**Decision criteria, stated before implementation**: (a) every existing
`commerce-core` unit/integration test continues to pass unchanged — most
importantly `top_k_ranking_orders_by_preference_score_deterministically`
(exercises the `Preference`-scored path) and
`ranking_with_no_preferences_still_returns_every_candidate_score_zero_and_deterministically_ordered`
(exercises the exact "no preferences" case this fix targets) — since both
tests' premises turn out to be compatible with an *additive default*, not
requiring a rewrite; (b) new unit tests demonstrate the signal is real
(distinguishes candidates), cheap (no `effective_attributes` merge, per
the existing P1-D cost fix this must not regress), deterministic, and
case-insensitive; (c) `cargo fmt`/`clippy -D warnings`/`test --workspace`/
`build --release` all clean.

**Implementation** (`crates/commerce-core/src/index/rank.rs`): added
`score_text_relevance(residual_lexical, product)` — counts how many of
the query's own unresolved tokens (`residual_lexical`) literally appear
(case-insensitively, whole-word) in the candidate's `title` (weight 2.0)
or any `AttributeValue::Text` attribute (weight 1.0, e.g. a WANDS-style
`description`). `execute_ranked` now calls this whenever
`query.preferences.is_empty()`, instead of returning `0.0` unconditionally;
the `Preference`-scored branch (and its `effective_attributes` merge) is
untouched. Both existing tests turned out to already be compatible with
this design without modification:
`ranking_with_no_preferences_still_returns_every_candidate_score_zero_and_deterministically_ordered`'s
query ("running shoes") fully resolves into structural constraints with
**zero** residual tokens, so `score_text_relevance` still returns `0.0` for
it (verified, not assumed — the test passed unchanged);
`top_k_ranking_orders_by_preference_score_deterministically`'s query has
non-empty `preferences`, so it never reaches the new branch at all. This
means the fix is purely additive: it only changes behavior for queries
that previously had empty `preferences` *and* non-empty `residual_lexical`
— exactly the gap I7-E04 identified, and nothing else.

Also corrected a now-stale doc comment on `execute_ranked_narrowed_by`
(P3-E03's admission-gated path, deliberately *not* extended by this fix)
that claimed "no ranking signal here either (same as `execute_ranked`'s
own FastPath case)" — no longer true of `execute_ranked` after this change.

**New tests** (`crates/commerce-core/src/index/rank.rs::tests`):
`empty_residual_scores_zero_regardless_of_title`,
`title_token_hit_outweighs_text_attribute_hit`,
`unmatched_token_contributes_nothing`, `matching_is_case_insensitive`
(direct unit tests of `score_text_relevance`), and
`execute_ranked_uses_the_default_signal_when_preferences_are_empty` (an
`execute_ranked`-level integration test against `fixtures::variant_safety_catalog`
with an empty lexicon, so every token of the query lands in
`residual_lexical` and `preferences` stays empty — the real-world shape
this fix targets).

**Result: CONFIRMED, all decision criteria met.**
- `cargo test -p commerce-core`: 42 passed, 0 failed (was 41 before this
  change; the 5 new tests replace no assertions, they are pure additions).
- `cargo test --workspace --all-features`: 0 failures anywhere in the
  workspace (no other crate's tests assumed `execute_ranked`'s old
  always-`0.0` behavior).
- `cargo fmt --all -- --check`: clean. `cargo clippy --workspace
  --all-targets --all-features -- -D warnings`: clean. `cargo build
  --workspace --release`: clean (4m39s).

**What this does and does not establish yet**: this confirms the fix is
*safe* (doesn't regress anything tested) and *real* (the new unit tests
directly exercise it discriminating between candidates). It does **not**
yet establish that this default signal materially improves FastPath's
measured relevance gap against Solr on any real catalog — that is P9-E02
below, the actual re-run of Phase 2's physical-advantage-by-query-class
measurement, on WANDS.

**Regression risk noted, not yet measured**: this fix necessarily removes
part of the P1-D cost optimization's original guarantee ("behavior is
identical... just without the wasted allocation") — behavior is no longer
identical when `residual_lexical` is non-empty, by design. The *cost*
guarantee (no `effective_attributes` merge) is preserved exactly. A
real-catalog latency re-measurement of `execute_ranked` alone (not just
correctness) is deferred to P9-E02's benchmark run rather than done in
isolation here, since P9-E02 needs a real WANDS query mix run anyway.

## P9-E01: fix disclosed defect #2 — Hybrid's TermSetQuery delegate restriction

**Hypothesis**: a bitmap-based restriction mechanism (a `FAST` u64
`product_ordinal` field on each Tantivy document, checked via
`RoaringBitmap::contains` per candidate the inner text query visits) costs
materially less per query than the reference `TantivyDelegate`'s
`TermSetQuery`-based restriction (a fresh Lucene/Tantivy `TermSetQuery`
built from up to ~60K `Term`s on every call, per P2-E17's own finding),
without changing which documents are returned.

**Decision criteria, stated before implementation**: (a) both mechanisms
return the identical top-10 document set on the same index/query/
restrict_to input, proving the comparison is fair; (b) bitmap-restrict's
mean per-query latency is at least 2x faster than TermSetQuery's — this
project's standing bar for a "material, not incremental" latency claim.
Anything short of 2x is FALSIFIED, not reframed as a qualified win.

**Implementation**: new `phase9-eval` crate.
`crates/phase9-eval/src/bitmap_delegate.rs` implements
`BitmapTantivyDelegate: commerce_core::plan::LexicalDelegate` — the
production-shaped replacement for Phase 2's reference `TantivyDelegate`.
Correctness rests on a fact confirmed by direct source read of
`commerce_core::plan::mod.rs` (not assumed): `execute_planned`'s
`verify_and_truncate` already re-checks `restrict_to` membership itself,
independent of whatever a `LexicalDelegate` does internally — so a
delegate is free to implement the restriction however is cheapest,
correctness never depends on it. The mechanism: a custom Tantivy `Query`/
`Weight`/`Scorer` (`BitmapRestrictQuery`) wraps the inner text query's
scorer and skips any candidate doc whose `product_ordinal` fast-field
value is not in the allowed `RoaringBitmap` — no term/FST construction at
all, O(1) membership check per doc the inner query would visit anyway.
`product_ordinal` is set to the real `ProductId.0` at index-build time
(once), not a per-query translation table.

`crates/phase9-eval/src/bin/p9_e01_bitmap_vs_termset_delegate.rs`
benchmarks this head-to-head against a faithful reproduction of Phase 2's
own reference pattern (`BooleanQuery::new([Must(text_query), Must(TermSetQuery::new(terms))])`,
copied from `crates/phase2-eval/src/bin/p1d_physical_advantage_eval.rs`),
on a shared synthetic 500,000-document index (20-word cyclic vocabulary
titles) with a 60,000-id `restrict_to` set — matching P2-E17's own
reported worst case — via `bench-harness::measured_repeat` (30 reps, 3
warmup).

**A real, unforced-error bug found and fixed during this cycle**: the
first version of this benchmark built the two arms against *separately*
constructed indices and destructured `build_index`'s original 4-tuple
return value with the wrong field order in one call site, silently
passing the ordinal field where the title field belonged. This produced
a working build with plausible-looking numbers (an 8-9x speedup) but a
`same_hits: false` result — caught by the pre-registered "same document
set" criterion itself, not discovered after the fact. Fixed by (a)
replacing `build_index`'s tuple return with a named `BuiltIndex` struct
(the class of bug is now a compile error, not a silent mismatch), and (b)
redesigning the benchmark to build ONE shared index for both arms
(generalizing `BitmapTantivyDelegate::new` to take a `Vec<Field>` of
default search fields rather than two hard-coded title/description
fields), removing the two-separately-built-indices confound entirely.
Recorded here per this project's "record failed experiments" discipline,
not silently patched over.

**A second real, disclosed finding, not smoothed over**: even after that
fix, arm (a)'s reported score was a constant `+1.0` higher than arm (b)'s
for every identical hit. Root cause: Tantivy's default `BooleanQuery`
combiner sums the scores of all `Occur::Must` clauses, and
`TermSetQuery`'s own `AutomatonWeight`-based scorer returns a flat `1.0`
for any match — so the reference `TantivyDelegate` pattern's reported
score is inflated by a constant, relevance-unrelated `+1.0` per hit
whenever it restricts via a `TermSetQuery` `Must` clause. This is a
second, previously unnoticed defect in the reference implementation
(beyond the construction-cost issue P2-E17 already flagged), which this
experiment's bitmap-restrict replacement does not reproduce — proven by
`bitmap_delegate::tests::score_is_exactly_the_inner_text_querys_score_not_rewritten`.
Because the offset is constant across every hit, it does not change
which 10 documents make the top-10 or their relative order, so it does
not undermine this experiment's own "same document set" correctness
criterion — but it is recorded as real, disclosed evidence, not silently
observed and dropped.

**Result: CONFIRMED, reproduced across 6 independent runs** (3 during
development, 3 for the record — `docs/research/artifacts/p9_e01_bitmap_vs_termset_run1/run{1,2,3}.txt`):

| run | TermSetQuery mean (ms) | bitmap mean (ms) | ratio | same doc set |
|---|---|---|---|---|
| 1 | 53.41 | 4.55 | 11.73x | yes |
| 2 | 52.63 | 4.52 | 11.64x | yes |
| 3 | 54.27 | 4.54 | 11.96x | yes |
| record 1 | 54.32 | 4.70 | 11.56x | yes |
| record 2 | 53.87 | 4.58 | 11.76x | yes |
| record 3 | 53.66 | 4.51 | 11.90x | yes |

Bitmap-restrict is consistently ~11.6-12.1x faster than TermSetQuery at a
60,000-candidate restriction set, decisively clearing the pre-registered
2x bar, with the identical top-10 document set returned in every run.

**Quality gate**: `cargo fmt --all -- --check` clean, `cargo clippy
--workspace --all-targets --all-features -- -D warnings` clean, `cargo
test --workspace --all-features` 0 failures (5 new `bitmap_delegate`
unit tests, including a direct proof that the bitmap filter never
rewrites a surviving hit's score), `cargo build --workspace --release`
clean.

**Scope, stated honestly**: this is a synthetic microbenchmark isolating
the restriction *mechanism's* cost, deliberately not a real-catalog
relevance or economics claim — that is exactly what P9-E02 tests next.
`BitmapTantivyDelegate` is not yet wired into any real WANDS pipeline or
`commerce_core::plan::execute_planned` call.

## P9-E02: WANDS physical-advantage-by-query-class re-run — REVISE (structural-routed relevance still trails Solr)

**Hypothesis**: with both disclosed defects fixed (P9-E00, P9-E01), and on
a catalog with genuine multi-entity structural data, `commerce_core`'s
structural/hybrid execution shows a materially different (not uniformly
STOP-leaning) relevance and/or latency picture for the structural-dominant
query classes than Phase 2 found on ESCI.

**Decision criteria, stated before implementation**: KEEP/PROCEED if
native NDCG@10 is within 10% relative of Solr's AND native mean latency
is >=2x lower, on the traffic that actually exercises structural
execution; REVISE if only one axis clears; STOP/negative if relevance
still trails materially — preserved as a genuine result, not forced.

**A correction, found before implementation, not assumed**: a
`compile_lexicon`-equivalent for WANDS was originally scoped as new work
(see the now-superseded text this section replaced). Direct source read
of `commerce_core::cold_start::profile::{CatalogProfile, compile_lexicon}`
found both are already fully catalog-agnostic — `CatalogProfile::build`
takes a generic `&Catalog` plus `&[Brand]`/`&[ProductType]`/`&[Category]`,
and `compile_lexicon` derives hard constraints from whatever it profiles.
No new lexicon compiler was needed; WANDS's own `IngestedCatalog.categories`/
`.product_types` (from `phase6a_eval::catalog::build_catalog`) plug in
directly (`brands: &[]`, since WANDS has none).

**Implementation** (`crates/phase9-eval/src/bin/p9_e02_wands_physical_advantage.rs`,
`crates/phase9-eval/src/wands_relevance.rs`): loads the real WANDS catalog/
queries/judgments; builds a native `CatalogIndex` + lexicon via
`CatalogProfile`/`compile_lexicon` (`min_enum_frequency=1`, no threshold
sweep this pass — a disclosed scope limit, not a hidden one); builds a
Tantivy index + `BitmapTantivyDelegate` (P9-E01); for each of the 480 real
queries, compiles it, classifies it via `round1_eval::query_taxonomy::classify9`,
executes it through the real `commerce_core::plan::execute_planned`
(FastPath/Hybrid/Punt router, exactly the "integrated hybrid system"
Issue #34 Section A asks for) and separately against a fresh, same-run
Solr baseline (`wands_bench` core, re-indexed immediately before this run
via `scripts/datasets/solr_index_wands.py`); scores both against WANDS's
real judgments via `wands_relevance::ndcg_recall_mrr`. A full warmup pass
(480 queries, both engines, discarded) precedes the measured pass — this
project's own P2-E16 precedent (an unwarmed Solr latency measurement was
previously found broken by exactly this omission) — after an initial
unwarmed run showed Solr's mean latency artificially inflated (~4.9ms
cold vs ~1.2-1.5ms warm).

**Result, adversarially inspected before being trusted**:

| routing | n | native NDCG@10 | Solr NDCG@10 | native ms | Solr ms |
|---|---|---|---|---|---|
| punt_routed (no structural constraint) | 330 (68.75%) | 0.666 | 0.621 | ~0.5 | ~1.1-1.4 |
| structural_routed (FastPath+Hybrid) | 150 (31.25%) | 0.1192-0.1194 | 0.1505 | ~0.5 | ~1.2-1.6 |

**The traffic-weighted overall number is real but misleading if read as a
structural-retrieval win, and this experiment's own first version
initially reported it that way before a routing-split breakdown was
added and caught it**: native NDCG@10=0.4951 vs Solr's 0.4740 (+4.46%
relative, traffic-weighted) looks like a KEEP-leaning result — but
`execute_planned`'s `Punt` outcome (330/480 queries, chosen whenever
`query.constraints` is empty) calls the delegate with `restrict_to: None`,
so "native" there is just embedded Tantivy's own plain-text relevance
against remote Solr's edismax relevance on the same title/description
text — an engine-choice question, **not** a test of commerce-native
structural retrieval at all. Splitting by routing outcome (added
specifically because the per-class table alone did not make this
distinction legible) shows the traffic-weighted "win" is driven entirely
by that 68.75% Punt-routed majority. On the 31.25% of traffic that
actually reaches `FastPath`/`Hybrid` — the traffic Issue #34 asks
about — native NDCG@10 trails Solr by a stable **-20.7% to -20.8%
relative** gap, reproduced across 6 independent runs (3 during
development, 3 for the record;
`docs/research/artifacts/p9_e02_wands_physical_advantage_run1/run{1,2,3}.txt`).
Native clears only the latency bar (2.25x-2.90x across runs, the range
itself reflecting how warm the persistent Solr server already was at
measurement time — both engines are compared fairly within any one run,
but the absolute Solr-side number depends on server warmth, a real,
disclosed operational fact, not a methodology defect).

**Per-class detail, not smoothed into the routing split alone**:
`variant_scoped_structural` (n=10, the smallest class) shows the single
worst native loss — 0.0396 vs Solr's 0.2995. Three sample queries
("pineapple", "peacock", "industrial") were inspected directly: the first
ties (both engines find the same lone Exact match); the second shows
native ranking two unjudged items *above* the real Exact match Solr
ranks first; the third shows native's entire top-3 unjudged while Solr's
top-3 are all Exact — real evidence that, even where native's structural
candidate set contains a match, its ranking signal (P9-E00's intrinsic
text-overlap default, or ties falling back to id order) is not yet
competitive with Solr's BM25 for surfacing it first. `structural_plus_lexical_residual`
(n=103, the largest structural-routed class, mostly `Hybrid`) is close —
native 0.1585-0.1591 vs Solr's 0.1619, within noise — showing P9-E01's
bitmap delegate is not itself the bottleneck; the gap concentrates in the
classes with little or no delegate involvement (`variant_scoped_structural`
is mostly `FastPath`).

**Why WANDS's richer schema didn't produce more pure-structural traffic,
a real finding, not a bug**: zero queries classified as `structural_exact_entity`,
`selective_multi_attribute_structural`, or `range_plus_structural`.
Inspection of 5 sample fully-unresolved queries ("salon chair", "dinosaur",
"chair and a half recliner", "sofa with ottoman", "driftwood mirror")
shows why: WANDS's `category_leaf`/`product_class` vocabulary is
compound/hierarchical ("Massage Chairs", "Furniture / Bedroom Furniture /
Beds & Headboards / Beds / Twin Beds"), not the single free-standing nouns
shoppers actually type ("chair"). `compile_lexicon`'s exact-lowercased-
phrase lexicon keying (inherited unchanged from ESCI's brand-token
design) rarely matches a WANDS query verbatim against that vocabulary —
a real limitation of the *lexicon-compilation* step, not of WANDS's
underlying data richness, and not fixed in this pass (out of scope for
"the smallest experiment that can answer the question").

**Quality gate**: `cargo fmt --all -- --check` clean, `cargo clippy
--workspace --all-targets --all-features -- -D warnings` clean, `cargo
test --workspace --all-features` 0 failures, `cargo build --workspace
--release` clean.

**Decision: REVISE.** Structural-routed traffic clears the latency bar
(>=2x, comfortably) but not the relevance bar (needed within -10%
relative, found at ~-20.7%). This is a real, reproduced negative result
for the relevance side of the commerce-native structural-execution thesis
on a second real catalog with genuinely richer structural entities than
ESCI — Phase 2's STOP-leaning relevance finding replicates rather than
reverses. It is not a clean STOP either: latency clears decisively, and
`structural_plus_lexical_residual` (the class the bitmap-delegate fix most
directly targets) is within noise of Solr, not a loss. See PHASE9_DECISION.md
for the full decision writeup and what this implies for Issue #34's
remaining scope.

**Named limitations, not resolved this pass**: (a) `min_enum_frequency`
was fixed at `1` (no threshold sweep, unlike Phase 2's {1,5,25,100}
sweep) — a real, disclosed scope limit; (b) the lexicon-compilation gap
identified above (compound category/product-class vocabulary rarely
matching literal query text) was diagnosed but not fixed; (c) latency was
measured single-shot per query (no `bench-harness::measured_repeat`
per-query repetition), adequate for a first-pass economics signal but not
as statistically rigorous as this project's own bootstrap-CI convention
elsewhere; (d) Solr's own latency figure is sensitive to how warm its
persistent server process already is, not fully isolated via a
steady-state warmup protocol.
