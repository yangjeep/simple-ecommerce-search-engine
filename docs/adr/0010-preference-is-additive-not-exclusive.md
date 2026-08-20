# ADR 0010: A `Preference` is additive, not exclusive — and confidence-tiered brand enforcement is REVISE, not KEEP

## Status

Accepted (Issue #6 P1-B, `docs/experiments/PHASE2_LOG.md` P2-E11).

## Context

Issue #6's reorientation named the concrete next semantic experiment as
testing the *enforcement* layer — not another canonicalization strategy —
after Issue #9/P2-E07–E10 established that better brand-string recognition
does not imply better hard-filter recall. P1-B tested a confidence-tiered
mechanism: a deterministic alias-normalized hard `Constraint`
(`StructuralConstraint::BrandAny`) for already-trusted brand strings that
share an identity after corporate-suffix/punctuation stripping, plus a
fuzzy, edit-distance-bounded soft `Preference`
(`Preference::StructuralBoost`) for otherwise-untrusted brand-shaped query
terms.

The first real-data run measured the fuzzy tier *regressing* relevance
(NDCG@10 0.2278→0.2095, Recall@10 0.1354→0.1248) rather than improving it.

## Decision

**A `Preference` is additive to lexical retrieval, never exclusive with
it.** `ir::query::apply_candidates` previously consumed a phrase resolving
to *only* a `Preference` exactly like a hard `Constraint` — removing it
from `residual_lexical`, and therefore from what any `LexicalDelegate`
ever sees. This was wrong by the `Preference` type's own stated contract
("a soft ranking signal compiled from the query but not enforced as a hard
filter") and is now fixed: a preference-resolved phrase stays in
`residual_lexical` too. The bug had zero prior real-data exposure — I7-E04
already found `compile_lexicon` never emitted a real `Preference` at any
threshold before this phase — so it existed, silently unreachable, since
whichever commit first added `Preference` resolution to `compile()`. P1-B's
fuzzy tier was simply the first real caller to exercise it. This is
recorded as a genuine architectural correction, not a tuning parameter: any
future producer of `Preference` candidates inherits the corrected,
type-honest semantics automatically, and does not need to rediscover this.

**The confidence-tiered *mechanism* is kept as a reusable primitive; the
specific enforcement thesis it was built to test is REVISE, not KEEP.**
`StructuralConstraint::BrandAny` and `Preference::StructuralBoost` are
real, bug-free, unit-tested generalizations of the existing `Brand`/
`Preference::Boost` types — genuinely useful extension points for a future
alias-grouping or soft-signal need — but on the real ESCI catalog, at both
trust thresholds tested, neither tier moved relevance, route coverage, or
candidate-set reduction beyond noise level. `brand_recall_gap_diagnostic.rs`'s
real-data root-cause breakdown found why: alias/spelling variance (the
entire territory these two enforcement tiers address) explains only
~1–5% of real brand-filter recall misses against judged-Exact products.
P2-E10's own "spelling/aliasing/formatting variation" framing was
directionally correct but quantitatively minor on this catalog — the
dominant ~95% of misses split into causes no enforcement-mechanism
refinement can address: generic English words mis-recognized as brands
(a canonicalization false-*positive* problem), sub-brand/product-line
naming (a containment-matching gap this design did not attempt), franchise/
media-property vs. licensed-manufacturer mismatch (needs real
entity-relationship knowledge, not string similarity), and missing brand
data entirely.

**The next P1 experiment is predictive semantic prefill (Issue #6 P1-C),
not a third enforcement-mechanism variant.** The franchise/manufacturer-
mismatch and missing-brand-data failure modes this ADR's diagnostic found
are concretely what P1-C is positioned to investigate: inferring latent
commerce structure (e.g. a franchise name implying a brand) that is not
literally present as a lexicon-resolvable phrase in either the query or a
simple string-similarity relationship to one.

## Consequences

- `commerce_core` gained two new domain-model variants
  (`StructuralConstraint::BrandAny`, `Preference::StructuralBoost`) and one
  new module (`cold_start::alias`), all unit-tested, no new dependencies.
- `apply_candidates`'s corrected semantics changed the *measured* value of
  `measure_coverage`'s "fully resolved" count against
  `REPRESENTATIVE_QUERY_SET` (12→10) and related fixture numbers in three
  other test files — a real, explained behavior change, not test drift;
  each updated assertion carries a comment tracing it back to this fix.
- No production default changes: `compile_lexicon` (Issue #9's baseline)
  remains what nothing in this campaign has yet displaced. The alias/fuzzy
  lexicon-compilation path (`compile_lexicon_with_alias_enforcement`) exists
  and is validated, but is not wired into any default pipeline — it is
  available for a future caller with a different real-data profile where
  alias variance genuinely dominates, not assumed to be dead code.
- `crates/bench-harness` (statistical rigor infrastructure) and
  `round1_eval::query_taxonomy` (the 9-class structural-shape taxonomy),
  built to support this experiment's own decision-grade measurement, are
  reusable for every subsequent Issue #6 experiment — the real
  infrastructure investment this cycle produced, independent of P1-B's own
  REVISE verdict.

## Alternatives considered

- **Ship the fuzzy tier as a production default once the `apply_candidates`
  bug was fixed, since the regression was gone.** Rejected: "no longer
  actively harmful" is not the same evidentiary bar as "materially helps."
  The post-fix numbers are noise-level, and the root-cause diagnostic
  independently explains why — shipping it would add real lexicon-build
  cost (mitigated, not eliminated, by the length-difference edit-distance
  prefilter added in the same cycle) for no measured benefit on this
  catalog.
- **Extend `alias_key` with a containment/prefix check to catch the
  sub-brand/product-line pattern (item 2 of the diagnostic's four causes)
  before moving on.** Deferred, not rejected: recorded as a small,
  well-scoped follow-up in `docs/experiments/PHASE2_LOG.md` P2-E11, but
  P2-E10's and this ADR's own "next highest-information experiment"
  discipline both point to P1-C as more informative right now — the
  containment fix would address a real but narrow (item 2 was a minority
  of the already-minority alias-explainable bucket) slice of the gap.
- **Treat the regression as disqualifying and abandon confidence-tiered
  enforcement entirely, including the type-level `Preference` fix.**
  Rejected: the regression was a bug in never-before-exercised code, not a
  property of the enforcement-tiering idea itself — conflating the two
  would have thrown away a real, generally-applicable correctness fix
  along with a specific experiment's negative result.
