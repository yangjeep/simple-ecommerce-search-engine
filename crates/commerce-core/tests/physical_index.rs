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
fn ordinal_of_round_trips_with_lexical_and_candidates() {
    let catalog = combined_catalog();
    let index = CatalogIndex::build(&catalog);

    let synthetic_hits = index.lexical_and_candidates(&["synthetic".to_string()]);
    let ordinal = index
        .ordinal_of(VariantId(201))
        .expect("variant 201 exists in the fixture");
    assert!(
        synthetic_hits.contains(ordinal),
        "the variant known to carry \"synthetic\" in its material text must be in the token index's own ordinal space"
    );
    assert!(index.ordinal_of(VariantId(999_999)).is_none());
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
fn brand_facet_counts_reflect_the_candidate_set_not_the_whole_catalog() {
    // Issue #17 (Phase 5): brand faceting is a dedicated method, not
    // answerable through `facet_counts("brand", ...)` -- brand lives in
    // its own structural `brand_bitmaps` index, not the generic
    // `enum_bitmaps` `facet_counts` reads.
    use commerce_core::domain::BrandId;
    use commerce_core::fixtures::cold_start_catalog;
    use commerce_core::ir::StructuralConstraint;

    let catalog = cold_start_catalog();
    let index = CatalogIndex::build(&catalog);

    let all = index.indexed_candidates(&[]);
    let facets = index.brand_facet_counts(&all);
    // Brand(1): running_shoes_nike (2 variants) + hiking_boots_nike (2) = 4.
    // Brand(2): running_shoes_aerowalk (2) + hiking_boots_aerowalk (1) = 3.
    assert_eq!(facets.get(&BrandId(1)), Some(&4));
    assert_eq!(facets.get(&BrandId(2)), Some(&3));

    // facet_counts("brand", ...) must NOT silently answer this -- it has
    // no "brand" entry in enum_values at all, so it returns empty.
    assert!(index.facet_counts("brand", &all).is_empty());

    let running_shoes_only = index.indexed_candidates(&[ResolvedConstraint::Structural(
        StructuralConstraint::ProductType(commerce_core::domain::ProductTypeId(1)),
    )]);
    let facets = index.brand_facet_counts(&running_shoes_only);
    assert_eq!(facets.get(&BrandId(1)), Some(&2));
    assert_eq!(facets.get(&BrandId(2)), Some(&2));
}

#[test]
fn facet_counts_by_scan_matches_facet_counts_exactly() {
    // Issue #17 (Phase 5) P5-E00: `facet_counts`/`brand_facet_counts` cost
    // O(global vocabulary) rather than O(|candidates|), which real
    // benchmarking against Solr found to be 35-420ms vs Solr's 1-3ms. The
    // `*_by_scan` siblings are an O(|candidates|) alternative; before
    // trusting any latency comparison, both must agree byte-for-byte with
    // the existing methods on every input, including the empty-candidate
    // edge case.
    use commerce_core::domain::BrandId;
    use commerce_core::fixtures::cold_start_catalog;
    use commerce_core::ir::StructuralConstraint;
    use roaring::RoaringBitmap;

    let catalog = cold_start_catalog();
    let index = CatalogIndex::build(&catalog);

    let all = index.indexed_candidates(&[]);
    assert_eq!(
        index.facet_counts("color", &all),
        index.facet_counts_by_scan(&all, &catalog, "color")
    );
    assert_eq!(
        index.brand_facet_counts(&all),
        index.brand_facet_counts_by_scan(&all, &catalog)
    );

    let waterproof_only =
        index.indexed_candidates(&[ResolvedConstraint::Attribute(Constraint::Boolean {
            attribute: "waterproof".to_string(),
            value: true,
        })]);
    assert_eq!(
        index.facet_counts("color", &waterproof_only),
        index.facet_counts_by_scan(&waterproof_only, &catalog, "color")
    );
    assert_eq!(
        index.brand_facet_counts(&waterproof_only),
        index.brand_facet_counts_by_scan(&waterproof_only, &catalog)
    );

    let running_shoes_only = index.indexed_candidates(&[ResolvedConstraint::Structural(
        StructuralConstraint::ProductType(commerce_core::domain::ProductTypeId(1)),
    )]);
    assert_eq!(
        index.brand_facet_counts(&running_shoes_only),
        index.brand_facet_counts_by_scan(&running_shoes_only, &catalog)
    );
    assert_eq!(
        index
            .brand_facet_counts_by_scan(&running_shoes_only, &catalog)
            .get(&BrandId(1)),
        Some(&2)
    );

    let empty = RoaringBitmap::new();
    assert!(index
        .facet_counts_by_scan(&empty, &catalog, "color")
        .is_empty());
    assert!(index
        .brand_facet_counts_by_scan(&empty, &catalog)
        .is_empty());
}

#[test]
fn facet_counts_ordinal_matches_facet_counts_by_scan_exactly() {
    // Issue #21 Phase 6D: `facet_counts_ordinal` is a dictionary/ordinal-
    // encoded alternative to `facet_counts_by_scan`, motivated by P6C-E01
    // (docs/experiments/PHASE6C_LOG.md) finding that Lucene's own
    // ordinal-based facet module beats a naive per-candidate scan. Before
    // trusting any latency comparison, it must agree byte-for-byte with
    // `facet_counts_by_scan` on every input, including the empty-candidate
    // edge case -- same discipline as the scan-vs-map test above.
    use commerce_core::fixtures::cold_start_catalog;
    use roaring::RoaringBitmap;

    let catalog = cold_start_catalog();
    let index = CatalogIndex::build(&catalog);

    let all = index.indexed_candidates(&[]);
    assert_eq!(
        index.facet_counts_ordinal(&all, "color"),
        index.facet_counts_by_scan(&all, &catalog, "color")
    );

    let waterproof_only =
        index.indexed_candidates(&[ResolvedConstraint::Attribute(Constraint::Boolean {
            attribute: "waterproof".to_string(),
            value: true,
        })]);
    assert_eq!(
        index.facet_counts_ordinal(&waterproof_only, "color"),
        index.facet_counts_by_scan(&waterproof_only, &catalog, "color")
    );

    let empty = RoaringBitmap::new();
    assert!(index.facet_counts_ordinal(&empty, "color").is_empty());

    // An attribute never indexed as a single-valued Enum at all (e.g. a
    // typo, or brand -- which lives in its own dedicated bitmap index, not
    // `enum_columns`) must return empty, not panic.
    assert!(index.facet_counts_ordinal(&all, "brand").is_empty());
    assert!(index
        .facet_counts_ordinal(&all, "not_a_real_attribute")
        .is_empty());
}

#[test]
fn product_type_facet_counts_reflect_the_candidate_set_not_the_whole_catalog() {
    // Phase 6A (Issue #23): `product_type`, like `brand`, has its own
    // dedicated `product_type_bitmaps` index -- populated by every
    // catalog since Gate 3, but never exercised with real, non-sentinel
    // values until WANDS (ESCI always assigns `ProductTypeId(0)`; see
    // `round1_eval::catalog`). Same structure/expected counts as the
    // brand test above, just keyed on the sibling field.
    use commerce_core::domain::ProductTypeId;
    use commerce_core::fixtures::cold_start_catalog;

    let catalog = cold_start_catalog();
    let index = CatalogIndex::build(&catalog);

    let all = index.indexed_candidates(&[]);
    let facets = index.product_type_facet_counts(&all);
    // ProductType(1) (running shoes): nike (2 variants) + aerowalk (2) = 4.
    // ProductType(2) (hiking boots): nike (2) + aerowalk (1) = 3.
    assert_eq!(facets.get(&ProductTypeId(1)), Some(&4));
    assert_eq!(facets.get(&ProductTypeId(2)), Some(&3));
    assert!(index.facet_counts("product_type", &all).is_empty());

    let waterproof_only =
        index.indexed_candidates(&[ResolvedConstraint::Attribute(Constraint::Boolean {
            attribute: "waterproof".to_string(),
            value: true,
        })]);
    let facets = index.product_type_facet_counts(&waterproof_only);
    // Only hiking_boots_nike/aerowalk and running_shoes_aerowalk are
    // waterproof=true (running_shoes_nike is false).
    assert_eq!(facets.get(&ProductTypeId(1)), Some(&2));
    assert_eq!(facets.get(&ProductTypeId(2)), Some(&3));
}

#[test]
fn category_facet_counts_reflect_the_candidate_set_not_the_whole_catalog() {
    // Same dedicated-bitmap facet method for `category`. `cold_start_catalog`
    // assigns every fixture product the same `CategoryId(1)` (no real
    // category diversity exists in this hand-authored fixture), so this
    // only exercises the single-bucket case directly; real, diverse
    // category cardinality is validated against the real WANDS catalog in
    // `phase6a-eval`, not fabricated here.
    use commerce_core::domain::CategoryId;
    use commerce_core::fixtures::cold_start_catalog;

    let catalog = cold_start_catalog();
    let index = CatalogIndex::build(&catalog);

    let all = index.indexed_candidates(&[]);
    let facets = index.category_facet_counts(&all);
    assert_eq!(facets.get(&CategoryId(1)), Some(&7));
    assert!(index.facet_counts("category", &all).is_empty());
}

#[test]
fn category_and_product_type_facet_counts_by_scan_match_exactly() {
    use commerce_core::fixtures::cold_start_catalog;
    use roaring::RoaringBitmap;

    let catalog = cold_start_catalog();
    let index = CatalogIndex::build(&catalog);

    let all = index.indexed_candidates(&[]);
    assert_eq!(
        index.category_facet_counts(&all),
        index.category_facet_counts_by_scan(&all, &catalog)
    );
    assert_eq!(
        index.product_type_facet_counts(&all),
        index.product_type_facet_counts_by_scan(&all, &catalog)
    );

    let waterproof_only =
        index.indexed_candidates(&[ResolvedConstraint::Attribute(Constraint::Boolean {
            attribute: "waterproof".to_string(),
            value: true,
        })]);
    assert_eq!(
        index.product_type_facet_counts(&waterproof_only),
        index.product_type_facet_counts_by_scan(&waterproof_only, &catalog)
    );

    let empty = RoaringBitmap::new();
    assert!(index
        .category_facet_counts_by_scan(&empty, &catalog)
        .is_empty());
    assert!(index
        .product_type_facet_counts_by_scan(&empty, &catalog)
        .is_empty());
}

#[test]
fn brand_category_product_type_facet_counts_ordinal_match_scan_exactly() {
    // Issue #21 Phase 6D (P6D-E02): ordinal-based counterparts to the
    // brand/category/product_type `_by_scan` methods. Same correctness
    // discipline as `facet_counts_ordinal_matches_facet_counts_by_scan_exactly`
    // above -- exact match required before any timing claim is trusted.
    use commerce_core::domain::BrandId;
    use commerce_core::fixtures::cold_start_catalog;
    use commerce_core::ir::StructuralConstraint;
    use roaring::RoaringBitmap;

    let catalog = cold_start_catalog();
    let index = CatalogIndex::build(&catalog);

    let all = index.indexed_candidates(&[]);
    assert_eq!(
        index.brand_facet_counts_ordinal(&all),
        index.brand_facet_counts_by_scan(&all, &catalog)
    );
    assert_eq!(
        index.category_facet_counts_ordinal(&all),
        index.category_facet_counts_by_scan(&all, &catalog)
    );
    assert_eq!(
        index.product_type_facet_counts_ordinal(&all),
        index.product_type_facet_counts_by_scan(&all, &catalog)
    );
    assert_eq!(
        index.brand_facet_counts_ordinal(&all).get(&BrandId(1)),
        Some(&4)
    );

    let running_shoes_only = index.indexed_candidates(&[ResolvedConstraint::Structural(
        StructuralConstraint::ProductType(commerce_core::domain::ProductTypeId(1)),
    )]);
    assert_eq!(
        index.brand_facet_counts_ordinal(&running_shoes_only),
        index.brand_facet_counts_by_scan(&running_shoes_only, &catalog)
    );
    assert_eq!(
        index.category_facet_counts_ordinal(&running_shoes_only),
        index.category_facet_counts_by_scan(&running_shoes_only, &catalog)
    );

    let empty = RoaringBitmap::new();
    assert!(index.brand_facet_counts_ordinal(&empty).is_empty());
    assert!(index.category_facet_counts_ordinal(&empty).is_empty());
    assert!(index.product_type_facet_counts_ordinal(&empty).is_empty());
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

/// Issue #6 P1-D (`docs/experiments/PHASE2_LOG.md` P2-E13): `execute_ranked`
/// was changed to skip the per-candidate `effective_attributes` merge
/// entirely when `query.preferences` is empty (found via a real ~1078ms
/// full-catalog-rank cost on the real 1.2M-product catalog). The fast
/// path must produce byte-for-byte the same result the old always-merge
/// code did: every score is `0.0` regardless (no preference can ever
/// match with an empty preference list), so ordering must still fall
/// back to the deterministic `(product_id, variant_id)` tie-break.
#[test]
fn ranking_with_no_preferences_still_returns_every_candidate_score_zero_and_deterministically_ordered(
) {
    let catalog = combined_catalog();
    let index = CatalogIndex::build(&catalog);
    // "running shoes" resolves to a structural/attribute constraint with
    // no descriptive terms at all, so query.preferences is empty -- the
    // exact real-query shape (a fully-resolved structural query, no
    // residual, no Preference) that exposed the real cost.
    let query = compile("running shoes", &shoe_lexicon());
    assert!(
        query.preferences.is_empty(),
        "test premise: this query must have no preferences: {query:?}"
    );

    let ranked = index.execute_ranked(&query, &catalog, 10);
    assert_eq!(
        ranked.len(),
        3,
        "expected all three variants as candidates: {ranked:?}"
    );
    assert!(
        ranked.iter().all(|hit| hit.score == 0.0),
        "every score must be exactly 0.0 with no preferences to match: {ranked:?}"
    );
    let ids: Vec<(ProductId, VariantId)> = ranked
        .iter()
        .map(|hit| (hit.product, hit.variant))
        .collect();
    let mut expected = ids.clone();
    expected.sort();
    assert_eq!(
        ids, expected,
        "with every score tied at 0.0, order must be the deterministic (product_id, variant_id) tie-break: {ranked:?}"
    );
}

#[test]
fn lexical_token_candidates_are_exact_token_not_substring() {
    // Round 1 R1-E07: lexical_postings is attribute-agnostic, whole-word
    // token matching, deliberately different from Constraint::Text's
    // per-attribute substring semantics. "synthetic" is a real token in
    // representative_query_catalog's "material" attribute ("Synthetic
    // mesh"); "synth" is a substring of it but not a stored token, so it
    // must find nothing via the token index even though a substring-based
    // Text constraint would match "synthetic".
    let catalog = combined_catalog();
    let index = CatalogIndex::build(&catalog);

    let hits = index.lexical_and_candidates(&["synthetic".to_string()]);
    assert_eq!(
        hits.len(),
        1,
        "expected exactly one variant tagged synthetic"
    );

    let no_hits = index.lexical_and_candidates(&["synth".to_string()]);
    assert!(
        no_hits.is_empty(),
        "a token index must not substring-match \"synth\" against the stored token \"synthetic\""
    );

    // "synthetic" (representative_query_catalog's material, "synthetic
    // mesh") and "knit" (variant_safety_catalog's material, "mesh knit")
    // never co-occur in the same variant's indexed text: AND must be
    // empty, OR must be the (non-empty) union of both products' variants.
    let and_disjoint = index.lexical_and_candidates(&["synthetic".to_string(), "knit".to_string()]);
    let or_union = index.lexical_or_candidates(&["synthetic".to_string(), "knit".to_string()]);
    assert!(
        and_disjoint.is_empty(),
        "synthetic and knit never co-occur in one variant's text"
    );
    assert!(
        !or_union.is_empty() && or_union.len() > and_disjoint.len(),
        "OR must be a proper superset of AND when the tokens are disjoint"
    );
}

#[test]
fn approximate_size_grows_with_more_indexed_data() {
    let small = CatalogIndex::build(&variant_safety_catalog());
    let large = CatalogIndex::build(&combined_catalog());
    assert!(small.approximate_size_bytes() > 0);
    assert!(
        large.approximate_size_bytes() > small.approximate_size_bytes(),
        "indexing more products/variants should not shrink the reported size"
    );
}

#[test]
fn approximate_ordinal_facet_bytes_is_accounted_for_within_approximate_size_bytes() {
    // P6D-E03: approximate_size_bytes() silently omitted every Phase 6D
    // ordinal/dictionary facet structure until this fix. Guard that
    // regression directly: the ordinal-facet contribution must be nonzero
    // (these fixtures index brand/category/product_type on every product)
    // and must never exceed the whole-index total it is a component of.
    let small = CatalogIndex::build(&variant_safety_catalog());
    let large = CatalogIndex::build(&combined_catalog());

    assert!(
        small.approximate_ordinal_facet_bytes() > 0,
        "typed-ID dictionaries/columns should be nonzero once brand/category/product_type are indexed"
    );
    assert!(small.approximate_ordinal_facet_bytes() <= small.approximate_size_bytes());
    assert!(large.approximate_ordinal_facet_bytes() <= large.approximate_size_bytes());
    assert!(
        large.approximate_ordinal_facet_bytes() >= small.approximate_ordinal_facet_bytes(),
        "indexing more products/variants should not shrink the ordinal-facet byte count"
    );
}
