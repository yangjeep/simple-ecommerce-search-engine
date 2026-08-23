//! Residual-lexical classification: Issue #42 R2 (`docs/experiments/ISSUE42_LOG.md#i42-r2`)
//! measured a real defect in `execute_planned`'s `Hybrid`/`Punt` handling —
//! any non-empty `query.residual_lexical` for which the [`super::LexicalDelegate`]
//! finds zero raw hits collapses the whole query to zero results, with no
//! distinction between a broadly generic descriptive word (should not veto)
//! and a genuinely specific, disqualifying attribute value (should veto).
//! R2's Treatment D cleared every preregistered GO-gate criterion (benign
//! recovery, adversarial false-recovery, zero query-time model calls) after
//! two rounds of adversarial review, and is the mechanism this module ports
//! into production. See `docs/adr/0012-residual-lexical-policy.md` for the
//! architectural decision and its alternatives.
//!
//! The signal is cross-type breadth: a token observed under at least
//! [`CROSS_TYPE_BREADTH_THRESHOLD`] *distinct* `ProductType`s anywhere in
//! the catalog reads as a broadly-used, generic term rather than a value
//! specific to one product family. Compiled once per [`crate::domain::Catalog`]
//! (ingestion time, not query time) — this is a structural fact about the
//! catalog, never a query-time model call (CLAUDE.md: "No LLM/model call in
//! the default query hot path").

use std::collections::{BTreeMap, BTreeSet};

use crate::domain::{AttributeValue, Catalog, ProductTypeId};

/// A residual-lexical token's classification, per [`ResidualPolicy`].
///
/// Only the two classes R2's own preregistered workload could actually
/// exercise and measure are implemented. The full design also named
/// `Contextual` (entity-dependent) and `Unknown` (too rare to classify) —
/// neither is ported here, since neither has GO-gate evidence behind it yet
/// (a disclosed scope decision, not a silent omission; see
/// `docs/experiments/ISSUE42_LOG.md#i42-r2`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResidualClass {
    /// Never observed anywhere in the catalog, or observed under fewer than
    /// [`CROSS_TYPE_BREADTH_THRESHOLD`] *other* product types: a specific
    /// attribute value that must still veto a zero-raw-hit query, exactly
    /// like today's behavior.
    Required,
    /// Observed under at least [`CROSS_TYPE_BREADTH_THRESHOLD`] *other*
    /// product types: a broadly-used, generically descriptive term, safe to
    /// bypass a zero-raw-hit delegate result in favor of the structural
    /// candidate set.
    Preferred,
}

/// How many *other* product types a token must be observed under (a title
/// word, a registered `Enum`/`MultiEnum` value, or a `true` Boolean
/// attribute name) before it classifies [`ResidualClass::Preferred`] rather
/// than [`ResidualClass::Required`]. R2's own fixture and adversarial
/// review (`docs/experiments/ISSUE42_LOG.md#i42-r2`'s "Second correction
/// round") is the evidence this exact threshold is what distinguishes a
/// real, specific value observed under exactly one other family (must
/// still veto) from a genuinely generic word observed broadly (safe to
/// bypass).
const CROSS_TYPE_BREADTH_THRESHOLD: usize = 2;

/// Compiled once from a whole [`Catalog`] snapshot — a lowercased token ->
/// every [`ProductTypeId`] it was observed under, anywhere in the catalog.
/// Recompiled whenever the catalog changes, exactly like [`crate::index::CatalogIndex`]
/// or [`crate::cold_start::CatalogProfile`]; never mutated incrementally.
pub struct ResidualPolicy {
    type_occurrences: BTreeMap<String, BTreeSet<ProductTypeId>>,
}

fn record_attribute_occurrence(
    map: &mut BTreeMap<String, BTreeSet<ProductTypeId>>,
    name: &str,
    value: &AttributeValue,
    product_type: ProductTypeId,
) {
    match value {
        AttributeValue::Enum(v) => {
            map.entry(v.to_lowercase())
                .or_default()
                .insert(product_type);
        }
        AttributeValue::MultiEnum(vs) => {
            for v in vs {
                map.entry(v.to_lowercase())
                    .or_default()
                    .insert(product_type);
            }
        }
        AttributeValue::Boolean(true) => {
            map.entry(name.to_lowercase())
                .or_default()
                .insert(product_type);
        }
        AttributeValue::Boolean(false) | AttributeValue::Numeric(_) | AttributeValue::Text(_) => {}
    }
}

impl ResidualPolicy {
    /// Scan every product's title words plus every Enum/MultiEnum
    /// attribute value and true-Boolean attribute name (product- and
    /// variant-level), recording which `ProductTypeId`s each lowercased
    /// token is observed under. Zero model calls; a single deterministic
    /// pass over `catalog`.
    pub fn compile(catalog: &Catalog) -> Self {
        let mut type_occurrences: BTreeMap<String, BTreeSet<ProductTypeId>> = BTreeMap::new();
        for product in &catalog.products {
            for word in product.title.split_whitespace() {
                let cleaned: String = word
                    .chars()
                    .filter(|c| c.is_alphanumeric())
                    .flat_map(|c| c.to_lowercase())
                    .collect();
                if !cleaned.is_empty() {
                    type_occurrences
                        .entry(cleaned)
                        .or_default()
                        .insert(product.product_type);
                }
            }
            for (name, value) in &product.attributes {
                record_attribute_occurrence(
                    &mut type_occurrences,
                    name,
                    value,
                    product.product_type,
                );
            }
            for variant in &product.variants {
                for (name, value) in &variant.attributes {
                    record_attribute_occurrence(
                        &mut type_occurrences,
                        name,
                        value,
                        product.product_type,
                    );
                }
            }
        }
        ResidualPolicy { type_occurrences }
    }

    /// Classify one residual-lexical token. `None` observed occurrence (the
    /// token appears nowhere in the compiled catalog) is the safest default
    /// — [`ResidualClass::Required`], zero positive evidence the token is
    /// harmless to bypass.
    ///
    /// Deliberately takes only `token`, not a `ProductTypeId` — the eval
    /// prototype this ports (`issue42-eval::r2_experimental::ResidualPolicy::classify`)
    /// carried an unused `_product_type` parameter whose own doc comment,
    /// backed by a dedicated regression test
    /// (`treatment_d_does_not_recover_a_compound_constraint_query_whose_wrong_variant_the_residual_word_would_have_excluded`),
    /// confirms an earlier "observed anywhere under this product type" use
    /// of that parameter was a real false-positive-producing defect, fixed
    /// by removing that condition entirely and keeping only cross-type
    /// breadth as the signal. Since the parameter never affected the
    /// result, production drops it from the signature outright; the
    /// caller-side "is there a corroborating `ProductType` constraint at
    /// all" check (`docs/adr/0012-residual-lexical-policy.md`) stays a
    /// caller responsibility, not something `classify` itself evaluates.
    pub fn classify(&self, token: &str) -> ResidualClass {
        match self.type_occurrences.get(&token.to_lowercase()) {
            None => ResidualClass::Required,
            Some(types) => {
                if types.len() >= CROSS_TYPE_BREADTH_THRESHOLD {
                    ResidualClass::Preferred
                } else {
                    ResidualClass::Required
                }
            }
        }
    }
}
