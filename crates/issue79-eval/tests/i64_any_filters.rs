//! Issue #64 (amendment 1, section 3): multi-select (`any_filters`, OR
//! within an attribute) and scope-filtered disjunctive requests must match
//! the independent oracle exactly under every candidate/facet/sort mode,
//! including self-exclusion of a multi-selected facet.

use std::collections::HashMap;

use commerce_core::domain::{
    attributes, AttributeValue, BrandId, Catalog, CategoryId, Inventory, Price, Product, ProductId,
    ProductTypeId, Variant, VariantId,
};
use commerce_core::index::CatalogIndex;
use issue79_eval::oracle::Oracle;
use issue79_eval::plp::{
    execute, parse_plp_request, CandidateMode, FacetMode, PlpContext, PlpRequest, SortMode,
    SortStructures,
};

const COLORS: [&str; 6] = ["black", "white", "red", "green", "blue", "grey"];
const STYLES: [&str; 3] = ["modern", "rustic", "classic"];

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
    filters: &[(&str, &str)],
    any_filters: &[(&str, &[&str])],
    facets: &[&str],
    sort: Option<(&str, bool)>,
) -> PlpRequest {
    PlpRequest {
        category: None,
        filters: filters
            .iter()
            .map(|(a, v)| ((*a).to_owned(), (*v).to_owned()))
            .collect(),
        any_filters: any_filters
            .iter()
            .map(|(a, vs)| {
                (
                    (*a).to_owned(),
                    vs.iter().map(|v| (*v).to_owned()).collect(),
                )
            })
            .collect(),
        ranges: Vec::new(),
        facets: facets.iter().map(|f| (*f).to_owned()).collect(),
        sort: sort.map(|(f, d)| (f.to_owned(), d)),
        top_k: 7,
        offset: 0,
        facet_mode: FacetMode::Legacy,
        sort_mode: SortMode::Legacy,
        cand_mode: CandidateMode::P0,
    }
}

#[test]
fn any_filters_match_the_oracle_in_every_mode() {
    let catalog = catalog();
    let index = CatalogIndex::build(&catalog);
    let mut structures = SortStructures::build(&index, &["rating"], &["rating"]);
    structures.all_ordinals = Some(index.all_ordinals_bitmap());
    let category_id_by_leaf: HashMap<String, CategoryId> = HashMap::new();
    let source_ids: HashMap<ProductId, String> = catalog
        .products
        .iter()
        .map(|p| (p.id, format!("src-{}", p.id.0)))
        .collect();
    let oracle = Oracle::new(&catalog);
    let facets = ["color", "style", "tags"];
    let requests = vec![
        // Multi-select color, faceted: color self-excludes, others see it.
        request(&[], &[("color", &["black", "white"])], &facets, None),
        // Scope-like filter (never faceted) + multi-select style.
        request(
            &[("style", "modern")],
            &[("color", &["red", "green", "blue"])],
            &facets,
            None,
        ),
        request(
            &[("style", "rustic")],
            &[("color", &["black", "white"])],
            &["color", "tags"],
            None,
        ),
        // Multi-select on an attribute that is not faceted.
        request(
            &[],
            &[("style", &["modern", "classic"])],
            &["color", "tags"],
            Some(("rating", true)),
        ),
        // Multi-select on a MultiEnum attribute, faceted.
        request(&[], &[("tags", &["t0", "t2"])], &facets, None),
        // A value that does not exist, alone and with a real one.
        request(&[], &[("color", &["no-such"])], &facets, None),
        request(&[], &[("color", &["no-such", "grey"])], &facets, None),
        // Two multi-selects.
        request(
            &[],
            &[
                ("color", &["black", "white"]),
                ("style", &["modern", "rustic"]),
            ],
            &facets,
            None,
        ),
    ];
    let mut checked = 0;
    for base in &requests {
        let expected = oracle
            .expected(base, &category_id_by_leaf, &source_ids)
            .expect("oracle");
        let mut modes: Vec<(FacetMode, SortMode, CandidateMode)> =
            vec![(FacetMode::Legacy, SortMode::Legacy, CandidateMode::P0)];
        for cand in [
            CandidateMode::P0,
            CandidateMode::P0r,
            CandidateMode::P1,
            CandidateMode::P2,
            CandidateMode::P2b,
        ] {
            for facet in [FacetMode::Ordinal, FacetMode::Bitmap, FacetMode::Hybrid] {
                for sort in [SortMode::Topk, SortMode::Presorted, SortMode::Hybrid] {
                    modes.push((facet, sort, cand));
                }
            }
        }
        for (facet_mode, sort_mode, cand_mode) in modes {
            let ctx = PlpContext {
                catalog: &catalog,
                index: &index,
                category_id_by_leaf: &category_id_by_leaf,
                source_id_by_product: &source_ids,
                structures: &structures,
                tau_f: Some(919.497_239_065_769_5),
                rho_s: Some(0.083_377_913_197_190_3),
            };
            let req = PlpRequest {
                facet_mode,
                sort_mode,
                cand_mode,
                ..base.clone()
            };
            let label = format!("{base:?} {facet_mode:?}/{sort_mode:?}/{cand_mode:?}");
            let response = execute(&ctx, &req).unwrap_or_else(|e| panic!("{label}: {e}"));
            assert_eq!(response.num_found, expected.num_found, "{label}");
            assert_eq!(response.facets, expected.facets, "{label}");
            // Legacy sort's asc comparator is #77's (missing values first);
            // only compare docs for non-legacy sort or descending/unsorted.
            if sort_mode != SortMode::Legacy || base.sort.as_ref().is_none_or(|(_, d)| *d) {
                assert_eq!(response.docs, expected.docs, "{label}");
            }
            checked += 1;
        }
    }
    assert_eq!(checked, requests.len() * 46);
}

#[test]
fn anyfilter_query_parameter_round_trips() {
    let req = parse_plp_request("/plp?filter=category_depth_1:D%C3%A9cor&anyfilter=style:modern%20%26%20contemporary|traditional&facets=style,color&topk=48")
        .expect("parse");
    assert_eq!(
        req.filters,
        [("category_depth_1".to_owned(), "Décor".to_owned())]
    );
    assert_eq!(
        req.any_filters,
        [(
            "style".to_owned(),
            vec!["modern & contemporary".to_owned(), "traditional".to_owned()]
        )]
    );
    assert!(parse_plp_request("/plp?anyfilter=style").is_err());
    assert!(parse_plp_request("/plp?anyfilter=style:").is_err());
}
