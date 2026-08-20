use super::attribute::AttributeMap;
use super::ids::VariantId;
use super::inventory::Inventory;
use super::price::Price;

/// A single purchasable SKU within a [`super::product::Product`]. Attributes
/// here are variant-specific (color, size, ...) and must never be evaluated
/// against a sibling variant's attributes.
#[derive(Debug, Clone, PartialEq)]
pub struct Variant {
    pub id: VariantId,
    pub attributes: AttributeMap,
    pub price: Price,
    pub inventory: Inventory,
}
