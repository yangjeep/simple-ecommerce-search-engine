use super::attribute::AttributeMap;
use super::constraint::{all_match, Constraint};
use super::ids::{ProductId, VariantId};
use super::product::Product;
use super::variant::Variant;

#[derive(Debug, Clone, Default)]
pub struct Catalog {
    pub products: Vec<Product>,
}

/// Combine a product's shared attributes with one variant's own attributes
/// into a single map representing *that variant alone*. Variant-level keys
/// override product-level keys. This is the only place attribute scopes are
/// merged; a query must never be evaluated against attributes pulled from
/// more than one variant of the same product.
pub fn effective_attributes(product: &Product, variant: &Variant) -> AttributeMap {
    let mut merged = product.attributes.clone();
    for (key, value) in &variant.attributes {
        merged.insert(key.clone(), value.clone());
    }
    merged
}

impl Catalog {
    /// Returns every (product, variant) pair whose *combined, per-variant*
    /// attributes satisfy every constraint. Because each variant is
    /// evaluated independently via [`effective_attributes`], a constraint
    /// set can never be satisfied by attribute values drawn from two
    /// different variants of the same product.
    pub fn search(&self, constraints: &[Constraint]) -> Vec<(ProductId, VariantId)> {
        let mut matches = Vec::new();
        for product in &self.products {
            for variant in &product.variants {
                let attrs = effective_attributes(product, variant);
                if all_match(constraints, &attrs) {
                    matches.push((product.id, variant.id));
                }
            }
        }
        matches
    }
}
