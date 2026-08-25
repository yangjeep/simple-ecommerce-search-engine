# Issue #55 Experiment Log — whole-word product-type hyponym expansion (`ProductTypeAny`)

Protocol: `docs/experiments/ISSUE55_PRODUCT_TYPE_HYPONYM_PROTOCOL.md`.

## I55-HYPONYM-E00 — mechanism implemented, correctness property tests pass, and it clears both preregistered ranking/recall gates by a wide margin — but a targeted real-vocabulary audit (P9-E08) found confirmed cross-family false positives, disqualifying the unconditional wiring

**Implementation**

- `crates/commerce-core/src/ir/structural.rs`: added
  `StructuralConstraint::ProductTypeAny(Vec<ProductTypeId>)`, structurally
  identical to the existing, already-adversarially-reviewed `BrandAny`
  (Issue #6 P1-B): `matches` returns `ids.contains(&product.product_type)`.
- `crates/commerce-core/src/ir/query.rs`: `single_valued_slot` and
  `is_entity_constraint` updated to treat `ProductTypeAny` identically to
  `ProductType`.
- `crates/commerce-core/src/index/mod.rs`: `structural_bitmap` adds a
  `ProductTypeAny` arm unioning `product_type_bitmaps` across the listed ids.
- `crates/commerce-core/src/cold_start/profile.rs`: added
  `product_type_hyponym_groups` (pure function: for two catalog product-type
  names A, B, if A's whitespace-split word set is a **proper subset** of
  B's, B is added to A's hyponym group) and initially wired it into
  `compile_non_brand_lexicon` so any product type with a non-empty hyponym
  group compiled to `ProductTypeAny([id] + hyponyms)` instead of plain
  `ProductType(id)`.
- Correctness property tests (`hyponym_tests`, 5 tests including a
  500-trial `ChaCha8Rng`-seeded randomized soundness+completeness check):
  all pass. `cargo test --workspace --all-features`: zero new failures
  at this stage.

**Gate 1 — candidate-set relevant-document recall** (`p9_e04`, n=15
`structural_routed` queries, candidate sets <=5000), with the mechanism
wired in:

| | Before (post-ingestion-fix baseline) | After (hyponym expansion wired) | Change |
|---|---|---|---|
| Mean recall | 0.4562 | 0.6968 | **+24.06pp** |
| Exact recall (n=11) | 0.4772 | 0.6910 | +21.4pp |
| Partial recall (n=15) | 0.4207 | 0.6604 | +23.97pp |
| Queries with 100% recall | 0/15 | 2/15 | +2 |

Far exceeds the preregistered >=5pp materiality bar — the largest single
effect size measured in this session.

**Gate 2 — ranking quality / no NDCG regression** (`p9_e04`'s H1/H3,
identical-candidate-set comparison; n=15):

| | Before | After |
|---|---|---|
| Native NDCG@10 (identical candidate set) | — | 0.5813 |
| Solr-restricted NDCG@10 (same set) | — | 0.5292 |
| H1 verdict | FALSIFIED (native not worse) | still FALSIFIED (native +9.84% vs. Solr) |
| H3 latency ratio (Solr/native) | 3.92x pre-fix (checkpoint 10 baseline) | 2.17x (still CONFIRMED >=2x) |

Both gates the protocol named as sufficient for KEEP were cleared. A
diagnostic-classification bug was found and fixed in the same pass
(`p9_e04`'s `has_entity_constraint` predicate only matched
`StructuralConstraint::ProductType`, not the new `ProductTypeAny`,
silently miscounting most of these queries as "attribute-only" in one
aggregate breakdown table — fixed to recognize both variants; confirmed
this is a diagnostic-only bug in `phase9-eval`, not a `commerce-core`
correctness issue).

**P9-E08 — real-vocabulary false-family audit (the protocol's own "no
wrong-family regression" gate, executed for the first time as a direct
check rather than inferred from aggregate NDCG)**

New binary `crates/phase9-eval/src/bin/p9_e08_hyponym_group_false_family_audit.rs`
dumps every hyponym pair `product_type_hyponym_groups` actually produces
from the real 42,994-product WANDS catalog vocabulary (929 distinct
product-type names, 245 groups, 36 single-word broader terms — the
highest-blast-radius case). Manual audit of the full dump
(`docs/research/artifacts/i55_product_type_hyponym/p9_e08_false_family_audit.txt`)
found multiple **confirmed, real cross-family false positives**, not
hypothetical edge cases:

1. **Cross-vertical polysemy**: `"beds"` (furniture) admits `"cat beds"`
   and `"dog beds & mats"` (pet products) — the word "beds" is genuinely
   shared vocabulary across two unrelated product verticals in this
   catalog.
2. **Ancestor-breadcrumb bleed**: `"candles"` admits
   `"décor & pillows / candles & holders / scented oils & diffusers"` —
   this catalog's `product_type_names` mix clean `product_class`-derived
   short names with full category-breadcrumb-path names (a design
   established by the immediately prior ingestion-fix checkpoint,
   `ISSUE55_PRODUCT_CLASS_INGESTION_DECISION.md`, for products lacking a
   clean `product_class`). "Candles" only appears in the *ancestor*
   segment ("candles & holders") of a sibling leaf category
   ("scented oils & diffusers") — a real product that is not a candle.
   Same mechanism produces `"hot tubs"` admitting `"saunas"`.
3. **Scattered non-adjacent word match**: `"bed accessories"` and
   `"bath accessories"` (two different, unrelated broader terms) both
   admit `"...shower curtains & accessories / shower curtain hooks"` —
   the word-SET subset test does not require the broader term's words to
   appear together as a phrase, so "bed" (from one path segment, "bed &
   bath") and "accessories" (from a different, non-adjacent segment,
   "curtains & accessories") can each independently satisfy the test
   without the phrase "bed accessories" appearing anywhere in the real
   path.

**Methodological finding, independent of the specific mechanism**: the
protocol's own preregistered "no wrong-family regression" gate assumed
an NDCG regression would surface a false-positive problem. It did not —
H1's aggregate NDCG *improved* (+9.84%) even with these confirmed
false positives live in the candidate pool, because native's ranking
signal still ranks genuinely relevant documents above the injected
false-family ones for the specific 15 measured queries, keeping NDCG@10
(a top-10-weighted metric) from visibly moving. **A targeted
real-vocabulary structural audit was necessary to catch this; the
aggregate ranking-quality gate alone would have missed it.**

**Correctness verdict**: per CLAUDE.md's non-negotiable "Cross-variant
false matches are bugs" discipline (generalized here to product-family
matches, the same principle the protocol's own gate named), the
unconditional wiring is not shippable as-is.

## Corrective action taken this checkpoint

`compile_non_brand_lexicon` reverted to always emit
`StructuralConstraint::ProductType(*id)` (never `ProductTypeAny`) — the
exact pre-checkpoint-11 safe behavior. `StructuralConstraint::ProductTypeAny`
itself, its `matches`/`single_valued_slot`/`is_entity_constraint`/
`structural_bitmap` support, `product_type_hyponym_groups` (now `pub`,
re-exported from `cold_start`), and the P9-E08 audit binary are all kept
— tested, correct, generically useful infrastructure for a better-designed
future mechanism, just not wired into production lexicon compilation
today. A new regression test,
`product_types_with_a_whole_word_subset_relationship_never_merge_into_one_constraint`
(`crates/commerce-core/tests/cold_start.rs`), locks in the safe default
using the same word-subset shape ("boots"/"hiking boots") that would
trigger the rejected heuristic, so a future re-wiring without solving the
false-positive problem fails immediately.

**Post-revert confirmation**: rerunning both `p9_e04` and `p9_e02`
against the reverted lexicon reproduces the pre-checkpoint-11
(post-ingestion-fix) baseline exactly — mean candidate-set recall
0.4562 (Exact 0.4772, Partial 0.4207, 0/15 full recall), H1 native
NDCG@10=0.4614 vs. Solr-restricted 0.4396, H3 latency ratio 3.70x, and
`p9_e02`'s FastPath NDCG@10=0.1611 / `structural_routed` NDCG@10=0.2953
/ traffic-weighted overall NDCG@10=0.6596 — all matching
`ISSUE55_PRODUCT_CLASS_INGESTION_DECISION.md`'s own recorded baseline,
confirming the revert is complete and the safe default is live, not a
partial or silent regression.

## Decision

See `docs/decisions/ISSUE55_PRODUCT_TYPE_HYPONYM_DECISION.md`.
