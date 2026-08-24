# Phase 4 Decision

**Decision: NARROW SUPPORT** for Issue #16's thesis, on the real
catalog/query evidence gathered this phase, for its first (Brand-only)
implication-rule class. A real, offline-proposed, historically-replay-
validated, compiled implication mechanism exists, produces genuinely
zero-false-positive real coverage on top of Issue #14's already-tight
frontier, and closes every one of Issue #16's own required adversarial-
safety cases -- including one real bug this phase's own adversarial
review found and fixed before it could reach a production table. The
recovered coverage (0.38% of the whole corpus, 85 of 21,155 currently-
rejected queries) is small in absolute terms and, stacked on Phase 3's
own already-budget-tight three-mechanism baseline, pushes total
degradation marginally over the nominal 2% line -- disclosed, not
smoothed over. This is not yet the "statistically/reproducibly
meaningful increase" Issue #16's own success criteria describe at
whole-workload scale, but it is a real, qualitatively different result
from every mechanism Phase 3 measured: the first with a genuinely zero
false-positive rate.

This document is Phase 4's terminal decision artifact for the work done
so far, in the same spirit `PHASE2_DECISION.md`/`PHASE3_DECISION.md`
closed their own phases -- not overwriting either, and not closing Issue
#16 as a whole (its own multi-fact, multi-class scope remains open for a
catalog with real structured multi-entity data), but synthesizing what
this phase's first implication-rule class (`docs/experiments/PHASE4_LOG.md`
P4-E00 through P4-E03) found.

## Recap: what this phase was asked to answer

Issue #16 asked whether an offline-proposed, historically-replayed,
validated, compiled semantic implication rule could supply the real
ranking/precision signal Phase 3's own admission family lacked, moving
some of the 94.20% of real traffic Issue #14's three mechanisms reject
onto the native FastPath without violating its relevance/correctness
budgets. Four sub-questions, all now answered for this phase's
deliberately narrowed (Brand-only) scope:

- **Does a generic, multi-fact-capable rule representation exist,
  separate from ordinary canonicalization/prefill?** Yes (P4-E00):
  `ImplicationRule`/`ImplicationTable`, a conjunction-of-facts type
  distinct from `ir::lexicon::Candidate` (competing alternatives) and
  from `cold_start::prefill` (live, inline, single-fact inference).
- **Can candidate rules be proposed offline, without a model call, and
  validated before promotion?** Yes (P4-E01): real title-phrase-to-brand
  co-occurrence (`cold_start::prefill`'s existing signal) proposes
  candidates; per-rule replay against real ESCI judgments and Solr scores
  gates promotion on a stated false-positive ceiling.
- **Does the compiled artifact actually reproduce the live pipeline's
  result, stay cheap, and stay disjoint from existing mechanisms?** Yes
  (P4-E02), all three verified directly, not assumed.
- **Is the mechanism safe against Issue #16's own required adversarial
  list?** Yes (P4-E03), after fixing one real bug this phase's own
  review found (a same-trigger conflict between two disagreeing promoted
  rules would previously have silently picked one arbitrarily).

## Architecture tested

`commerce_core::control_plane::implication` (new this phase):
`ImplicationRule { trigger, implies: Vec<ResolvedConstraint>, provenance,
confidence, status }` and `ImplicationTable::compile`, which only ever
admits `Promoted` rules and excludes (abstains on) any trigger where
multiple promoted rules disagree. `apply_implications` is a pure,
deterministic phrase-window/hashmap lookup consulted ahead of Issue #14's
three existing admission mechanisms -- no model call, no live index
access, matching Issue #16's own required online/offline separation.

The offline side (`crates/phase4-eval`) reuses `cold_start::prefill`'s
already-validated (P1-C) real title-phrase-to-brand co-occurrence signal
as the candidate proposer -- no new catalog-mining infrastructure was
built where an existing, real, zero-model-call signal already existed.

**Scope, stated explicitly and load-bearing throughout this phase**: the
real ESCI catalog's `product_type`/`category` fields are always sentinel
(`round1_eval::catalog`'s own documented limitation, reconfirmed this
phase). This phase's rules therefore only ever imply a single Brand fact
each -- narrower than Issue #16's own illustrative multi-fact example
("air force 1" -> Brand + ProductType + ProductLine). The type itself
supports multiple simultaneous implied facts; only real data to validate
a second fact kind is missing, not the mechanism.

## Datasets / workloads

The same real evidence base Phase 2/3 established, reused unchanged: the
real 1,215,854-product Amazon ESCI catalog, the real 22,458-query
human-judged corpus, and (no new Solr querying this phase) P3-E06's
already-persisted whole-corpus Solr NDCG CSV. A real Tantivy title-only
index was built fresh (4-5s) for candidate proposal, reusing the same
approach `phase2_eval::prefill_eval` already validated.

## Measured results

Full tables and raw artifacts: `docs/experiments/PHASE4_LOG.md` P4-E00-
E03, `docs/research/artifacts/p4e0{1,2}_run1/`.

| Question | Evidence | Result |
|---|---|---|
| Rule representation (multi-fact-capable) | P4-E00 | KEEP -- type built, 10 RED-first tests |
| Offline propose/replay/promote, real signal | P4-E01 | KEEP -- 813 candidates, 108 promoted, 85/22,458 (0.38%) newly admitted, 0 false positives |
| Compiled-artifact reproducibility/disjointness/latency | P4-E02 | KEEP -- exact reproduction, verified 0 overlap, enrichment step cheap (0.0006ms); full-path latency gap disclosed unresolved |
| Adversarial safety (Issue #16's full required list) | P4-E03 | KEEP -- all 6 cases closed; 1 real bug found and fixed (same-trigger conflict) |
| Relevance impact on newly-admitted queries | P4-E01/E02 | Real, substantial per-query ranking-quality gap (native NDCG 0.5769 vs Solr 0.6841 on the same 85 queries) -- the same "no ranking signal" pattern every Phase 3 lexical mechanism showed, but with zero outright failures |
| Whole-workload budget impact | P4-E01/E02 | Isolated marginal contribution clears every RQ2 budget with wide margin (0.174% relative vs 2%); stacked on P3-E16's own already-tight 1.98% baseline, combined total reaches 2.16%, marginally over budget |

## Failed experiments / self-caught bugs (preserved, not erased)

Two real defects were found and fixed this phase, both via this
project's own "actively try to kill every favorable result" discipline
applied *before* trusting a result, not after a bug report:

1. **The missing-brand-field sentinel** (P4-E01): the first real run
   (tight thresholds) promoted 24 rules; inspecting the report before
   trusting it found 7 (29%) were spurious matches to `BrandId(0)`,
   `round1_eval::catalog`'s sentinel for "this real product has no brand
   field at all." Generic book/media phrases ("james patterson",
   "thriller series") scored a spuriously high catalog "purity" toward
   the sentinel. Fixed by excluding `BrandId(0)` from candidate
   proposals. **Flagged, not retroactively fixed**: `cold_start::prefill`'s
   already-shipped `predict_brand_from_phrase` (used by P1-C, NARROW
   verdict) has the identical exposure and no such exclusion -- whether
   this materially affected P1-C's own P2-E12 numbers is untested, left
   as an explicit unresolved risk rather than re-litigated (out of this
   phase's scope per "do not re-run superseded historical work").
2. **The same-trigger silent-overwrite bug** (P4-E03): `ImplicationTable::compile`'s
   naive `HashMap` collection would have silently kept an arbitrary one
   of two disagreeing promoted rules sharing a trigger -- never
   previously exercised because every prior test/real run only produced
   one promoted rule per trigger. Fixed RED-first before any production
   use of the table, not found via a failing real-data run (no real rule
   set has hit this yet) but via direct adversarial code review, exactly
   the discipline Issue #16/#18 both call for.

An unresolved, honestly-disclosed measurement gap (not a defect):
P4-E02's full admit-and-execute latency (mean 0.0504ms) is ~80x its own
isolated `apply_implications` cost and 30-45x Phase 3's own previously-
reported small-candidate-set figure, and candidate-set size for the same
sample does not explain the gap. Not resolved here -- stated as an open
question, since even the unexplained full-path p99 (0.19ms) remains two
orders of magnitude below a Solr round-trip and does not threaten the
fallback-tax invariant at the scale measured.

## Unresolved risks

1. **The recovered coverage is small in absolute terms** (0.38% of the
   whole corpus, 0.40% of currently-rejected traffic) -- real and
   genuinely zero-false-positive, but far from Issue #16's own "moves a
   substantial portion of currently-fallback traffic" strong-result bar,
   and far from Issue #18's >=50%/P50 north star for search coverage.
2. **Combined with Phase 3's own tightest baseline, total degradation
   sits marginally over the nominal 2% budget** (2.16% vs 2.0%) -- a
   deployment-margin decision (per P3-E17's own precedent), not resolved
   to a specific "back off by X" recommendation here.
3. **The missing-brand-field sentinel's potential effect on P1-C's own
   already-published numbers is untested** -- flagged, not re-audited.
4. **The full admit-and-execute latency gap is real and unexplained** --
   candidate-set size does not account for it; whether it is
   `admit_structurally_anchored_lexical`'s own execution path cost or
   measurement-loop overhead was not isolated.
5. **Only one implication class (product-line/model phrase -> Brand) has
   been tested.** Issue #16 names several other classes (collection/
   family -> brand, colloquial phrase -> typed attribute/product type,
   context-dependent implications) -- untested here, deliberately, since
   this catalog's own field population (Brand-only real signal) does not
   support validating most of them.
6. **No genuinely multi-merchant real catalog exists to validate a typed
   `scope` field against** -- deferred rather than built speculatively
   (P4-E03's own explicit scope note).
7. Every unresolved risk `PHASE2_DECISION.md`/`PHASE3_DECISION.md`
   already named and this phase didn't touch remains open (no incremental
   index update path; no concurrent-load benchmarking; single shared/
   virtualized environment; `FastPath`'s zero-ranking-signal relevance
   cost, still unfixed and the root cause of this phase's own per-query
   ranking gap too).

## What would be built next if scaling up (conditional -- see decision)

1. **A second implication class**, once a real catalog with genuine
   structured `product_type`/`category`/`ProductLine` data is available
   to validate against -- this phase's own `ImplicationRule` type already
   supports multi-fact rules without a redesign.
2. **A default ranking signal for the admitted subset** (the same
   unfixed Phase 2/3 gap): would directly reduce the real per-query
   NDCG gap this phase's own admitted queries still show (native 0.5769
   vs Solr 0.6841), independent of the implication mechanism itself.
3. **Root-causing the full-path latency gap** this phase disclosed but
   did not resolve, before treating the ~0.05ms figure as a stable
   production estimate.
4. **A deployment-margin decision** on how far to back off from Phase
   3's own tightest baseline before adding this phase's implications on
   top, given the combined 2.16% figure.
5. **Revisiting `cold_start::prefill`'s own sentinel-brand exposure**,
   flagged but not fixed this phase.

## What should explicitly not be built yet

- **A `scope` field or any multi-merchant machinery** -- no real data to
  validate it against; would be exactly the "designing for hypothetical
  future requirements" CLAUDE.md warns against.
- **A ProductType/Category-implying rule class** on this specific real
  catalog -- those fields are always sentinel; any such rule would be
  unvalidatable against real data by construction.
- **A production LLM-backed proposer** or any model call in the online
  path -- untouched by this phase's results either way, per CLAUDE.md's
  hard rule.
- **Chasing more coverage within this exact Brand-only class via looser
  thresholds alone** -- the sensitivity sweep already done (P4-E01,
  purity>=0.8/occurrence>=10 vs >=0.9/occurrence>=20) shows diminishing,
  not qualitatively new, returns; a materially different class or signal
  source is the next lever, not further threshold tuning on this one.

## What this decision does and does not claim

**Claims**: on the real 1,215,854-product Amazon ESCI catalog and real
22,458-query human-judged corpus, an offline-proposed, historically-
replay-validated, compiled Brand-implication mechanism exists, is
disjoint from and stacks additively on Issue #14's own three admission
mechanisms (verified, not assumed), recovers a real, small, genuinely
zero-false-positive coverage gain, and passes every one of Issue #16's
required adversarial-safety tests -- including a real bug this phase's
own review found and fixed before any production exposure. Two real
defects were found and fixed via self-directed adversarial review before
either could have produced a misleading result.

**Does not claim**: that this recovers a "substantial portion" of
currently-fallback traffic (0.38% of the whole corpus, far from it); that
the combined system safely clears Phase 3's own 2% budget once stacked
(it does not, marginally); that this phase's Brand-only scoping
generalizes to the multi-fact ProductType/ProductLine examples Issue #16
itself illustrates (untested, and unvalidatable against this specific
real catalog); that `cold_start::prefill`'s own already-shipped P1-C
mechanism is unaffected by the sentinel-brand exposure this phase found
(untested, flagged); or that Issue #16 as a whole is closed -- only that
its first, deliberately narrowed, evidence-scoped implication-rule class
has reached a defensible NARROW SUPPORT verdict, and the next real lever
is a different implication class or a different real catalog, not more
tuning of this one.
