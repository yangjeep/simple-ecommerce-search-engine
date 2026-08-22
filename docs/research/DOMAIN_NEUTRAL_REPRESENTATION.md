# Domain-Neutral Semantic Representation (Issue #35, Workstream A)

**Status: FROZEN as of this document's initial commit.** Per Issue #35's
own definition of done ("Methodology specification is frozen before
scoring against benchmark truth") and its explicit warning ("Avoid
encoding current footwear/ESCI-specific conclusions as generic
primitives"), this representation is derived from (a) Issue #35's own
listed required primitives, and (b) a direct, static analysis of what
`commerce_core`'s *existing* type system does and does not already
generalize — not from any Phase 9/WANDS benchmark *outcome*. Nothing
below encodes a WANDS-specific or ESCI-specific conclusion; the WANDS
lexicon-compilation gap found in Phase 9 (P9-E02) is cited nowhere in
this document, deliberately, per the "do not tune the methodology after
seeing the benchmark truth it is supposed to predict" rule. Changing this
document after any blind-replay or unseen-vertical result lands would
violate that rule; extend it only via a dated addendum, never a silent edit.

## Purpose

Define the smallest set of generic semantic primitives a *fixed*
discovery/serving architecture needs to describe a specialization
opportunity on a previously unseen commerce vertical, per Issue #35's
core design principle: "Algorithm fixed; discovered schema/context
variable." This is a specification to freeze, not new production code —
implementing it is separate future work, tracked in
`docs/experiments/ISSUE35_LOG.md`.

## Method: what already generalizes vs. what doesn't, in the current codebase

Before proposing anything new, `commerce_core`'s existing type system was
read directly (not assumed) against Issue #35's required primitive list.

### Already domain-neutral (real, reusable, evidence cited)

- **Attribute/value families**: `commerce_core::domain::Constraint`
  (`domain/constraint.rs:28-49`) — `Enum{attribute, value}`,
  `MultiEnumContains{attribute, value}`, `Boolean{attribute, value}`,
  `Numeric{attribute, op, value}`, `Text{attribute, contains}` — every
  variant is keyed by a free-string `attribute` name, not a fixed set of
  known attribute names. This already covers Issue #35's required
  "enum/multi-enum," "boolean," and (partially — see gaps below)
  "numeric/range" and "text residuals" categories, for *any* vertical's
  attribute vocabulary, with zero code changes needed per new attribute.
- **Value representation**: `commerce_core::domain::AttributeValue`
  (`domain/attribute.rs:7-13`) — `Enum(String)`, `MultiEnum(Vec<String>)`,
  `Boolean(bool)`, `Numeric(f64)`, `Text(String)` — generic value-type
  primitives, not vertical-specific.
- **Deterministic profiling before model use**: `commerce_core::cold_start::profile::{CatalogProfile, compile_lexicon}`
  (`cold_start/profile.rs:69-256`) — confirmed by direct use in Phase 9
  (P9-E02) to work unmodified against a catalog (WANDS) it was never
  written for: `CatalogProfile::build` takes a generic `&Catalog` plus
  `&[Brand]`/`&[ProductType]`/`&[Category]`, and derives a lexicon purely
  by counting/deduplicating real attribute occurrences — no model call,
  matching Issue #35 Workstream B's "deterministic profiling should
  precede model use" requirement and its "no one LLM call per SKU" rule.
- **Confidence**: `commerce_core::ir::lexicon::Candidate.confidence: f64`
  (`ir/lexicon.rs:6-14`) already exists as a first-class field on every
  candidate resolution, generic to any vertical's vocabulary.
- **Ambiguity preservation**: `commerce_core::ir::query::AmbiguousSpan`
  (`ir/query.rs:29-32`) already preserves a multi-candidate phrase rather
  than silently picking a winner — CLAUDE.md's "preserve ambiguity
  explicitly" is already a real, generic mechanism, not vertical-specific.
- **Physical representation, generically**: `commerce_core::index::CatalogIndex`'s
  bitmap/postings machinery (`enum_bitmaps`, `product_type_bitmaps`,
  `category_bitmaps`, numeric range structures) operates over whatever
  attribute names a catalog happens to have — the *physical* layer is
  already attribute-name-agnostic.

### NOT domain-neutral (real falsification-criterion matches, found by direct code read)

- **Entity families are a fixed, closed enum, not a discovered one.**
  `commerce_core::ir::structural::StructuralConstraint`
  (`ir/structural.rs:11-27`) has exactly six hard-coded variants: `Brand`,
  `BrandAny`, `ProductType`, `Category`, `PriceUnderCents`,
  `PriceOverCents`. A genuinely unseen vertical's entity families (Issue
  #35's own examples: "Voltage," "Capacitance," "Fitment," "OEM Number")
  have **no representation in this enum without a Rust source change** —
  this is exactly Issue #35's own falsification criterion, "representation
  requires vertical-specific serving code," already true of the current
  architecture, found by direct reading, not by running any vertical
  through it.
- **No Relationship primitive exists anywhere.** Issue #35 requires
  representing entity relationships (e.g. "this part fits models X, Y,
  Z" — a fitment relationship). `commerce_core::domain` has no
  cross-entity or cross-product relationship type at all; every
  attribute is scoped to exactly one `Product`/`Variant`.
- **No first-class Hierarchy primitive exists.** WANDS's own
  `category_depth_1..6` (`crates/phase6a-eval/src/catalog.rs:115-125`) is
  the closest real precedent, but it is an ad hoc, fixed-depth-6 set of
  independent `Enum` attributes, not a recursive/variable-depth Hierarchy
  type any future vertical's own taxonomy depth could bind to generically.
- **No Quantity/unit primitive exists.** `Constraint::Numeric`/
  `AttributeValue::Numeric` are bare `f64` — "12" is indistinguishable
  from "12 inches" vs. "12 volts." A genuinely unit-aware vertical
  (electronics: Voltage, Capacitance) has no way to express a unit
  dimension without conflating differently-unitted numeric attributes.
- **No Provenance primitive exists.** Nothing in `commerce_core::domain`
  or `commerce_core::ir` records whether a fact came from raw catalog
  data, a deterministic profiling rule, or a promoted control-plane
  implication (`commerce_core::control_plane::implication` tracks
  promotion state for *brand-implication rules specifically*, not as a
  general provenance primitive attached to arbitrary facts).
- **Admission predicates and fallback requirements are policy constants,
  not discovered/versioned artifacts.** `commerce_core::plan::PlannerPolicy`
  (`plan/mod.rs:99-116`) and `commerce_core::admission::AdmissionPolicy`
  are real, but their thresholds (`selectivity_threshold`,
  `delegate_oversample`) are caller-supplied constants tuned per
  experiment, not something a discovery pipeline emits as part of a
  versioned "merchant semantic profile" (Issue #35 Workstream F).

## The frozen representation

Given the above, the domain-neutral representation Issue #35's discovery
pipeline needs to be able to emit is the existing `Constraint`/
`AttributeValue` primitives **plus** four genuinely new generic types this
codebase does not yet have. These are specified here as a target shape
(Rust-flavored, since this project is Rust-native and any real
implementation will need to compile into actual types), not yet
implemented:

```rust
/// A discovered entity family (generalizes StructuralConstraint's fixed
/// Brand/ProductType/Category variants into one open-ended kind).
/// `EntityFamilyId` is assigned per-catalog at discovery time, exactly
/// the way CategoryId/ProductTypeId already are (ir/structural.rs) --
/// only the *number of families* becomes variable instead of fixed at 3.
struct EntityFamily {
    id: EntityFamilyId,
    name: String,           // discovered, e.g. "voltage_rating", "fitment_model"
    cardinality: Cardinality, // SingleValued | MultiValued
    value_type: ValueType,   // Enum | Numeric(Option<Unit>) | Boolean | Text | Hierarchical
}

enum Cardinality { SingleValued, MultiValued }

enum ValueType {
    Enum,
    MultiEnum,
    Boolean,
    Numeric { unit: Option<Unit> },
    Text,
    Hierarchical { max_observed_depth: u8 },
}

/// Generalizes WANDS's ad hoc category_depth_1..6 into a variable-depth
/// path, keyed the same way CategoryId already is.
struct HierarchyPath {
    family: EntityFamilyId,
    segments: Vec<String>,  // root-to-leaf, variable length
}

/// A discovered unit dimension for a Numeric family -- absent today,
/// needed so "12" (inches) and "12" (volts) are never silently comparable.
struct Unit {
    dimension: String,       // e.g. "length", "voltage", "capacitance"
    symbol: String,          // e.g. "in", "V", "F"
}

/// A discovered cross-entity relationship (e.g. fitment: this product
/// relates to a set of other entities under a named relationship kind).
/// Absent today; every existing attribute is single-product-scoped.
struct Relationship {
    kind: String,            // discovered, e.g. "fits_model", "compatible_with"
    from: ProductId,
    to: Vec<EntityReference>,
    confidence: f64,         // reuses the existing Candidate.confidence shape
}

enum EntityReference {
    Product(ProductId),
    EntityValue(EntityFamilyId, String),
}

/// Attaches provenance to any discovered fact -- absent today except for
/// control_plane::implication's promotion-specific bookkeeping.
enum Provenance {
    RawCatalogField { source_field: String },
    DeterministicProfile { rule: String },
    PromotedImplication { rule_id: String, promoted_at: String },
}

/// A versioned, inspectable output of the discovery pipeline for one
/// catalog -- Issue #35 Workstream F's "Merchant Semantic Profile,"
/// specified here as a type shape, not yet populated by any pipeline.
struct MerchantSemanticProfile {
    catalog_fingerprint: String,
    entity_families: Vec<EntityFamily>,
    hierarchies: Vec<HierarchyPath>,
    relationships: Vec<Relationship>,
    high_confidence_structural_coverage: f64,
    medium_confidence_structural_coverage: f64,
    unresolved_coverage: f64,
    predicted_safe_admission_coverage: f64,
    admission_predicate_version: String,
    fallback_requirement: FallbackRequirement,
}

enum FallbackRequirement {
    NoFallbackNeeded,
    DelegateRequired { estimated_traffic_share: f64 },
}
```

## What this does and does not decide

- This freezes the *shape* discovery needs to emit. It does **not**
  decide the discovery *algorithm* (Workstream B), the blind-replay
  protocol/scoring rubric (Workstream C), or which unseen verticals to
  test (Workstream D) — those are separate, subsequent Issue #35
  workstreams, tracked in `docs/experiments/ISSUE35_LOG.md`.
- This does **not** claim `commerce_core` should be rewritten to use
  these types today. Whether to generalize `StructuralConstraint` into a
  real `EntityFamily`-based representation, and whether doing so helps or
  hurts the *measured* Phase 9 system, is exactly the kind of question
  Issue #35's own "epistemic boundary" rule reserves for after the
  methodology is frozen and blind-replayed — conflating the two now would
  violate CLAUDE.md's decision discipline.
- The gaps identified above (no Relationship/Hierarchy-as-first-class/
  Unit/Provenance primitive) are recorded as real, disclosed
  architectural facts about the *current* system, found by direct code
  reading — they are not yet failures of the *methodology* being defined
  here, since the methodology has not yet been run against anything.

## Provenance of this document

Derived from: Issue #35's own listed required primitives and Workstream A
instructions; direct reads of `commerce_core::domain::constraint`,
`commerce_core::domain::attribute`, `commerce_core::ir::structural`,
`commerce_core::ir::lexicon`, `commerce_core::ir::query`,
`commerce_core::cold_start::profile`, `commerce_core::control_plane::implication`,
`commerce_core::plan`, `commerce_core::admission`; and
`crates/phase6a-eval/src/catalog.rs`'s WANDS ingestion as the one
concrete precedent for what a real second vertical's schema looks like
(cited only for its *structural shape* — depth-6 hierarchy, no brand,
no price — not for any Phase 9 *measurement outcome*, which this document
does not reference).
