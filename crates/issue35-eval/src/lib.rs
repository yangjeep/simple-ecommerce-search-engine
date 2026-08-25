//! Issue #35 (`docs/experiments/ISSUE35_ESCI_ELECTRONICS_PROTOCOL.md`):
//! ingestion for a real, unseen-vertical (electronics) slice of the
//! Amazon Shopping Queries Dataset ("ESCI"), built to test whether
//! this project's existing discovery/serving pipeline
//! (`CatalogProfile`, `compile_lexicon`, `CatalogIndex`,
//! `execute_planned`) generalizes with **zero** `commerce-core`
//! changes and **zero** hand-authored vertical ontology.
//!
//! ESCI carries no `product_type`/`category` field at all (unlike
//! WANDS's `product_class`/`category_leaf`). Rather than hand-author a
//! keyword-based category classifier -- itself exactly the kind of
//! "manually authored vertical ontology" Issue #35's first-pass rule
//! prohibits -- every product's `product_type`/`category` are left as
//! unregistered sentinel ids, invisible to `CatalogProfile`'s lexicon.
//! This mirrors `phase6a-eval`'s own `UNKNOWN_PRODUCT_TYPE` pattern for
//! genuinely absent data, not a new mechanism. `Brand` (from ESCI's
//! real `product_brand` field) and a generic `color` attribute (from
//! `product_color`) are the only structural/attribute signals
//! populated, since both are pre-existing, vertical-agnostic concepts
//! already used identically for WANDS and the Magento fixture.
//!
//! ESCI also has no real Product/Variant grouping (flat products) and
//! no price data, both already-disclosed limitations
//! (`docs/decisions/ISSUE55_H3_DECISION.md` notes the same for WANDS's
//! own Product/Variant gap). Single-variant ingestion with placeholder
//! price/inventory is used, exactly as `phase6a-eval` already does for
//! WANDS.

use std::collections::BTreeMap;

use commerce_core::domain::{
    attributes, AttributeValue, Brand, BrandId, Catalog, CategoryId, Inventory, Price, Product,
    ProductId, ProductTypeId, Variant, VariantId,
};

/// Never registered in the `brands`/`product_types`/`categories` lists
/// passed to `CatalogProfile::build`, so it is structurally valid on
/// `Product` but invisible to the lexicon -- exactly `phase6a-eval`'s
/// own `UNKNOWN_PRODUCT_TYPE` precedent, reused for a second field
/// (`category`) this dataset also cannot honestly populate.
const UNKNOWN_PRODUCT_TYPE: ProductTypeId = ProductTypeId(0);
const UNKNOWN_CATEGORY: CategoryId = CategoryId(0);
/// ESCI carries no price data; every product gets this same disclosed
/// placeholder rather than an invented distribution. No price-range
/// query is evaluated against this slice.
const PLACEHOLDER_PRICE_CENTS: i64 = 0;

#[derive(Debug, serde::Deserialize)]
pub struct EsciProduct {
    pub product_id: String,
    pub title: String,
    pub description: String,
    pub bullet_point: String,
    pub brand: String,
    pub color: String,
}

#[derive(Debug, serde::Deserialize)]
pub struct EsciJudgment {
    pub product_id: String,
    pub label: String,
}

#[derive(Debug, serde::Deserialize)]
pub struct EsciQuery {
    pub query: String,
    pub judgments: Vec<EsciJudgment>,
}

pub fn load_products(path: &str) -> Vec<EsciProduct> {
    std::fs::read_to_string(path)
        .expect("read esci_electronics_products.jsonl")
        .lines()
        .map(|line| serde_json::from_str(line).expect("parse EsciProduct"))
        .collect()
}

pub fn load_queries(path: &str) -> Vec<EsciQuery> {
    std::fs::read_to_string(path)
        .expect("read esci_electronics_queries.jsonl")
        .lines()
        .map(|line| serde_json::from_str(line).expect("parse EsciQuery"))
        .collect()
}

/// `esci_label` -> graded relevance gain, the standard mapping used in
/// ESCI-derived ranking literature (not invented for this checkpoint):
/// Exact=1.0, Substitute=0.1, Complement=0.01, Irrelevant=0.0.
pub fn label_gain(label: &str) -> f64 {
    match label {
        "Exact" => 1.0,
        "Substitute" => 0.1,
        "Complement" => 0.01,
        "Irrelevant" => 0.0,
        other => panic!("unknown esci_label {other:?}"),
    }
}

pub struct Ingested {
    pub catalog: Catalog,
    pub brands: Vec<Brand>,
    /// `esci_electronics_products.jsonl`'s `product_id` (a real ASIN) ->
    /// this catalog's own `ProductId`, needed to translate judgment
    /// files (keyed by ASIN) into this project's own id space.
    pub product_id_by_asin: BTreeMap<String, ProductId>,
}

/// Builds a `Catalog` from the real ESCI electronics slice. Zero
/// `commerce-core` changes: every type used here
/// (`Product`/`Variant`/`Brand`/`AttributeValue`) already exists and is
/// already used identically for WANDS/Magento ingestion.
pub fn build_catalog(products: &[EsciProduct]) -> Ingested {
    let mut brand_id_by_name: BTreeMap<String, BrandId> = BTreeMap::new();
    let mut brands = Vec::new();
    let mut next_brand_id = 1u32;

    let mut catalog_products = Vec::with_capacity(products.len());
    let mut product_id_by_asin = BTreeMap::new();

    for (i, p) in products.iter().enumerate() {
        let brand_id = if p.brand.trim().is_empty() {
            None
        } else {
            Some(*brand_id_by_name.entry(p.brand.clone()).or_insert_with(|| {
                let id = BrandId(next_brand_id);
                brands.push(Brand {
                    id,
                    name: p.brand.clone(),
                });
                next_brand_id += 1;
                id
            }))
        };

        let mut attrs = vec![
            ("description", AttributeValue::Text(p.description.clone())),
            ("bullet_point", AttributeValue::Text(p.bullet_point.clone())),
        ];
        if !p.color.trim().is_empty() {
            attrs.push(("color", AttributeValue::Enum(p.color.clone())));
        }

        let product_id = ProductId(i as u64 + 1);
        let variant_id = VariantId(i as u64 + 1);
        catalog_products.push(Product {
            id: product_id,
            product_type: UNKNOWN_PRODUCT_TYPE,
            // `Brand(0)` (unregistered) for products with no real brand
            // string -- not a fabricated brand, structurally identical
            // to `UNKNOWN_PRODUCT_TYPE`'s own "absent, not guessed"
            // convention.
            brand: brand_id.unwrap_or(BrandId(0)),
            category: UNKNOWN_CATEGORY,
            title: p.title.clone(),
            attributes: attributes(attrs),
            variants: vec![Variant {
                id: variant_id,
                attributes: attributes([]),
                price: Price::usd(PLACEHOLDER_PRICE_CENTS),
                inventory: Inventory::in_stock(1),
            }],
        });
        product_id_by_asin.insert(p.product_id.clone(), product_id);
    }

    Ingested {
        catalog: Catalog {
            products: catalog_products,
        },
        brands,
        product_id_by_asin,
    }
}
