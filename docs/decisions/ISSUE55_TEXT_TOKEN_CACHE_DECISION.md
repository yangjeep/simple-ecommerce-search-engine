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
