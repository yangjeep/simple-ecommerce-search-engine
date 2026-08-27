use commerce_core::cold_start::{
    compile_lexicon, compile_lexicon_with_promoted_hyponyms, coverage_holes,
    generate_shopper_queries, CatalogProfile,
};
use commerce_core::control_plane::{HyponymRelation, PromotedHyponyms, RuleProvenance};
use commerce_core::domain::{
    attributes, Brand, BrandId, Catalog, Category, CategoryId, Inventory, Price, Product,
    ProductId, ProductType, ProductTypeId, Variant, VariantId,
};
use commerce_core::fixtures::{
    cold_start_brands, cold_start_catalog, cold_start_categories, cold_start_product_types,
    shoe_semantic_context, REPRESENTATIVE_QUERY_SET,
};
use commerce_core::ir::{compile, measure_coverage, ResolvedConstraint, StructuralConstraint};

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
    let lexicon = compile_lexicon(&profile, 1);
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

/// Round 1 R1-E02/E02b canonicalization fix
/// (`docs/experiments/ROUND1_LOG.md`): a `min_enum_frequency` threshold
/// above 1 must exclude one-off enum values from the lexicon entirely
/// (they become safely `residual`, not a wrong hard filter) while keeping
/// values seen more than once. In `cold_start_catalog`, "red" and "blue"
/// are each seen exactly once (one variant each); "black" (two variants,
/// one per brand) and the deliberately planted "green" collision (color
/// on one product, a feature tag on another) are each seen twice.
#[test]
fn min_enum_frequency_excludes_one_off_values_but_keeps_repeated_ones() {
    let profile = build_profile();
    assert_eq!(profile.enum_occurrence_count("red"), 1);
    assert_eq!(profile.enum_occurrence_count("blue"), 1);
    assert_eq!(profile.enum_occurrence_count("black"), 2);
    assert_eq!(profile.enum_occurrence_count("green"), 2);

    let unfiltered = compile_lexicon(&profile, 1);
    let red_unfiltered = compile("red running shoes", &unfiltered);
    assert!(
        red_unfiltered.residual_lexical.is_empty(),
        "threshold 1 must not filter anything: {red_unfiltered:?}"
    );

    let filtered = compile_lexicon(&profile, 2);
    let red_filtered = compile("red running shoes", &filtered);
    assert!(
        red_filtered.residual_lexical.contains(&"red".to_string()),
        "\"red\" is seen once in the fixture; threshold 2 must exclude it as residual, not silently keep it: {red_filtered:?}"
    );

    // "black" is seen twice (once per brand) and must survive threshold 2,
    // still resolving to a real constraint, not filtered away.
    let black_filtered = compile("black running shoes", &filtered);
    assert!(
        black_filtered.residual_lexical.is_empty() && black_filtered.ambiguous.is_empty(),
        "\"black\" is seen twice; threshold 2 must keep it resolvable: {black_filtered:?}"
    );

    // "green" is seen twice (the deliberate collision) and must survive
    // threshold 2 -- still ambiguous (two sources), not filtered away.
    let green_filtered = compile("green running shoes", &filtered);
    assert_eq!(
        green_filtered.ambiguous.len(),
        1,
        "\"green\" is seen twice; threshold 2 must keep it (still ambiguous, not residual): {green_filtered:?}"
    );

    // Brand/product-type vocabulary is never subject to this threshold,
    // even though "nike" and "running shoes" are also low-frequency in
    // this tiny fixture -- they come from the curated Brand/ProductType
    // registry, not raw per-product enum values.
    let brand_and_type = compile("nike running shoes", &filtered);
    assert!(
        brand_and_type.residual_lexical.is_empty() && brand_and_type.ambiguous.is_empty(),
        "brand/product-type resolution must be unaffected by min_enum_frequency: {brand_and_type:?}"
    );
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
    let catalog_lexicon = compile_lexicon(&profile, 1);
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
    let catalog_derived = compile_lexicon(&profile, 1);

    let hand_report = measure_coverage(REPRESENTATIVE_QUERY_SET, hand_curated.lexicon());
    let catalog_report = measure_coverage(REPRESENTATIVE_QUERY_SET, &catalog_derived);

    // Was 12 (E004). Now 10, a real, expected drop, not a regression:
    // Issue #6 P1-B (`docs/experiments/PHASE2_LOG.md` P2-E11) fixed
    // `apply_candidates` so a phrase resolving to *only* a soft
    // `Preference` stays in `residual_lexical` too (a Preference must
    // never make a lexical delegate blind to the phrase that produced
    // it). `measure_coverage`'s own definition of "fully resolved" (no
    // ambiguity AND no residual) is unchanged and correct -- the two
    // representative queries that resolve purely to preferences
    // ("cushioned breathable running shoes" and one other) now correctly
    // count as carrying residual text, since they legitimately do.
    assert_eq!(hand_report.fully_resolved, 10);
    assert_ne!(
        hand_report, catalog_report,
        "the two lexicons should not resolve identically"
    );
}

fn product_type_pair_catalog(name_a: &str, name_b: &str) -> (Catalog, Vec<ProductType>) {
    let product_a = Product {
        id: ProductId(1),
        product_type: ProductTypeId(1),
        brand: BrandId(1),
        category: CategoryId(1),
        title: format!("Sample {name_a}"),
        attributes: attributes([]),
        variants: vec![Variant {
            id: VariantId(1),
            attributes: attributes([]),
            price: Price::usd(4_999),
            inventory: Inventory::in_stock(1),
        }],
    };
    let product_b = Product {
        id: ProductId(2),
        product_type: ProductTypeId(2),
        brand: BrandId(1),
        category: CategoryId(1),
        title: format!("Sample {name_b}"),
        attributes: attributes([]),
        variants: vec![Variant {
            id: VariantId(2),
            attributes: attributes([]),
            price: Price::usd(8_999),
            inventory: Inventory::in_stock(1),
        }],
    };
    let types = vec![
        ProductType {
            id: ProductTypeId(1),
            name: name_a.to_string(),
        },
        ProductType {
            id: ProductTypeId(2),
            name: name_b.to_string(),
        },
    ];
    (
        Catalog {
            products: vec![product_a, product_b],
        },
        types,
    )
}

fn compile_lexicon_for_types(name_a: &str, name_b: &str) -> commerce_core::ir::SemanticLexicon {
    let (catalog, product_types) = product_type_pair_catalog(name_a, name_b);
    let brands = vec![Brand {
        id: BrandId(1),
        name: "Aerowalk".to_string(),
    }];
    let categories = vec![Category {
        id: CategoryId(1),
        name: "Footwear".to_string(),
    }];
    let profile = CatalogProfile::build(&catalog, &brands, &product_types, &categories);
    compile_lexicon(&profile, 1)
}

/// Supersedes the checkpoint-14 assertion here
/// (`docs/decisions/ISSUE55_HYPONYM_LEAF_ONLY_DECISION.md`), itself
/// superseded by Issue #55 A1
/// (`docs/decisions/ISSUE55_HYPONYM_PROMOTION_GATE_DECISION.md`): the
/// leaf-only-restricted mechanism can still *generate* the "boots" ->
/// "hiking boots" candidate correctly (neither name is a breadcrumb
/// path, so leaf-only restriction changes nothing for this pair --
/// hiking boots genuinely are a kind of boots), but a syntactically-valid
/// candidate must no longer install as a live route by default. This is
/// the direct end-to-end regression guard for A1's fix: plain
/// `compile_lexicon` (no recorded promotion) must resolve "boots" to its
/// own type only, and only an explicit PROMOTE verdict may activate the
/// `ProductTypeAny` expansion.
#[test]
fn clean_whole_word_subset_product_types_require_explicit_promotion_to_merge() {
    let (catalog, product_types) = product_type_pair_catalog("Boots", "Hiking Boots");
    let brands = vec![Brand {
        id: BrandId(1),
        name: "Aerowalk".to_string(),
    }];
    let categories = vec![Category {
        id: CategoryId(1),
        name: "Footwear".to_string(),
    }];
    let profile = CatalogProfile::build(&catalog, &brands, &product_types, &categories);

    // Production default: no promoted relations, so the syntactic
    // candidate must never install as a live route.
    let default_lexicon = compile_lexicon(&profile, 1);
    let compiled = compile("boots", &default_lexicon);
    assert_eq!(compiled.constraints.len(), 1, "{compiled:?}");
    assert_eq!(
        compiled.constraints[0],
        ResolvedConstraint::Structural(StructuralConstraint::ProductType(ProductTypeId(1))),
        "an unpromoted hyponym candidate must never install as a live route by default: \
         {compiled:?}"
    );

    // Promoting the relation is what activates the expansion.
    let promoted = PromotedHyponyms::compile(
        1,
        [
            HyponymRelation::candidate("boots", "hiking boots", RuleProvenance::Catalog, 1.0)
                .promote(),
        ],
    );
    let treatment_lexicon = compile_lexicon_with_promoted_hyponyms(&profile, 1, &promoted);
    let compiled_treatment = compile("boots", &treatment_lexicon);
    assert_eq!(compiled_treatment.constraints.len(), 1, "{compiled_treatment:?}");
    assert_eq!(
        compiled_treatment.constraints[0],
        ResolvedConstraint::Structural(StructuralConstraint::ProductTypeAny(vec![
            ProductTypeId(1),
            ProductTypeId(2)
        ])),
        "\"boots\" must admit the genuine, PROMOTED hyponym \"hiking boots\" too: \
         {compiled_treatment:?}"
    );

    // The more specific term has no hyponyms of its own here, so it
    // still resolves to exactly its own type.
    let compiled_hiking = compile("hiking boots", &treatment_lexicon);
    assert_eq!(compiled_hiking.constraints.len(), 1, "{compiled_hiking:?}");
    assert_eq!(
        compiled_hiking.constraints[0],
        ResolvedConstraint::Structural(StructuralConstraint::ProductType(ProductTypeId(2))),
        "{compiled_hiking:?}"
    );
}

/// Regression guard for the actual defect checkpoint 11's audit found
/// (`docs/decisions/ISSUE55_PRODUCT_TYPE_HYPONYM_DECISION.md`): a clean
/// broader term must never merge with a product type whose name is a
/// breadcrumb path, when the broader term's word only appears in that
/// path's *ancestor* segment, not its leaf -- the real "candles"
/// admitting "...candles & holders / scented oils & diffusers" false
/// positive, reproduced end-to-end through `compile_lexicon`/`compile`
/// (complementing `cold_start::profile::hyponym_tests`'s own
/// pure-function-level coverage of the same shape).
#[test]
fn ancestor_only_word_match_on_a_path_derived_product_type_never_merges() {
    let lexicon = compile_lexicon_for_types(
        "Candles",
        "Décor & Pillows / Candles & Holders / Scented Oils & Diffusers",
    );

    let compiled = compile("candles", &lexicon);
    assert_eq!(compiled.constraints.len(), 1, "{compiled:?}");
    assert_eq!(
        compiled.constraints[0],
        ResolvedConstraint::Structural(StructuralConstraint::ProductType(ProductTypeId(1))),
        "\"candles\" must resolve to its own exact product type only -- it must not admit a \
         product whose leaf is \"scented oils & diffusers\" just because \"candles\" appears in \
         that product's ancestor breadcrumb: {compiled:?}"
    );
}
