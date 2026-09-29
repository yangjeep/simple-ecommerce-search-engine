//! Issue #63 (amendment 1, section 3 B1): every candidate representation
//! (`cand_mode` p0/p0r/p1/p2/p2b) must produce exactly the independent
//! oracle's `/plp` answer (num_found, facet maps, top-K docs) under every
//! non-legacy facet/sort mode and at planner constants on both sides of each
//! crossover. The catalog is multi-variant, with product- and variant-level
//! attributes, missing values, numeric ties, `-0.0`/`0.0` and a `MultiEnum`
//! attribute, so dense match-all paths cannot pass by coincidence.

use std::collections::HashMap;

use commerce_core::domain::{
    attributes, AttributeValue, BrandId, Catalog, CategoryId, Inventory, Price, Product, ProductId,
    ProductTypeId, Variant, VariantId,
};
use commerce_core::index::CatalogIndex;
use issue79_eval::oracle::Oracle;
use issue79_eval::plp::{
    execute, CandidateMode, FacetMode, PlpContext, PlpRequest, SortMode, SortStructures,
};

const COLORS: [&str; 6] = ["black", "white", "red", "green", "blue", "grey"];
const STYLES: [&str; 3] = ["modern", "rustic", "classic"];
const CATEGORIES: [&str; 3] = ["Chairs", "Tables", "Beds"];

fn catalog() -> Catalog {
    // A small deterministic LCG so the fixture is varied but reproducible.
    let mut state = 63u64;
    let mut next = move |modulo: u64| {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1);
        (state >> 33) % modulo
    };
    let mut products = Vec::new();
    let mut variant_id = 1000u64;
    for p in 0..60u64 {
        let style = (next(4) < 3).then(|| STYLES[next(3) as usize]);
        let mut product_attrs = Vec::new();
        if let Some(style) = style {
            product_attrs.push(("style", AttributeValue::Enum(style.to_owned())));
        }
        let variant_count = 1 + next(3);
        let mut variants = Vec::new();
        for _ in 0..variant_count {
            let mut attrs = Vec::new();
            if next(5) != 0 {
                attrs.push((
                    "color",
                    AttributeValue::Enum(COLORS[next(6) as usize].to_owned()),
                ));
            }
            match next(6) {
                0 => {}
                1 => attrs.push(("rating", AttributeValue::Numeric(-0.0))),
                2 => attrs.push(("rating", AttributeValue::Numeric(0.0))),
                _ => attrs.push(("rating", AttributeValue::Numeric((next(5) as f64) / 2.0))),
            }
            if next(3) == 0 {
                attrs.push((
                    "tags",
                    AttributeValue::MultiEnum(vec!["sale".to_owned(), format!("t{}", next(3))]),
                ));
            }
            variants.push(Variant {
                id: VariantId(variant_id),
                attributes: attributes(attrs),
                price: Price::usd(100),
                inventory: Inventory::in_stock(1),
            });
            variant_id += 1;
        }
        products.push(Product {
            id: ProductId(p),
            product_type: ProductTypeId(1),
            brand: BrandId(1),
            category: CategoryId(next(3) as u32),
            title: format!("product {p}"),
            attributes: attributes(product_attrs),
            variants,
        });
    }
    Catalog { products }
}

fn request(
    category: Option<&str>,
    filters: &[(&str, &str)],
    ranges: &[(&str, &str, f64)],
    facets: &[&str],
    sort: Option<(&str, bool)>,
    offset: usize,
) -> PlpRequest {
    PlpRequest {
        category: category.map(str::to_owned),
        filters: filters
            .iter()
            .map(|(a, v)| ((*a).to_owned(), (*v).to_owned()))
            .collect(),
        ranges: ranges
            .iter()
            .map(|(a, o, v)| ((*a).to_owned(), (*o).to_owned(), *v))
            .collect(),
        facets: facets.iter().map(|f| (*f).to_owned()).collect(),
        sort: sort.map(|(f, d)| (f.to_owned(), d)),
        top_k: 7,
        offset,
        facet_mode: FacetMode::Legacy,
        sort_mode: SortMode::Legacy,
        cand_mode: CandidateMode::P0,
    }
}

#[test]
fn every_candidate_mode_matches_the_oracle() {
    let catalog = catalog();
    let index = CatalogIndex::build(&catalog);
    let mut structures = SortStructures::build(&index, &["rating"], &["rating"]);
    structures.all_ordinals = Some(index.all_ordinals_bitmap());
    let category_id_by_leaf: HashMap<String, CategoryId> = CATEGORIES
        .iter()
        .enumerate()
        .map(|(i, name)| ((*name).to_owned(), CategoryId(i as u32)))
        .collect();
    let source_id_by_product: HashMap<ProductId, String> = catalog
        .products
        .iter()
        .map(|p| (p.id, format!("src-{}", p.id.0)))
        .collect();
    let oracle = Oracle::new(&catalog);
    let facets = ["color", "style", "tags", "absent"];
    let requests = vec![
        request(None, &[], &[], &facets, None, 0),
        request(None, &[], &[], &facets, None, 5),
        request(None, &[("color", "black")], &[], &facets, None, 0),
        request(
            None,
            &[("color", "white"), ("style", "modern")],
            &[],
            &facets,
            None,
            2,
        ),
        request(Some("Tables"), &[], &[], &["color"], None, 0),
        request(None, &[], &[], &["style"], Some(("rating", true)), 0),
        request(None, &[], &[], &["color"], Some(("rating", false)), 3),
        request(
            None,
            &[("color", "red")],
            &[],
            &["color"],
            Some(("rating", true)),
            0,
        ),
        request(
            None,
            &[],
            &[("rating", "gte", 1.0)],
            &["color"],
            Some(("rating", true)),
            1,
        ),
        request(
            None,
            &[("color", "no-such-color")],
            &[],
            &["color"],
            None,
            0,
        ),
    ];
    let mut modes: Vec<(FacetMode, SortMode, Option<f64>, Option<f64>)> = Vec::new();
    for facet in [FacetMode::Ordinal, FacetMode::Bitmap] {
        for sort in [SortMode::Topk, SortMode::Presorted] {
            modes.push((facet, sort, None, None));
        }
    }
    for tau in [0.0, 919.497_239_065_769_5, 1e12] {
        for rho in [0.0, 0.083_377_913_197_190_3, 1e12] {
            modes.push((FacetMode::Hybrid, SortMode::Hybrid, Some(tau), Some(rho)));
        }
    }
    let mut checked = 0;
    for base in &requests {
        let expected = oracle
            .expected(base, &category_id_by_leaf, &source_id_by_product)
            .expect("oracle");
        for cand_mode in [
            CandidateMode::P0,
            CandidateMode::P0r,
            CandidateMode::P1,
            CandidateMode::P2,
            CandidateMode::P2b,
        ] {
            for &(facet_mode, sort_mode, tau_f, rho_s) in &modes {
                let ctx = PlpContext {
                    catalog: &catalog,
                    index: &index,
                    category_id_by_leaf: &category_id_by_leaf,
                    source_id_by_product: &source_id_by_product,
                    structures: &structures,
                    tau_f,
                    rho_s,
                };
                let req = PlpRequest {
                    facet_mode,
                    sort_mode,
                    cand_mode,
                    ..base.clone()
                };
                let label = format!(
                    "{base:?} {cand_mode:?} {facet_mode:?}/{sort_mode:?} {tau_f:?} {rho_s:?}"
                );
                let response = execute(&ctx, &req).unwrap_or_else(|e| panic!("{label}: {e}"));
                assert_eq!(response.num_found, expected.num_found, "{label}");
                assert_eq!(response.facets, expected.facets, "{label}");
                // `PlpDoc` equality compares sort values numerically, so -0.0
                // and 0.0 are the same value, as the preregistered order says.
                assert_eq!(response.docs, expected.docs, "{label}");
                checked += 1;
            }
        }
    }
    assert_eq!(checked, requests.len() * 5 * 13);
}

#[test]
fn match_all_modes_report_their_dense_paths() {
    let catalog = catalog();
    let index = CatalogIndex::build(&catalog);
    let mut structures = SortStructures::build(&index, &["rating"], &["rating"]);
    structures.all_ordinals = Some(index.all_ordinals_bitmap());
    let empty_leaf = HashMap::new();
    let ids = HashMap::new();
    let ctx = PlpContext {
        catalog: &catalog,
        index: &index,
        category_id_by_leaf: &empty_leaf,
        source_id_by_product: &ids,
        structures: &structures,
        tau_f: Some(1e12),
        rho_s: Some(0.0),
    };
    let base = PlpRequest {
        facet_mode: FacetMode::Hybrid,
        sort_mode: SortMode::Hybrid,
        ..request(None, &[], &[], &["color", "tags"], None, 0)
    };
    let paths = |cand_mode| {
        let response = execute(
            &ctx,
            &PlpRequest {
                cand_mode,
                ..base.clone()
            },
        )
        .expect("execute");
        assert!(
            response.diag.base_match_all
                == matches!(cand_mode, CandidateMode::P2 | CandidateMode::P2b)
        );
        (
            response
                .diag
                .facet_diag
                .iter()
                .map(|f| f.path.clone())
                .collect::<Vec<_>>(),
            response.diag.sort_path,
        )
    };
    // tau = 1e12 sends a single-valued facet to the ordinal path; tags is
    // MultiEnum, so it is always bitmap.
    assert_eq!(paths(CandidateMode::P0).0, ["ordinal", "bitmap"]);
    assert_eq!(paths(CandidateMode::P2).0, ["ordinal_all", "bitmap_all"]);
    assert_eq!(paths(CandidateMode::P2b).0, ["bitmap_all", "bitmap_all"]);
    assert_eq!(paths(CandidateMode::P2).1, "bounded_unsorted_all");
    // Legacy modes are #77's code and exist only over P0.
    let legacy = PlpRequest {
        cand_mode: CandidateMode::P2,
        ..request(None, &[], &[], &["color"], None, 0)
    };
    assert!(execute(&ctx, &legacy).is_err());
    // P1 without the prebuilt bitmap is an error, never a silent fallback.
    let bare = SortStructures::default();
    let ctx_bare = PlpContext {
        structures: &bare,
        ..ctx
    };
    let p1 = PlpRequest {
        cand_mode: CandidateMode::P1,
        ..base.clone()
    };
    assert!(execute(&ctx_bare, &p1).is_err());
}
