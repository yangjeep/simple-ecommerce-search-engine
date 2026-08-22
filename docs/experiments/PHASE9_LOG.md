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
measured relevance gap against Solr on any real catalog — that is P9-E01
below, the actual re-run of Phase 2's physical-advantage-by-query-class
measurement, on WANDS.

**Regression risk noted, not yet measured**: this fix necessarily removes
part of the P1-D cost optimization's original guarantee ("behavior is
identical... just without the wasted allocation") — behavior is no longer
identical when `residual_lexical` is non-empty, by design. The *cost*
guarantee (no `effective_attributes` merge) is preserved exactly. A
real-catalog latency re-measurement of `execute_ranked` alone (not just
correctness) is deferred to P9-E01/E02's benchmark run rather than done
in isolation here, since P9-E01 needs a real WANDS query mix run anyway.

## P9-E01: (next) fix disclosed defect #2 — Hybrid's TermSetQuery delegate restriction

Not yet started. See task tracker.

## P9-E02: (next) WANDS lexicon compiler + relevance module + physical-advantage re-run

Not yet started. Requires: a WANDS-schema-aware lexicon compiler
(Category/ProductType/attribute-aware, analogous to but distinct from
`compile_lexicon`, which is ESCI/Brand-shaped), a WANDS relevance-grading
module (3-way `Exact`/`Partial`/`Irrelevant` label scale — confirmed via
direct scan of `dataset_cache/wands/label.csv`: 25,614 Exact / 146,633
Partial / 61,201 Irrelevant across 233,448 judgments over 480 queries —
distinct from `round1_eval::relevance::EsciLabel`'s 4-way scale, no code
currently maps WANDS labels to NDCG grades at all), and a fresh Solr
baseline query path (the existing `wands_bench` Solr core already indexes
`title`/`description` as real `text_general` fields per
`scripts/datasets/solr_index_wands.py`, so free-text relevance queries are
already supported by the existing schema — reusable, not rebuilt).
