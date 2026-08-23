//! E2b's end-to-end relevance check (`docs/experiments/ISSUE42_PROTOCOL.md`'s
//! E2b amendment 1, "Metrics" section): builds a real `commerce_core::domain::Catalog`
//! from WANDS's real `product.csv`, materializing ONLY the fields a given
//! baseline's own accepted [`Descriptor`]s cover as typed structural
//! attributes -- so each baseline (statistics-only / LLM+validator /
//! oracle) gets its own, differently-typed `Catalog`, exactly reflecting
//! what it actually decided to accept, never a shared "everything
//! typed" catalog that would hide a baseline's real abstentions.
//!
//! **A disclosed scoping decision, not a silent simplification**: this
//! does not run WANDS's 480 real queries through `commerce_core::ir::query::compile`
//! (which would require a full `cold_start::CatalogProfile`/`compile_lexicon`
//! pass over 7,961 real feed keys, most of them outside this
//! experiment's 36-key sample and outside its own scope). Instead, each
//! real query's text is matched, case-insensitively, as a literal
//! substring against every accepted Enum descriptor's own real known
//! values -- producing a real, structural-only
//! `commerce_core::domain::Constraint` list per query, run through
//! `commerce_core::domain::Catalog::search` (real, unmodified, exact
//! structural matching), never a fabricated or hand-picked candidate
//! set. This exercises real structural retrieval, not the full query
//! IR pipeline -- disclosed as a real, bounded scope decision, not
//! presented as an end-to-end test of `commerce_core::ir::query::compile`
//! itself.

use std::collections::HashMap;

use commerce_core::domain::{
    attributes, AttributeValue, Catalog, Constraint, Inventory, Price, Product, ProductId,
    ProductType, ProductTypeId, Variant, VariantId,
};

use crate::e2b_schema::{Descriptor, SemanticRole};
use crate::e2b_workload::wands_dataset_path;

pub struct IngestedWandsCatalog {
    pub catalog: Catalog,
    pub wands_id_to_product_id: HashMap<String, ProductId>,
    /// The accepted key -> real distinct values actually observed for it
    /// in this ingestion, used to build each query's naive
    /// keyword-matched constraint list (see this module's own doc
    /// comment for why this is a disclosed, scoped substitute for full
    /// `ir::query::compile`).
    pub enum_like_values: HashMap<String, Vec<String>>,
}

fn accepted_typed_keys(descriptors: &[Descriptor]) -> Vec<&Descriptor> {
    descriptors
        .iter()
        .filter(|d| {
            !d.abstain
                && matches!(
                    d.semantic_role,
                    SemanticRole::Enum | SemanticRole::Boolean | SemanticRole::Numeric
                )
        })
        .collect()
}

/// The real CSV column name a descriptor describes. Oracle/statistics-only
/// descriptors are always authored with `key` already equal to the real
/// feed key (`real_key: None`); an LLM-proposal descriptor from the
/// anonymized/noisy perturbation configs carries the alias it was shown
/// (e.g. `feature_5`) in `key` and only the harness-resolved true column
/// name in `real_key` -- matching product.csv's own columns must always
/// go through this, never `d.key` directly, or every anonymized/noisy
/// proposal silently fails to materialize (looks like a real field with
/// zero coverage, not a lookup bug).
fn real_key_of(d: &Descriptor) -> &str {
    d.real_key.as_deref().unwrap_or(&d.key)
}

/// Builds a real `Catalog` from `dataset_cache/wands/product.csv`,
/// materializing exactly the fields `accepted`'s own Enum/Boolean/Numeric
/// descriptors cover (by real key) as typed attributes on each product.
/// Every WANDS record becomes exactly one `Product` with exactly one
/// `Variant` -- the same degenerate mapping `phase6a_eval::catalog::build_catalog`
/// already uses for WANDS, since it has no real variant-grouping
/// concept. `product_class` interns to `ProductTypeId` (same field
/// `phase6a_eval` already uses for this purpose); `Price::usd(0)`/
/// `Inventory::in_stock(1)` sentinels, since WANDS has no real
/// price/availability data (same disclosed limitation
/// `phase6a_eval::catalog`'s own doc comment already records).
pub fn build_catalog(accepted: &[Descriptor]) -> IngestedWandsCatalog {
    let typed = accepted_typed_keys(accepted);
    let typed_keys: std::collections::BTreeSet<&str> =
        typed.iter().map(|d| real_key_of(d)).collect();
    let typed_by_key: HashMap<&str, &Descriptor> =
        typed.iter().map(|d| (real_key_of(d), *d)).collect();

    let content = std::fs::read_to_string(wands_dataset_path("product.csv"))
        .expect("read dataset_cache/wands/product.csv");
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
    let mut wands_id_to_product_id = HashMap::new();
    let mut catalog_products = Vec::new();
    let mut enum_like_values: HashMap<String, std::collections::BTreeSet<String>> = HashMap::new();

    const UNKNOWN_PRODUCT_TYPE: ProductTypeId = ProductTypeId(0);

    for (ordinal, line) in lines.enumerate() {
        let fields: Vec<&str> = line.split('\t').collect();
        let Some(&wands_id) = fields.get(product_id_idx) else {
            continue;
        };
        let product_id = ProductId(ordinal as u64);
        wands_id_to_product_id.insert(wands_id.to_string(), product_id);

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
                let value = match descriptor.semantic_role {
                    SemanticRole::Numeric => v.parse::<f64>().ok().map(AttributeValue::Numeric),
                    SemanticRole::Boolean => match v {
                        "yes" => Some(AttributeValue::Boolean(true)),
                        "no" => Some(AttributeValue::Boolean(false)),
                        _ => None,
                    },
                    SemanticRole::Enum => Some(AttributeValue::Enum(v.to_string())),
                    _ => None,
                };
                if let Some(value) = value {
                    if descriptor.semantic_role == SemanticRole::Enum {
                        enum_like_values
                            .entry(k.to_string())
                            .or_default()
                            .insert(v.to_string());
                    }
                    // A key can repeat within one product's own blob
                    // (e.g. two dimension entries); keep the first real
                    // value seen, never silently overwrite it with a
                    // later, possibly-unrelated one.
                    if !attrs.iter().any(|(name, _)| name == k) {
                        attrs.push((k.to_string(), value));
                    }
                }
            }
        }

        let product = Product {
            id: product_id,
            product_type,
            brand: commerce_core::domain::BrandId(0),
            category: commerce_core::domain::CategoryId(0),
            title: String::new(),
            attributes: attrs.into_iter().collect(),
            variants: vec![Variant {
                id: VariantId(ordinal as u64),
                attributes: attributes([]),
                price: Price::usd(0),
                inventory: Inventory::in_stock(1),
            }],
        };
        catalog_products.push(product);
    }

    IngestedWandsCatalog {
        catalog: Catalog {
            products: catalog_products,
        },
        wands_id_to_product_id,
        enum_like_values: enum_like_values
            .into_iter()
            .map(|(k, v)| (k, v.into_iter().collect()))
            .collect(),
    }
}

/// A real, disclosed, scoped substitute for `commerce_core::ir::query::compile`
/// (see this module's own doc comment): matches `query_text` (lowercased)
/// as a literal substring against every accepted Enum descriptor's own
/// real known values. Boolean/Numeric descriptors are not matched here
/// (WANDS's own 480 real queries are overwhelmingly noun-phrase product
/// searches, e.g. "salon chair", not attribute-value phrasings a naive
/// substring match against "yes"/"no" or a bare number could safely
/// interpret) -- a real, disclosed limitation of this scoped check, not
/// silently pretended away.
pub fn naive_constraints_for_query(
    query_text: &str,
    enum_like_values: &HashMap<String, Vec<String>>,
) -> Vec<Constraint> {
    let query_lower = query_text.to_lowercase();
    let mut out = Vec::new();
    for (attribute, values) in enum_like_values {
        for value in values {
            if value.len() >= 3 && query_lower.contains(value.as_str()) {
                out.push(Constraint::Enum {
                    attribute: attribute.clone(),
                    value: value.clone(),
                });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::e2b_schema::{Operator, PhysicalPrimitive, Scope, Significance, ValueType};

    fn enum_descriptor(key: &str) -> Descriptor {
        Descriptor {
            key: key.to_string(),
            real_key: None,
            semantic_role: SemanticRole::Enum,
            value_type: ValueType::String,
            scope: Scope::Product,
            supported_operators: vec![Operator::Eq],
            aliases: vec![],
            relationship_semantics: None,
            retrieval_significance: Significance::RetrievalSignificant,
            candidate_physical_primitive: PhysicalPrimitive::BitmapEnum,
            confidence: 0.9,
            evidence: "test".to_string(),
            abstain: false,
        }
    }

    #[test]
    fn build_catalog_materializes_only_accepted_enum_fields() {
        let accepted = vec![enum_descriptor("color")];
        let ingested = build_catalog(&accepted);
        assert!(ingested.catalog.products.len() > 40_000);
        assert!(ingested.enum_like_values.contains_key("color"));
        assert!(!ingested.enum_like_values.contains_key("style"));
        // No product should carry a "style" attribute at all, since it
        // was not in `accepted`.
        assert!(ingested
            .catalog
            .products
            .iter()
            .all(|p| !p.attributes.contains_key("style")));
    }

    #[test]
    fn naive_constraints_finds_a_real_color_value_in_a_real_query() {
        let accepted = vec![enum_descriptor("color")];
        let ingested = build_catalog(&accepted);
        let constraints =
            naive_constraints_for_query("blue accent chair", &ingested.enum_like_values);
        assert!(constraints
            .iter()
            .any(|c| matches!(c, Constraint::Enum { attribute, value } if attribute == "color" && value == "blue")));
    }

    /// Regression test for a real bug: `build_catalog` used to match the
    /// real CSV feature key against a descriptor's `key` field directly,
    /// which is correct for the oracle/statistics-only baselines (their
    /// `key` is always the true feed column) but wrong for an
    /// LLM-proposal descriptor from the anonymized/noisy perturbation
    /// configs, whose `key` holds the alias it was shown (e.g.
    /// `feature_5`) and only `real_key` holds the true column name. The
    /// bug silently produced a `Catalog` with zero matching attributes
    /// for every such descriptor -- not an error, just empty coverage,
    /// which made the E2b eval binary's "LLM + validator" end-to-end
    /// baseline score 0 queries even though `color` really is present in
    /// the accepted set.
    #[test]
    fn build_catalog_resolves_real_key_when_key_is_an_alias() {
        let aliased = Descriptor {
            key: "feature_1".to_string(),
            real_key: Some("color".to_string()),
            ..enum_descriptor("feature_1")
        };
        let ingested = build_catalog(&[aliased]);
        assert!(
            ingested.enum_like_values.contains_key("color"),
            "must materialize under the real feed key, not the alias shown to the proposing pass"
        );
        assert!(!ingested.enum_like_values.contains_key("feature_1"));
        assert!(ingested
            .catalog
            .products
            .iter()
            .any(|p| p.attributes.contains_key("color")));
    }
}
