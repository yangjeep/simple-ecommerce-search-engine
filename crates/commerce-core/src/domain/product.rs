use super::attribute::AttributeMap;
use super::ids::{BrandId, CategoryId, ProductId, ProductTypeId};
use super::variant::Variant;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Brand {
    pub id: BrandId,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Category {
    pub id: CategoryId,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProductType {
    pub id: ProductTypeId,
    pub name: String,
}

/// A sellable product. `attributes` holds facts shared by every variant
/// (e.g. waterproof, material); attributes that vary per-SKU (color, size)
/// belong on the [`Variant`] instead. See [`super::catalog::effective_attributes`]
/// for how the two are combined without leaking across variants.
#[derive(Debug, Clone, PartialEq)]
pub struct Product {
    pub id: ProductId,
    pub product_type: ProductTypeId,
    pub brand: BrandId,
    pub category: CategoryId,
    pub title: String,
    pub attributes: AttributeMap,
    pub variants: Vec<Variant>,
}
