# Issue #55 — fixing WANDS `product_class` ingestion gaps

Log: `docs/experiments/ISSUE55_PRODUCT_CLASS_INGESTION_LOG.md`. Protocol:
`docs/experiments/ISSUE55_PRODUCT_CLASS_INGESTION_PROTOCOL.md`.

## Verdict: KEEP (as a correctness/data-hygiene fix) — REJECT as a fix for `structural_routed`'s relevance gap

A focused investigation found a real, disclosed, verified WANDS
ingestion defect: 6.64% of the real catalog had null `product_class`
(falling back to an unmatchable sentinel even when the same record's
own category hierarchy implied a real type) and 5.23% had
pipe-delimited multi-class strings ingested as one opaque, unmatchable
compound string. The fix (fall back to the deepest available category
depth; use the first pipe-delimited segment) is implemented, correct by
direct construction, and passes every correctness check
(`cargo test --workspace --all-features` unchanged).

**It does not materially move either preregistered gate.** Candidate-set
relevant-document recall moved +1.02 percentage points (0.4460 ->
0.4562), below the preregistered >=5pp bar. `p9_e02`'s own `FastPath`,
`structural_routed`, and traffic-weighted-overall NDCG@10 numbers are
byte-identical before and after, for this specific 480-query judged
corpus. The reason, confirmed directly from `p9_e04`'s own printed
qualitative examples (unchanged before/after): the specific
relevant-but-missed products for the queries this benchmark measures
already had real, valid `product_class` values (e.g. "Kids Beds") that
simply don't textually match the query's own resolved term (e.g.
"beds") — a category-hierarchy/synonym mismatch, not the null/garbled
ingestion gap this fix targets. This fix's own affected population
(11.86% of the catalog) evidently has little overlap with what these
specific 15-21 measured queries are missing.

## Why KEEP the fix anyway

CLAUDE.md's own research discipline does not require every correct fix
to move a specific benchmark's headline number to be worth keeping.
This fix: (1) is independently verified correct (no test regression,
correct by direct inspection of the ingestion logic); (2) recovers real
information a source record already carries instead of discarding it
(using `category_depth_N` when `product_class` is absent is not
invented data); (3) produces a measured, non-zero, if sub-threshold,
improvement in candidate-set recall, meaning it is not inert — it is
simply outweighed by a larger, different mechanism for this specific
query population; (4) is a one-line-scope, zero-risk, disclosed
ingestion-hygiene improvement with no plausible downside. Reverting a
correct fix because it did not move one specific benchmark's number
would itself be a form of overfitting to that benchmark. It is kept as
correct WANDS-adapter behavior, not advertised as resolving
`structural_routed`'s own relevance gap.

## Why REJECT as an answer to the coverage-gap question

The original, preregistered question this checkpoint asked was whether
this specific fix would recover `structural_routed`'s own missing-recall
problem. It does not, materially. `PHASE9_DECISION.md`'s (and this
session's own checkpoint 7's) STOP-leaning finding — structural-routed
relevance still trails Solr materially even with disclosed defects fixed
— stands, now with a more precise attribution: the dominant remaining
mechanism is category-hierarchy/synonym mismatch (a real WANDS product
tagged "Kids Beds" not matching a query that resolves to "beds"), not
data-ingestion gaps or ranking-signal weakness (both already
independently ruled out or fixed this session).

## What this does and does not change

- **Adopts** the ingestion fix as correct, harmless WANDS-adapter
  behavior (`crates/phase6a-eval/src/catalog.rs`), not as a claim about
  relevance impact.
- **Does not reopen or contradict** `ISSUE55_EMPTY_RESIDUAL_FIX_DECISION.md`
  or `ISSUE55_AMBIGUOUS_ROUTING_DECISION.md` — this checkpoint measures
  a different, coverage-focused mechanism, and confirms (does not
  reverse) their own STOP-leaning/negligible-impact findings for
  `structural_routed`'s whole-workload picture.
- **Precisely names, but does not implement**, the actual dominant
  remaining mechanism as a candidate follow-up: category/product-type
  synonym or hierarchy-aware matching (e.g. a `ProductTypeAny`-style
  constraint, mirroring the existing `BrandAny` alias-group mechanism,
  so a query resolving to "beds" can also match products tagged with a
  known-related child type like "Kids Beds"). Not implemented here — it
  is a larger, more architecturally significant mechanism (a new
  constraint-matching capability, not an ingestion tweak) deserving its
  own preregistered experiment, not a speculative addition to this
  measurement-scoped checkpoint.
- **No `commerce-core` production code changed.** All changes are in
  `crates/phase6a-eval/src/catalog.rs` (an eval-crate WANDS data
  adapter, not a product dependency).

## Adversarial review

- **Checked whether the fix could be silently wrong (e.g. assigning an
  incorrect fallback type)**: no — the fallback uses only data the same
  record already carries (`category_depth_N`, already extracted and
  used elsewhere in the same function for `category_depth_N` attributes)
  and the first-segment choice for pipe-delimited strings is disclosed
  as a real product-held value, never invented.
- **Checked whether "zero movement" on `p9_e02`'s numbers could itself
  indicate a bug (the fix silently not taking effect)**: no — the
  candidate-set recall diagnostic (a different, more granular metric)
  DID move (+1.02pp), confirming the fix is live and has a real, if
  small, effect; the byte-identical NDCG numbers are a real null result
  on this specific query population, not evidence the code path never
  ran.
- **Checked whether a larger fallback (e.g. using `category_leaf`'s
  full path instead of just the deepest depth segment) might have shown
  a bigger effect**: not tested — `category_leaf` is a multi-segment
  path string (e.g. "Baby & Kids / Toddler & Kids Bedroom Furniture /
  Kids Beds"), which would need parsing/splitting to be useful as a
  `ProductType` key and was judged a larger change than this
  ingestion-only checkpoint's own scope; the deepest `category_depth_N`
  segment already provides the same leaf-level specificity without
  needing new parsing logic.

## Traceability

Source: `crates/phase6a-eval/src/catalog.rs` (`effective_product_class`,
additive). Raw evidence:
`docs/research/artifacts/i55_product_class_ingestion/p9_e04_after_fix.txt`,
`p9_e02_after_fix.txt`.
