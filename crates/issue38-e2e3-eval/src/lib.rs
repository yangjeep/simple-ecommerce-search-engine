//! Issue #38 E2/E3: deterministic synthetic catalogs.
//!
//! E2 needs a catalog from a commerce vertical *materially different* from
//! WANDS (home furnishings) -- not a renamed furniture category, a
//! genuinely different attribute schema and shopper vocabulary. E3 needs a
//! realistic mixed-category merchant: several incompatible product
//! families in one catalog, sharing ambiguous field *names* with
//! different value vocabularies and even different `AttributeValue`
//! *kinds* per family, sparse attributes, noisy titles, and deliberately
//! cross-category-ambiguous queries.
//!
//! Every generator here is a pure function of a `ChaCha8Rng` seeded from a
//! fixed constant (see each module's `SEED`) -- no wall-clock, no
//! uncontrolled randomness. Re-running any generator reproduces the exact
//! same catalog byte-for-byte, which is what makes the ground-truth
//! relevance judgments trustworthy: they are computed directly from the
//! same generation parameters that produced the catalog, not hand-labeled
//! after the fact.
//!
//! No `commerce_core` production code is touched by anything in this
//! crate -- every synthetic attribute maps onto the existing
//! `AttributeValue::{Enum, MultiEnum, Numeric, Boolean, Text}` kinds and
//! the existing `Product`/`Variant`/`Catalog` domain types, exactly like
//! `phase6a_eval::catalog` does for real WANDS data. E2's fitment
//! relationship (a variant compatible with a *set* of `"{year} {make}
//! {model}"` phrases, e.g. `"2015 honda civic"`) is represented as
//! `AttributeValue::MultiEnum` on the variant, queried via the
//! already-existing `Constraint::MultiEnumContains` -- deliberately not a
//! new `StructuralConstraint` variant, since Issue #38 and CLAUDE.md both
//! ask that schema flexibility be handled by the existing generic
//! mechanism, not a new vertical-specific one. The value is a natural,
//! space-joined phrase rather than a pipe-joined key specifically so
//! `ir::query::compile`'s real phrase-lexicon lookup (exact space-joined
//! token windows only) can actually discover it from real query text --
//! see `automotive.rs`'s `fitment_tag` doc comment.

pub mod apparel;
pub mod automotive;
pub mod failure_taxonomy;
pub mod furniture_synth;
pub mod ground_truth;
pub mod ingest;
pub mod mixed_merchant;

use commerce_core::domain::AttributeValue;

/// One synthetic product, family-tagged only for this crate's own
/// reporting/failure-taxonomy purposes -- `family` is never read by
/// `ingest::build_catalog` or by any `commerce_core` code path, so it
/// cannot leak into a "search-time category intelligence" shortcut.
#[derive(Debug, Clone, PartialEq)]
pub struct SynthProduct {
    pub external_id: String,
    pub family: &'static str,
    pub product_type: String,
    pub category_path: String,
    pub brand: String,
    pub title: String,
    pub product_attributes: Vec<(String, AttributeValue)>,
    pub variants: Vec<SynthVariant>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SynthVariant {
    pub external_id: String,
    pub variant_attributes: Vec<(String, AttributeValue)>,
    pub price_cents: i64,
}

/// A synthetic shopper query with a machine-checkable provenance tag
/// (`template`) identifying which ground-truth rule generated it, so
/// failure-taxonomy analysis can group results by *why* a query was
/// asked, not just by its text.
#[derive(Debug, Clone, PartialEq)]
pub struct SynthQuery {
    pub id: String,
    pub text: String,
    pub template: &'static str,
}

pub use phase9_eval::wands_relevance::WandsLabel as RelevanceLabel;

#[cfg(test)]
mod tests {
    use super::*;
    use commerce_core::cold_start::{compile_lexicon, CatalogProfile};
    use commerce_core::domain::{AttributeValue, Constraint};
    use commerce_core::ir::compile as compile_query;
    use commerce_core::ir::{ResolvedConstraint, StructuralConstraint};

    /// Every family's generator must be a pure function of its fixed
    /// `SEED` -- re-running it must reproduce the exact same catalog and
    /// workload, byte-for-byte, since that reproducibility is the whole
    /// basis for trusting the programmatically-derived ground truth.
    #[test]
    fn every_family_generator_is_deterministic() {
        for n in [0usize, 1, 37] {
            assert_eq!(
                automotive::generate_catalog(n),
                automotive::generate_catalog(n),
                "automotive::generate_catalog not deterministic at n={n}"
            );
            assert_eq!(
                apparel::generate_catalog(n),
                apparel::generate_catalog(n),
                "apparel::generate_catalog not deterministic at n={n}"
            );
            assert_eq!(
                furniture_synth::generate_catalog(n),
                furniture_synth::generate_catalog(n),
                "furniture_synth::generate_catalog not deterministic at n={n}"
            );
        }
        let auto_products = automotive::generate_catalog(40);
        assert_eq!(
            automotive::generate_workload(&auto_products),
            automotive::generate_workload(&auto_products),
            "automotive::generate_workload not deterministic"
        );
        let apparel_products = apparel::generate_catalog(40);
        assert_eq!(
            apparel::generate_workload(&apparel_products),
            apparel::generate_workload(&apparel_products),
            "apparel::generate_workload not deterministic"
        );
        let furniture_products = furniture_synth::generate_catalog(40);
        assert_eq!(
            furniture_synth::generate_workload(&furniture_products),
            furniture_synth::generate_workload(&furniture_products),
            "furniture_synth::generate_workload not deterministic"
        );
        let mixed = mixed_merchant::generate_mixed_catalog(20, 20, 20);
        assert_eq!(
            mixed_merchant::generate_mixed_catalog(20, 20, 20),
            mixed,
            "generate_mixed_catalog not deterministic"
        );
        assert_eq!(
            mixed_merchant::generate_cross_category_workload(&mixed),
            mixed_merchant::generate_cross_category_workload(&mixed),
            "generate_cross_category_workload not deterministic"
        );
    }

    #[test]
    fn automotive_ground_truth_is_self_consistent() {
        let products = automotive::generate_catalog(120);
        let (queries, judgments) = automotive::generate_workload(&products);
        ground_truth::assert_self_consistent(&products, &judgments);
        ground_truth::assert_every_query_of_template_has_an_exact_match(
            &queries,
            &judgments,
            "exact_lookup",
        );
        ground_truth::assert_every_query_of_template_has_an_exact_match(
            &queries,
            &judgments,
            "fitment_exact",
        );
    }

    #[test]
    fn apparel_ground_truth_is_self_consistent() {
        let products = apparel::generate_catalog(80);
        let (queries, judgments) = apparel::generate_workload(&products);
        ground_truth::assert_self_consistent(&products, &judgments);
        // Regression coverage for the adversarial-review finding: these
        // two templates now derive their query value from a real
        // generated product specifically so an Exact match is guaranteed,
        // not merely a same-type Partial fallback.
        ground_truth::assert_every_query_of_template_has_an_exact_match(
            &queries,
            &judgments,
            "attribute_plus_entity",
        );
        ground_truth::assert_every_query_of_template_has_an_exact_match(
            &queries,
            &judgments,
            "size_numeric_keyword",
        );
    }

    #[test]
    fn furniture_ground_truth_is_self_consistent() {
        let products = furniture_synth::generate_catalog(60);
        let (queries, judgments) = furniture_synth::generate_workload(&products);
        ground_truth::assert_self_consistent(&products, &judgments);
        ground_truth::assert_every_query_of_template_has_an_exact_match(
            &queries,
            &judgments,
            "attribute_plus_entity",
        );
    }

    #[test]
    fn mixed_merchant_cross_category_ground_truth_is_self_consistent() {
        let products = mixed_merchant::generate_mixed_catalog(60, 60, 60);
        let (queries, judgments) = mixed_merchant::generate_cross_category_workload(&products);
        ground_truth::assert_self_consistent(&products, &judgments);
        ground_truth::assert_every_query_of_template_has_an_exact_match(
            &queries,
            &judgments,
            "size_schema_conflict",
        );
        ground_truth::assert_every_query_of_template_has_an_exact_match(
            &queries,
            &judgments,
            "same_catalog_control",
        );
    }

    /// Directly exercises the fitment-phrase fix: `ir::query::compile`'s
    /// real phrase-lexicon lookup must actually discover a
    /// `Constraint::MultiEnumContains` fitment match from real query text
    /// (not merely from a hand-constructed `CommerceQuery`) -- this is the
    /// one thing E2 exists to test, so it must be proven to reach the real
    /// production `compile()` path, not just be present in the catalog.
    #[test]
    fn fitment_phrase_is_discoverable_by_the_real_compile_lexicon() {
        let products = automotive::generate_catalog(200);
        let ingested = ingest::build_catalog(&products);
        let profile = CatalogProfile::build(
            &ingested.catalog,
            &ingested.brands,
            &ingested.product_types,
            &ingested.categories,
        );
        let lexicon = compile_lexicon(&profile, 1);

        // Find a real product with a real fitment value to query for.
        let target = products
            .iter()
            .find(|p| p.product_type == "Brake Pads")
            .expect("at least one Brake Pads product at n=200");
        let fitment_value = target.variants[0]
            .variant_attributes
            .iter()
            .find_map(|(k, v)| match (k.as_str(), v) {
                ("compatible_fitment", AttributeValue::MultiEnum(vs)) => vs.first().cloned(),
                _ => None,
            })
            .expect("Brake Pads always has compatible_fitment (has_fitment=true)");
        // The fitment value must itself be a natural "{year} {make}
        // {model}" phrase (three space-separated words), not a
        // pipe-joined key -- the defect this test guards against.
        assert_eq!(
            fitment_value.split_whitespace().count(),
            3,
            "fitment value {fitment_value:?} is not a 3-word phrase"
        );

        let query_text = format!("brake pads for {fitment_value}");
        let compiled = compile_query(&query_text, &lexicon);

        let has_product_type_entity = compiled.constraints.iter().any(|c| {
            matches!(
                c,
                ResolvedConstraint::Structural(StructuralConstraint::ProductType(_))
            )
        });
        assert!(
            has_product_type_entity,
            "expected a resolved ProductType entity constraint from {query_text:?}, got {:?}",
            compiled.constraints
        );

        let has_fitment_match = compiled.constraints.iter().any(|c| {
            matches!(
                c,
                ResolvedConstraint::Attribute(Constraint::MultiEnumContains { attribute, value })
                    if attribute == "compatible_fitment" && value == &fitment_value
            )
        });
        assert!(
            has_fitment_match,
            "expected compile() to resolve a MultiEnumContains(compatible_fitment={fitment_value:?}) \
             hard constraint from real query text {query_text:?}, got {:?} -- if this fails, the \
             fitment value format is once again undiscoverable by the real phrase lexicon",
            compiled.constraints
        );
    }
}
