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

## P4-E00 — `ImplicationRule`/`ImplicationTable` type + RED-first tests

**Evidence class**: mechanism only, unit-tested (no real data needed for
the type itself).

New `commerce_core::control_plane::implication` module:
`ImplicationRule { trigger, implies: Vec<ResolvedConstraint>, provenance,
confidence, status }` and a compiled `ImplicationTable` that only ever
stores `Promoted` rules -- `ImplicationTable::compile` silently drops any
`Candidate`/`Withdrawn` rule at construction, so the online serving path
(`apply_implications`) is structurally incapable of applying an
unvalidated or retracted rule, mirroring
`control_plane::provider::ModelProvider`'s own "enforced by where it's
called from" discipline. `apply_implications` reuses
`cold_start::prefill::apply_predictive_prefill`'s established safety rule
(never override an explicit Brand/BrandAny constraint) and adds a second,
Issue #16-required one: if two matched triggers in the same query imply
different Brand values, abstain entirely rather than guessing.

8 RED-first tests: normal application; a never-promoted candidate rule
never applies; a withdrawn rule never applies (even after having been
promoted); explicit-brand suppression; brand-disagreement abstention;
brand-agreement across two triggers applies normally; no match; an empty
table short-circuits. 35/35 `commerce-core` tests pass (27 pre-existing +
8 new).

**Decision**: KEEP the mechanism (type only, no real-data verdict yet).
Next: P4-E01's offline propose/replay/promote pipeline.

## P4-E01 — offline propose/replay/promote: a first, small, real, zero-false-positive Brand-implication result

**Evidence class**: real (full 1,215,854-product catalog, full
22,458-query judged corpus, a real Tantivy title-only index built fresh
for candidate proposal). No new Solr querying: reuses P3-E06's
already-persisted `whole_corpus_solr_ndcg.csv` for every query's own real
Solr score, exactly like P3-E08–E17 do.

**Hypothesis** (stated in this log before implementation, per this
project's autonomy contract): among queries Issue #14's admission
mechanisms currently reject, a nonzero, real, safe subset can be
converted to admission-eligible by adding one offline-proposed,
replay-validated, promoted Brand-implication fact derived from real
title-phrase-to-brand co-occurrence, without exceeding Issue #14's RQ2
budgets or showing a categorically worse false-positive rate than Phase
3's own worst KEPT mechanism (15.35%, P3-E05 at unlimited cap).

**Method**: `phase4-eval::bin::p4e01_implication_propose_replay_promote`.

1. **Baseline** (fixed, held constant across baseline and treatment): all
   three existing admission mechanisms at P3-E16's own promoted
   `<=2.0%`-budget three-way operating point (`structural_cap=2,
   anchored_cap=20, single_token_cap=10`) -- 1,303 baseline-admitted,
   21,155 baseline-rejected, exactly matching P3-E16's own reported 1,303
   admitted/5.80% coverage figure (a direct cross-check that this
   experiment's admission replication is correct).
2. **Propose**: scan every baseline-rejected query's raw text for
   2-3-word windows (74,305 unique phrases found); for each, compute
   `cold_start::prefill::predict_brand_from_phrase` against a real
   title-only Tantivy index over this same catalog (built fresh, 4-5s).
   A phrase becomes a `Candidate` rule if it clears a purity/occurrence
   threshold and is not simply the predicted brand's own name (P1-C's
   existing rule, reused).
3. **Replay**: for each candidate rule independently, apply it alone (a
   solo `ImplicationTable`) to every query whose raw text contains its
   trigger, check admission at the same fixed caps, and for every
   newly-admitted query execute natively and score NDCG@10/Recall@10/MRR
   against the real ESCI judgments, comparing to that query's persisted
   real Solr score.
4. **Promote**: a rule promotes only if it recovers >=1 query and its own
   false-positive rate (native NDCG==0 while Solr found >=1 relevant
   result, P3-E05/E09's own definition) does not exceed 15.35%.
5. **Combined measurement**: apply every promoted rule together (so
   `apply_implications`'s cross-trigger abstention logic is exercised,
   not just each rule in isolation).

### A real bug self-caught before trusting the first result: the missing-brand-field sentinel

The first real run (thresholds purity>=0.9/occurrence>=20, matching
`prefill_eval.rs`'s own real-run tier) promoted 24 rules -- inspecting
the promoted-rule report before trusting it (this project's own "actively
try to kill every favorable result" discipline) found **7 of the 24
(29%) were spurious**: phrases like "james patterson", "romantic
comedy", "thriller series", "kindle unlimited" all "implied" `BrandId(0)`
-- `round1_eval::catalog::build_catalog`'s own sentinel for "this real
product has no brand field at all" (`brand.unwrap_or(BrandId(0))`).
Generic book/media phrases are overwhelmingly common in exactly this
unbranded slice of the real catalog, so they scored a spuriously high
"purity" toward the sentinel -- asserting `Brand=BrandId(0)` is not a
genuine trigger-implies-brand fact, it means "this phrase correlates with
missing brand data." This is the same real-catalog data-quality hazard
P2-E15/P3-E02 already found for diaper products' missing `size`
attribute, recurring here for a different field. Fixed by excluding
`BrandId(0)` from candidate proposals outright; rerunning confirmed the
fix (0/16 promoted rules were the sentinel afterward, at the tight
threshold). **This risk is not necessarily unique to this experiment**:
`cold_start::prefill`'s own already-shipped `predict_brand_from_phrase`
(P1-C, NARROW verdict) calls the identical function and has no such
exclusion either -- flagged as an unresolved risk below, not retroactively
re-audited here (out of this phase's scope per "do not re-run superseded
historical work").

### Result — a small, sensitivity-checked, zero-false-positive real coverage gain

| candidate thresholds | candidates generated | promoted | newly admitted | coverage (% of whole corpus) | native NDCG (mean) | Solr NDCG (mean, same subset) | false positives | isolated marginal degradation | combined degradation (stacked on P3-E16's 1.98%) |
|---|---|---|---|---|---|---|---|---|---|
| purity>=0.9, occurrence>=20 (tight, matches `prefill_eval.rs`'s own real-run tier) | 314 | 16 | 17 | 0.08% | 0.4411 | 0.6050 | 0/17 (0.00%) | 0.0531% relative | 2.04% relative |
| purity>=0.8, occurrence>=10 (loose) | 813 | 108 | 85 | 0.38% | 0.5769 | 0.6841 | 0/85 (0.00%) | 0.174% relative | 2.16% relative |

Both threshold points share the same qualitative shape: **zero false
positives** (every admitted query where native NDCG=0, Solr also found no
relevant result -- native never uniquely failed where Solr succeeded),
but a real, substantial per-query ranking-quality gap on the admitted
subset (native NDCG meaningfully below Solr's own NDCG on the identical
queries) -- the same "no ranking signal" pattern (`execute_ranked` has no
signal when `query.preferences` is empty, P2-E17's original finding)
every lexical-narrowing-based admission mechanism in this campaign has
shown. **Isolated (measured the way every other Phase 3 mechanism is
measured, i.e. against the pure-Solr-only background, not stacked on an
already-tight baseline), implications' own marginal contribution clears
every RQ2 budget by a wide margin at both threshold points** (0.05%/0.17%
relative, versus a 2% budget). It is only when **stacked on top of
P3-E16's own already-tight `<=2.0%`-budget three-way baseline** (itself
sitting at 1.98%, per P3-E17's own finding that this exact point's CI
already crosses 2%) that the combined total (2.04%/2.16%) nudges over the
nominal 2% line -- the identical "a mechanism whose own isolated
measurement clears budget comfortably can still push a shared budget over
when combined with an already-near-the-edge baseline" pattern P3-E10
first demonstrated.

The loose-threshold sweep recovers 5x the coverage (85 vs 17 admitted)
at a comparable, still-tiny isolated cost, with no new false positives
and no recurrence of the sentinel-brand pattern (verified directly against
the promoted-rule report) -- adopted as this phase's default going
forward. A representative sample of promoted rules at this threshold:
"north face"->a real apparel brand, "la roche"/"la roche posay"/"roche
posay"->a consistent real skincare brand across all three phrasings,
"fisher price"->a real toy brand, "porter cable"->a real power-tools
brand, "bowers wilkins"->a real audio brand, "dr browns"->a real baby
brand -- every one a genuine, real-catalog-grounded product-line/model-to-
brand fact, not a string coincidence.

**Decision**: **KEEP the propose/replay/promote mechanism and the
sentinel-exclusion fix; the mechanism produces real, zero-false-positive
implication rules from real data.** The *combined-with-P3-E16's-baseline*
operating point (0.38% additional coverage, pushing total degradation
from 1.98% to 2.16%) is a genuine, small, marginal-over-budget result --
disclosed plainly, not smoothed over, matching P3-E16/E17's own honest
framing rather than either overstating success or hiding the overshoot.
Per Issue #16's own success criteria, this is not yet a "statistically/
reproducibly meaningful increase" at whole-workload scale (0.38% of
94.20% currently-rejected traffic), but it *is* the first mechanism in
this entire research campaign (Phase 3 and Phase 4 alike) whose own
false-positive rate on newly-admitted real queries is genuinely zero --
qualitatively different from every lexical-narrowing mechanism measured
before it, all of which showed some nonzero false-positive rate.

**Unresolved risk, not closed here**: `cold_start::prefill`'s
already-shipped `predict_brand_from_phrase` (used by P1-C, NARROW
verdict) has no `BrandId(0)`-sentinel exclusion. Whether this materially
affected P1-C's own P2-E12 numbers is untested -- out of this phase's
scope to re-audit (Issue #18's "do not re-run superseded historical work"
), but worth a human decision on whether to revisit.

Raw artifacts: `docs/research/artifacts/p4e01_run1/` (both threshold
runs' logs, the loose-threshold run's `rule_report.csv`/
`per_query_report.csv`).

**Next**: P4-E02 hardens the loose-threshold propose/replay/promote
pipeline's output into the deployable, compiled-lookup shape Issue #16
itself asks for. P4-E03 runs Issue #16's full required adversarial-safety
list as dedicated fixture tests, since this real-data run already
surfaced one real adversarial case (the sentinel brand) organically
rather than needing a synthetic fixture to find it.

## P4-E02 — hardening: compiled-artifact reproducibility, disjointness verification, native latency

**Evidence class**: real, whole-workload -- loads *only* P4-E01's
already-persisted promoted-rule CSV (no title index, no candidate
generation, no replay logic anywhere on this path -- the exact
deployable shape Issue #16 itself specifies: "query span -> compiled
implication lookup -> ... -> native execute OR immediate Solr
fallback").

**Hypothesis**: P4-E01's propose/replay/promote pipeline and its
resulting coverage/degradation numbers should be exactly reproducible
from the compiled artifact alone, with implication-admitted queries
verifiably (not just assumedly) disjoint from baseline-admitted ones,
and `apply_implications`'s own native cost should be cheap.

**Method**: `phase4-eval::bin::p4e02_compiled_table_latency_and_reproducibility`
loads `docs/research/artifacts/p4e01_run1/rule_report_loose_threshold.csv`,
filters to `PROMOTE` rows, reconstructs each `ImplicationRule`, and
compiles the table via `ImplicationTable::compile` -- then re-runs the
same real-corpus measurement, explicitly checks disjointness, and times
`apply_implications` in isolation versus the full admit-and-execute path.

### Result

1. **Reproducibility: exact.** 85/22,458 newly admitted (0.38% coverage),
   0 false positives, whole-workload degradation 2.16% relative --
   identical to P4-E01's live pipeline in every figure.
2. **Disjointness: verified, not assumed.** `assert_eq!(overlap, 0)`
   against the real computed intersection of implication-admitted and
   baseline-admitted qid sets (both real `HashSet<u64>`, not a
   by-construction claim left unchecked) -- confirmed 0.
3. **`apply_implications` itself is cheap, isolated separately from
   execution**: mean=0.0006ms, p99=0.0032ms over 600 samples (30 reps x
   20 queries) -- a pure in-memory phrase-window/hashmap lookup, as
   expected.
4. **The full admit-and-execute path is real, and slower than expected,
   disclosed rather than glossed over**: mean=0.0504ms, p50=0.0367ms,
   p99=0.1946ms -- roughly 80x the isolated enrichment cost, and
   30-45x P3-E02/E05's own previously-reported ~0.0011-0.0015ms
   small-candidate-set figure. Candidate-set sizes for this same sample
   (1-15) are comparable to P3-E02/E05's own `<=10`-candidate latency
   sample, so candidate-set size alone does **not** explain the gap --
   stated honestly rather than guessed at: this experiment did not
   isolate how much of the remainder is
   `admit_structurally_anchored_lexical`'s own lexical-narrowing
   execution path (two index lookups plus a bitmap intersection plus a
   re-verification pass, never itself benchmarked at this fine a grain
   in Phase 3) versus this loop's own measurement overhead. The one
   claim this experiment *does* support directly: `apply_implications`
   itself is not the cost driver, whatever the remainder turns out to be
   -- it is a negligible fraction of the full-path latency either way,
   and even the full-path p99 (0.19ms) remains two orders of magnitude
   below a Solr round-trip (~2.5ms mean, per every prior Phase 3
   fallback-tax measurement), so this is not a concern for the
   fallback-tax invariant regardless of its unresolved root cause.

**Decision**: KEEP. The compiled-artifact deployment shape works exactly
as specified (reproducible, disjoint by verification, cheap enrichment
step); the full-path latency gap is a real, disclosed open question
(flagged, not resolved) that does not threaten Issue #14's fallback-tax
invariant at the magnitudes measured.

Raw artifacts: `docs/research/artifacts/p4e02_run1/`.

**Next**: P4-E03 runs Issue #16's full required adversarial-safety list
as dedicated fixture tests (wrong-brand over-constraint, ambiguous
product-family names, merchant-specific naming conflicts, generic-word/
product-name collisions, mutually incompatible facts, stale/withdrawn
rules) -- P4-E00's own unit tests already cover disagreement-abstention
and withdrawn/candidate-never-applied; P4-E01 already surfaced one real
adversarial case (the sentinel brand) organically. P4-E03 closes the
remaining required cases with explicit fixtures.

## Experiment index

- **P4-E00** — `ImplicationRule`/`ImplicationTable` type, compiled lookup,
  RED-first tests. KEEP.
- **P4-E01** — offline propose (real title-phrase-brand co-occurrence,
  reusing `cold_start::prefill`) + historical replay validation + promote.
  KEEP the mechanism; small, real, zero-false-positive coverage gain,
  marginally over budget only when stacked on P3-E16's own already-tight
  baseline.
- **P4-E02** — hardening: compiled-artifact reproducibility (exact),
  disjointness (verified), native latency (enrichment cheap, full-path
  gap disclosed unresolved). KEEP.
- **P4-E03** — adversarial safety tests per Issue #16's required list.
  (next)
