use commerce_core::domain::{BrandId, Constraint, NumericOp, ProductId, VariantId};
use commerce_core::fixtures::{
    representative_query_catalog, shoe_lexicon, variant_safety_catalog, REPRESENTATIVE_QUERY,
};
use commerce_core::ir::{compile, Candidate, ResolvedConstraint, StructuralConstraint};

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

/// Round 1 R1-E03's single most severe finding, fixed and pinned: "not
/// red" used to compile to a REQUIRED red constraint (the exact opposite
/// of stated intent). It must instead resolve everything else normally
/// and put "red" in `residual_lexical`, never as a positive constraint.
#[test]
fn negation_prevents_the_negated_phrase_from_becoming_a_positive_constraint() {
    let query = compile("Nike running shoes not red", &shoe_lexicon());
    assert_eq!(
        query.constraints,
        vec![
            ResolvedConstraint::Structural(StructuralConstraint::Brand(
                commerce_core::domain::BrandId(1)
            )),
            ResolvedConstraint::Structural(StructuralConstraint::ProductType(
                commerce_core::domain::ProductTypeId(1)
            )),
        ],
        "no color=Red constraint must be present: {:?}",
        query.constraints
    );
    assert!(query.ambiguous.is_empty());
    assert_eq!(query.residual_lexical, vec!["red".to_string()]);
}

/// The same fix must also suppress an *ambiguous* phrase, not just a
/// single-candidate one -- "leather" is Phase 0's deliberately ambiguous
/// collision; negated, it must not appear as an ambiguous span either.
#[test]
fn negation_suppresses_an_ambiguous_phrase_instead_of_flagging_it_ambiguous() {
    let query = compile("running shoes that aren't leather", &shoe_lexicon());
    assert_eq!(
        query.constraints,
        vec![ResolvedConstraint::Structural(
            StructuralConstraint::ProductType(commerce_core::domain::ProductTypeId(1))
        )]
    );
    assert!(
        query.ambiguous.is_empty(),
        "negated \"leather\" must not surface as ambiguous: {:?}",
        query.ambiguous
    );
    assert!(query.residual_lexical.contains(&"leather".to_string()));
}

/// Issue #6 P1-D / P2-E14 (`docs/experiments/PHASE2_LOG.md`): a real P1-D
/// sweep found `selective_multi_attribute_structural` at 100% zero-result
/// for commerce-native (22/22 real queries) -- e.g. "harry potter lego"
/// independently resolving both "harry potter" and "lego" to *different*
/// `Brand` ids, which the compiler then hard-ANDed together. A product has
/// exactly one brand, so two distinct hard `Brand`/`BrandAny`/`ProductType`/
/// `Category` constraints (a "single-valued entity slot") can never be
/// jointly satisfied by any real product -- that AND is not a narrow
/// query, it is a guaranteed-empty one. The first phrase to claim a slot
/// keeps its hard constraint (matches this compiler's existing leftmost/
/// longest-match-first bias); a second, conflicting phrase must fall back
/// to residual free text instead of a second, mutually-exclusive hard
/// constraint.
#[test]
fn conflicting_same_slot_entity_constraints_do_not_get_and_ed_together() {
    let mut lex = shoe_lexicon();
    lex.insert(
        "adidas",
        vec![Candidate::constraint(
            ResolvedConstraint::Structural(StructuralConstraint::Brand(BrandId(2))),
            1.0,
        )],
    );

    let query = compile("nike adidas running shoes", &lex);

    assert_eq!(
        query.constraints,
        vec![
            ResolvedConstraint::Structural(StructuralConstraint::Brand(BrandId(1))),
            ResolvedConstraint::Structural(StructuralConstraint::ProductType(
                commerce_core::domain::ProductTypeId(1)
            )),
        ],
        "only the first brand match (nike) may become a hard constraint: {:?}",
        query.constraints
    );
    assert!(
        query.residual_lexical.contains(&"adidas".to_string()),
        "the conflicting second brand match must fall back to residual free text: {:?}",
        query.residual_lexical
    );
    assert!(query.ambiguous.is_empty());
}

/// Identical repeated matches of the same entity are a harmless no-op, not
/// a conflict -- "nike nike shoes" must not discard the second "nike".
#[test]
fn identical_repeated_entity_matches_are_not_treated_as_a_conflict() {
    let query = compile("nike nike running shoes", &shoe_lexicon());
    assert_eq!(
        query.constraints,
        vec![
            ResolvedConstraint::Structural(StructuralConstraint::Brand(BrandId(1))),
            ResolvedConstraint::Structural(StructuralConstraint::Brand(BrandId(1))),
            ResolvedConstraint::Structural(StructuralConstraint::ProductType(
                commerce_core::domain::ProductTypeId(1)
            )),
        ]
    );
    assert!(query.residual_lexical.is_empty());
}

#[test]
fn descriptive_terms_compile_as_preferences_not_hard_constraints() {
    let query = compile("cushioned breathable running shoes", &shoe_lexicon());
    assert_eq!(query.preferences.len(), 2);
    assert_eq!(query.constraints.len(), 1);
    // Issue #6 P1-B: a Preference is a soft ranking signal, never a hard
    // filter, so the phrase that produced it must remain searchable via
    // the lexical residual path too -- a real-data regression (a
    // preference-only match silently dropping real retrieval signal,
    // `docs/experiments/PHASE2_LOG.md` P2-E11) found this the hard way
    // when a fuzzy soft brand match started actually being exercised for
    // the first time.
    assert_eq!(
        query.residual_lexical,
        vec!["cushioned".to_string(), "breathable".to_string()]
    );
}
