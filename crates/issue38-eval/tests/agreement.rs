//! Issue #38 E1: before trusting any latency/memory/allocation comparison
//! between paths A (hard-coded `CatalogIndex`), B (`CompiledEnumIndex`),
//! and C (`GenericStore`), all three must return the *identical* hit set
//! for the identical query on the identical catalog -- a benchmark that
//! silently compares differently-correct engines is worthless. This is
//! the same "index only changes how a match is found, never what counts
//! as one" contract `commerce-core`'s own `tests/physical_index.rs`
//! establishes for `CatalogIndex` itself, extended here to the two new
//! paths this experiment adds.

use std::collections::BTreeSet;

use commerce_core::domain::{
    attributes, AttributeValue, BrandId, Catalog, CategoryId, Inventory, Price, Product, ProductId,
    ProductTypeId, Variant, VariantId,
};
use commerce_core::index::CatalogIndex;
use commerce_core::ir::{ResolvedConstraint, StructuralConstraint};
use issue38_eval::{CompiledEnumIndex, CompiledOrdinalIndex, GenericStore, GenericValue};
use roaring::RoaringBitmap;

/// `CatalogIndex`'s public API exposes ordinal->`VariantId` and
/// `VariantId`->`(Product, Variant)` lookups separately (see
/// `variant_id_at`/`lookup_variant`), but not a direct "bitmap ->
/// `(ProductId, VariantId)` pairs" helper -- every real caller either
/// wants the `ProductId` set alone (`candidate_product_ids`) or a fully
/// ranked/executed query. Reconstructing pairs here, from public API only,
/// is exactly what a real external caller comparing hit sets would do.
fn candidates_as_pairs(
    index: &CatalogIndex,
    catalog: &Catalog,
    bitmap: &RoaringBitmap,
) -> Vec<(ProductId, VariantId)> {
    bitmap
        .iter()
        .map(|ord| {
            let vid = index
                .variant_id_at(ord)
                .expect("ordinal built from this index");
            let (product, _) = index
                .lookup_variant(catalog, vid)
                .expect("variant looked up from this catalog");
            (product.id, vid)
        })
        .collect()
}

/// Three product types (one repeated on two products), each with two
/// variants, so a real query has more than one matching product and more
/// than one non-matching product -- exercises both hit and miss paths.
fn small_catalog() -> Catalog {
    fn variant(id: u64, cents: i64) -> Variant {
        Variant {
            id: VariantId(id),
            attributes: attributes([]),
            price: Price::usd(cents),
            inventory: Inventory::in_stock(5),
        }
    }
    fn product(id: u64, product_type: u32, variants: Vec<Variant>) -> Product {
        Product {
            id: ProductId(id),
            product_type: ProductTypeId(product_type),
            brand: BrandId(1),
            category: CategoryId(1),
            title: format!("product {id}"),
            attributes: attributes([("color", AttributeValue::Enum("black".to_string()))]),
            variants,
        }
    }
    Catalog {
        products: vec![
            product(1, 10, vec![variant(101, 1000), variant(102, 1200)]),
            product(2, 10, vec![variant(201, 1500)]),
            product(3, 20, vec![variant(301, 900)]),
            product(4, 30, vec![variant(401, 2000), variant(402, 2100)]),
        ],
    }
}

fn product_type_name(id: ProductTypeId) -> String {
    match id.0 {
        10 => "Coffee & Cocktail Tables".to_string(),
        20 => "Beds".to_string(),
        30 => "Wallpaper".to_string(),
        other => panic!("unexpected product_type id in fixture: {other}"),
    }
}

#[test]
fn all_three_paths_agree_on_a_query_type_with_multiple_hits() {
    let catalog = small_catalog();
    let a = CatalogIndex::build(&catalog);
    let b = CompiledEnumIndex::compile(&catalog, "product_type", |p| {
        product_type_name(p.product_type)
    });
    let b2 = CompiledOrdinalIndex::compile(&catalog, |p| product_type_name(p.product_type));
    let c = GenericStore::build(&catalog, |p| {
        vec![(
            "product_type".to_string(),
            GenericValue::Str(product_type_name(p.product_type)),
        )]
    });

    let a_hits: BTreeSet<(ProductId, VariantId)> = candidates_as_pairs(
        &a,
        &catalog,
        &a.indexed_candidates(&[ResolvedConstraint::Structural(
            StructuralConstraint::ProductType(ProductTypeId(10)),
        )]),
    )
    .into_iter()
    .collect();
    let b_hits: BTreeSet<(ProductId, VariantId)> = b
        .candidate_product_variant_ids(&b.query_eq("product_type", "Coffee & Cocktail Tables"))
        .into_iter()
        .collect();
    let b2_hits: BTreeSet<(ProductId, VariantId)> = b2
        .candidate_product_variant_ids(&b2.query_eq("Coffee & Cocktail Tables"))
        .into_iter()
        .collect();
    let c_hits: BTreeSet<(ProductId, VariantId)> = c
        .query_eq(
            "product_type",
            &GenericValue::Str("Coffee & Cocktail Tables".to_string()),
        )
        .into_iter()
        .collect();

    let expected: BTreeSet<(ProductId, VariantId)> = [
        (ProductId(1), VariantId(101)),
        (ProductId(1), VariantId(102)),
        (ProductId(2), VariantId(201)),
    ]
    .into_iter()
    .collect();

    assert_eq!(a_hits, expected, "path A (hard-coded) hit set wrong");
    assert_eq!(
        b_hits, expected,
        "path B (compiled schema) disagrees with path A: {b_hits:?}"
    );
    assert_eq!(
        b2_hits, expected,
        "path B2 (compiled ordinal schema) disagrees with path A: {b2_hits:?}"
    );
    assert_eq!(
        c_hits, expected,
        "path C (runtime-generic) disagrees with path A: {c_hits:?}"
    );
}

#[test]
fn all_three_paths_agree_on_a_query_type_with_a_single_hit() {
    let catalog = small_catalog();
    let a = CatalogIndex::build(&catalog);
    let b = CompiledEnumIndex::compile(&catalog, "product_type", |p| {
        product_type_name(p.product_type)
    });
    let c = GenericStore::build(&catalog, |p| {
        vec![(
            "product_type".to_string(),
            GenericValue::Str(product_type_name(p.product_type)),
        )]
    });

    let a_hits: BTreeSet<(ProductId, VariantId)> = candidates_as_pairs(
        &a,
        &catalog,
        &a.indexed_candidates(&[ResolvedConstraint::Structural(
            StructuralConstraint::ProductType(ProductTypeId(20)),
        )]),
    )
    .into_iter()
    .collect();
    let b_hits: BTreeSet<(ProductId, VariantId)> = b
        .candidate_product_variant_ids(&b.query_eq("product_type", "Beds"))
        .into_iter()
        .collect();
    let c_hits: BTreeSet<(ProductId, VariantId)> = c
        .query_eq("product_type", &GenericValue::Str("Beds".to_string()))
        .into_iter()
        .collect();

    let expected: BTreeSet<(ProductId, VariantId)> =
        [(ProductId(3), VariantId(301))].into_iter().collect();

    assert_eq!(a_hits, expected);
    assert_eq!(b_hits, expected);
    assert_eq!(c_hits, expected);
}

#[test]
fn all_three_paths_agree_a_value_absent_from_the_catalog_returns_nothing() {
    let catalog = small_catalog();
    let a = CatalogIndex::build(&catalog);
    let b = CompiledEnumIndex::compile(&catalog, "product_type", |p| {
        product_type_name(p.product_type)
    });
    let c = GenericStore::build(&catalog, |p| {
        vec![(
            "product_type".to_string(),
            GenericValue::Str(product_type_name(p.product_type)),
        )]
    });

    let a_hits = a.indexed_candidates(&[ResolvedConstraint::Structural(
        StructuralConstraint::ProductType(ProductTypeId(999)),
    )]);
    assert!(a_hits.is_empty());
    assert!(b.query_eq("product_type", "Never Seen").is_empty());
    assert!(c
        .query_eq("product_type", &GenericValue::Str("Never Seen".to_string()))
        .is_empty());
}
