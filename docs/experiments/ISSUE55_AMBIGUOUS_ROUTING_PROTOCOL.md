# Issue #55 Preregistered Protocol — does the planner silently discard preserved ambiguity?

Committed before this round's measurement is run.

## 0. What this is testing

While investigating the empty-residual regression (checkpoint 6/7), the
two real WANDS queries responsible ("driftwood mirror", "marble") were
found to compile with empty `residual_lexical` *and* empty
`preferences`, which made them look like pure zero-signal queries.
Printing `compiled.ambiguous` directly shows this is wrong: every token
in both queries resolves to 2-3 genuinely ambiguous attribute readings
(e.g. "marble" could be `color=marble`, `material=marble`, or
`primarymaterial=marble` — all three are plausible against this
catalog's schema), correctly captured by `ir::query::compile`'s own
documented "no silent flattening of an ambiguous phrase" contract
(`crates/commerce-core/src/ir/query.rs:256-261`).

`plan()` (`crates/commerce-core/src/plan/mod.rs:166-199`) never reads
`query.ambiguous` anywhere — its FastPath/Hybrid/Punt decision is a
function of `residual_lexical`/`constraints` only. A query whose
ambiguity was carefully preserved by the compiler is therefore routed
identically to a query with genuinely zero signal: `FastPath`, scored
`0.0` for every candidate by `execute_ranked` (per this session's own
fix), and returned in arbitrary `(product_id, variant_id)` order. This
is a candidate violation of this project's own hard rule ("preserve
ambiguity/abstention when evidence is insufficient") and directly tests
Issue #55's H1 hypothesis ("hybrid-by-construction query planning").

## 1. Hypothesis

**H0 (real, measurable relevance defect)**: queries matching this
pattern (`ambiguous` non-empty, `residual_lexical` and `preferences`
both empty) are a real, non-trivial share of WANDS traffic, and their
native NDCG@10 is materially worse (>=10% relative, this project's
standing bar) than Solr's on the same queries — i.e. the planner's
blindness to `ambiguous` is costing real relevance, not just a latency
curiosity.

**H1 (rare or harmless)**: this pattern is vanishingly rare in real
traffic, or native's NDCG on this population is not materially worse
than Solr's (e.g. because Solr's own BM25 on 1-2 ambiguous tokens
doesn't do meaningfully better than arbitrary order either, or the
judged-relevant set for these queries is small/noisy). Either outcome
would mean this is a real, disclosed, but low-priority gap, not a
falsification of the planning architecture.

Both outcomes are informative; neither contradicts this session's prior
checkpoints. This measures a case those checkpoints's own scoring
function correctly treats as evidence-free (`0.0` for everyone) — the
question is whether the *compiler* already had better evidence
(`ambiguous`) that the *planner* is discarding before it ever reaches
scoring.

## 2. Baseline / dataset / treatment

Baseline: current branch HEAD (after the checkpoint 7 fix). Dataset:
the same real WANDS catalog + 480 queries + fresh Solr 9.10.1 used
throughout this session. Treatment: none — measurement only, via a new
diagnostic binary that classifies every real WANDS query by whether it
matches the pattern, then reports NDCG@10 (native vs. Solr) restricted
to that subpopulation, reusing `p9_e02`'s own NDCG/labeling
infrastructure so the comparison is apples-to-apples with every prior
checkpoint's numbers.

## 3. Metrics / gates

- **Population size**: count of the 480 real queries matching the
  pattern (`ambiguous` non-empty AND `residual_lexical` empty AND
  `preferences` empty), reported regardless of outcome — a tiny
  population (e.g. these same 2 queries) still needs disclosing even if
  it doesn't meet a "material" bar.
- **CONFIRMED (real defect)**: population size >= 5 real queries AND
  native NDCG@10 on that population is >=10% relatively worse than
  Solr's NDCG@10 on the identical queries (this project's standing
  materiality bar, matching `p9_e04`'s own H1 gate).
- **FALSIFIED/low-priority**: population too small to generalize from
  (<5 queries) or the NDCG gap does not clear the bar.
- Either outcome: if the pattern is confirmed real, name (but do not
  implement in this same measurement-only checkpoint) a candidate fix —
  e.g. treating non-empty `ambiguous` as residual-lexical-equivalent for
  `plan()`'s FastPath eligibility check, so these queries fall through
  to `Punt`/`Hybrid` instead of a zero-signal `FastPath` scan.

Repetitions: NDCG is deterministic given fixed judgments (no repetition
needed); Solr latency is not the subject of this checkpoint, so no
fresh-Solr-restart discipline is required here (correctness/relevance
only, not a latency claim).
