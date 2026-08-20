//! Issue #14 (Phase 3): `commerce_core::admission`'s safe/complete
//! admission contract. Each test isolates exactly one rejection reason so
//! a regression can be traced to the specific check that broke, matching
//! `tests/plan.rs`'s own one-outcome-per-test discipline.

use commerce_core::admission::{
    admit, admit_lexically_narrowed, execute_lexically_narrowed, AdmissionDecision,
    AdmissionPolicy, RejectReason,
};
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

/// 11 products, real titles this time (needed to populate
/// `lexical_postings`, which indexes `Product::title` -- Issue #14
/// P3-E03's real-data diagnostic found "every residual token found
/// somewhere in this catalog's text" is the split that matters most).
/// Exactly one Nike product's title mentions "waterproof"; product 5
/// (an Other-brand product) also does, so a residual-only ("no brand
/// mentioned") query for "waterproof" narrows to 2 candidates.
fn eleven_product_catalog_with_titles() -> Catalog {
    let mut products = Vec::new();
    for i in 0..11u64 {
        let brand = if i == 0 { NIKE } else { OTHER };
        let title = if i == 0 {
            "Nike Waterproof Trail Runner".to_string()
        } else if i == 5 {
            "Other Waterproof Hiking Boot".to_string()
        } else {
            format!("Product {i}")
        };
        products.push(Product {
            id: ProductId(i),
            product_type: ProductTypeId(1),
            brand,
            category: CategoryId(1),
            title,
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

fn query_with_residual(constraints: Vec<ResolvedConstraint>, residual: Vec<&str>) -> CommerceQuery {
    CommerceQuery {
        constraints,
        preferences: vec![],
        ambiguous: vec![],
        residual_lexical: residual.into_iter().map(String::from).collect(),
    }
}

#[test]
fn lexically_narrows_a_residual_token_found_in_exactly_the_expected_titles() {
    let catalog = eleven_product_catalog_with_titles();
    let index = CatalogIndex::build(&catalog);
    let query = query_with_residual(vec![], vec!["waterproof"]);

    let result = admit_lexically_narrowed(&query, &index, 10);

    let (_, count) = result.expect("waterproof is a real token in this catalog's titles");
    assert_eq!(
        count, 2,
        "exactly products 0 and 5 mention \"waterproof\" in their title"
    );
}

#[test]
fn combines_lexical_narrowing_with_an_existing_structural_constraint() {
    let catalog = eleven_product_catalog_with_titles();
    let index = CatalogIndex::build(&catalog);
    let query = query_with_residual(
        vec![ResolvedConstraint::Structural(StructuralConstraint::Brand(
            NIKE,
        ))],
        vec!["waterproof"],
    );

    let result = admit_lexically_narrowed(&query, &index, 10);

    let (_, count) = result.expect("Brand=Nike AND \"waterproof\" should combine to one product");
    assert_eq!(
        count, 1,
        "only product 0 is both Nike-branded and waterproof"
    );
}

#[test]
fn rejects_lexical_narrowing_when_the_combined_candidate_set_exceeds_the_cap() {
    let catalog = eleven_product_catalog_with_titles();
    let index = CatalogIndex::build(&catalog);
    let query = query_with_residual(vec![], vec!["waterproof"]);

    let result = admit_lexically_narrowed(&query, &index, 1);

    assert!(
        result.is_none(),
        "2 real candidates exceeds a cap of 1, so this must reject: {result:?}"
    );
}

/// The most important negative case: a residual token this catalog's own
/// text never contains at all must reject outright (`None`), not
/// "safely narrow to zero candidates" -- the two are different claims,
/// and only the latter would be safe to admit.
#[test]
fn rejects_lexical_narrowing_when_a_residual_token_appears_in_no_title_at_all() {
    let catalog = eleven_product_catalog_with_titles();
    let index = CatalogIndex::build(&catalog);
    let query = query_with_residual(vec![], vec!["zzznonexistentword"]);

    let result = admit_lexically_narrowed(&query, &index, 1_000);

    assert!(
        result.is_none(),
        "an out-of-vocabulary residual token must never be treated as a safe zero-candidate match: {result:?}"
    );
}

/// P3-E03 real-data measurement (`docs/experiments/PHASE3_LOG.md`) found
/// 24.9% of eligible queries had a combined candidate count of exactly
/// zero -- every residual token individually known somewhere in the
/// catalog, but their AND-combination (or the AND-combination with an
/// existing structural constraint) empty. Before this fix, `count == 0`
/// passed the `count > max_lexical_narrowed_candidates` check trivially
/// (0 is never greater than any cap), so the query was "admitted" to a
/// guaranteed-empty native result -- exactly the same unsafe claim this
/// function's own doc comment already rejects for a single out-of-
/// vocabulary token ("a residual token that never appears... makes the
/// whole query ineligible... rather than safely narrows to zero
/// candidates"), just not applied to the *combined* count. "runner" and
/// "boot" are each individually known (product 0's and product 5's title
/// respectively) but no single product's title contains both words, so
/// their AND is empty even though neither token is out-of-vocabulary.
#[test]
fn rejects_lexical_narrowing_when_every_token_is_known_but_the_combined_set_is_empty() {
    let catalog = eleven_product_catalog_with_titles();
    let index = CatalogIndex::build(&catalog);
    let query = query_with_residual(vec![], vec!["runner boot"]);

    let result = admit_lexically_narrowed(&query, &index, 1_000);

    assert!(
        result.is_none(),
        "\"runner\" and \"boot\" are each individually known but no product has both -- a \
         guaranteed-empty combined set must reject exactly like an out-of-vocabulary token, not \
         be treated as a safe zero-candidate admission: {result:?}"
    );
}

#[test]
fn rejects_lexical_narrowing_for_an_ambiguous_query_regardless_of_residual_content() {
    let catalog = eleven_product_catalog_with_titles();
    let index = CatalogIndex::build(&catalog);
    let mut query = query_with_residual(vec![], vec!["waterproof"]);
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

    let result = admit_lexically_narrowed(&query, &index, 1_000);

    assert!(result.is_none());
}

#[test]
fn rejects_lexical_narrowing_when_residual_is_already_empty() {
    let catalog = eleven_product_catalog_with_titles();
    let index = CatalogIndex::build(&catalog);
    let query = query_with(vec![ResolvedConstraint::Structural(
        StructuralConstraint::Brand(NIKE),
    )]);

    let result = admit_lexically_narrowed(&query, &index, 1_000);

    assert!(
        result.is_none(),
        "this mechanism is for the UnresolvedResidual case specifically, not a general-purpose recheck"
    );
}

/// `execute_lexically_narrowed` must still re-verify every hard
/// constraint itself, never trusting the caller's `narrow_by` bitmap --
/// mirrors `plan::execute_planned`'s own "commerce_core re-verifies
/// every hard constraint against every returned hit itself" contract.
/// Deliberately hands it a `narrow_by` bitmap covering every ordinal in
/// the catalog (as if the caller's own narrowing were wrong/absent) and
/// checks that a real Brand constraint still filters correctly.
#[test]
fn execute_lexically_narrowed_still_reverifies_hard_constraints_independent_of_narrow_by() {
    let catalog = eleven_product_catalog_with_titles();
    let index = CatalogIndex::build(&catalog);
    let query = query_with(vec![ResolvedConstraint::Structural(
        StructuralConstraint::Brand(NIKE),
    )]);
    let everything = index.indexed_candidates(&[]); // every ordinal, not a real narrowing

    let hits = execute_lexically_narrowed(&index, &query, &everything, &catalog, 10);

    assert_eq!(
        hits.len(),
        1,
        "only the one real Nike product must survive, regardless of narrow_by: {hits:?}"
    );
    assert_eq!(hits[0].product, ProductId(0));
}
