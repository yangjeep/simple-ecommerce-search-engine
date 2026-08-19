use commerce_core::domain::{Catalog, Constraint, NumericOp, ProductId, VariantId};
use commerce_core::fixtures::{
    representative_query_catalog, shoe_lexicon, variant_safety_catalog, REPRESENTATIVE_QUERY,
};
use commerce_core::index::CatalogIndex;
use commerce_core::ir::{compile, CommerceQuery, ResolvedConstraint};

fn combined_catalog() -> Catalog {
    let mut catalog = variant_safety_catalog();
    catalog
        .products
        .extend(representative_query_catalog().products);
    catalog
}

fn sorted(mut hits: Vec<(ProductId, VariantId)>) -> Vec<(ProductId, VariantId)> {
    hits.sort();
    hits
}

fn assert_index_matches_linear_scan(query: &CommerceQuery, catalog: &Catalog) {
    let index = CatalogIndex::build(catalog);
    let indexed = sorted(index.execute(query, catalog));
    let linear = sorted(query.execute(catalog));
    assert_eq!(
        indexed, linear,
        "physical index disagreed with the linear-scan ground truth for {query:?}"
    );
}

#[test]
fn index_matches_linear_scan_for_the_representative_query() {
    let query = compile(REPRESENTATIVE_QUERY, &shoe_lexicon());
    assert_index_matches_linear_scan(&query, &combined_catalog());
}

#[test]
fn index_matches_linear_scan_for_every_gate1_adversarial_case() {
    let catalog = variant_safety_catalog();
    let cases = [
        vec![
            ResolvedConstraint::Attribute(Constraint::Enum {
                attribute: "color".to_string(),
                value: "Black".to_string(),
            }),
            ResolvedConstraint::Attribute(Constraint::Numeric {
                attribute: "size".to_string(),
                op: NumericOp::Eq,
                value: 9.0,
            }),
        ],
        vec![
            ResolvedConstraint::Attribute(Constraint::Enum {
                attribute: "color".to_string(),
                value: "Black".to_string(),
            }),
            ResolvedConstraint::Attribute(Constraint::Numeric {
                attribute: "size".to_string(),
                op: NumericOp::Eq,
                value: 8.0,
            }),
        ],
        vec![Constraint::Boolean {
            attribute: "waterproof".to_string(),
            value: true,
        }]
        .into_iter()
        .map(ResolvedConstraint::Attribute)
        .collect(),
        vec![Constraint::Text {
            attribute: "material".to_string(),
            contains: "mesh".to_string(),
        }]
        .into_iter()
        .map(ResolvedConstraint::Attribute)
        .collect(),
    ];
    for constraints in cases {
        let query = CommerceQuery {
            constraints,
            ..CommerceQuery::default()
        };
        assert_index_matches_linear_scan(&query, &catalog);
    }
}

#[test]
fn text_only_query_narrows_then_verifies_correctly() {
    // No structural/attribute-indexable constraint at all: indexed_candidates
    // must fall back to "everything", and the Text constraint must still be
    // enforced (not silently dropped just because there's nothing to index).
    let query = CommerceQuery {
        constraints: vec![ResolvedConstraint::Attribute(Constraint::Text {
            attribute: "material".to_string(),
            contains: "synthetic".to_string(),
        })],
        ..CommerceQuery::default()
    };
    assert_index_matches_linear_scan(&query, &combined_catalog());
    let index = CatalogIndex::build(&combined_catalog());
    let hits = index.execute(&query, &combined_catalog());
    assert_eq!(hits, vec![(ProductId(2), VariantId(201))]);
}

#[test]
fn exact_id_lookup_finds_the_right_product_and_variant() {
    let catalog = variant_safety_catalog();
    let index = CatalogIndex::build(&catalog);

    let (product, variant) = index
        .lookup_variant(&catalog, VariantId(102))
        .expect("variant 102 exists in the fixture");
    assert_eq!(product.id, ProductId(1));
    assert_eq!(variant.id, VariantId(102));

    assert!(index.lookup_variant(&catalog, VariantId(999)).is_none());
    assert!(index.lookup_product(&catalog, ProductId(1)).is_some());
    assert!(index.lookup_product(&catalog, ProductId(999)).is_none());
}

#[test]
fn facet_counts_reflect_the_candidate_set_not_the_whole_catalog() {
    let catalog = variant_safety_catalog();
    let index = CatalogIndex::build(&catalog);

    let all = index.indexed_candidates(&[]);
    let facets = index.facet_counts("color", &all);
    assert_eq!(facets.get("Black"), Some(&1));
    assert_eq!(facets.get("Red"), Some(&1));

    let waterproof_only =
        index.indexed_candidates(&[ResolvedConstraint::Attribute(Constraint::Boolean {
            attribute: "waterproof".to_string(),
            value: true,
        })]);
    let facets = index.facet_counts("color", &waterproof_only);
    assert_eq!(facets.get("Black"), Some(&1));
    assert_eq!(facets.get("Red"), Some(&1));

    let black_only = index.indexed_candidates(&[ResolvedConstraint::Attribute(Constraint::Enum {
        attribute: "color".to_string(),
        value: "Black".to_string(),
    })]);
    let facets = index.facet_counts("color", &black_only);
    assert_eq!(facets.get("Black"), Some(&1));
    assert_eq!(facets.get("Red"), None);
}

#[test]
fn top_k_ranking_orders_by_preference_score_deterministically() {
    let catalog = combined_catalog();
    let index = CatalogIndex::build(&catalog);
    // Nothing structurally restricts these three variants apart (all three
    // are running shoes), but only the variant_safety_catalog product is
    // tagged both "cushioned" and "breathable" (features: [cushioned,
    // breathable]); representative_query_catalog's product is only tagged
    // "cushioned" (features: [cushioned]). So variants 101/102 must score
    // higher than variant 201 and be ranked ahead of it.
    let query = compile("cushioned breathable running shoes", &shoe_lexicon());
    let ranked = index.execute_ranked(&query, &catalog, 10);

    assert_eq!(
        ranked.len(),
        3,
        "expected all three variants as candidates: {ranked:?}"
    );
    assert!(
        ranked.windows(2).all(|w| w[0].score >= w[1].score),
        "not sorted by score desc: {ranked:?}"
    );
    assert_eq!(
        ranked[0].score, 1.0,
        "top hits should match both preference terms: {ranked:?}"
    );
    assert_eq!(ranked[1].score, 1.0);
    assert_eq!(
        ranked[2].score, 0.5,
        "the third variant only matches one preference term: {ranked:?}"
    );
    assert_eq!(ranked[2].variant, VariantId(201));
}
