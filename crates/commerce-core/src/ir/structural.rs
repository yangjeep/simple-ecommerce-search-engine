use crate::domain::{BrandId, CategoryId, Constraint, Product, ProductTypeId, Variant};

/// Constraints over fields that are already typed on [`Product`]/[`Variant`]
/// (brand, product type, category, price) rather than the generic attribute
/// map. Kept separate from [`Constraint`] because these fields are
/// structural: there is never any question of which "attribute name" a
/// brand or a price refers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StructuralConstraint {
    Brand(BrandId),
    ProductType(ProductTypeId),
    Category(CategoryId),
    PriceUnderCents(i64),
    PriceOverCents(i64),
}

impl StructuralConstraint {
    pub fn matches(&self, product: &Product, variant: &Variant) -> bool {
        match self {
            StructuralConstraint::Brand(id) => product.brand == *id,
            StructuralConstraint::ProductType(id) => product.product_type == *id,
            StructuralConstraint::Category(id) => product.category == *id,
            StructuralConstraint::PriceUnderCents(cents) => variant.price.amount_cents < *cents,
            StructuralConstraint::PriceOverCents(cents) => variant.price.amount_cents > *cents,
        }
    }
}

/// A single resolved term of a compiled query: either an attribute-level
/// [`Constraint`] or a [`StructuralConstraint`]. Both are evaluated against
/// one variant's combined view, so mixing the two kinds is still
/// variant-safe (see `domain::catalog::effective_attributes`).
#[derive(Debug, Clone, PartialEq)]
pub enum ResolvedConstraint {
    Attribute(Constraint),
    Structural(StructuralConstraint),
}
