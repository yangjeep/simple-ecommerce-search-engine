# Issue #55 — whole-workload economics of the H3 ranking fixes

Log: `docs/experiments/ISSUE55_WHOLE_WORKLOAD_LOG.md`. Protocol:
`docs/experiments/ISSUE55_WHOLE_WORKLOAD_PROTOCOL.md`.

## Verdict: REFINE

The rank-scaling and text-token-cache fixes' own isolated finding
(native's ranking pass 4.6x-8.2x faster on an identical candidate set,
`p9_e04`) does **not** carry over uniformly to `p9_e02`'s real
end-to-end `structural_routed` traffic. A matched 6-run-each,
fresh-Solr comparison found the aggregate ratio moving from 2.545x
(before) to 1.762x (after) — smaller, and in the direction the
preregistered protocol's H1 ("no visible effect") did not anticipate
either, since it moved but in apparent contradiction with the
conditional finding.

This is not because Hybrid traffic dilutes a FastPath gain (that
hypothesis was tested directly and falsified: Hybrid is flat
before/after, exactly as expected since it never calls
`execute_ranked`). It is because the text-token-cache fix, real and
sizeable on the small-to-moderate candidate sets both `p9_e04`'s own
cap and Hybrid's real traffic sit in, **regresses by ~1.7x-2.3x on
queries whose `residual_lexical` and `preferences` are both empty**
(amplified to a full-catalog scale in this WANDS run) — a real,
disclosed segment of the WANDS run's own `FastPath` traffic (2 of 7
queries, "driftwood mirror" and "marble") that `p9_e04`'s own
`MAX_CANDIDATES=5000` cap already excludes from its isolated
measurement, and that the checkpoint-5 synthetic benchmark's own
`Text`-attribute-free product model never exercised even though it
swept candidate sizes up to 100,000.

**Root cause, confirmed via three independent measurements
(single-shot embedded, 6x6 matched runs, and a dedicated 200-rep
same-process microbenchmark) that all agree on direction and rough
magnitude**: an initial cache-locality theory (large per-product
`HashSet<String>`s built from real WANDS description text making
full-catalog scans cache-hostile) was tested directly and **falsified**
— both zero-constraint queries also compile to an empty
`residual_lexical` and empty `preferences`, so neither the old nor the
new scoring code ever touches a title/description token set for them.
The actual cause, confirmed by reading `execute_ranked`
(`crates/commerce-core/src/index/rank.rs:205-217`): the post-fix code
does an unconditional `index.product_location.get(&product)` (a
`HashMap` lookup) for every candidate *before* checking whether
`residual_lexical` is empty, whereas the pre-fix code's equivalent
check was the first line of `score_text_relevance` itself, so an
empty-residual query did zero work per candidate, not one hashmap
lookup per candidate. At real `Hybrid`/typical-`FastPath` candidate
sizes (median ~570-590), this one extra lookup is negligible next to
the tokenization cost the fix removes for non-empty-residual queries.
At 42,994 candidates with an empty residual, there was no tokenization
cost to remove in the first place, so the fix's only effect is 42,994
added lookups — a pure regression. This is why every prior measurement
of this fix (all on non-empty-residual or moderate-candidate-set
traffic) reported an unqualified win.

## What this does and does not change

- **Does not reverse** the text-token-cache checkpoint's KEEP verdict
  for the population it was actually measured on (candidate sets up to
  ~5,000, the vast majority of real `structural_routed`/`Hybrid`
  traffic in this WANDS run). That gain is real, reproduced again this
  checkpoint (Hybrid nat_ms flat and fast, consistent with the prior
  checkpoint's own finding).
- **Narrows its scope**: the fix should not be presented as an
  unconditional win — it regresses any query whose `residual_lexical`
  and `preferences` are both empty, an effect whose absolute size scales
  with candidate-set size. A dated addendum is added to
  `ISSUE55_TEXT_TOKEN_CACHE_DECISION.md` and `PHASE9_DECISION.md`'s H3
  addendum, disclosing this boundary rather than silently limiting the
  claim going forward.
- **Does not change the underlying `execute_planned` routing or
  correctness** — no candidate set, ranking order, or NDCG changed in
  any measurement this checkpoint took; this is purely a latency
  finding on an existing, correctness-preserving code path.
- **Names, but does not resolve**, the obvious fix (hoist the
  `residual_lexical.is_empty() && preferences.is_empty()` short-circuit
  in `execute_ranked` before the `product_location` lookup, so an
  empty-residual query pays no per-candidate lookup at all, matching the
  pre-fix code's own zero-work behavior for this case) as the natural
  next falsification-loop experiment — implementing it now, in the same
  checkpoint that discovered the regression, would violate this
  project's own preregister-before-you-know-the-answer discipline.

## Why REFINE, not KEEP or REJECT

KEEP would misrepresent a fix with a confirmed, real regression on part
of its own deployed traffic as an unconditional win. REJECT would
discard a fix that is a clear, large, reproduced improvement on the
traffic segment (`Hybrid`, and non-outlier `FastPath`) that makes up
the overwhelming majority of real `structural_routed` queries in this
dataset. REFINE — keep the fix, disclose its now-known boundary, and
schedule a follow-up experiment to close the gap — matches what was
actually found.

## Adversarial review

- **Alternative explanation checked and ruled out**: `select_nth_unstable_by`
  regressing on heavily-tied real score distributions. A dedicated
  degenerate-tie microbenchmark (0%-100% ties, n=42,994) showed partial
  selection staying faster than a full sort at every tie fraction,
  including 100% ties (1.7x-15.9x). Not the cause.
- **Alternative explanation checked and ruled out**: cross-process
  noise. The single-shot 6x6 matched comparison (before: mean 1.366ms,
  tight 1.258-1.528ms range; after: mean 2.464ms, tight 2.280-2.918ms
  range — non-overlapping across all 6 runs each) and the dedicated
  200-rep same-process diagnostic (~2.1ms -> ~3.8-4.2ms, both queries)
  independently agree; a noise explanation would not produce two
  non-overlapping, reproduced measurements by two different methods.
- **Checked whether the 2 zero-constraint queries are an artifact of
  this checkpoint's own diagnostic rather than real traffic**: no —
  `compile()`'s output for "driftwood mirror" and "marble" (pure
  noun-phrase text with no structural signal in WANDS's own lexicon)
  is the correct, expected zero-constraint result, not a bug; these are
  real queries from the real WANDS query set routed by the real,
  unmodified planner.
- **Alternative explanation checked and ruled out**: a cache-locality
  theory (large per-product `HashSet<String>`s from real WANDS
  description text making full-catalog scans cache-hostile) — this was
  the first hypothesis reached for and was backed by a real
  measurement (WANDS descriptions average 71.4 words/product vs. the
  checkpoint-5 synthetic benchmark's zero `Text` attributes), but a
  direct check (printing `compiled.preferences`/`residual_lexical` for
  both queries) found both empty, meaning neither the pre-fix nor
  post-fix scoring code ever touches a token set for these queries.
  Ruled out by evidence, not by re-reasoning, and preserved in the log
  rather than deleted once the real cause was found.
- **Checked whether this generalizes beyond WANDS's specific query
  mix**: the confirmed mechanism (an unconditional per-candidate
  `HashMap` lookup hoisted ahead of an emptiness check) is a property of
  `execute_ranked`'s own code, not of WANDS's data — it will reproduce
  on any catalog/query combination that reaches `execute_ranked` with
  empty `residual_lexical` and `preferences`. What is WANDS-specific is
  only how often that combination occurs and how large the candidate
  sets involved are; not measured for other datasets, flagged as a
  scope boundary rather than asserted.

## Traceability

Raw evidence: `docs/research/artifacts/i55_whole_workload/`
(`before_fixes_fresh_solr/`, `after_fixes_fresh_solr/`,
`fastpath_hybrid_breakdown/`, `fastpath_hybrid_breakdown_before_fixes/`,
`p9_e05_before_fix.txt`, `p9_e05_after_fix.txt`). Source changes:
`crates/phase9-eval/src/bin/p9_e02_wands_physical_advantage.rs` (additive
FastPath/Hybrid breakdown rows), `p9_e04_isolated_ranking_and_execution.rs`
(additive candidate-set-size-by-outcome diagnostic), new
`p9_e05_full_catalog_ranking_tail.rs` (dedicated regression measurement).
No `commerce-core` production code changed in this checkpoint — this is
a measurement/diagnosis checkpoint, not a fix.

### Follow-up (2026-08-25)

The named fix (hoist the emptiness check before the `product_location`
lookup) is implemented and validated in
`docs/decisions/ISSUE55_EMPTY_RESIDUAL_FIX_DECISION.md`: KEEP, regression
eliminated, no new regression on the confirmed win (a small additional
gain, in fact), and the whole-workload `structural_routed` aggregate
returns to ~2.48x — essentially this checkpoint's own pre-Issue-55
baseline (2.545x), now understood to reflect a distinct
candidate-materialization cost (`index.execute()`/`lookup_variant`) that
was never in this issue's scope, not a failure of the ranking fixes.
