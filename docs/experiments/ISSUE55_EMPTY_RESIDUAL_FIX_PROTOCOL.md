# Issue #55 Preregistered Protocol — hoist the empty-residual short-circuit in `execute_ranked`

Committed before this round's fix is implemented, per this repository's
governance ("preregister gates... before implementation").

## 0. What this is testing

The whole-workload checkpoint
(`docs/decisions/ISSUE55_WHOLE_WORKLOAD_DECISION.md`) found and
root-caused a real ~1.7x-2.3x regression in `execute_ranked`: the
post-fix code performs `index.product_location.get(&product)` (a
`HashMap` lookup) for every candidate before checking whether
`query.residual_lexical` is empty, whereas the pre-fix code's
equivalent check (`residual_lexical.is_empty()`, inside
`score_text_relevance`) was its first line and did zero work per
candidate in that case. This experiment tests the obvious, named fix:
hoist that check in `execute_ranked` itself, before the lookup.

## 1. Hypothesis

**H0**: moving the `query.residual_lexical.is_empty() &&
query.preferences.is_empty()` check before the `product_location`
lookup eliminates the regression (full-catalog empty-residual latency
returns to ~pre-fix baseline, ~2.0-2.2ms for the two known WANDS
queries) **without** changing output for any query (proven by existing
+ extended equivalence tests) and **without** regressing the
already-confirmed win on non-empty-residual queries (`rank_bench.rs`,
`p9_e04`). **H1**: the reordering has some other cost or introduces a
behavior change the equivalence tests miss.

## 2. Baseline / dataset / treatment

Baseline: current branch HEAD (`crates/commerce-core/src/index/rank.rs`
as committed after the whole-workload checkpoint). Dataset: same real
WANDS catalog + 480 queries + fresh Solr 9.10.1 used throughout this
session. Treatment: reorder the emptiness check in `execute_ranked`
ahead of the `product_location` lookup; no other logic changes.

## 3. Metrics / gates

- **Correctness**: existing `score_text_relevance_precomputed_matches_live_tokenization_across_randomized_inputs`
  (500 trials, already covers empty residual) must still pass unchanged.
  New test added directly against `execute_ranked` (not just the inner
  scoring helper) on a multi-product catalog with empty
  residual_lexical/preferences, asserting identical output before/after.
- **Regression fix**: `p9_e05_full_catalog_ranking_tail` mean latency
  for both known queries returns to within ~10% of the recorded
  pre-both-fixes baseline (~2.0-2.2ms), down from the ~3.8-4.2ms
  post-fix regression.
- **No new regression on the confirmed win**: `rank_bench.rs`'s
  criterion benchmark (non-empty residual, realistic scores) shows no
  material slowdown at any of its n=100/1,000/10,000/100,000 points
  relative to the currently-recorded numbers.
- **Whole-workload**: rerun the 6-run fresh-Solr `structural_routed`
  FastPath-only breakdown from checkpoint 6; report whatever it shows
  (KEEP/document as real-if-modest if it improves; flag for further
  investigation if it does not), per Issue #55's own whole-workload
  contract — not gated on a specific number since FastPath's n=7 own
  aggregate is dominated by only 2 outlier queries and residual Solr
  variance is a known confound.

Repetitions: same conventions as prior checkpoints (500-trial property
tests for correctness; 200-rep same-process for the targeted latency
check; 6 fresh-Solr runs for the whole-workload check).
