# Issue #55 Preregistered Protocol — whole-word product-type hyponym expansion (`ProductTypeAny`)

Committed before this round's implementation, per this repository's
governance and CLAUDE.md's "Add RED correctness/regression tests before
production fixes where practical" — this is a `commerce-core` production
code change (touches `StructuralConstraint`, a shared, structural-match
mechanism), not an eval-crate-only diagnostic, and is held to that
higher bar.

## 0. What this is testing

Two prior checkpoints this session established, on real WANDS data:
`structural_routed` traffic has a large, real relevance gap vs. Solr
(FastPath NDCG@10 0.1611 vs. Solr 0.4670), that native's *ranking*
signal is not the cause (`p9_e04`'s H1, native ranking is not worse on
an identical candidate set), and that native's structural *candidate
set* misses ~55% of judged-relevant documents. A follow-on
investigation and ingestion fix
(`docs/decisions/ISSUE55_PRODUCT_CLASS_INGESTION_DECISION.md`) found
this is dominated by **category-hierarchy/synonym mismatch**: real
WANDS products tagged with a more specific type (e.g. `product_class="Kids Beds"`)
do not match a query resolving to a broader, related type (`"beds"`),
even though a shopper would reasonably expect a "beds" search to surface
kids' beds too.

This experiment tests a conservative, whole-word hyponym-expansion
mechanism: when compiling the lexicon, if product-type name A's
whitespace-separated word set is a **proper subset** of product-type
name B's word set (e.g. `{"beds"}` ⊂ `{"kids", "beds"}`), B's `ProductTypeId`
is added to A's lexicon entry as an admissible alternative, using a new
`StructuralConstraint::ProductTypeAny(Vec<ProductTypeId>)` — structurally
identical to the existing, already-adversarially-reviewed `BrandAny`
mechanism (Issue #6 P1-B), not a new kind of enforcement tier. This is
an **ingestion/lexicon-compile-time-only** change: `ir::query::compile`,
`execute_planned`, and every other serving-time function are untouched
— the lexicon simply resolves a broader term to a wider, still fully
deterministic and pre-validated, constraint.

**Deliberately whole-word, not substring**: "table" must not match
inside "turntable" (one word, no space) the way raw substring
containment would allow, since a turntable is not a kind of table. This
is the specific, disclosed correctness boundary of this mechanism.

## 1. Hypothesis

**H0**: this expansion materially improves `structural_routed`/`FastPath`
candidate-set coverage and/or NDCG@10 on real WANDS data, without
introducing a net relevance regression anywhere (Hybrid, Punt, or the
`p9_e04` H1 ranking-quality check) — i.e. the broader candidate sets it
admits are net-positive for ranking quality, not diluting it with
irrelevant hyponyms.

**H1**: the mechanism either has negligible measured effect (few real
WANDS product-type name pairs satisfy the whole-word-subset relation)
or is net-negative (admitting hyponyms measurably dilutes NDCG more
than it helps recall) — a real, informative negative result about
whether even a conservative hierarchy heuristic is worth its own
correctness/precision risk, not assumed away.

## 2. Baseline / dataset / treatment

Baseline: current branch HEAD (includes the `product_class` ingestion
fix from the immediately prior checkpoint). Dataset: the same real
WANDS catalog + 480 queries + fresh Solr 9.10.1 used throughout this
session. Treatment: `StructuralConstraint::ProductTypeAny` added to
`commerce-core`; `cold_start::profile::compile_non_brand_lexicon`
computes whole-word hyponym groups from `profile.product_type_names`
(already-collected real catalog vocabulary, no new data) and emits
`ProductTypeAny` instead of `ProductType` for any product type with a
non-empty hyponym group. `Category` lexicon entries are **not** changed
in this checkpoint — scoped to `ProductType` only, since that is what
the investigation's own concrete examples ("beds"/"Kids Beds") showed.

## 3. Metrics / gates

- **Correctness (hard gate, checked first)**: a randomized property test
  proving the hyponym-group computation is symmetric-safe (a broader
  term's group only ever contains genuine word-superset names, never the
  reverse) and a direct unit test on a small, hand-built, adversarial
  product-type vocabulary (including a `"table"`/`"turntable"` pair)
  proving no cross-word/substring false match. `cargo test --workspace --all-features`
  must show zero new failures.
- **No wrong-family regression**: `p9_e02`'s own wrong-family
  false-positive check (already part of the harness's gate elsewhere,
  reused here via the same NDCG/recall diagnostics) must not newly
  report violations attributable to this mechanism.
- **KEEP/document as real**: `p9_e04`'s candidate-set relevant-document
  recall improves materially (>=5pp, matching the prior checkpoint's
  own materiality bar for direct comparability) from its current
  baseline (0.4562, post-ingestion-fix), AND `FastPath`/`structural_routed`
  NDCG@10 does not regress anywhere it currently holds (Hybrid, Punt
  untouched by construction since `ProductType`/`ProductTypeAny` only
  affects structural candidate-set membership, not their own ranking
  paths).
- **REVISE/REJECT**: recall improves but NDCG regresses (net-negative
  hyponym dilution) or fails to clear the materiality bar (real but too
  small an effect, or too few real WANDS product-type pairs satisfy the
  whole-word-subset relation to matter).

Repetitions: NDCG/recall are deterministic given fixed judgments and a
fixed lexicon (no repetition needed); no new latency claim is made in
this checkpoint (a `ProductTypeAny` candidate-set union over a handful
of ids is not expected to be measurably more expensive than a single-id
lookup, and is not this checkpoint's own subject), so no fresh-Solr
discipline is required for the primary gates. If latency is reported
for completeness, it follows this session's established discipline.
