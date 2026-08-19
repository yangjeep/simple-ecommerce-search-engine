use commerce_core::cold_start::{
    compile_lexicon, coverage_holes, generate_shopper_queries, CatalogProfile,
};
use commerce_core::fixtures::{
    cold_start_brands, cold_start_catalog, cold_start_categories, cold_start_product_types,
    shoe_semantic_context, REPRESENTATIVE_QUERY_SET,
};
use commerce_core::ir::{compile, measure_coverage};

fn build_profile() -> CatalogProfile {
    CatalogProfile::build(
        &cold_start_catalog(),
        &cold_start_brands(),
        &cold_start_product_types(),
        &cold_start_categories(),
    )
}

#[test]
fn profile_compresses_ten_variants_into_a_small_distinct_vocabulary() {
    let profile = build_profile();
    // 2 brands + 2 product types + 1 category + 1 boolean attribute
    // (waterproof) + 8 distinct enum/multi-enum value strings (black, red,
    // brown, green, blue, cushioned, insulated, breathable) = 14, derived
    // from 4 products / 7 variants worth of attribute occurrences.
    assert_eq!(profile.distinct_value_count(), 14);
}

#[test]
fn generated_queries_are_deterministic_across_runs() {
    let profile = build_profile();
    let first = generate_shopper_queries(&profile);
    let second = generate_shopper_queries(&profile);
    assert_eq!(first, second);
    assert_eq!(first.len(), 30, "{first:?}"); // 15 templates x 2 product types
}

/// The core Gate 6 measurement: replay catalog-derived queries against the
/// catalog-derived lexicon and identify coverage holes. Exactly two holes
/// are expected — both from the deliberate "green" collision the fixture
/// was built to contain (see `fixtures::cold_start_catalog`'s doc
/// comment) — proving the profiler surfaces the collision as ambiguity
/// instead of silently guessing one meaning.
#[test]
fn coverage_holes_are_exactly_the_deliberate_green_collision() {
    let profile = build_profile();
    let lexicon = compile_lexicon(&profile);
    let generated = generate_shopper_queries(&profile);
    let generated_refs: Vec<&str> = generated.iter().map(String::as_str).collect();

    let holes = coverage_holes(&generated_refs, &lexicon);
    // Product types are visited in `BTreeMap` key order ("hiking boots"
    // sorts before "running shoes"), so the hiking-boots hole appears first.
    assert_eq!(
        holes,
        vec!["green hiking boots", "green running shoes"],
        "{holes:?}"
    );

    let report = measure_coverage(&generated_refs, &lexicon);
    assert_eq!(report.total_queries, 30);
    assert_eq!(report.had_ambiguity, 2);
    assert_eq!(report.had_residual, 0);
    assert_eq!(report.fully_resolved, 28);

    // Confirm it really is the collision, not a missing entry: "green"
    // must resolve to two candidates, one per attribute it was seen on.
    let compiled = compile("green running shoes", &lexicon);
    assert_eq!(compiled.ambiguous.len(), 1);
    assert_eq!(compiled.ambiguous[0].candidates.len(), 2);
}

/// Independent-evidence cross-check: does a lexicon compiled purely from
/// catalog data (zero hand curation, zero aliases) cover any of the
/// hand-authored Gate 4/5 query set it was never built from? This
/// measures the real number rather than asserting a hand-predicted one —
/// the two lexicons share some vocabulary (Nike, waterproof, "running
/// shoes", several colors) and diverge on others (no "sneakers"/
/// "trainers" aliases are catalog-derivable; the catalog-derived lexicon
/// additionally knows "blue", which the hand-curated one never did).
#[test]
fn catalog_derived_lexicon_partially_covers_the_hand_authored_query_set() {
    let profile = build_profile();
    let catalog_lexicon = compile_lexicon(&profile);
    let report = measure_coverage(REPRESENTATIVE_QUERY_SET, &catalog_lexicon);

    // Sanity bounds, not a hand-predicted exact figure: some overlap must
    // exist (shared brand/product-type/waterproof/color vocabulary) but
    // full coverage is impossible (aliases like "sneakers"/"trainers" are
    // not derivable from catalog data alone).
    assert!(report.fully_resolved > 0, "{report:?}");
    assert!(report.fully_resolved < report.total_queries, "{report:?}");

    // Spot check: "blue running shoes" was residual against the
    // hand-curated lexicon (E004) but the catalog-derived one does know
    // "blue" (the Aerowalk running shoe variant), so it must resolve here.
    let blue = compile("blue running shoes", &catalog_lexicon);
    assert!(
        blue.ambiguous.is_empty() && blue.residual_lexical.is_empty(),
        "{blue:?}"
    );

    // Spot check: alias-only vocabulary is not catalog-derivable, so
    // queries relying purely on it must still show up as residual.
    let sneakers_only = compile("sneakers", &catalog_lexicon);
    assert!(
        !sneakers_only.residual_lexical.is_empty(),
        "{sneakers_only:?}"
    );
}

/// The hand-curated (Gate 2/4) and catalog-derived (Gate 6) lexicons are
/// two independently-built views of overlapping vocabulary; this is not
/// asserting one is "better," only that both exist and can be compared on
/// the same query set with `measure_coverage`.
#[test]
fn hand_curated_and_catalog_derived_lexicons_are_independently_comparable() {
    let hand_curated = shoe_semantic_context();
    let profile = build_profile();
    let catalog_derived = compile_lexicon(&profile);

    let hand_report = measure_coverage(REPRESENTATIVE_QUERY_SET, hand_curated.lexicon());
    let catalog_report = measure_coverage(REPRESENTATIVE_QUERY_SET, &catalog_derived);

    assert_eq!(hand_report.fully_resolved, 12); // unchanged from E004
    assert_ne!(
        hand_report, catalog_report,
        "the two lexicons should not resolve identically"
    );
}
