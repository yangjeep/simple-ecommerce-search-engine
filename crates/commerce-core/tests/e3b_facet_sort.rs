//! Issue #79 (E3b): correctness of the new facet (bitmap-count) and sort
//! (candidate top-K, precomputed order) execution paths against brute-force
//! references, on a multi-variant catalog with missing values, ties and a
//! `MultiEnum` attribute.

use commerce_core::domain::{
    attributes, effective_attributes, AttributeValue, BrandId, Catalog, CategoryId, Inventory,
    Price, Product, ProductId, ProductTypeId, Variant, VariantId,
};
use commerce_core::index::sort::{
    choose_facet_path, choose_sort_path, top_k_presorted, top_k_scan, Direction, FacetPath,
    NumericSortColumn, PresenceBitmap, SortPath,
};
use commerce_core::index::CatalogIndex;
use roaring::RoaringBitmap;

fn variant(id: u64, attrs: Vec<(&'static str, AttributeValue)>) -> Variant {
    Variant {
        id: VariantId(id),
        attributes: attributes(attrs),
        price: Price::usd(0),
        inventory: Inventory::in_stock(1),
    }
}

/// 4 products / 9 variants. Product-level `material` (shared by every
/// variant of a product) and variant-level `color`/`rating`, with gaps:
/// some variants have no color, some no rating, and ratings tie.
fn catalog() -> Catalog {
    let product = |id: u64, material: Option<&'static str>, variants: Vec<Variant>| Product {
        id: ProductId(id),
        product_type: ProductTypeId(1),
        brand: BrandId(1),
        category: CategoryId(1),
        title: format!("product {id}"),
        attributes: attributes(
            material
                .map(|m| vec![("material", AttributeValue::Enum(m.to_owned()))])
                .unwrap_or_default(),
        ),
        variants,
    };
    let e = |v: &str| AttributeValue::Enum(v.to_owned());
    let n = AttributeValue::Numeric;
    Catalog {
        products: vec![
            product(
                1,
                Some("oak"),
                vec![
                    variant(10, vec![("color", e("black")), ("rating", n(4.5))]),
                    variant(11, vec![("color", e("red")), ("rating", n(3.0))]),
                    variant(12, vec![("rating", n(4.5))]),
                ],
            ),
            product(
                2,
                None,
                vec![
                    variant(20, vec![("color", e("black"))]),
                    variant(21, vec![("color", e("black")), ("rating", n(5.0))]),
                ],
            ),
            product(
                3,
                Some("pine"),
                vec![variant(30, vec![("color", e("white")), ("rating", n(4.5))])],
            ),
            product(
                4,
                Some("oak"),
                vec![
                    variant(40, vec![("color", e("red")), ("rating", n(0.0))]),
                    variant(41, vec![("rating", n(-0.0))]),
                    variant(42, vec![("color", e("white"))]),
                ],
            ),
        ],
    }
}

fn all(index: &CatalogIndex) -> RoaringBitmap {
    (0..index.ordinal_count() as u32).collect()
}

/// Brute-force reference: per-variant values in ordinal order, sorted by
/// (present first, value asc/desc, ordinal asc).
fn reference_sort(
    catalog: &Catalog,
    candidates: &RoaringBitmap,
    field: &str,
    direction: Direction,
) -> Vec<(u32, Option<f64>)> {
    let mut rows = Vec::new();
    let mut ordinal = 0u32;
    for product in &catalog.products {
        for v in &product.variants {
            if candidates.contains(ordinal) {
                let value = match effective_attributes(product, v).get(field) {
                    Some(AttributeValue::Numeric(x)) => Some(*x),
                    _ => None,
                };
                rows.push((ordinal, value));
            }
            ordinal += 1;
        }
    }
    rows.sort_by(|a, b| {
        let by_value = match (a.1, b.1) {
            (Some(x), Some(y)) => {
                let o = x.partial_cmp(&y).unwrap();
                if direction == Direction::Descending {
                    o.reverse()
                } else {
                    o
                }
            }
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal,
        };
        by_value.then(a.0.cmp(&b.0))
    });
    rows
}

#[test]
fn bitmap_facet_counts_match_legacy_and_ordinal_for_every_candidate_subset() {
    let catalog = catalog();
    let index = CatalogIndex::build(&catalog);
    let n = index.ordinal_count() as u32;
    // Every subset of the 9 ordinals: exhaustive, including empty and full.
    for mask in 0u32..(1 << n) {
        let candidates: RoaringBitmap = (0..n).filter(|i| mask & (1 << i) != 0).collect();
        for attr in ["color", "material", "absent"] {
            let legacy = index.facet_counts(attr, &candidates);
            assert_eq!(
                index.facet_counts_bitmap(&candidates, attr),
                legacy,
                "{attr} {mask:b}"
            );
            assert_eq!(
                index.facet_counts_ordinal(&candidates, attr),
                legacy,
                "{attr} {mask:b}"
            );
        }
    }
}

#[test]
fn facet_counts_are_per_variant_and_skip_missing_values() {
    let catalog = catalog();
    let index = CatalogIndex::build(&catalog);
    let counts = index.facet_counts_bitmap(&all(&index), "color");
    // black: 10, 20, 21; red: 11, 40; white: 30, 42; variants 12/41 have none.
    assert_eq!(counts.get("black"), Some(&3));
    assert_eq!(counts.get("red"), Some(&2));
    assert_eq!(counts.get("white"), Some(&2));
    assert_eq!(counts.values().sum::<u64>(), 7);
    // Product-level attribute counts every variant of the product.
    let materials = index.facet_counts_bitmap(&all(&index), "material");
    assert_eq!(materials.get("oak"), Some(&6));
    assert_eq!(materials.get("pine"), Some(&1));
    assert_eq!(index.enum_cardinality("color"), 3);
}

#[test]
fn multi_enum_attributes_are_refused_by_the_ordinal_guard() {
    let mut catalog = catalog();
    catalog.products[0].variants[0].attributes.insert(
        "tags".to_owned(),
        AttributeValue::MultiEnum(vec!["a".into(), "b".into()]),
    );
    catalog.products[1].variants[0]
        .attributes
        .insert("tags".to_owned(), AttributeValue::Enum("a".into()));
    let index = CatalogIndex::build(&catalog);
    assert!(index.attribute_is_single_valued_enum("color"));
    assert!(!index.attribute_is_single_valued_enum("tags"));
    let everything = all(&index);
    // Bitmap counting keeps legacy semantics (counts each MultiEnum element)...
    let legacy = index.facet_counts("tags", &everything);
    assert_eq!(index.facet_counts_bitmap(&everything, "tags"), legacy);
    assert_eq!(legacy.get("a"), Some(&2));
    // ...while the ordinal column under-counts it, which is why the planner
    // must never route such an attribute there.
    assert_ne!(index.facet_counts_ordinal(&everything, "tags"), legacy);
    assert_eq!(
        choose_facet_path(1, 1_000_000, 1e12, false),
        FacetPath::BitmapCount
    );
}

#[test]
fn both_sort_strategies_match_the_reference_for_every_subset_direction_and_limit() {
    let catalog = catalog();
    let index = CatalogIndex::build(&catalog);
    let column = NumericSortColumn::build(&index, "rating").unwrap();
    let presence = PresenceBitmap::build(&index, "rating").unwrap();
    let sorted = index.numeric_sorted("rating").unwrap();
    let n = index.ordinal_count() as u32;
    for mask in 0u32..(1 << n) {
        let candidates: RoaringBitmap = (0..n).filter(|i| mask & (1 << i) != 0).collect();
        for direction in [Direction::Ascending, Direction::Descending] {
            let reference = reference_sort(&catalog, &candidates, "rating", direction);
            for limit in [0usize, 1, 2, 3, 5, 9, 20] {
                let want: Vec<(u32, Option<f64>)> = reference.iter().copied().take(limit).collect();
                let scan = top_k_scan(&candidates, &column, direction, limit);
                let got: Vec<(u32, Option<f64>)> =
                    scan.hits.iter().map(|h| (h.ordinal, h.value)).collect();
                assert_eq!(got, want, "scan {direction:?} limit {limit} mask {mask:b}");
                let pre = top_k_presorted(sorted, &presence, &candidates, direction, limit);
                let got: Vec<(u32, Option<f64>)> =
                    pre.hits.iter().map(|h| (h.ordinal, h.value)).collect();
                assert_eq!(
                    got, want,
                    "presorted {direction:?} limit {limit} mask {mask:b}"
                );
            }
        }
    }
}

#[test]
fn sort_orders_missing_last_ties_by_ordinal_and_zero_signs_together() {
    let catalog = catalog();
    let index = CatalogIndex::build(&catalog);
    let column = NumericSortColumn::build(&index, "rating").unwrap();
    let everything = all(&index);
    let desc: Vec<u32> = top_k_scan(&everything, &column, Direction::Descending, 9)
        .hits
        .iter()
        .map(|h| h.ordinal)
        .collect();
    // 5.0 (4); 4.5 ties (0, 2, 5) by ordinal; 3.0 (1); 0.0 and -0.0 (6, 7)
    // tie by ordinal; then missing (3, 8) in ordinal order.
    assert_eq!(desc, vec![4, 0, 2, 5, 1, 6, 7, 3, 8]);
    let asc: Vec<u32> = top_k_scan(&everything, &column, Direction::Ascending, 9)
        .hits
        .iter()
        .map(|h| h.ordinal)
        .collect();
    assert_eq!(asc, vec![6, 7, 1, 0, 2, 5, 4, 3, 8]);
}

#[test]
fn presorted_stops_early_on_dense_candidates_and_scan_inspects_all() {
    let catalog = catalog();
    let index = CatalogIndex::build(&catalog);
    let column = NumericSortColumn::build(&index, "rating").unwrap();
    let presence = PresenceBitmap::build(&index, "rating").unwrap();
    let sorted = index.numeric_sorted("rating").unwrap();
    let everything = all(&index);
    let pre = top_k_presorted(sorted, &presence, &everything, Direction::Descending, 2);
    assert_eq!(pre.inspected, 2);
    let scan = top_k_scan(&everything, &column, Direction::Descending, 2);
    assert_eq!(scan.inspected, 9);
}

#[test]
fn sort_path_rule_switches_at_the_preregistered_boundary() {
    // presorted iff |C|^2 >= rho * limit * N.
    assert_eq!(
        choose_sort_path(100, 48, 1000, 1.0),
        SortPath::CandidateTopK
    );
    assert_eq!(choose_sort_path(220, 48, 1000, 1.0), SortPath::Presorted);
    assert_eq!(choose_sort_path(0, 48, 1000, 0.0), SortPath::Presorted);
    // bitmap iff |C| >= tau * V.
    assert_eq!(
        choose_facet_path(99, 10, 10.0, true),
        FacetPath::OrdinalScan
    );
    assert_eq!(
        choose_facet_path(100, 10, 10.0, true),
        FacetPath::BitmapCount
    );
}
