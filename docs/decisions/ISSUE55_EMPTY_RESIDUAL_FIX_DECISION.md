# Issue #55 — hoisting the empty-residual short-circuit in `execute_ranked`

Log: `docs/experiments/ISSUE55_EMPTY_RESIDUAL_FIX_LOG.md`. Protocol:
`docs/experiments/ISSUE55_EMPTY_RESIDUAL_FIX_PROTOCOL.md`.

## Verdict: KEEP

The whole-workload checkpoint
(`docs/decisions/ISSUE55_WHOLE_WORKLOAD_DECISION.md`) found and
root-caused a real ~1.7x-2.3x regression: `execute_ranked` paid an
unconditional `product_location` `HashMap` lookup per candidate before
checking whether there was any residual lexical text to score, where
the pre-fix code's equivalent check was free. The named fix — check
`residual_lexical`/`preferences` emptiness once, before the lookup —
is implemented and validated against all four preregistered gates:

1. **Correctness preserved**: existing 500-trial property test still
   passes unchanged; a new test directly exercising `execute_ranked` (not
   just the inner scoring helper) confirms identical output on a
   multi-product catalog.
2. **Regression eliminated**: `p9_e05`'s two known pathological queries
   return to their pre-any-Issue-55-fix baseline (~2.1-2.4ms, down from
   the regressed ~3.8-4.2ms).
3. **No new regression on the confirmed win — exceeded**: `rank_bench.rs`
   shows a small additional 2.5-8.2% improvement at every candidate-set
   size (100 through 100,000), from hoisting two per-query checks out of
   the per-candidate loop.
4. **Whole-workload measured**: the `structural_routed` 6-run aggregate
   ratio returns to 2.483x, effectively the same as the original
   pre-Issue-55 baseline (2.545x) within the already-disclosed
   Solr-JVM-warmup noise band.

## Why this is not a second reversal, and not a disappointment

Gate 4's number returning close to the *original* pre-Issue-55 baseline
could look like "the fixes net out to nothing" — it does not. `p9_e05`
in isolation, combined with the whole-workload checkpoint's own
tie-heavy microbenchmark (selection step ~0.2-0.4ms even at full-catalog
scale and 100% ties), shows the two pathological queries' ~2ms total
cost is now dominated by `index.execute()`/`lookup_variant` materializing
the full 42,994-candidate set — work no Issue #55 fix ever targeted or
claimed to reduce. The scoring/sorting portion these three fixes *did*
target is now measurably, consistently cheap (confirmed independently at
least four times across this and the two prior checkpoints: the
isolated `p9_e04` H3 result, the synthetic `rank_bench`/`select_top_k_bench`
benchmarks, this checkpoint's own `rank_bench` rerun, and `p9_e05`'s own
non-regression on Hybrid). The `structural_routed` aggregate simply
also contains a cost this issue never claimed to fix, which happens to
dominate two specific real queries' absolute latency. Reporting the
number honestly, rather than declaring victory on a metric these fixes
don't fully control, is the point of Issue #55's own whole-workload
contract.

## What this does and does not change

- **Confirms and completes** the text-token-cache checkpoint's KEEP
  verdict: the previously-disclosed regression boundary (queries with
  empty `residual_lexical`/`preferences`) is closed. No further
  addendum narrowing that fix's scope is needed — the combined state of
  all three Issue #55 ranking fixes has no known regression.
- **Does not claim** to reduce `structural_routed`'s own full-catalog
  candidate-materialization cost — that was never in scope and remains
  a distinct, real cost, named as a candidate follow-up (either optimize
  `index.execute()`/`lookup_variant` for the unconstrained case, or
  revisit whether a 100%-of-catalog candidate set should route to
  `FastPath` at all — a planner-policy question raised, not resolved, by
  the whole-workload checkpoint). A follow-up checkpoint
  (`docs/decisions/ISSUE55_AMBIGUOUS_ROUTING_DECISION.md`) confirmed this
  is about candidate-set size specifically, not the reason
  `residual_lexical` ended up empty: queries where ambiguity absorbs all
  content reproduce this exact pathology when unnarrowed (`0.0` NDCG),
  but not when a real constraint elsewhere keeps the candidate set small.
- **No correctness change, no NDCG change, no candidate-set change** in
  any measurement across all three checkpoints in this sub-thread — this
  and the prior checkpoint are latency-only findings on an existing,
  correctness-preserving code path.

## Adversarial review

- **Checked whether the ~2.483x vs. 2.545x whole-workload gap (2.4%) is
  a residual effect needing further explanation**: no — both are 6-run
  means with within-condition swings of 1.5-2x from the already-disclosed
  Solr-JVM-warmup confound; 2.4% is well inside that noise band.
- **Checked whether the criterion "improvement" (not just parity) on the
  confirmed-win path is itself suspicious**: no — it has an identified,
  mechanistic cause (two boolean checks moved from per-candidate to
  per-query), consistent in direction and rough magnitude across all
  four tested candidate-set sizes, not a single cherry-picked point.
- **Checked whether the new `execute_ranked`-level test is redundant
  with the existing scoring-only property test**: no — the existing test
  calls `score_text_relevance_precomputed` directly with a
  caller-constructed `PrecomputedTextTokens`, never exercising
  `execute_ranked`'s own branch structure or the `product_location`
  lookup at all; the new test is the first to prove `execute_ranked`
  itself (not just its inner helper) behaves identically after the
  reorder, on a catalog with more than one product (ties matter).

## Traceability

Source change: `crates/commerce-core/src/index/rank.rs` (`execute_ranked`
reordered; 1 new test). Raw evidence:
`docs/research/artifacts/i55_whole_workload/p9_e05_after_hoist_fix.txt`,
`fastpath_hybrid_breakdown_after_hoist_fix/run{1-6}.txt`; `cargo bench`
output captured in the log above (criterion's own before/after
comparison against its persisted `target/criterion` baseline, not a
separately-saved file).
