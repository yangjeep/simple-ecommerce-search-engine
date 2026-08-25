use crate::domain::{BrandId, CategoryId, Constraint, Product, ProductTypeId, Variant};

/// Constraints over fields that are already typed on [`Product`]/[`Variant`]
/// (brand, product type, category, price) rather than the generic attribute
/// map. Kept separate from [`Constraint`] because these fields are
/// structural: there is never any question of which "attribute name" a
/// brand or a price refers to.
///
/// Not [`Copy`] since [`StructuralConstraint::BrandAny`] owns a `Vec`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StructuralConstraint {
    Brand(BrandId),
    /// Issue #6 P1-B (`cold_start::profile::compile_lexicon_with_alias_enforcement`):
    /// matches if `product.brand` is any of the listed ids, rather than
    /// exactly one. Represents a *deterministic* alias group — brand
    /// strings that already passed the same trust gate `Brand` uses and
    /// collapsed to the same identity key after corporate-suffix/
    /// punctuation normalization (`cold_start::alias::alias_key`), e.g.
    /// "Nike", "Nike Inc", "Nike, Inc." A single-element list behaves
    /// identically to `Brand`, so this is a strict generalization, not a
    /// new enforcement tier by itself.
    BrandAny(Vec<BrandId>),
    ProductType(ProductTypeId),
    /// Issue #55 (`docs/experiments/ISSUE55_PRODUCT_TYPE_HYPONYM_PROTOCOL.md`):
    /// matches if `product.product_type` is any of the listed ids, exactly
    /// mirroring [`StructuralConstraint::BrandAny`]'s own shape and
    /// safety argument -- a *deterministic* whole-word hyponym group
    /// (e.g. "beds" admitting the more specific "kids beds"), computed at
    /// lexicon-compile time from real catalog vocabulary, never guessed
    /// or learned online. A single-element list behaves identically to
    /// [`StructuralConstraint::ProductType`].
    ProductTypeAny(Vec<ProductTypeId>),
    Category(CategoryId),
    PriceUnderCents(i64),
    PriceOverCents(i64),
}

impl StructuralConstraint {
    pub fn matches(&self, product: &Product, variant: &Variant) -> bool {
        match self {
            StructuralConstraint::Brand(id) => product.brand == *id,
            StructuralConstraint::BrandAny(ids) => ids.contains(&product.brand),
            StructuralConstraint::ProductType(id) => product.product_type == *id,
            StructuralConstraint::ProductTypeAny(ids) => ids.contains(&product.product_type),
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
