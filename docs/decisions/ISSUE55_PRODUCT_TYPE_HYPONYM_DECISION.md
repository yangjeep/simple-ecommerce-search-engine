# Issue #55 — whole-word product-type hyponym expansion (`ProductTypeAny`)

Log: `docs/experiments/ISSUE55_PRODUCT_TYPE_HYPONYM_LOG.md`. Protocol:
`docs/experiments/ISSUE55_PRODUCT_TYPE_HYPONYM_PROTOCOL.md`.

## Verdict: REJECT (the unconditional wiring) — infrastructure KEPT, unwired

A whole-word hyponym-expansion mechanism (admit a broader product-type
query to any other real catalog product-type whose word set is a proper
superset — e.g. `{"beds"}` admitting `{"kids","beds"}`) was implemented
exactly as preregistered and **cleared both named gates by a wide
margin**: candidate-set relevant-document recall improved +24.06pp
(0.4562 -> 0.6968, far above the >=5pp bar), and `p9_e04`'s H1/H3
ranking-quality and speed checks continued to hold (native NDCG still
not worse than Solr's on the identical candidate set; native still
>=2x faster). This is the largest single effect size measured across
every checkpoint this session.

**It is rejected anyway.** A targeted real-vocabulary audit
(`p9_e08_hyponym_group_false_family_audit`, run specifically to execute
the protocol's own preregistered "no wrong-family regression" gate as a
direct check rather than an inference from aggregate NDCG) found the
mechanism produces **confirmed, real cross-family false positives** on
this catalog's actual vocabulary — not synthetic edge cases:

- `"beds"` admits `"cat beds"` / `"dog beds & mats"` (furniture vs. pet
  products — genuine cross-vertical lexical polysemy in this catalog).
- `"candles"` admits `"...candles & holders / scented oils & diffusers"`
  and `"hot tubs"` admits `"...hot tubs & saunas / saunas"` — the word
  only appears in an *ancestor* category-breadcrumb segment of a sibling
  leaf product, not in the leaf product's own name.
- `"bed accessories"` and `"bath accessories"` (two unrelated broader
  terms) both admit `"...shower curtain hooks"` / `"...shower curtains
  & shower liners"` — the word-SET subset test does not require matched
  words to be adjacent, so two words from non-adjacent, unrelated path
  segments can each independently satisfy it.

CLAUDE.md is explicit that "Cross-variant false matches are bugs" and
that correctness is non-negotiable; the protocol itself named "no
wrong-family regression" as a gate. A mechanism that admits pet-bed
products into a furniture-bed search, or diffuser products into a
candle search, on real catalog data, fails that gate regardless of how
large its recall win is elsewhere. Per CLAUDE.md's research discipline:
"Negative results are first-class outputs. Do not turn a failed gate
into a feature roadmap."

## Why this needed a dedicated real-vocabulary audit, not just the aggregate NDCG gate

The protocol assumed an NDCG regression would surface a false-positive
problem ("already part of the harness's gate elsewhere"). It did not:
H1's NDCG@10 *improved* with the false positives live in the candidate
pool, because native's ranking signal still ranked genuinely relevant
documents above the injected false-family ones for the specific 15
queries this benchmark measures — NDCG@10 is top-10-weighted and a
minority of low-ranked false positives in a larger candidate pool does
not necessarily move it. **This is a generalizable methodology finding
for this project going forward**: an aggregate ranking-quality metric is
not a substitute for a direct, targeted structural/vocabulary audit when
a change alters *candidate-set membership* rather than *ranking* — the
two can move independently, and this checkpoint is a concrete,
reproduced example of NDCG staying flat-to-positive while a real
precision defect was live underneath it.

## What was kept vs. reverted

**Reverted** (the unsafe part): `compile_non_brand_lexicon`
(`crates/commerce-core/src/cold_start/profile.rs`) no longer calls
`product_type_hyponym_groups` when compiling the production lexicon —
every product type compiles to exactly its own `ProductType(id)`, the
pre-checkpoint-11 safe behavior. Confirmed by rerunning `p9_e04`
post-revert: mean candidate-set recall returns to the pre-checkpoint-11
baseline (0.4562), proving the revert is complete, not partial.

**Kept** (tested, correct, generically useful infrastructure, just not
wired into production today):

- `StructuralConstraint::ProductTypeAny(Vec<ProductTypeId>)` and its
  `matches`/`single_valued_slot`/`is_entity_constraint`/`structural_bitmap`
  support in `commerce-core` — structurally identical to, and no less
  safe than, the existing `BrandAny` (Issue #6 P1-B); the correctness
  risk found here is in the specific *hyponym-detection heuristic* that
  decides which ids to group, not in the `ProductTypeAny` matching
  primitive itself, which is a straightforward, already-tested set
  membership check.
- `product_type_hyponym_groups` (now `pub`, re-exported from
  `commerce_core::cold_start`) and its 5 correctness property tests
  (including a 500-trial randomized soundness+completeness check) — the
  *pure function* is exactly as sound as its own tests prove (every
  produced pair really is a genuine whole-word superset); the problem is
  that whole-word-superset is an insufficiently strict proxy for "is a
  kind of" on this catalog's real, breadcrumb-mixed vocabulary, not that
  the function has a bug.
- `p9_e08_hyponym_group_false_family_audit` — a reusable audit tool for
  any future hyponym/synonym-expansion mechanism proposed against this
  or another catalog.
- A new regression test,
  `product_types_with_a_whole_word_subset_relationship_never_merge_into_one_constraint`
  (`crates/commerce-core/tests/cold_start.rs`), locking in the safe
  default so a future change cannot silently re-wire the rejected
  heuristic without addressing the false-positive problem.

## Named follow-up (not implemented here)

The underlying insight — real WANDS relevance is measurably limited by
category-hierarchy/synonym mismatch
(`ISSUE55_PRODUCT_CLASS_INGESTION_DECISION.md`) and a hyponym-style fix
has a large *available* effect size (+24pp recall) if it can be made
precision-safe — is not falsified, only this specific implementation of
it. A future attempt should not rely on bag-of-words containment over
mixed clean-name/breadcrumb-path strings. Two structurally different
directions, either independently preregistered:

1. **True category-hierarchy prefix containment**: for the
   breadcrumb-path-derived product types specifically, admit B as a
   hyponym of A only when A's *full path* is a genuine ancestor prefix
   of B's *full path* in the real category tree (not a word-bag test) —
   this exactly captures "beds" -> "beds / king size beds" while
   structurally excluding sibling leaves like "candles" -> "scented oils
   & diffusers" (different branch, not a prefix). Does not by itself
   solve cross-vertical polysemy for clean, non-path product_class names
   like "beds" vs. "cat beds" (neither carries a shared category-tree
   ancestor path in this representation) — would need those excluded
   from participating in any hyponym relation at all, at the cost of
   losing part of the measured recall win (the "recliners" ->
   "furniture / .../ recliners / gray recliners" case, for example, mixes
   a clean broader name with a path-derived narrower one).
2. **Contiguous-phrase containment** (broader name's word sequence must
   appear as a contiguous, not scattered, subsequence in the narrower
   name) — cheaper to implement, fixes the "bed accessories"/"bath
   accessories" scattered-word class of defect, but does **not** fix
   the ancestor-breadcrumb-bleed class (a single-word broader term like
   "candles" is trivially "contiguous" wherever it appears) or the
   cross-vertical polysemy class ("beds" vs. "cat beds" both contain
   "beds" as a genuine contiguous match).

Neither direction fully closes both defect classes on its own; a real
fix likely needs to combine (1)'s hierarchy constraint with an explicit
restriction against ever treating a clean `product_class`-derived short
name as automatically hyponymous with an unrelated clean short name
(only path-vs.-path, hierarchy-verified pairs, or curated/explicit
groups analogous to `BrandAny`'s alias-key trust gate). This is
correctly scoped as its own preregistered experiment, not a speculative
addition here.

## Adversarial review

This finding is itself the product of the adversarial-review discipline
this session has used since checkpoint 8: the recall improvement (+24pp,
this session's largest effect size) was not accepted at face value.
Before recording a decision, the second preregistered gate was actually
executed as a direct real-vocabulary audit rather than inferred from
aggregate NDCG — exactly the audit the original protocol named but had
not yet run. That audit is what surfaced the disqualifying defect. No
separate multi-agent adversarial workflow was additionally run for this
specific finding: the false-family examples are demonstrated directly
and unambiguously from real catalog data (e.g. "bed accessories" and
"bath accessories" both literally resolving to the same shower-curtain
narrower set), not a subtle interpretive claim requiring independent
verification the way checkpoint 8's architectural-significance question
did.

- **Checked whether the false positives are hypothetical/synthetic
  rather than real**: no — every example quoted above is a literal line
  from `p9_e08`'s dump against the real 42,994-product WANDS catalog,
  not a constructed adversarial case.
- **Checked whether reverting could itself be an overcorrection (e.g.
  the false positives are rare enough to be immaterial)**: 36 of 245
  produced groups (14.7%) have a single-word broader term (the highest
  blast-radius shape), and the audit surfaced multiple distinct false
  positives within casual manual review of a few dozen rows — this is
  not a rare tail case.
- **Checked whether the revert silently left partial state (e.g. some
  callers still expecting `ProductTypeAny`)**: `structural_bitmap`,
  `single_valued_slot`, and `is_entity_constraint` all still handle
  `ProductTypeAny` correctly (harmless dead code path today, exercised
  by the direct unit/property tests and by `p3e14_solr_baseline_gap_audit`'s
  exhaustive match arms) — no partial-revert risk, since nothing in
  production ever constructs a `ProductTypeAny` value once
  `compile_non_brand_lexicon` no longer does.
- **Checked whether the diagnostic-classification bug fix in `p9_e04`
  (`has_entity_constraint` not recognizing `ProductTypeAny`) could itself
  have produced a misleading recall number**: no — that bug only affected
  one aggregate breakdown table (entity-vs-attribute-only Exact recall),
  not the primary recall/NDCG/latency numbers this decision is based on,
  which were computed identically before and after that particular fix.

## Traceability

Source: `crates/commerce-core/src/ir/structural.rs`,
`crates/commerce-core/src/ir/query.rs`,
`crates/commerce-core/src/index/mod.rs`,
`crates/commerce-core/src/cold_start/profile.rs`,
`crates/commerce-core/tests/cold_start.rs`,
`crates/phase9-eval/src/bin/p9_e04_isolated_ranking_and_execution.rs`,
`crates/phase9-eval/src/bin/p9_e08_hyponym_group_false_family_audit.rs`,
`crates/phase3-eval/src/bin/p3e14_solr_baseline_gap_audit.rs`. Raw
evidence: `docs/research/artifacts/i55_product_type_hyponym/`
(`p9_e04_after_fix.txt`, `p9_e04_after_fix_v2.txt` — diagnostic-fix
rerun, `p9_e04_after_revert.txt` / `p9_e02_after_revert.txt` —
safe-default confirmation, `p9_e08_false_family_audit.txt` — the full
real-vocabulary dump).
