# ADR 0013: An identifier serving primitive — a calibrated classifier gating an exact-match dictionary, resolved ahead of the lexical delegate

## Status

Accepted (Issue #42 R3, `docs/experiments/ISSUE42_LOG.md#i42-r3`).

## Context

ADR 0009 established `commerce_core::plan::execute_planned`'s three-outcome
execution contract and its unconditional correctness rule: every delegate
hit is re-verified against `CommerceQuery::matches_variant` before being
returned. That verification has always had a gap ADR 0009 could not close,
because the type it needed did not exist yet: `plan::LexicalHit` carried
only a `ProductId`, never a `VariantId`, so `plan::verify_and_truncate`
could only ever resolve a hit to "the product's first variant that happens
to satisfy every structural constraint" — arbitrary relative to whatever
text a delegate actually matched. For a shopper query that is really an
exact identifier/part-number lookup, this is a genuine cross-variant
correctness defect, not a cosmetic one (CLAUDE.md: "Product/variant
correctness is non-negotiable. Cross-variant false matches are bugs.").
`commerce_core::ir::compile` compounds the gap: it has no identifier-token
classification at all, and its own tokenizer only splits on whitespace, so
a hyphenated identifier typed as one word (e.g. `IA-1234-BP`) survives
untouched as a single `residual_lexical` entry with no special handling.

Issue #42 R3 (`docs/experiments/ISSUE42_LOG.md#i42-r3`) measured three
treatments against this gap on a real, held-out automotive catalog: **A**,
the unmodified product-level lexical delegate, which found 0 of 2,972
held-out identifiers (it never indexes variant-level text at all); **B**, a
general per-variant Tantivy text index, which achieved 100% Recall@1 / 0%
false-match on genuinely unique queries but matched a bare prefix query it
should have rejected — a real general-purpose-text-index limitation; and
**C**, a calibrated classifier (`IdentifierClassifier`) gating a dedicated
exact/normalized-key dictionary (`IdentifierDictionary`), which matched B's
Recall@1/false-match numbers, correctly rejected the same prefix/near-miss
adversarial queries B failed, and built/updated at a small fraction of B's
cost. **Treatment C passed every preregistered GO-gate criterion**, after
two rounds of adversarial review found and fixed real defects — most
notably, R3's second correction round found the classifier had silently
narrowed from a preregistered multi-signal design to uniqueness ratio
alone, undisclosed, and that a real non-identifier field (`lumens`, a
Numeric attribute) measured only 0.01 below the acceptance cutoff on that
one signal — a genuine margin risk. A candidate second signal
(`format_consistency`, a character-class "shape" measure) was tried and
**empirically failed**: it scored the real identifier field *lower* than
several genuine non-identifiers, because brand/product-type name segments
of varying word count spread a real identifier's own occurrences across
multiple signatures. That negative result is part of the evidence trail,
not erased (CLAUDE.md: "Record failed experiments") — `variant_scoped`
(already computed, never previously read) turned out to be the signal that
discriminates correctly, for a structural reason: a real identifier is set
directly on each `Variant`; the near-miss field is not.

## Decision

**A new `index::identifier` submodule ports R3's validated mechanism into
`commerce_core`, unmodified in its calibrated statistics and cutoffs.**
`compute_field_stats(catalog: &Catalog) -> BTreeMap<String, FieldStats>`
measures, per field, total occurrences, distinct-normalized-value count,
uniqueness ratio, mean Shannon entropy, format consistency, and whether the
field is ever set directly on a `Variant` — over stringified attribute
values regardless of `AttributeValue` variant, never a field's name.
`IdentifierClassifier::accepts` gates on `uniqueness_ratio >=
MIN_UNIQUENESS_RATIO (0.95) && variant_scoped` — R3's own fully-corrected,
held-out-validated condition. `format_consistency` is still computed and
reported (`FieldStats::format_consistency`), per R3's own negative-result
discipline, but does not gate. `shannon_entropy_bits`'s character counts
and `compute_field_stats`'s signature-tally both use `BTreeMap`, not
`HashMap` — R3's own previously-caught determinism bug, preserved here.
`IdentifierDictionary::build` indexes every accepted field's normalized
values to every `(ProductId, VariantId)` that carries them; `lookup`
returns every match a collision produces, never arbitrated to one.

**A new safeguard, added during production integration, beyond R3's own
experimental scope: `MIN_IDENTIFIER_SAMPLE_SIZE: usize = 100`, additionally
required by `IdentifierClassifier::accepts`.** R3's calibration and
held-out catalogs both had 1,500+ products, so a spurious small-sample
accept was never exercised there. `commerce_core`'s own test suite
(`fixtures.rs`, `tests/*.rs`) includes catalogs with as few as 2-10
products, where a field with `n` occurrences, all distinct by construction,
always measures `uniqueness_ratio == 1.0` regardless of how small `n` is —
a real risk of a tiny fixture field being spuriously accepted as an
identifier on sample-size noise alone, not semantic identifier-ness. `100`
sits well below R3's own calibration set's 1,500 occurrences for
`part_number`, so it cannot change R3's own experimental conclusion (the
real identifier field still clears it with wide margin) — it only closes a
gap R3's own evaluation had no catalog small enough to expose.
`crates/commerce-core/src/index/mod.rs`'s
`a_small_catalog_never_spuriously_accepts_an_identifier_shaped_field` test
was written RED (failing) against a version of `accepts` without this
condition, confirmed to fail for the expected reason, then made GREEN by
adding it — direct, run confirmation, not merely reasoned about.

**`CatalogIndex::build` computes field stats once per catalog and builds a
dictionary for every accepted field**, stored in a new private
`identifier_dictionaries: Vec<(String, IdentifierDictionary)>` field. A new
public `identifier_lookup(&self, token: &str) -> Vec<(ProductId,
VariantId)>` unions and deduplicates (via a `BTreeSet`) every accepted
dictionary's match for `token`; `identifier_field_count` reports how many
fields were accepted, for observability.

**`plan::LexicalHit` gains one new, additive field: `pub variant:
Option<VariantId>`.** Every existing delegate in this workspace constructs
`variant: None` — none can resolve a specific variant today, only a
product — so this is purely a widening of what a *future* delegate can
express. `plan::verify_and_truncate`'s resolution logic now prefers the
named variant when present, but only if it also satisfies every
constraint — falling back to today's first-satisfying-variant behavior
only when no variant is named, never when a named variant fails:

```rust
let variant = match hit.variant {
    Some(vid) => product.variants.iter().find(|v| v.id == vid && query.matches_variant(product, v)),
    None => product.variants.iter().find(|v| query.matches_variant(product, v)),
};
let Some(variant) = variant else { continue };
```

A named variant is a recall hint, never a bypass of `commerce_core`'s own
correctness ownership — the same invariant ADR 0009 established for
`restrict_to`, preserved exactly for this new field.

**`execute_planned`'s `Hybrid` and `Punt` arms try an identifier-dictionary
lookup before ever calling the delegate.** A new private `identifier_hits`
helper unions `index.identifier_lookup(token)` across every token in
`query.residual_lexical`, re-verifies each candidate against
`query.matches_variant` (and, for `Hybrid`, `restrict_to` membership), and
returns up to `k` `PlannedHit`s with `score: 1.0` (a fixed maximal score, by
convention, for an exact deterministic match). When non-empty, that result
is returned immediately — the delegate is never called for this query.
When empty (no token matched any accepted field, including a catalog with
no accepted fields at all), execution falls through to today's/R2's exact
existing logic, unchanged. This ordering is a direct reading of CLAUDE.md's
Mission — "structural retrieval is primary where semantics are known" — an
exact identifier match is precisely that: a strictly higher-precision
signal than general lexical ranking.

## Consequences

- `commerce_core::index` gains one new private submodule (`identifier`),
  re-exporting `FieldStats`, `IdentifierClassifier`, and
  `IdentifierDictionary` — no new external dependency.
- `plan::LexicalHit`'s shape changed (one new field), which the compiler
  used to find every construction site. **15 call sites across 9 files**
  were migrated to `variant: None` — a compile-fix only, zero behavior
  change for every existing delegate: `crates/commerce-core/tests/plan.rs`
  (6 sites), `crates/commerce-core/src/plan/mod.rs`'s own test module (2
  sites, `restrict_to_independently_excludes_a_constraint_satisfying_hit`),
  `crates/issue42-eval/src/r2_experimental.rs` (1 site),
  `crates/phase2-eval/src/bin/planner_integration_eval.rs`,
  `crates/phase2-eval/src/bin/punt_path_adversarial_eval.rs`,
  `crates/phase2-eval/src/bin/alias_enforcement_eval.rs`,
  `crates/phase2-eval/src/bin/p1d_physical_advantage_eval.rs`,
  `crates/phase2-eval/src/bin/prefill_eval.rs` (1 site each), and
  `crates/phase9-eval/src/bitmap_delegate.rs` (1 site).
  `cargo test -p issue42-eval --release` (both R2's and R3's own existing
  tests) and `cargo test -p phase9-eval`/`cargo test -p phase2-eval`
  continue to pass unchanged after this migration.
- `CatalogIndex::build` gained a real, disclosed, small added build cost:
  a second, independent `O(products x variants)` scan of the catalog
  (`compute_field_stats`), beyond the existing per-attribute indexing pass.
  Not hidden or folded silently into the existing loop, since the two
  passes have genuinely different shapes (the existing loop indexes as it
  goes; field-stats computation needs every occurrence of a field
  collected before a ratio can be computed). Only fields that pass the
  classifier additionally pay `IdentifierDictionary::build`'s own
  `O(products x variants)` cost, expected to be rare in practice (R3's own
  held-out catalog accepted exactly 1 of 18 candidate fields).
- New regression tests: `crates/commerce-core/src/index/mod.rs` gained its
  first `#[cfg(test)] mod tests` (a positive accept-and-resolve case, and
  the RED-before-GREEN small-sample-safeguard negative case);
  `crates/commerce-core/src/plan/mod.rs`'s existing test module gained
  three: identifier-resolves-the-specific-named-variant,
  named-variant-fails-a-constraint-and-is-rejected-without-fallback, and
  identifier-miss-falls-through-to-the-delegate-path-unchanged.
- No new production dependency anywhere in this change.

## Alternatives considered

- **A general per-variant text index (R3's Treatment B), extended into
  production instead of the classifier+dictionary.** Rejected: R3 measured
  B as substantially slower to build and incrementally update than C for
  the same field, with no better correctness on the held-out set — and B
  additionally matched a bare prefix query it should have rejected, a real
  false-match risk C does not share.
- **A required (non-`Option`) `VariantId` on `LexicalHit`, rather than
  `Option<VariantId>`.** Rejected: this would force every existing
  delegate — none of which can resolve a specific variant today — to
  fabricate one just to satisfy the type, either duplicating
  `verify_and_truncate`'s own first-satisfying-variant fallback logic
  inside every delegate or fabricating an arbitrary value with no real
  meaning. `Option`, defaulting to `None`, keeps every existing delegate's
  code and behavior completely unchanged while still letting a future
  delegate that *can* name a variant do so.
- **Gate `IdentifierClassifier::accepts` on `format_consistency` (or an
  equivalent fixed-length-ratio signal) as a second required condition,**
  as R3's own preregistered protocol originally described. Rejected: R3's
  second correction round measured this directly and found it scores the
  real identifier field *lower* than several genuine non-identifiers on
  real data (varying brand/product-type name segment lengths spread a real
  identifier's own occurrences across multiple character-class
  signatures) — gating on it would have incorrectly rejected the real
  identifier field. Recorded as a negative result (CLAUDE.md), not
  hidden: `format_consistency` remains computed and reported, but
  `variant_scoped` is the second signal that actually gates.
