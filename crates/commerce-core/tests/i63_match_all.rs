//! Issue #63 (Infra E3, amended): correctness of the match-all candidate
//! representations and the dense full-catalog facet paths against the
//! existing, already-verified facet routines, on a multi-variant catalog
//! with product- and variant-level attributes, missing values and a
//! `MultiEnum` attribute.

use commerce_core::domain::{
    attributes, AttributeValue, BrandId, Catalog, CategoryId, Constraint, Inventory, NumericOp,
    Price, Product, ProductId, ProductTypeId, Variant, VariantId,
};
use commerce_core::index::{CandidateSet, CatalogIndex};
use commerce_core::ir::{ResolvedConstraint, StructuralConstraint};
use roaring::RoaringBitmap;

fn variant(id: u64, attrs: Vec<(&'static str, AttributeValue)>) -> Variant {
    Variant {
        id: VariantId(id),
        attributes: attributes(attrs),
        price: Price::usd(100 * id as i64),
        inventory: Inventory::in_stock(1),
    }
}

/// 3 products / 7 variants: product-level `material`, variant-level
/// `color` (with gaps), a `MultiEnum` `tags` attribute and a numeric
/// `rating`.
fn catalog() -> Catalog {
    let e = |v: &str| AttributeValue::Enum(v.to_owned());
    let m = |vs: &[&str]| AttributeValue::MultiEnum(vs.iter().map(|v| (*v).to_owned()).collect());
    let product = |id: u64, material: Option<&'static str>, variants: Vec<Variant>| Product {
        id: ProductId(id),
        product_type: ProductTypeId(1),
        brand: BrandId(id as u32),
        category: CategoryId((id % 2) as u32),
        title: format!("product {id}"),
        attributes: attributes(
            material
                .map(|v| vec![("material", AttributeValue::Enum(v.to_owned()))])
                .unwrap_or_default(),
        ),
        variants,
    };
    Catalog {
        products: vec![
            product(
                1,
                Some("oak"),
                vec![
                    variant(
                        10,
                        vec![
                            ("color", e("black")),
                            ("tags", m(&["a", "b"])),
                            ("rating", AttributeValue::Numeric(4.0)),
                        ],
                    ),
                    variant(11, vec![("color", e("red"))]),
                    variant(12, vec![("tags", m(&["b"]))]),
                ],
            ),
            product(
                2,
                None,
                vec![
                    variant(20, vec![("color", e("black"))]),
                    variant(21, vec![("rating", AttributeValue::Numeric(2.0))]),
                ],
            ),
            product(
                3,
                Some("pine"),
                vec![
                    variant(30, vec![("color", e("white")), ("tags", m(&["c"]))]),
                    variant(31, vec![("color", e("black"))]),
                ],
            ),
        ],
    }
}

fn full(index: &CatalogIndex) -> RoaringBitmap {
    (0..index.ordinal_count() as u32).collect()
}

#[test]
fn all_ordinals_bitmap_materializations_are_identical() {
    let index = CatalogIndex::build(&catalog());
    let expected = full(&index);
    assert_eq!(index.all_ordinals_bitmap(), expected);
    assert_eq!(index.all_ordinals_bitmap_by_range(), expected);
    assert_eq!(
        CatalogIndex::build(&Catalog::default()).all_ordinals_bitmap_by_range(),
        RoaringBitmap::new()
    );
}

#[test]
fn candidate_set_is_match_all_exactly_when_no_indexable_constraint_exists() {
    let index = CatalogIndex::build(&catalog());
    let text_only = vec![ResolvedConstraint::Attribute(Constraint::Text {
        attribute: "note".to_owned(),
        contains: "x".to_owned(),
    })];
    for constraints in [Vec::new(), text_only] {
        let set = index.candidate_set(&constraints);
        assert!(matches!(set, CandidateSet::All { .. }), "{constraints:?}");
        assert_eq!(set.len(), index.ordinal_count() as u64);
        assert_eq!(set.materialize(), index.indexed_candidates(&constraints));
    }
    let indexable = [
        vec![ResolvedConstraint::Attribute(Constraint::Enum {
            attribute: "color".to_owned(),
            value: "black".to_owned(),
        })],
        vec![ResolvedConstraint::Structural(
            StructuralConstraint::Category(CategoryId(1)),
        )],
        // An indexable constraint that matches nothing is an empty set, not
        // match-all.
        vec![ResolvedConstraint::Attribute(Constraint::Numeric {
            attribute: "rating".to_owned(),
            op: NumericOp::Gt,
            value: 100.0,
        })],
    ];
    for constraints in indexable {
        let set = index.candidate_set(&constraints);
        assert!(matches!(set, CandidateSet::Set(_)), "{constraints:?}");
        assert_eq!(set.materialize(), index.indexed_candidates(&constraints));
        assert_eq!(set.len(), index.indexed_candidates(&constraints).len());
    }
}

#[test]
fn candidate_set_first_ordinals_match_the_materialized_order() {
    let index = CatalogIndex::build(&catalog());
    let all = index.candidate_set(&[]);
    for limit in 0..10 {
        let expected: Vec<u32> = full(&index).iter().take(limit).collect();
        assert_eq!(all.first_ordinals(limit), expected, "limit {limit}");
    }
}

#[test]
fn dense_full_catalog_facets_match_the_bitmap_candidate_routines() {
    let index = CatalogIndex::build(&catalog());
    let all = full(&index);
    for attr in ["color", "material", "tags", "rating", "absent"] {
        let legacy = index.facet_counts(attr, &all);
        assert_eq!(
            index.facet_counts_bitmap_all(attr),
            legacy,
            "bitmap_all {attr}"
        );
        assert_eq!(
            index.facet_counts_bitmap(&all, attr),
            legacy,
            "bitmap {attr}"
        );
        // The ordinal paths count single-valued Enum only (the documented
        // `facet_counts_ordinal` semantics); the dense scan must match the
        // candidate scan exactly, MultiEnum attribute included.
        assert_eq!(
            index.facet_counts_ordinal_all(attr),
            index.facet_counts_ordinal(&all, attr),
            "ordinal_all {attr}"
        );
    }
}

#[test]
fn read_only_views_are_consistent_with_the_facet_routines() {
    let index = CatalogIndex::build(&catalog());
    let dictionary = index.enum_dictionary("color").expect("color dictionary");
    let column = index.enum_column("color").expect("color column");
    assert_eq!(column.len(), index.ordinal_count());
    for (value_ord, value) in dictionary.iter().enumerate() {
        let bitmap = index
            .enum_value_bitmap("color", value)
            .expect("value bitmap");
        let from_column: RoaringBitmap = column
            .iter()
            .enumerate()
            .filter(|(_, &v)| v == value_ord as u32)
            .map(|(ord, _)| ord as u32)
            .collect();
        assert_eq!(bitmap, &from_column, "{value}");
    }
    assert!(index.enum_column("tags").is_none() || !index.attribute_is_single_valued_enum("tags"));
    assert!(index.enum_value_bitmap("color", "no-such-value").is_none());
    assert!(index.enum_dictionary("absent").is_none());
}

#[test]
fn presorted_over_match_all_matches_presorted_over_the_full_bitmap() {
    use commerce_core::index::sort::{
        top_k_presorted, top_k_presorted_all, Direction, PresenceBitmap,
    };
    let n = AttributeValue::Numeric;
    let ratings = [
        Some(4.5),
        None,
        Some(0.0),
        Some(-0.0),
        Some(4.5),
        Some(f64::NAN),
        None,
        Some(1.0),
        Some(0.0),
    ];
    let catalog = Catalog {
        products: ratings
            .iter()
            .enumerate()
            .map(|(i, rating)| Product {
                id: ProductId(i as u64),
                product_type: ProductTypeId(1),
                brand: BrandId(1),
                category: CategoryId(1),
                title: format!("p{i}"),
                attributes: attributes([]),
                variants: vec![variant(
                    100 + i as u64,
                    rating.map(|r| vec![("rating", n(r))]).unwrap_or_default(),
                )],
            })
            .collect(),
    };
    let index = CatalogIndex::build(&catalog);
    let sorted = index.numeric_sorted("rating").expect("rating is numeric");
    let presence = PresenceBitmap::build(&index, "rating").expect("presence");
    let everything = full(&index);
    for direction in [Direction::Ascending, Direction::Descending] {
        for limit in 0..=ratings.len() + 1 {
            let expected = top_k_presorted(sorted, &presence, &everything, direction, limit);
            let actual = top_k_presorted_all(
                sorted,
                &presence,
                index.ordinal_count() as u32,
                direction,
                limit,
            );
            let key = |o: &commerce_core::index::sort::SortOutcome| {
                o.hits
                    .iter()
                    .map(|h| (h.ordinal, h.value.map(f64::to_bits)))
                    .collect::<Vec<_>>()
            };
            assert_eq!(key(&actual), key(&expected), "{direction:?} {limit}");
            assert_eq!(
                actual.inspected, expected.inspected,
                "{direction:?} {limit}"
            );
        }
    }
}
