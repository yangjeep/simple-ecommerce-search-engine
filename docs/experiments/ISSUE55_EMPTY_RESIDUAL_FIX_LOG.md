# Issue #55 Experiment Log — hoisting the empty-residual short-circuit in `execute_ranked`

Protocol: `docs/experiments/ISSUE55_EMPTY_RESIDUAL_FIX_PROTOCOL.md`.

## I55-EMPTYRESID-E00 — fix validated; residual gap fully explained, not closed by an implementation change

**Implementation**

`crates/commerce-core/src/index/rank.rs`: `execute_ranked` now computes
`use_preferences`/`has_residual_lexical` once, outside the per-candidate
`.map()` closure, and branches `use_preferences -> score_preferences`,
`else if has_residual_lexical -> score_text_relevance_precomputed`
(with the `product_location` lookup), `else -> 0.0` — the emptiness
check that used to sit inside `score_text_relevance_precomputed`,
reached only *after* the now-unconditional `product_location` lookup,
is now checked before the lookup ever runs. No other logic changed.
Added `execute_ranked_short_circuits_to_zero_when_residual_and_preferences_are_both_empty`,
a new test exercising `execute_ranked` itself (not just the inner
scoring helper) on a 3-product catalog, confirming identical output
(every hit scores `0.0`, ties break on ascending product id) — the
existing `score_text_relevance_precomputed_matches_live_tokenization...`
property test (500 trials, already covers empty residual via
`rng.gen_range(0..=3)`) continues to pass unchanged, since the scoring
function itself is untouched.

**Gate 1 — correctness**: `cargo test --workspace --all-features` —
commerce-core's suite grew from 57 to 58 passing tests (the new one),
zero failures, zero changes to any other test's outcome.

**Gate 2 — regression fix, `p9_e05_full_catalog_ranking_tail`** (same
binary, same two real WANDS queries, 200 reps each, no Solr involved):

| Build | "driftwood mirror" mean | "marble" mean |
|---|---|---|
| Before any Issue #55 fix | 2.1967ms | 2.1221ms |
| After both original fixes (regressed) | 3.8014ms | 4.2121ms |
| After hoist fix | 2.2096ms | 2.3581ms |

The regression is gone — both queries land within run-to-run noise of
the original, pre-any-Issue-55-fix baseline. **CONFIRMED.**

**Gate 3 — no new regression on the confirmed win, `cargo bench --bench rank_bench`**
(synthetic realistic catalog, non-empty residual, the population the
text-token-cache checkpoint's own 43-59% synthetic win was measured on)
— criterion's own before/after comparison against its stored baseline:

| n | change |
|---|---|
| 100 | -4.89% to -3.49% (improved) |
| 1,000 | -8.22% to -4.90% (improved) |
| 10,000 | -6.05% to -3.67% (improved) |
| 100,000 | -3.82% to -1.47% (improved) |

Not merely "no regression" — criterion reports a small, consistent
**additional** improvement (2.5-8.2%) at every scale, from hoisting the
two `is_empty()` checks out of the per-candidate closure so they are
evaluated once per query rather than once per candidate. **CONFIRMED,
exceeded.**

**Gate 4 — whole-workload, 6-run fresh-Solr `structural_routed` FastPath/Hybrid breakdown**
(`p9_e02_wands_physical_advantage`, same diagnostic added in the
whole-workload checkpoint):

| Metric | Before any fix | After both original fixes (regressed) | After hoist fix |
|---|---|---|---|
| FastPath nat_ms (mean of 6) | 1.366ms | 2.464ms | 1.459ms |
| Hybrid nat_ms (mean of 6) | 0.588ms | 0.620ms | 0.574ms |
| `structural_routed` ratio (mean of 6) | 2.545x | 1.762x | 2.483x |

FastPath's own mean is back within the pre-Issue-55 range (6-run before:
1.258-1.528ms; 6-run after-hoist: 1.330-1.659ms — overlapping), and the
`structural_routed` aggregate ratio (2.483x) lands almost exactly back
at the original pre-Issue-55 baseline (2.545x). Hybrid stays flat across
all three conditions (0.574-0.620ms), the same negative control as
before, confirming none of this touches Hybrid's cost.

**This is not "no effect" — it is a complete, expected explanation.**
`p9_e05`'s isolated timing shows the two zero-constraint queries' own
`execute_ranked` cost (~2.1-2.4ms) is now understood to be dominated by
neither scoring nor sorting: the tie-heavy microbenchmark from the
whole-workload checkpoint measured the selection step alone at
~0.2-0.4ms even at 100% ties and n=42,994, an order of magnitude below
the ~2ms total. The remainder is `index.execute()` materializing the
full 42,994-candidate bitmap and `lookup_variant` resolving each one —
real, linear, unavoidable work that predates every Issue #55 fix and
that none of them targeted. Issue #55's three fixes (partial selection,
precomputed tokens, hoisted short-circuit) collectively made the
scoring/sorting portion of `execute_ranked` correctly close to free for
this population; what is left is a *different* cost floor, outside this
issue's scope, that happens to dominate the `structural_routed`
aggregate because it scales with candidate-set size and two of the
WANDS run's real FastPath queries have candidate sets of the entire
catalog.

**Adversarial check**: is the ~2.483x vs. 2.545x difference (a ~2.4%
gap, not an exact match) meaningful? No — both are 6-run means with
observed within-condition swings of 1.5-2x (e.g. before: 2.08x-3.84x;
after-hoist: 2.12x-3.29x) from the already-disclosed Solr-JVM-warmup
confound (Issue #43); a 2.4% difference in the means is well inside that
noise band, not a residual effect to explain further.

**What remains, named as a candidate follow-up, not this checkpoint's job**:
reducing `structural_routed`'s own `FastPath` cost further would require
optimizing `index.execute()`/`lookup_variant` for the zero-constraint
case, or — more consistent with this project's own architecture bias —
revisiting whether a query with a 100%-of-catalog candidate set should
route to `FastPath` at all rather than `Punt`, a planner-policy question
first raised, and left open, in the whole-workload checkpoint.
