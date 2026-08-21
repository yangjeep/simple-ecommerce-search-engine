//! Adapts real WANDS records into `commerce_core::domain` types.
//!
//! Mirrors `round1_eval::catalog`'s structure and honesty discipline: no
//! semantics are invented beyond what `scripts/datasets/profile_wands.py`
//! found the source data genuinely supports (see
//! `docs/research/artifacts/p6a_dataset_acquisition/manifest.json`).
//!
//! Field mapping:
//! - `category_leaf` (the deepest available hierarchy segment, as a full
//!   prefix path) -> `Product::category`, using Commerce Core's
//!   dedicated `CategoryId`/`category_bitmaps` index. This is the first
//!   time this project has exercised that index with real, non-sentinel
//!   values -- ESCI always assigns `CategoryId(0)`.
//! - `product_class` -> `Product::product_type` (`ProductTypeId`),
//!   likewise the first real exercise of `product_type_bitmaps`.
//! - `category_depth_1..6` -> typed `Enum` attributes valued by the full
//!   prefix path at that depth, enabling "subtree browse at depth N" as
//!   a plain `Constraint::Enum` filter, reusing the exact same generic
//!   `enum_bitmaps` machinery Phase 5 used for `color`. No new query
//!   semantics were added to `commerce_core` for this.
//! - `color`, `style`, `primarymaterial`, `material`, `shape` -> typed
//!   `Enum` attributes on the variant (mirroring ESCI's own `color`
//!   placement), each present only when the source record has a value.
//! - `rating_count`/`average_rating`/`review_count` -> typed `Numeric`
//!   attributes. These are NOT a price/availability substitute -- WANDS
//!   has neither field, confirmed absent (see the acquisition manifest)
//!   -- they are carried through only as a real, disclosed analog for a
//!   "sort by rating" PLP workload, never presented as price.
//! - No parent-ASIN/variant-grouping equivalent exists in WANDS: every
//!   record becomes exactly one `Product` with exactly one `Variant`,
//!   the same degenerate mapping `round1_eval::catalog` already uses.
//! - `Price::usd(0)`/`Inventory::in_stock(0)` sentinels, since WANDS has
//!   no price or availability data at all (recorded, not invented).

use std::collections::HashMap;

use commerce_core::domain::{
    attributes, AttributeValue, Catalog, Category, CategoryId, Inventory, Price, Product,
    ProductId, ProductType, ProductTypeId, Variant, VariantId,
};

use crate::data::WandsProduct;

fn normalize(s: &str) -> String {
    s.trim().to_lowercase()
}

pub struct IngestedCatalog {
    pub catalog: Catalog,
    /// WANDS product_id string -> the ProductId assigned during
    /// ingestion, so query/label judgments (which reference the WANDS
    /// id) can be joined back.
    pub wands_id_to_product_id: HashMap<String, ProductId>,
    pub categories: Vec<Category>,
    pub product_types: Vec<ProductType>,
}

/// Build a `Catalog` from real WANDS records. Category and product_class
/// strings are interned by exact string identity (not case-normalized --
/// unlike ESCI's brand field, WANDS' hierarchy/class strings come from
/// one internal Wayfair taxonomy, not free-text sellers, so the
/// casing-collision hazard Phase 5 found for brand does not apply here;
/// this is verified during profiling, not assumed).
pub fn build_catalog(products: &[WandsProduct]) -> IngestedCatalog {
    let mut category_ids: HashMap<String, CategoryId> = HashMap::new();
    let mut categories: Vec<Category> = Vec::new();
    let mut product_type_ids: HashMap<String, ProductTypeId> = HashMap::new();
    let mut product_types: Vec<ProductType> = Vec::new();
    let mut wands_id_to_product_id = HashMap::with_capacity(products.len());
    let mut catalog_products = Vec::with_capacity(products.len());

    const UNKNOWN_CATEGORY: CategoryId = CategoryId(0);
    const UNKNOWN_PRODUCT_TYPE: ProductTypeId = ProductTypeId(0);

    for (ordinal, p) in products.iter().enumerate() {
        let product_id = ProductId(ordinal as u64);
        wands_id_to_product_id.insert(p.id.clone(), product_id);

        let category = p
            .category_leaf
            .as_deref()
            .map(|raw| {
                let key = raw.to_string();
                *category_ids.entry(key.clone()).or_insert_with(|| {
                    let id = CategoryId(categories.len() as u32 + 1);
                    categories.push(Category {
                        id,
                        name: key.clone(),
                    });
                    id
                })
            })
            .unwrap_or(UNKNOWN_CATEGORY);

        let product_type = p
            .product_class
            .as_deref()
            .map(|raw| {
                let key = normalize(raw);
                *product_type_ids.entry(key.clone()).or_insert_with(|| {
                    let id = ProductTypeId(product_types.len() as u32 + 1);
                    product_types.push(ProductType {
                        id,
                        name: key.clone(),
                    });
                    id
                })
            })
            .unwrap_or(UNKNOWN_PRODUCT_TYPE);

        let mut product_attrs = Vec::new();
        if let Some(desc) = &p.description {
            product_attrs.push(("description", AttributeValue::Text(desc.clone())));
        }
        for (depth, value) in p.category_depths() {
            let key: &'static str = match depth {
                1 => "category_depth_1",
                2 => "category_depth_2",
                3 => "category_depth_3",
                4 => "category_depth_4",
                5 => "category_depth_5",
                _ => "category_depth_6",
            };
            product_attrs.push((key, AttributeValue::Enum(value.to_string())));
        }

        let mut variant_attrs = Vec::new();
        if let Some(color) = &p.color {
            variant_attrs.push(("color", AttributeValue::Enum(color.clone())));
        }
        if let Some(style) = &p.style {
            variant_attrs.push(("style", AttributeValue::Enum(style.clone())));
        }
        if let Some(m) = &p.primarymaterial {
            variant_attrs.push(("primarymaterial", AttributeValue::Enum(m.clone())));
        }
        if let Some(m) = &p.material {
            variant_attrs.push(("material", AttributeValue::Enum(m.clone())));
        }
        if let Some(s) = &p.shape {
            variant_attrs.push(("shape", AttributeValue::Enum(s.clone())));
        }
        if let Some(r) = p.rating_count {
            variant_attrs.push(("rating_count", AttributeValue::Numeric(r)));
        }
        if let Some(r) = p.average_rating {
            variant_attrs.push(("average_rating", AttributeValue::Numeric(r)));
        }
        if let Some(r) = p.review_count {
            variant_attrs.push(("review_count", AttributeValue::Numeric(r)));
        }

        catalog_products.push(Product {
            id: product_id,
            product_type,
            brand: commerce_core::domain::BrandId(0),
            category,
            title: p.title.clone(),
            attributes: attributes(product_attrs),
            variants: vec![Variant {
                id: VariantId(ordinal as u64),
                attributes: attributes(variant_attrs),
                price: Price::usd(0),
                inventory: Inventory::in_stock(0),
            }],
        });
    }

    IngestedCatalog {
        catalog: Catalog {
            products: catalog_products,
        },
        wands_id_to_product_id,
        categories,
        product_types,
    }
}
