//! Issue #14 (Phase 3): `commerce_core::admission`'s safe/complete
//! admission contract. Each test isolates exactly one rejection reason so
//! a regression can be traced to the specific check that broke, matching
//! `tests/plan.rs`'s own one-outcome-per-test discipline.

use commerce_core::admission::{admit, AdmissionDecision, AdmissionPolicy, RejectReason};
use commerce_core::domain::{
    attributes, BrandId, Catalog, CategoryId, Constraint, Inventory, Price, Product, ProductId,
    ProductTypeId, Variant, VariantId,
};
use commerce_core::index::CatalogIndex;
use commerce_core::ir::{AmbiguousSpan, Candidate, CommerceQuery, ResolvedConstraint};
use commerce_core::ir::{Preference, StructuralConstraint};

const NIKE: BrandId = BrandId(1);
const OTHER: BrandId = BrandId(2);

/// 11 products: exactly 1 Nike (selective, ~9%) and 10 Other-brand
/// (non-selective, ~91%) -- the same shape `tests/plan.rs`'s fixture
/// uses, so a `max_candidates` policy can be tested against both a
/// clearly-safe and a clearly-unsafe real candidate count.
fn eleven_product_catalog() -> Catalog {
    let mut products = Vec::new();
    for i in 0..11u64 {
        let brand = if i == 0 { NIKE } else { OTHER };
        products.push(Product {
            id: ProductId(i),
            product_type: ProductTypeId(1),
            brand,
            category: CategoryId(1),
            title: format!("Product {i}"),
            attributes: attributes([]),
            variants: vec![Variant {
                id: VariantId(100 + i),
                attributes: attributes([]),
                price: Price::usd(1_000),
                inventory: Inventory::in_stock(10),
            }],
        });
    }
    Catalog { products }
}

fn query_with(constraints: Vec<ResolvedConstraint>) -> CommerceQuery {
    CommerceQuery {
        constraints,
        preferences: vec![],
        ambiguous: vec![],
        residual_lexical: vec![],
    }
}

#[test]
fn admits_a_query_whose_candidate_count_is_within_the_policy_cap() {
    let catalog = eleven_product_catalog();
    let index = CatalogIndex::build(&catalog);
    let query = query_with(vec![ResolvedConstraint::Structural(
        StructuralConstraint::Brand(NIKE),
    )]);
    let policy = AdmissionPolicy { max_candidates: 5 };

    let decision = admit(&query, &index, &policy);

    assert_eq!(decision, AdmissionDecision::Admit { candidates: 1 });
    assert!(decision.is_admit());
}

#[test]
fn rejects_a_query_whose_candidate_count_exceeds_the_policy_cap() {
    let catalog = eleven_product_catalog();
    let index = CatalogIndex::build(&catalog);
    let query = query_with(vec![ResolvedConstraint::Structural(
        StructuralConstraint::Brand(OTHER),
    )]);
    let policy = AdmissionPolicy { max_candidates: 5 };

    let decision = admit(&query, &index, &policy);

    assert_eq!(
        decision,
        AdmissionDecision::Reject(RejectReason::NotSelectiveEnough { candidates: 10 })
    );
    assert!(!decision.is_admit());
}

/// A candidate count exactly at the cap must admit -- `max_candidates` is
/// inclusive, and this pins the boundary rather than leaving it to
/// whichever comparison operator happened to get typed.
#[test]
fn admits_a_query_whose_candidate_count_exactly_equals_the_policy_cap() {
    let catalog = eleven_product_catalog();
    let index = CatalogIndex::build(&catalog);
    let query = query_with(vec![ResolvedConstraint::Structural(
        StructuralConstraint::Brand(OTHER),
    )]);
    let policy = AdmissionPolicy { max_candidates: 10 };

    let decision = admit(&query, &index, &policy);

    assert_eq!(decision, AdmissionDecision::Admit { candidates: 10 });
}

#[test]
fn rejects_an_ambiguous_query_regardless_of_selectivity() {
    let catalog = eleven_product_catalog();
    let index = CatalogIndex::build(&catalog);
    let mut query = query_with(vec![ResolvedConstraint::Structural(
        StructuralConstraint::Brand(NIKE),
    )]);
    query.ambiguous.push(AmbiguousSpan {
        text: "leather".to_string(),
        candidates: vec![
            Candidate::constraint(
                ResolvedConstraint::Attribute(Constraint::Enum {
                    attribute: "color".to_string(),
                    value: "Brown".to_string(),
                }),
                1.0,
            ),
            Candidate::constraint(
                ResolvedConstraint::Attribute(Constraint::Text {
                    attribute: "material".to_string(),
                    contains: "leather".to_string(),
                }),
                1.0,
            ),
        ],
    });
    // A generously-wide policy: ambiguity must reject even when the
    // resolved structural constraint alone would easily fit the cap.
    let policy = AdmissionPolicy {
        max_candidates: 1_000,
    };

    let decision = admit(&query, &index, &policy);

    assert_eq!(decision, AdmissionDecision::Reject(RejectReason::Ambiguous));
}

#[test]
fn rejects_a_query_with_unresolved_residual_lexical_text() {
    let catalog = eleven_product_catalog();
    let index = CatalogIndex::build(&catalog);
    let mut query = query_with(vec![ResolvedConstraint::Structural(
        StructuralConstraint::Brand(NIKE),
    )]);
    query.residual_lexical.push("waterproof".to_string());
    let policy = AdmissionPolicy {
        max_candidates: 1_000,
    };

    let decision = admit(&query, &index, &policy);

    assert_eq!(
        decision,
        AdmissionDecision::Reject(RejectReason::UnresolvedResidual)
    );
}

#[test]
fn rejects_a_query_with_no_structural_constraint_at_all() {
    let catalog = eleven_product_catalog();
    let index = CatalogIndex::build(&catalog);
    let query = query_with(vec![]);
    let policy = AdmissionPolicy {
        max_candidates: 1_000,
    };

    let decision = admit(&query, &index, &policy);

    assert_eq!(
        decision,
        AdmissionDecision::Reject(RejectReason::NoStructuralConstraint)
    );
}

/// A `Preference`-only compiled query (no hard `constraints`, per
/// `ir::query::compile`'s own contract) must still reject: a `Preference`
/// is a soft ranking signal, never something to narrow a candidate set
/// by, so it cannot make a query "complete" for native execution any more
/// than an entirely-unresolved query can. Guards against a future
/// `admit` change that mistakes a non-empty `preferences` list for
/// evidence of structure.
#[test]
fn rejects_a_preference_only_query_even_though_preferences_is_non_empty() {
    let catalog = eleven_product_catalog();
    let index = CatalogIndex::build(&catalog);
    let mut query = query_with(vec![]);
    query.preferences.push(Preference::Boost {
        attribute: "features".to_string(),
        value: "cushioned".to_string(),
        weight: 1.0,
    });
    query.residual_lexical.push("cushioned".to_string());
    let policy = AdmissionPolicy {
        max_candidates: 1_000,
    };

    let decision = admit(&query, &index, &policy);

    assert!(!decision.is_admit());
}
