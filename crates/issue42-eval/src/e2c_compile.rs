//! E2c's compiled-primitive pipeline (`docs/experiments/ISSUE45_PROTOCOL.md`
//! section 8): ingests `Promoted` `CanonicalDescriptor`s into a real,
//! unmodified `commerce_core::domain::Catalog`, then a real, unmodified
//! `commerce_core::index::CatalogIndex` is built from it by the caller
//! (this module only produces the `Catalog`; building the index is a
//! one-line `CatalogIndex::build(&catalog)` call at the use site, exactly
//! matching `e2b_serving_overhead_eval`'s own precedent).
//!
//! **A deliberate, disclosed extension beyond `e2b_ingest::build_catalog`**:
//! that function's own `accepted_typed_keys` structurally excludes
//! `Identifier`/`Relationship` roles from ever being ingested as
//! attribute values at all -- meaning `commerce_core`'s own real
//! `IdentifierClassifier` machinery (which `CatalogIndex::build` runs
//! automatically over every ingested attribute) was never exercised by
//! any E2b baseline, regardless of how confidently a descriptor proposed
//! `Identifier`. Since E2c makes physical-primitive selection real
//! (R1: primitive is a function of role) rather than advisory, testing
//! "does a canonical `Identifier` descriptor actually compile into a
//! real `IdentifierDictionary`" requires ingesting it as
//! `AttributeValue::Text` so the automatic classifier scan can reach it.
//! `Relationship` is never ingested here either way (R7 never lets it
//! reach `Promoted`).
//!
//! No `commerce_core` production code is modified by this module -- real
//! `commerce_core` types and functions are used exactly as shipped, from
//! this experimental crate only, matching `e2b_ingest.rs`'s own already-
//! established precedent.

use std::collections::HashMap;

use commerce_core::domain::{
    attributes, AttributeValue, BrandId, Catalog, CategoryId, Inventory, Price, Product, ProductId,
    ProductType, ProductTypeId, Variant, VariantId,
};

use crate::e2b_schema::SemanticRole;
use crate::e2b_workload::wands_dataset_path;
use crate::e2c_schema::CanonicalDescriptor;

pub struct CompiledWandsCatalog {
    pub catalog: Catalog,
}

fn typed_value(role: SemanticRole, v: &str) -> Option<AttributeValue> {
    match role {
        SemanticRole::Numeric => v.parse::<f64>().ok().map(AttributeValue::Numeric),
        SemanticRole::Boolean => match v {
            "yes" => Some(AttributeValue::Boolean(true)),
            "no" => Some(AttributeValue::Boolean(false)),
            _ => None,
        },
        SemanticRole::Enum => Some(AttributeValue::Enum(v.to_string())),
        // Extension beyond e2b_ingest (see module doc comment): ingested
        // as Text so CatalogIndex::build's own automatic
        // IdentifierClassifier scan can reach it.
        SemanticRole::Identifier => Some(AttributeValue::Text(v.to_string())),
        SemanticRole::FreeText | SemanticRole::Relationship | SemanticRole::Ignore => None,
    }
}

/// Builds a real `Catalog` from `dataset_cache/wands/product.csv`,
/// materializing exactly the fields `promoted`'s own Enum/Boolean/
/// Numeric/Identifier descriptors cover (by real key) as typed
/// attributes -- structurally identical to `e2b_ingest::build_catalog`
/// except for the Identifier-as-Text extension above, and reading
/// `real_key` directly (every `CanonicalDescriptor` already carries the
/// real key, never an alias, since canonicalization resolves that before
/// producing a `Promoted` outcome).
pub fn build_wands_catalog(promoted: &[CanonicalDescriptor]) -> CompiledWandsCatalog {
    let typed_by_key: HashMap<&str, &CanonicalDescriptor> = promoted
        .iter()
        .filter(|d| {
            matches!(
                d.semantic_role,
                SemanticRole::Enum
                    | SemanticRole::Boolean
                    | SemanticRole::Numeric
                    | SemanticRole::Identifier
            )
        })
        .map(|d| (d.real_key.as_str(), d))
        .collect();
    let typed_keys: std::collections::BTreeSet<&str> = typed_by_key.keys().copied().collect();

    let content = std::fs::read_to_string(wands_dataset_path("product.csv")).expect(
        "read dataset_cache/wands/product.csv -- run scripts/datasets/fetch_wands.sh first",
    );
    let mut lines = content.lines();
    let header = lines.next().expect("product.csv must have a header row");
    let columns: Vec<&str> = header.split('\t').collect();
    let features_idx = columns
        .iter()
        .position(|&c| c == "product_features")
        .unwrap();
    let product_id_idx = columns.iter().position(|&c| c == "product_id").unwrap();
    let product_class_idx = columns.iter().position(|&c| c == "product_class").unwrap();

    let mut product_type_ids: HashMap<String, ProductTypeId> = HashMap::new();
    let mut product_types: Vec<ProductType> = Vec::new();
    let mut catalog_products = Vec::new();
    const UNKNOWN_PRODUCT_TYPE: ProductTypeId = ProductTypeId(0);

    for (ordinal, line) in lines.enumerate() {
        let fields: Vec<&str> = line.split('\t').collect();
        if fields.get(product_id_idx).is_none() {
            continue;
        }
        let product_id = ProductId(ordinal as u64);

        let product_type = fields
            .get(product_class_idx)
            .filter(|s| !s.is_empty())
            .map(|&raw| {
                *product_type_ids.entry(raw.to_string()).or_insert_with(|| {
                    let id = ProductTypeId(product_types.len() as u32 + 1);
                    product_types.push(ProductType {
                        id,
                        name: raw.to_string(),
                    });
                    id
                })
            })
            .unwrap_or(UNKNOWN_PRODUCT_TYPE);

        let mut attrs: Vec<(String, AttributeValue)> = Vec::new();
        if let Some(&features) = fields.get(features_idx) {
            for part in features.split('|') {
                let part = part.trim();
                let Some((k, v)) = part.split_once(':') else {
                    continue;
                };
                let (k, v) = (k.trim(), v.trim());
                if v.is_empty() || !typed_keys.contains(k) {
                    continue;
                }
                let descriptor = typed_by_key[k];
                if let Some(value) = typed_value(descriptor.semantic_role, v) {
                    if !attrs.iter().any(|(name, _)| name == k) {
                        attrs.push((k.to_string(), value));
                    }
                }
            }
        }

        catalog_products.push(Product {
            id: product_id,
            product_type,
            brand: BrandId(0),
            category: CategoryId(0),
            title: String::new(),
            attributes: attrs.into_iter().collect(),
            variants: vec![Variant {
                id: VariantId(ordinal as u64),
                attributes: attributes([]),
                price: Price::usd(0),
                inventory: Inventory::in_stock(1),
            }],
        });
    }

    CompiledWandsCatalog {
        catalog: Catalog {
            products: catalog_products,
        },
    }
}
