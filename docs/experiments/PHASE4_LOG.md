# Phase 4 Experiment Log — Issue #16: Learned Semantic Implication Rules

## Governing context

Phase 3 (`docs/experiments/PHASE3_LOG.md`, `PHASE3_DECISION.md`) reached a
**NARROW SUPPORT** verdict for Issue #14's safe-offload thesis: three
independently-KEPT, disjoint admission mechanisms
(`commerce_core::admission::{admit, admit_structurally_anchored_lexical,
admit_single_token_lexical}`) safely intercept up to 5.80% of real traffic
within a 2% relevance-degradation budget. Both rejection-reason classes
covering 99.18% of rejected real traffic (unresolved residual lexical
text, ambiguous queries) were mined to a terminal KEEP/REJECT/BOUNDED
verdict within that admission family, and the mining loop itself was
judged exhausted (P3-E16's own boundary analysis): no further safe
coverage exists within structural/lexical token-presence verification
without a real ranking or semantic signal.

Issue #16 asks the natural next question: can an **offline-proposed,
historically-replayed, validated, and compiled** semantic implication
(e.g. "air force 1" implies Brand=NIKE) supply that missing signal, moving
some of the currently-rejected 94.20% of traffic onto the native FastPath
without violating Issue #14's relevance/correctness budgets? No LLM/model
call is permitted in the online serving hot path (CLAUDE.md hard rule);
offline proposal generation must not require a real model API key in any
test (CLAUDE.md hard rule).

## Prior-art survey (before any new code)

A full survey of existing infrastructure was done before designing this
phase's mechanism, per this project's own "reuse established evidence,
do not rebuild" discipline. Summary (full detail in this phase's design
notes, cross-referenced against exact file/line citations):

1. **`crates/commerce-core/src/control_plane/`** (Gate 5, ADR 0005) already
   implements an observe -> propose -> replay -> promote skeleton with a
   `ModelProvider` trait boundary (`FixtureModelProvider`, deterministic,
   no API key) and a `PrecisionOracle` judged-relevance validation gate.
   **Reusable**: the propose/replay/promote *discipline* and gate logic.
   **Not reusable as-is**: `Proposal`/`ir::lexicon::Candidate` are
   single-fact (one term -> one `ResolvedConstraint` or `Preference`);
   `SemanticLexicon`'s multi-`Candidate`-per-key slot means *ambiguous
   alternative readings*, never *simultaneous conjunctive facts*. A
   multi-fact implication rule needs a new type, not a repurposing of
   this one.
2. **`cold_start::profile::compile_lexicon`** hard-codes every
   `Candidate::confidence` to `1.0` (confirmed again this phase, same
   finding as P3-E11) and tracks no cross-field co-occurrence at all.
3. **Issue #9's three-baseline canonicalization work**
   (`cold_start::canonicalize`, `PHASE2_LOG.md` P2-E07-E10) supplies the
   concrete template for "model-assisted without a real API key in
   tests": a one-time, out-of-band offline labeling pass produces a
   static, committed (or gitignored, per its own data-size discipline)
   lookup artifact; the runtime `impl` is a fixed table lookup, never a
   live model call. This phase reuses that exact pattern's *shape* for
   any future model-based proposer, while implementing the first,
   evidence-grounded proposer as a real-data-frequency/co-occurrence
   miner (see below) rather than a static label file, since real catalog
   signal is directly available and sufficient to test this phase's
   first hypothesis without needing any offline model pass at all.
4. **`cold_start::prefill::predict_brand_from_phrase`** (P1-C, NARROW
   verdict, `PHASE2_LOG.md` P2-E12) already computes a real,
   zero-model-call title-phrase-to-brand co-occurrence signal
   (`PrefillPrediction{brand, purity, occurrence}`). **Reusable as a
   candidate-generation signal source.** **Why Issue #16 is not simply
   re-litigating P1-C's own NARROW verdict**: P1-C applies its prediction
   *live, inline, at query-compile time*, per-query, against a title
   index — never as a separately-proposed, replay-validated, versioned,
   *promoted* rule compiled into a static table ahead of time. It was
   also evaluated on integrated retrieval metrics, not directly against
   Issue #14's admission/coverage frontier. Issue #16 asks a different
   question with the same underlying signal: does the offline-propose/
   replay-validate/promote/compile *shape* (not the live-inline shape)
   let this signal source safely expand the *admission* frontier
   specifically, measured the way P3-Ex measures it?
5. **Real catalog structural-field population, the load-bearing
   constraint on this phase's scope** (`round1_eval::catalog`'s own doc
   comment, confirmed again directly): every real ingested product has
   `product_type = UNKNOWN_PRODUCT_TYPE` and `category = UNKNOWN_CATEGORY`
   — always, sentinel, no exceptions. **Brand is the only typed
   structural field with real, non-sentinel, per-product-diverse signal
   in this catalog.** No `ProductLine`/`Collection` typed concept exists
   anywhere in `commerce_core::domain`/`ir` today.

**Direct, load-bearing consequence for this phase's scope**: Issue #16's
own illustrative example ("air force 1" -> ProductLine=AIR_FORCE_1,
Brand=NIKE, ProductType=SNEAKER) cannot be validated in full against this
real catalog. The Brand=NIKE fact is real and testable (real per-product
brand signal exists). The ProductType=SNEAKER fact is not: asserting it
would mean asserting into a field that is 100% sentinel for every real
product in this corpus, with nothing real to replay it against. Per
Issue #16's own explicit caution ("Do not promote a rule merely because
world knowledge says it is true. It must be useful and safe for the
indexed catalog/workload"), **this phase's first implication-rule class
is deliberately scoped to Brand-only implied facts**: `trigger phrase ->
Brand=X`, tested on real query/catalog evidence. This is narrower than
Issue #16's own multi-fact illustrative example, stated here explicitly
rather than silently substituted, per this project's "if coverage stays
small, narrow the claim instead of overstating it" discipline. The
generic rule *representation* (see P4-E00) still supports multiple
implied facts per trigger, so a second real catalog with genuine
structured `product_type`/`category` data could exercise the same
mechanism's multi-fact capability without a redesign — this phase's
narrowing is an evidence-availability limit, not a representation limit.

## Falsifiable hypothesis (stated before implementation, per this
## project's autonomy contract)

Among queries Issue #14's admission mechanisms currently reject (94.20%
of the real 22,458-query corpus), a nonzero, real, and *safe* subset can
be converted to admission-eligible by adding exactly one offline-
proposed, historically-replay-validated, promoted Brand-implication fact
derived from real title-phrase-to-brand co-occurrence
(`cold_start::prefill`'s existing signal), compiled into a static,
versioned lookup table consulted before admission — without exceeding
Issue #14's own RQ2 relevance-degradation budgets (0%/0.5%/1%/2%), and
without a materially increased false-positive/over-constraint rate
relative to the mechanisms Phase 3 already measured.

**Falsification conditions, stated up front**: if promoted rules recover
negligible coverage (mirroring P3-E12's BOUNDED finding for frequency-
resolved ambiguity), or if the false-positive/over-constraint rate proves
categorically worse than Phase 3's own worst KEPT mechanism (P3-E05's
15.35% at unlimited cap), this is REJECT/BOUNDED, not a claim of success
overstated to fit the hypothesis.

## Measurement plan (defined before implementation)

Following the exact P3-Ex discipline: RED-first tests for the new type
and every safety gate; full real-corpus replay (22,458 queries); overlap/
disjointness check against all three existing KEPT mechanisms before any
combined-coverage claim; relevance point estimate against real ESCI
judgments and live Solr; native latency; adversarial falsification per
Issue #16's own required list (wrong-brand over-constraint, ambiguous
product-family names, merchant-specific naming conflicts, generic-word-
vs-product-name collisions, mutually incompatible implied facts, stale/
withdrawn rules); explicit KEEP/REJECT/BOUNDED verdict; raw artifacts
preserved under `docs/research/artifacts/p4e{NN}_run1/`.

## Experiment index

- **P4-E00** — `ImplicationRule`/`ImplicationTable` type, compiled lookup,
  RED-first tests. (next)
- **P4-E01** — offline propose (real title-phrase-brand co-occurrence,
  reusing `cold_start::prefill`) + historical replay validation + promote.
- **P4-E02** — wire promoted implications ahead of the existing admission
  mechanisms; full real-corpus measurement against Issue #14's frontier.
- **P4-E03** — adversarial safety tests per Issue #16's required list.
