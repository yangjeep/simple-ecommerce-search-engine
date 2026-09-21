//! Dataset B: a tiny, deterministic, hand-verifiable multi-variant fixture,
//! used only to gate product/variant correctness (never for headline
//! performance numbers) -- per #77's amended preregistration.
//!
//! Shape (exactly the #77 spec):
//!
//! ```text
//! Product A
//!   Variant A1: color=black, size=8,  width=wide,   available=true
//!   Variant A2: color=red,   size=9,  width=narrow, available=true
//!
//! Product B
//!   Variant B1: color=black, size=9,  width=wide,   available=true
//!
//! Product C
//!   Variant C1: color=black, size=9,  width=narrow, available=false
//! ```
//!
//! The critical case is **same-product cross-variant leakage**: Product A
//! has `black` on A1 (size 8) and `size=9` on A2 (red) -- no single variant
//! of A has both `black` and `size=9`. A naive implementation that indexes
//! attributes at the product level (aggregating every variant's values onto
//! one document/record) would incorrectly let a `color=black AND size=9`
//! query match Product A by combining A1's color with A2's size. Every
//! engine's document model for this fixture must be **one flat document per
//! variant** (matching how `commerce_core::domain::catalog::effective_attributes`
//! merges product+variant attributes into a single per-variant view,
//! never mixing two variants) -- this is also exactly how Dataset A's WANDS
//! rows are already modeled (one row = one product = one variant), so no
//! new per-engine variant-modeling machinery is needed: the same flat
//! per-variant document shape used for Dataset A is reused here.

use commerce_core::domain::{
    attributes, AttributeValue, Brand, BrandId, Catalog, Category, CategoryId, Inventory, Price,
    Product, ProductId, ProductType, ProductTypeId, Variant, VariantId,
};
use serde::{Deserialize, Serialize};

/// One flat, per-variant fixture document -- the exact shape every
/// competitor engine indexes for Dataset B (and the shape WANDS rows
/// already have for Dataset A: one row, one variant, no nesting).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FixtureVariantDoc {
    pub id: String,
    pub product_id: String,
    pub color: String,
    pub size: String,
    pub width: String,
    pub available: bool,
}

#[must_use]
pub fn fixture_documents() -> Vec<FixtureVariantDoc> {
    vec![
        FixtureVariantDoc {
            id: "A1".to_owned(),
            product_id: "A".to_owned(),
            color: "black".to_owned(),
            size: "8".to_owned(),
            width: "wide".to_owned(),
            available: true,
        },
        FixtureVariantDoc {
            id: "A2".to_owned(),
            product_id: "A".to_owned(),
            color: "red".to_owned(),
            size: "9".to_owned(),
            width: "narrow".to_owned(),
            available: true,
        },
        FixtureVariantDoc {
            id: "B1".to_owned(),
            product_id: "B".to_owned(),
            color: "black".to_owned(),
            size: "9".to_owned(),
            width: "wide".to_owned(),
            available: true,
        },
        FixtureVariantDoc {
            id: "C1".to_owned(),
            product_id: "C".to_owned(),
            color: "black".to_owned(),
            size: "9".to_owned(),
            width: "narrow".to_owned(),
            available: false,
        },
    ]
}

/// One correctness-oracle query: a set of equality filters over the
/// fixture's fields, and the exact expected set of matching product ids
/// (computed by hand from the fixture above, not derived from any engine).
#[derive(Debug, Clone)]
pub struct OracleQuery {
    pub name: &'static str,
    /// (field, value) equality filters, ANDed together. `available` uses
    /// `"true"`/`"false"` as the value string.
    pub filters: &'static [(&'static str, &'static str)],
    pub expected_product_ids: &'static [&'static str],
}

/// The four oracle queries #77 specifies. `cross_variant_leakage_negative`
/// is the critical one: it must return `{B, C}`, never `A`, even though A
/// individually has both `color=black` (on A1) and `size=9` (on A2).
#[must_use]
pub const fn oracle_queries() -> &'static [OracleQuery] {
    &[
        OracleQuery {
            name: "full_positive_match",
            filters: &[
                ("color", "black"),
                ("size", "9"),
                ("width", "wide"),
                ("available", "true"),
            ],
            expected_product_ids: &["B"],
        },
        OracleQuery {
            name: "cross_variant_leakage_negative",
            filters: &[("color", "black"), ("size", "9")],
            expected_product_ids: &["B", "C"],
        },
        OracleQuery {
            name: "color_only",
            filters: &[("color", "black")],
            expected_product_ids: &["A", "B", "C"],
        },
        OracleQuery {
            name: "available_only",
            filters: &[("available", "true")],
            expected_product_ids: &["A", "B"],
        },
    ]
}

/// Evaluate one [`OracleQuery`] directly against [`fixture_documents`] (a
/// flat per-variant scan, the same semantics every competitor's flat-doc
/// query must reproduce) -- used to assert the oracle itself is internally
/// consistent (tested below) and as the ground truth doc comment/rationale
/// for every engine adapter's query translation.
#[must_use]
pub fn evaluate_oracle_by_scan(query: &OracleQuery) -> Vec<String> {
    let mut matched_products: Vec<String> = fixture_documents()
        .into_iter()
        .filter(|doc| {
            query.filters.iter().all(|(field, value)| match *field {
                "color" => doc.color == *value,
                "size" => doc.size == *value,
                "width" => doc.width == *value,
                "available" => doc.available.to_string() == *value,
                other => panic!("oracle query references unknown fixture field {other:?}"),
            })
        })
        .map(|doc| doc.product_id)
        .collect();
    matched_products.sort();
    matched_products.dedup();
    matched_products
}

/// Builds Dataset B as a `commerce_core::domain::Catalog`, for native's own
/// in-process correctness testing (and as the source of truth the
/// `i77_native_plp_server` binary serves over `/correctness`). Every
/// attribute here is placed on the *variant*, not the product, matching the
/// flat-per-variant modeling every other engine also uses for this fixture.
#[must_use]
pub fn build_fixture_catalog() -> Catalog {
    let brand = Brand {
        id: BrandId(0),
        name: "fixture-brand".to_owned(),
    };
    let category = Category {
        id: CategoryId(0),
        name: "fixture-category".to_owned(),
    };
    let product_type = ProductType {
        id: ProductTypeId(0),
        name: "fixture-type".to_owned(),
    };

    let make_variant = |doc: &FixtureVariantDoc, ordinal: u64| Variant {
        id: VariantId(ordinal),
        attributes: attributes([
            ("color", AttributeValue::Enum(doc.color.clone())),
            ("size", AttributeValue::Enum(doc.size.clone())),
            ("width", AttributeValue::Enum(doc.width.clone())),
            ("available", AttributeValue::Boolean(doc.available)),
        ]),
        price: Price::usd(0),
        inventory: if doc.available {
            Inventory::in_stock(1)
        } else {
            Inventory::out_of_stock()
        },
    };

    let docs = fixture_documents();
    let mut products: Vec<Product> = Vec::new();
    for (product_ordinal, product_id) in ["A", "B", "C"].iter().enumerate() {
        let variants: Vec<Variant> = docs
            .iter()
            .enumerate()
            .filter(|(_, doc)| doc.product_id == *product_id)
            .map(|(i, doc)| make_variant(doc, i as u64))
            .collect();
        products.push(Product {
            id: ProductId(product_ordinal as u64),
            product_type: product_type.id,
            brand: brand.id,
            category: category.id,
            title: format!("fixture product {product_id}"),
            attributes: attributes([]),
            variants,
        });
    }
    Catalog { products }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oracle_by_scan_matches_hand_derived_expectations() {
        for query in oracle_queries() {
            let actual = evaluate_oracle_by_scan(query);
            let mut expected: Vec<String> = query
                .expected_product_ids
                .iter()
                .map(|s| s.to_string())
                .collect();
            expected.sort();
            assert_eq!(actual, expected, "oracle query {:?} mismatch", query.name);
        }
    }

    #[test]
    fn cross_variant_leakage_negative_excludes_product_a() {
        let query = &oracle_queries()[1];
        assert_eq!(query.name, "cross_variant_leakage_negative");
        let actual = evaluate_oracle_by_scan(query);
        assert!(
            !actual.contains(&"A".to_owned()),
            "cross-variant leakage: product A matched black+size9 despite \
             no single variant of A having both attributes"
        );
    }

    #[test]
    fn native_catalog_search_matches_oracle_via_commerce_core() {
        use commerce_core::domain::Constraint;
        let catalog = build_fixture_catalog();
        for query in oracle_queries() {
            let constraints: Vec<Constraint> = query
                .filters
                .iter()
                .map(|(field, value)| match *field {
                    "available" => Constraint::Boolean {
                        attribute: (*field).to_owned(),
                        value: *value == "true",
                    },
                    _ => Constraint::Enum {
                        attribute: (*field).to_owned(),
                        value: (*value).to_owned(),
                    },
                })
                .collect();
            let hits = catalog.search(&constraints);
            let mut actual_products: Vec<String> = hits
                .iter()
                .map(|(product_id, _variant_id)| {
                    let idx = product_id.0 as usize;
                    ["A", "B", "C"][idx].to_owned()
                })
                .collect();
            actual_products.sort();
            actual_products.dedup();
            let mut expected: Vec<String> = query
                .expected_product_ids
                .iter()
                .map(|s| s.to_string())
                .collect();
            expected.sort();
            assert_eq!(
                actual_products, expected,
                "commerce_core::Catalog::search mismatch for query {:?}",
                query.name
            );
        }
    }
}
