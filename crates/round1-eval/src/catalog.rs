//! Adapts real ESCI records into `commerce_core::domain` types.
//!
//! The source data has no price, category, or product-type field at all
//! (see `docs/experiments/ROUND1_LOG.md` R1-E01) — only title,
//! description, bullet points, brand, and color. Rather than changing
//! `commerce_core`'s domain model (which would touch every Phase 0
//! fixture and test), every real product is assigned sentinel
//! `ProductTypeId(0)`/`CategoryId(0)`/`Price::usd(0)`. This keeps Phase
//! 0's evidence base untouched and is the more honest choice: it is
//! recorded explicitly (not hidden) that product-type/category/price
//! structural constraints are not meaningfully testable against this
//! catalog, rather than inventing ground truth that doesn't exist in the
//! source data.

use std::collections::HashMap;

use commerce_core::domain::{
    attributes, AttributeValue, Brand, BrandId, Catalog, CategoryId, Inventory, Price, Product,
    ProductId, ProductTypeId, Variant, VariantId,
};

use crate::data::RealProduct;

pub const UNKNOWN_PRODUCT_TYPE: ProductTypeId = ProductTypeId(0);
pub const UNKNOWN_CATEGORY: CategoryId = CategoryId(0);

fn normalize(s: &str) -> String {
    s.trim().to_lowercase()
}

pub struct IngestedCatalog {
    pub catalog: Catalog,
    /// ESCI ASIN string -> the ProductId assigned during ingestion, so
    /// query judgments (which reference ASINs) can be joined back.
    pub asin_to_product_id: HashMap<String, ProductId>,
    /// One entry per distinct normalized brand string, for
    /// `cold_start::CatalogProfile::build`'s brand registry parameter.
    pub brands: Vec<Brand>,
}

/// Build a `Catalog` from real ESCI records. Brand names are interned by
/// normalized (trimmed, lowercased) string so "Aero Pure" and "AERO
/// PURE" collapse to one `BrandId` — a real ingestion pipeline would do
/// at least this much; the *un*-normalized collision count is a profiler
/// metric (see `profile.rs`), not swallowed here silently.
pub fn build_catalog(products: &[RealProduct]) -> IngestedCatalog {
    let mut brand_ids: HashMap<String, BrandId> = HashMap::new();
    let mut brands: Vec<Brand> = Vec::new();
    let mut asin_to_product_id = HashMap::with_capacity(products.len());
    let mut catalog_products = Vec::with_capacity(products.len());

    for (ordinal, p) in products.iter().enumerate() {
        let product_id = ProductId(ordinal as u64);
        asin_to_product_id.insert(p.id.clone(), product_id);

        let brand = p.brand.as_deref().map(|raw| {
            let key = normalize(raw);
            *brand_ids.entry(key).or_insert_with_key(|key| {
                let id = BrandId(brands.len() as u32 + 1);
                brands.push(Brand {
                    id,
                    name: key.clone(),
                });
                id
            })
        });

        let mut product_attrs = Vec::new();
        if let Some(desc) = &p.description {
            product_attrs.push(("description", AttributeValue::Text(desc.clone())));
        }
        if let Some(bullets) = &p.bullets {
            product_attrs.push(("bullets", AttributeValue::Text(bullets.clone())));
        }

        let mut variant_attrs = Vec::new();
        if let Some(color) = &p.color {
            variant_attrs.push(("color", AttributeValue::Enum(color.clone())));
        }

        catalog_products.push(Product {
            id: product_id,
            product_type: UNKNOWN_PRODUCT_TYPE,
            brand: brand.unwrap_or(BrandId(0)),
            category: UNKNOWN_CATEGORY,
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
        asin_to_product_id,
        brands,
    }
}
