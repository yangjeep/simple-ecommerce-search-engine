# Issue #55 Decision — precomputing title/text-attribute tokenization; H3 reversed

Full protocol: `docs/experiments/ISSUE55_TEXT_TOKEN_CACHE_PROTOCOL.md`.
Full log/raw numbers: `docs/experiments/ISSUE55_TEXT_TOKEN_CACHE_LOG.md`.
Raw artifacts: `docs/research/artifacts/i55_text_token_cache/`,
`docs/research/artifacts/i55_rank_scaling/text_token_cache_after.txt`.

## What was tested

The Issue #55 ranking-scaling checkpoint found `score_text_relevance` —
`execute_ranked`'s real, shipping default ranking signal — re-tokenizes
every candidate's title and text attributes from scratch on every query
call, and that this dominated `execute_ranked`'s total cost more than the
full-sort inefficiency that checkpoint fixed. This experiment precomputes
those token sets once at `CatalogIndex::build` time instead.

## Verdict: **KEEP** — and it reverses a standing architecture finding

Precomputation is proven byte-identical to the original live-tokenization
scoring (500-trial randomized property test, zero divergences, covering
empty titles/attributes and non-matching residual tokens). Synthetic
benchmark: a consistent **43-59% reduction** in `execute_ranked`'s total
cost across candidate-set sizes from 100 to 100,000 — far exceeding this
experiment's own preregistered >=10% bar.

**Combined with the prior checkpoint's sort fix, and re-measured on the
exact real WANDS data and methodology that originally produced Phase 9's
0.42x-0.60x "native slower" finding**: the H3 latency ratio
(Solr-restricted / native, identical candidate set) is now **4.59x-8.19x
across 6 runs against a freshly restarted Solr**, and 3.23x-6.32x across
6 runs against a partially warm one — native clears the project's own
>=2x speed bar in every single one of 12 post-fix runs, where every
pre-fix condition measured (the original publication, and two Issue #43
re-audit conditions) fell short of it, several falling below 1x (native
*slower*). NDCG (H1) is exactly unchanged in every run, confirming the
fixes altered performance only, never output.

**This reverses `docs/decisions/PHASE9_DECISION.md`'s own H3 verdict**,
from FALSIFIED (native measurably slower than Solr on an identical,
real, structural-routed candidate set) to CONFIRMED (native measurably,
consistently faster, by several times). That document is corrected via a
dated addendum — the original figures are preserved verbatim, not
rewritten, per this project's evidence-preservation discipline; they were
real and correctly measured against the code that existed then. What
they were measuring was never a fundamental scaling limit, but two
concrete, now-fixed, now-regression-tested implementation defects.

## Action taken

- `crates/commerce-core/src/index/{rank.rs,mod.rs}`: `PrecomputedTextTokens`
  added, `execute_ranked` uses it. 1 new property test (500 trials).
  Original `score_text_relevance` kept as the `#[cfg(test)]`-only
  reference the new path is checked against.
- `docs/decisions/PHASE9_DECISION.md`: dated addendum reversing the H3
  verdict, with full before/after real-data numbers, appended (not a
  rewrite).
- `docs/decisions/README.md`'s chronology gains an entry.
- Full workspace quality gate passes clean.
- No GitHub issue to close — answers a "what would be built next" item
  the prior checkpoint itself raised.

## Architecture delta — this one matters

This is not an incremental tuning result. Issue #38 E1 (and, by
extension, any future work treating native's ranking-pass cost as a
settled input) previously had to treat native's execution-speed advantage
on structural-routed, ranking-bound traffic as **undetermined at best,
falsified at worst**. It is now **measured, reproducible, and favorable**
on the same real dataset and query population that previously falsified
it — a direct, positive update to the project's core "faster" pillar,
specifically for the workload class (structural-routed, ranked) Phase 9
identified as the one still in question after Phase 2's original
whole-engine-replacement STOP. This does not resolve the broader,
traffic-weighted whole-workload economics question (P9-E02's full
480-query mix is dominated by Punt-routed traffic, ~95.6% post-fix,
unaffected by either of these fixes) — that remains a distinct, larger
measurement named as this checkpoint's own next question.

### Addendum (2026-08-25) — scope boundary found: regresses ~1.7x-2.3x on queries with empty residual-lexical/preferences

The whole-workload follow-up checkpoint
(`docs/decisions/ISSUE55_WHOLE_WORKLOAD_DECISION.md`) found that this
fix's gain does **not** hold at every candidate-set size. On the 2 of
this WANDS run's 21 `structural_routed` queries with zero structural
constraints ("driftwood mirror", "marble" — `compile()`'s correct,
expected output for pure noun-phrase text with no lexicon signal),
`indexed_candidates` returns the entire 42,994-product catalog, and
`execute_ranked` regresses by ~1.7x-2.3x after this fix (confirmed by
three independent measurements: a single-shot embedded run, a 6x6
matched-run comparison, and a dedicated 200-rep same-process
microbenchmark, all agreeing on direction and magnitude — see the
whole-workload log for full numbers).

**Root cause**: both queries compile to empty `residual_lexical` and
empty `preferences`, so this is not a token-lookup or cache-locality
cost at all — the whole-workload checkpoint tested and falsified that
theory directly (both sets are never touched for these two queries in
either build). The real cause is in `execute_ranked`
(`crates/commerce-core/src/index/rank.rs:205-217`): the post-fix code
does an unconditional `index.product_location.get(&product)` `HashMap`
lookup for every candidate before checking whether `residual_lexical`
is empty, whereas the pre-fix `score_text_relevance` checked emptiness
as its very first line and did zero work (no lookup, no tokenization)
for exactly this case. At real `Hybrid`/typical-`FastPath` candidate
sizes (median ~570-590), one extra hashmap lookup per candidate is
negligible next to the tokenization cost the fix removes for
non-empty-residual queries. At 42,994 candidates with an empty residual
(these two queries), there was no tokenization cost to remove in the
first place, so the fix's only effect is 42,994 added lookups — a pure
regression. Full investigation, including the falsified cache-locality
theory, is in the whole-workload log.

**This does not reverse the KEEP verdict or the H3 reversal above** for
the traffic segment they were actually measured on (candidate sets up to
~5,000 — the large majority of real `structural_routed`/`Hybrid`
traffic in this dataset, where the gain is real and reproduced again in
the follow-up checkpoint). It narrows the claim: this fix is a
confirmed win whenever a query has real residual lexical tokens to
score, and a confirmed regression (proportional to candidate-set size)
for queries with empty `residual_lexical` and `preferences`. The fix —
hoist the emptiness check before the `product_location` lookup — is
named as a follow-up experiment, not implemented speculatively here.

**Update (2026-08-25)**: that follow-up is implemented and validated in
`docs/decisions/ISSUE55_EMPTY_RESIDUAL_FIX_DECISION.md` — KEEP, the
regression is closed, and the confirmed-win path gains a small
additional improvement from the same reorder. This scope-boundary
addendum is preserved verbatim rather than deleted, per this project's
evidence-preservation discipline, but no longer describes a live
regression.
