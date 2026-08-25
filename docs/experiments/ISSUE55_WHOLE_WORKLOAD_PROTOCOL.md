# Issue #55 Preregistered Protocol — whole-workload economics of the H3 ranking fixes

Committed before this round's measurement is run, per this repository's
governance and Issue #55's own measurement contract: "every performance
experiment must report both conditional and whole-workload effects...
a spectacular fast-path microbenchmark is not a win if admission/fallback
makes the whole workload slower or less relevant."

## 0. What this is testing

The two prior Issue #55 checkpoints (ranking-scaling,
text-token-cache) measured a **conditional** effect: `execute_ranked`'s
own cost, and H3's identical-candidate-set isolation (n=15
structural-routed WANDS queries specifically). That isolation is real and
correctly scoped for what it answers (is native's *ranking pass* fast
once retrieval/coverage is held constant?) — but P9-E02's own routing
data already on record shows `structural_routed` (FastPath+Hybrid) is
only 21/480 (4.375%) of real WANDS query traffic; the other 459/480
(95.6%) is `Punt`-routed (delegate-only, no structural narrowing, does
not exercise `execute_ranked`'s ranking-signal path the same way). This
experiment asks: given that small real traffic share, does the H3
reversal move the **whole-workload**, traffic-weighted picture at all,
or is it a large conditional effect on a small slice that washes out in
aggregate?

## 1. Hypothesis

**H0 (whole-workload effect is real but small)**: the traffic-weighted
overall NDCG/latency numbers move by an amount roughly proportional to
`structural_routed`'s own ~4.375% traffic share — a real, disclosed,
but modest whole-workload effect, not comparable in magnitude to the
21-query conditional reversal itself. **H1 (no visible effect)**: the
whole-workload numbers are statistically indistinguishable from before,
because `structural_routed`'s share is too small relative to
`Punt`-routed traffic's own latency variance to detect. Neither outcome
would contradict the conditional H3 finding — this measures a different,
complementary question Issue #55's own contract requires reporting
alongside it, not instead of it.

## 2. Baseline / dataset / treatment

Baseline: current branch HEAD (both Issue #55 ranking fixes already
applied). Dataset: the same real WANDS catalog + 480 queries + fresh
Solr 9.10.1 already used throughout this session's Issue #55/#43
checkpoints. Treatment: none — this reruns `p9_e02_wands_physical_advantage`
unmodified, exactly as P9-E02/P9-E06/the Issue #43 checkpoint did, the
only change being which `commerce_core` build it links against.

## 3. Metrics / gates

- Traffic-weighted overall NDCG@10 and latency ratio (native vs. Solr),
  and the `structural_routed`-only breakdown, both already part of this
  binary's own printed output.
- **KEEP/document as real-if-modest**: `structural_routed`'s own NDCG gap
  and latency ratio move in the same direction and rough magnitude as the
  isolated H3 measurement already found; the traffic-weighted overall
  number moves by a smaller, plausibly-proportional amount.
- **Flag for further investigation**: the traffic-weighted overall number
  does not move as expected, or moves in an unexplained direction — would
  indicate an interaction this checkpoint's own isolation missed.

Repetitions: 3 independent full-480-query runs, matching P9-E02's own
original manifest convention exactly.
