use commerce_core::domain::{Constraint, NumericOp, ProductId, VariantId};
use commerce_core::fixtures::{
    representative_query_catalog, shoe_lexicon, variant_safety_catalog, REPRESENTATIVE_QUERY,
};
use commerce_core::ir::{compile, ResolvedConstraint, StructuralConstraint};

#[test]
fn compiles_representative_query_into_expected_typed_constraints() {
    let query = compile(REPRESENTATIVE_QUERY, &shoe_lexicon());

    let expected = vec![
        ResolvedConstraint::Attribute(Constraint::Enum {
            attribute: "color".to_string(),
            value: "Black".to_string(),
        }),
        ResolvedConstraint::Structural(StructuralConstraint::Brand(
            commerce_core::domain::BrandId(1),
        )),
        ResolvedConstraint::Attribute(Constraint::Boolean {
            attribute: "waterproof".to_string(),
            value: true,
        }),
        ResolvedConstraint::Structural(StructuralConstraint::ProductType(
            commerce_core::domain::ProductTypeId(1),
        )),
        ResolvedConstraint::Attribute(Constraint::Numeric {
            attribute: "size".to_string(),
            op: NumericOp::Eq,
            value: 9.0,
        }),
        ResolvedConstraint::Structural(StructuralConstraint::PriceUnderCents(15_000)),
    ];

    assert_eq!(query.constraints, expected);
    assert!(query.preferences.is_empty());
    assert!(query.ambiguous.is_empty());
    assert!(query.residual_lexical.is_empty());
}

/// Ties Gate 1 and Gate 2 together: the compiled representative query must
/// not match the variant-safety fixture, because that catalog has no
/// variant that is simultaneously black *and* size 9 (black is size 8,
/// size 9 is red). This guards against a regression where the IR executor
/// re-introduces the cross-variant flattening bug Gate 1 ruled out.
#[test]
fn representative_query_does_not_cross_variant_match() {
    let query = compile(REPRESENTATIVE_QUERY, &shoe_lexicon());
    let hits = query.execute(&variant_safety_catalog());
    assert!(hits.is_empty(), "cross-variant false match: {hits:?}");
}

#[test]
fn representative_query_matches_a_catalog_that_actually_has_it() {
    let query = compile(REPRESENTATIVE_QUERY, &shoe_lexicon());
    let hits = query.execute(&representative_query_catalog());
    assert_eq!(hits, vec![(ProductId(2), VariantId(201))]);
}

#[test]
fn ambiguous_term_is_preserved_not_silently_resolved() {
    let query = compile("leather boots", &shoe_lexicon());
    assert!(query.constraints.is_empty());
    assert_eq!(query.ambiguous.len(), 1);
    assert_eq!(query.ambiguous[0].text, "leather");
    assert_eq!(query.ambiguous[0].candidates.len(), 2);
    assert_eq!(query.residual_lexical, vec!["boots".to_string()]);
}

#[test]
fn unrecognized_brand_becomes_residual_lexical_not_dropped() {
    let query = compile("Reebok running shoes", &shoe_lexicon());
    assert_eq!(query.residual_lexical, vec!["reebok".to_string()]);
    assert_eq!(
        query.constraints,
        vec![ResolvedConstraint::Structural(
            StructuralConstraint::ProductType(commerce_core::domain::ProductTypeId(1))
        )]
    );
    assert!(query.ambiguous.is_empty());
}

#[test]
fn descriptive_terms_compile_as_preferences_not_hard_constraints() {
    let query = compile("cushioned breathable running shoes", &shoe_lexicon());
    assert_eq!(query.preferences.len(), 2);
    assert_eq!(query.constraints.len(), 1);
    assert!(query.residual_lexical.is_empty());
}
