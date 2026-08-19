# ADR 0009: The structural-plus-delegated-lexical execution contract, and where a bitmap index's memory cost actually lives

## Status

Accepted (Issue #6 priorities 5 and 6, `docs/experiments/PHASE2_LOG.md` P2-E05/P2-E06).

## Context

ADR 0008 (Issue #5's NARROW decision) committed to delegating lexical
retrieval/ranking to Tantivy rather than rebuilding it, and named the
integration design — "how the structural planning layer and the delegated
Tantivy index compose at query time... an explicit `FastPath`/`Hybrid`/`Punt`
execution-outcome contract" — as unbuilt, deferred work (Issue #6
priority 5). P2-E01 validated Tantivy's *standalone* relevance recovery
but explicitly did not validate the composition. Issue #6 priority 6
deferred a related, adjacent question: whether delegating storage to
Tantivy's mmap model closes R1-E04's measured RSS gap (commerce-core
+3.76GB vs. Solr +175MB for the same real catalog), pending priority 5.

Both are now done, with real-data evidence
(`docs/experiments/PHASE2_LOG.md` P2-E05, P2-E06). What follows records
the architectural decisions that evidence produced — not just the
features, but the shape they ended up taking after two rounds of
real-data-caught correction and one deliberate self-review pass.

## Decision

**The execution contract is three outcomes, decided once per query,
before any delegate call**: `FastPath` (fully resolved structurally, the
delegate is never invoked — this is Gate 3's entire point), `Hybrid` (a
genuinely selective structural constraint narrows first, the delegate
ranks free text only within that narrowed set), `Punt` (no structural
constraint, or a non-selective one — R1-E05's near-universal-bitmap
collapse — so the structural layer skips narrowing entirely and the
delegate searches the whole corpus, with any structural constraints
re-verified against only the delegate's small returned hit set instead of
ever materializing a huge candidate bitmap).

**`commerce_core` defines the delegate mechanism, not a delegate.**
`plan::LexicalDelegate` is a trait; no lexical-engine crate is a
dependency of `commerce_core` itself — this mirrors
`control_plane::provider::ModelProvider`/`control_plane::precision::PrecisionOracle`
exactly, which was already this project's established pattern for "give
`commerce_core` a mechanism, not an implementation" before this ADR. The
first real implementation (`TantivyDelegate`) lives in `phase2-eval`.

**`commerce_core` owns correctness unconditionally, regardless of
outcome or delegate behavior.** Every delegate hit is re-verified against
`CommerceQuery::matches_variant` (the same method `execute()` uses, not a
parallel implementation) before it is ever returned. A delegate that
ignores a restriction, or ranks something structurally ineligible highly,
cannot produce an incorrect result — only a wasted one. This was not
incidental: it is what made two of the three real bugs found during
integration (below) safe to fix without a correctness regression window,
because the verification layer was never the thing that was wrong.

**Deployment-tunable parameters are a named policy type, not bare
function parameters.** `PlannerPolicy{selectivity_threshold,
delegate_oversample}` replaced what began as a bare `f64` and a private
const, found via explicit self-review against the question "are we
accidentally introducing a feature flag where a semantic type or policy
should exist?" This is the concrete extension point for a future
per-vertical or per-merchant override (the `Commerce Core -> Vertical
Context -> Merchant Context -> Behaviorally Learned Context` layering
Issue #5's archaeology section proposes) — a new tuning dimension gets a
new field here, not a new function parameter threaded through `plan`/
`execute_planned`, and not a conditional branch inside them.

**Canonicalization thresholds must apply uniformly across every
raw-catalog-derived vocabulary, not per-field by assumption.** P2-E02
(Round 1) canonicalized `color` (and other enum attribute values) by
occurrence frequency, and explicitly exempted `Brand`/`ProductType`/
`Category` on the stated assumption that those came from "an
already-curated registry." That assumption was correct for `ProductType`/
`Category` (this project's own small, deliberate registries) but false
for `Brand` on the real ESCI catalog: `round1_eval::catalog::build_catalog`
interns brand from a raw per-product field with no validation, exactly
like `color`. Real measurement: 206,227 distinct real "brand" strings,
49.4% occurring on exactly one product, overwhelmingly seller-junk text.
The general principle, not just the specific fix: **a canonicalization
threshold's scope must be justified by where the vocabulary actually
comes from, verified against the real ingestion pipeline, not assumed
from a field's semantic role.** `compile_lexicon`'s `min_enum_frequency`
now gates brand the same way it gates enum values.

**The bitmap/range index is not where `commerce-core`'s memory
disadvantage lives.** R1-E04 found commerce-core's RSS grew 3.76GB
against Solr's 175MB for the same real catalog and called it "a real,
measured memory-architecture disadvantage" without decomposing which
layer caused it — Tantivy integration didn't exist yet to make that
decomposition possible. P2-E06 measured it directly: `CatalogIndex::build`
itself (the actual `RoaringBitmap`/hash-map structure Gate 3 built)
contributes only ~828MB of that total. The dominant cost (~4,552MB before
any index exists) is simply holding a typed Rust representation of 1.2M
real, attribute-heavy products — raw parsed structs plus the
`String`-heavy `AttributeMap`/`Product`/`Variant` domain model built from
them. Delegating lexical retrieval to Tantivy adds a real but modest
~1.2GB of its own (build + reader + a full real 22,458-query warm-up) —
confirming ADR 0008's directional bet — but this **cannot**, by
construction, close the dominant share of the original gap, because that
share was never lexical-index-shaped. A future memory-representation
optimization (string interning, dense IDs, columnar attribute storage —
CLAUDE.md's own "likely physical primitives" list) needs to target the
domain-model layer, not the bitmap index, to matter.

**A parameter kept even after being proven redundant in its one current
use, when it names a real future extension point.** `verify_and_truncate`'s
`restrict_to` check is, right now, provably redundant with
`matches_variant`'s own constraint check — `execute_planned`'s only
caller always derives `restrict_to` from `query.constraints`, and
`matches_variant` already checks that same set completely, so the
membership check can never independently change today's outcome. Found
via a deliberate RED-evidence check (a test believed to isolate
`restrict_to` stayed GREEN even with the check removed, because the
test's query had an unrelated constraint that already excluded the same
product). Kept, not deleted: `restrict_to` is the extension point for a
future restriction *not* derivable from `query.constraints` at all — a
merchandising/curated-collection policy applied above the query layer
(Issue #5 section 12's merchandising-policy category is exactly this
shape). A white-box unit test
(`plan::tests::restrict_to_independently_excludes_a_constraint_satisfying_hit`)
now isolates and proves the mechanism's independent behavior directly,
since the integration test suite's one call pattern cannot.

## Consequences

- `commerce_core` gained one new public module (`plan`) and no new
  dependencies. The dependency boundary ADR 0008 established (Tantivy
  enters the workspace via `phase2-eval`, never `commerce_core`) held
  through this integration without exception.
- Real relevance of the *integrated* system (structural routing +
  delegated ranking + `commerce_core`-owned verification) reaches 89% of
  standalone Tantivy's NDCG@10/Recall@10 at the best canonicalization
  threshold measured (100), not full parity — an open, further-improvable
  gap, not a closed one. No single canonicalization threshold is adopted
  as a production default by this ADR: P2-E05 found a real trade-off
  between integrated relevance (favors a higher threshold) and `FastPath`
  coverage (favors a lower one), left as a downstream deployment decision.
- The `phase2-eval` crate now has five real-data evaluation binaries
  (`tantivy_relevance_eval`, `canonicalization_eval`, `precision_gate_eval`,
  `planner_integration_eval`, `memory_representation_eval`), each
  self-contained per this project's established one-binary-per-experiment
  convention — no shared library code was introduced across them, since
  none was asked for by the actual work.
- Extension points now exist, deliberately, for exactly two of the
  categories Issue #5 section 12's archaeology workstream is expected to
  surface findings in: **planner policy** (`PlannerPolicy`) and
  **retrieval primitive** (`LexicalDelegate`). Findings in other
  categories (a new canonicalization rule, a new Commerce IR construct, a
  new ranking feature, a merchandising policy) are expected to extend
  their own existing subsystems (`cold_start`, `ir`, `index::rank`, or a
  new type not yet built) rather than `plan` — no speculative
  infrastructure for those categories was added ahead of an actual
  finding requiring it.

## Alternatives considered

- **Push `restrict_to` filtering entirely into the delegate's own query
  language, with no `commerce_core`-side re-verification.** Rejected:
  this would make correctness contingent on every delegate implementation
  getting its own filtering right, violating "product/variant correctness
  is non-negotiable" (CLAUDE.md) — and was specifically what the second
  real bug (an under-filtering delegate) would have made worse, not
  better, had `commerce_core`'s own re-verification not existed
  independently of delegate behavior.
- **Delete `verify_and_truncate`'s `restrict_to` check once found
  redundant with `matches_variant` in the current call pattern.**
  Rejected: redundant *today*, in the one call pattern that exists, is
  not the same as unnecessary as an interface — see Decision above.
- **Fold `min_enum_frequency`'s brand-gating fix into a separate,
  brand-specific threshold parameter rather than reusing the existing
  one.** Rejected: brand and enum-attribute vocabulary have the same real
  failure mode (unvalidated raw per-product field values) and the same
  fix (occurrence-frequency canonicalization) — a second parameter would
  have added a distinction real evidence did not support.
- **Treat R1-E04's original 3.76GB figure as fully explained by "the
  bitmap index" and pursue an index-representation optimization (string
  interning, dense IDs) directly, without first decomposing the cost.**
  Rejected: P2-E06 shows this would very likely have targeted the wrong
  layer — the bitmap index itself is ~828MB of the ~5.4GB total, not the
  dominant cost.
