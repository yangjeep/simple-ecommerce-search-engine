# Issue #55 — Hybrid-bucket zero-hit mechanism: decision

Full log: `docs/experiments/ISSUE55_HYBRID_ZERO_HIT_MECHANISM_LOG.md`.
Raw artifacts: `docs/research/artifacts/i55_hybrid_zero_hit_probe/{automotive,electronics,beauty}.txt`.

## Governing question

`docs/decisions/ISSUE55_ROUTING_OUTCOME_REPLICATION_DECISION.md` named
automotive's still-large post-fix Hybrid gap (-38.75%) as an open
thread: is it an unexplained relevance gap, or does it have a concrete,
disclosed mechanism?

## Finding: a real, partial mechanism, confirmed by reading the code path directly

A qualitative probe of automotive's Hybrid-routed queries found several
cases where native returns **literally zero hits** (not just a low
ranking) for a Brand-constrained query with a real relevant product in
the catalog (e.g. `"castrol 10w30"`: native NDCG=0.0, Solr NDCG=0.4791,
Solr's top-3 includes a real `Exact`-labeled match). Two compounding,
independently-confirmed reasons:

1. **Tokenization**: the delegate's Tantivy `QueryParser` uses the
   default tokenizer, which splits on punctuation. `"10W-30"` in a real
   title indexes as two tokens (`"10w"`, `"30"`); the residual query
   token `"10w30"` (concatenated, brand already consumed into the
   structural constraint) is a single token that matches neither --
   consistent with, though not independently unit-tested beyond, the
   observed symptom.
2. **Issue #42 R2's residual-lexical fallback never fires on this data**,
   for two independent reasons: `issue35-eval`'s harness never passes a
   `residual_policy` at all, and even if it did, the fallback's own
   `corroborating_product_type` precondition can never be satisfied by
   any ESCI vertical, since none of the three register product types at
   all (an already-disclosed Issue #35 scope boundary, not new here).

**Quantified across all three verticals** (Hybrid-routed, NDCG-scoreable
queries): automotive 14/32 (43.75%) zero-hit, 4/32 (12.5%) a confirmed
"native missed a recoverable answer" case (zero-hit AND Solr found real
relevance); electronics 8/48 (16.7%) / 1/48 (2.1%); beauty 15/38 (39.5%)
/ 3/38 (7.9%). Automotive ranks worst on both metrics, consistent with
(but not a clean, complete explanation of) its own worst Hybrid NDCG gap.

## Verdict: KEEP as a confirmed, disclosed, partial-explanation finding. No production change made.

This checkpoint is a measurement, not a fix. Per this project's own
discipline (name a follow-up rather than implement speculatively), no
change was made to `residual_fallback_hits`, the delegate's tokenizer, or
`issue35-eval`'s harness. Both named mechanisms are corroborated by
reading the actual code path (`crates/phase9-eval/src/bitmap_delegate.rs`'s
`QueryParser::for_index` call; `crates/commerce-core/src/plan/mod.rs`'s
`residual_fallback_hits` preconditions; `crates/issue35-eval/src/lib.rs`'s
never-registered product types), not inferred from the aggregate number
alone -- the same standard this project has applied to every other
mechanism claim this session.

**Important, deliberately checked nuance**: relaxing R2's
`corroborating_product_type` requirement to also accept a `Brand`
constraint would **not** reintroduce a correctness/wrong-family risk --
`index.execute_ranked` (which the fallback calls) already goes through
`index.execute`, which enforces `query.constraints` (including `Brand`)
*before* ranking, the same mechanism `FastPath` itself relies on for
safety. The `ProductType`-specific requirement was a deliberate
precision/confidence scope choice in the original R2 design
(`ISSUE42_LOG.md#i42-r2`, `docs/adr/0012-residual-lexical-policy.md`),
not a correctness guardrail -- so relaxing it is a genuine, answerable
design question (does accepting Brand-only corroboration recover real
recall without degrading precision on ambiguous residual text?), not a
free win. Named below, not implemented here.

## Real caveats, disclosed rather than smoothed over

- **Not a full explanation.** The zero-hit-rate/NDCG-gap correlation
  does not hold cleanly across all three verticals (beauty's zero-hit
  rate is close to automotive's, but its NDCG gap is close to
  electronics'). Other, unidentified factors also contribute to the
  Hybrid-bucket gap.
- **Tokenization mechanism is plausible and code-path-confirmed, not
  isolated-unit-tested.** No standalone Tantivy tokenizer test was
  written to prove `"10w30"` vs. `"10W-30"` in isolation; the claim
  rests on Tantivy's documented default-tokenizer behavior plus the
  observed real symptom, not a fresh minimal reproduction.
- **Small n.** Automotive's "recoverable miss" count is 4 queries; not
  a large-sample claim.

## What this does NOT establish

- Not a claim that fixing either mechanism would close automotive's
  Hybrid gap -- at most 12.5% of automotive's evaluated Hybrid queries
  are confirmed "recoverable misses" of this specific kind; the
  remaining gap has other, undiagnosed causes.
- Not a recommendation to relax R2's `ProductType` requirement --
  the correctness analysis above only establishes that doing so would be
  *safe*, not that it would be a net *precision* win; that is exactly
  the open question named below.
- Not a claim about WANDS, which does carry real `ProductType` data and
  is therefore not subject to mechanism (b) at all.

## Next questions (named, not implemented here)

1. **Precision-safe residual-fallback broadening**: preregister and test
   whether accepting `Brand` (not just `ProductType`) as
   `residual_fallback_hits`'s corroborating constraint recovers some of
   the quantified recoverable-miss cases without degrading precision on
   queries where Brand alone is a weak corroborator (e.g. a very large
   brand with unrelated sub-categories) -- a real design question, not
   a mechanical fix.
2. **Delegate tokenization robustness**: does adding a normalization
   step (e.g. stripping/collapsing hyphens in both indexed titles and
   query tokens, or a word-delimiter-style filter) for alphanumeric
   part-number-style tokens recover real recall on automotive/technical
   verticals without a precision cost elsewhere -- independent of (1),
   since it would also help `FastPath`/`Punt` outcomes, not just
   `Hybrid`.
3. What explains the remaining, larger share of automotive's Hybrid gap
   not covered by this mechanism -- not investigated here.
