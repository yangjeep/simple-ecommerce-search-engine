//! Small, purpose-built extension generator for the one R1 workload case
//! `issue38_e2e3_eval`'s existing generators do not produce:
//! `docs/experiments/ISSUE42_PROTOCOL.md`'s R1 workload row 8, "a single
//! token verified present as Enum, Numeric, Identifier-shaped text, and
//! plain lexical text across the catalog."
//!
//! Per the protocol's dataset-extension discipline this follows the same
//! determinism standard as every `issue38_e2e3_eval` generator -- but
//! unlike those, which sample many products from an RNG, this catalog is
//! a small, fully hand-specified fixture (four products, one per
//! interpretation): there is nothing to sample, so no `ChaCha8Rng` is
//! needed for it to be deterministic. Re-running [`build`] always
//! produces the exact same `Catalog`, byte-for-byte, by construction.

use commerce_core::domain::{
    attributes, AttributeValue, BrandId, Catalog, CategoryId, Inventory, Price, Product, ProductId,
    ProductType, ProductTypeId, Variant, VariantId,
};

/// The one token every product in this fixture shares, interpreted four
/// different ways depending on which product you look at.
pub const SAME_TOKEN: &str = "34";

pub struct SameTokenFourWays {
    pub catalog: Catalog,
    pub product_types: Vec<ProductType>,
    /// Enum `size` = "34" (the apparel-style interpretation).
    pub enum_variant: (ProductId, VariantId),
    /// Numeric `size` = 34.0 (the automotive-style interpretation).
    pub numeric_variant: (ProductId, VariantId),
    /// Text `part_number` = "34" (an identifier-shaped value that must
    /// never be treated as a Numeric or Enum size match).
    pub identifier_variant: (ProductId, VariantId),
    /// No structured attribute holds "34" at all -- the token appears
    /// only in the product's free-text `title`, so only residual lexical
    /// matching (never a typed constraint) can find it.
    pub lexical_only_variant: (ProductId, VariantId),
}

pub fn build() -> SameTokenFourWays {
    let product_types = vec![
        ProductType {
            id: ProductTypeId(1),
            name: "Same-Token Enum Type".to_string(),
        },
        ProductType {
            id: ProductTypeId(2),
            name: "Same-Token Numeric Type".to_string(),
        },
        ProductType {
            id: ProductTypeId(3),
            name: "Same-Token Identifier Type".to_string(),
        },
        ProductType {
            id: ProductTypeId(4),
            name: "Same-Token Lexical-Only Type".to_string(),
        },
    ];

    let enum_product = Product {
        id: ProductId(1),
        product_type: ProductTypeId(1),
        brand: BrandId(1),
        category: CategoryId(1),
        title: "Same-Token Enum Product".to_string(),
        attributes: attributes([]),
        variants: vec![Variant {
            id: VariantId(10),
            attributes: attributes([("size", AttributeValue::Enum(SAME_TOKEN.to_string()))]),
            price: Price::usd(2999),
            inventory: Inventory::in_stock(5),
        }],
    };

    let numeric_product = Product {
        id: ProductId(2),
        product_type: ProductTypeId(2),
        brand: BrandId(1),
        category: CategoryId(1),
        title: "Same-Token Numeric Product".to_string(),
        attributes: attributes([(
            "size",
            AttributeValue::Numeric(SAME_TOKEN.parse().expect("SAME_TOKEN is numeric-looking")),
        )]),
        variants: vec![Variant {
            id: VariantId(20),
            attributes: attributes([]),
            price: Price::usd(1999),
            inventory: Inventory::in_stock(5),
        }],
    };

    let identifier_product = Product {
        id: ProductId(3),
        product_type: ProductTypeId(3),
        brand: BrandId(1),
        category: CategoryId(1),
        title: "Same-Token Identifier Product".to_string(),
        attributes: attributes([]),
        variants: vec![Variant {
            id: VariantId(30),
            attributes: attributes([("part_number", AttributeValue::Text(SAME_TOKEN.to_string()))]),
            price: Price::usd(999),
            inventory: Inventory::in_stock(5),
        }],
    };

    let lexical_only_product = Product {
        id: ProductId(4),
        product_type: ProductTypeId(4),
        brand: BrandId(1),
        category: CategoryId(1),
        title: format!("Model {SAME_TOKEN} Anniversary Edition"),
        attributes: attributes([]),
        variants: vec![Variant {
            id: VariantId(40),
            attributes: attributes([]),
            price: Price::usd(4999),
            inventory: Inventory::in_stock(5),
        }],
    };

    SameTokenFourWays {
        catalog: Catalog {
            products: vec![
                enum_product,
                numeric_product,
                identifier_product,
                lexical_only_product,
            ],
        },
        product_types,
        enum_variant: (ProductId(1), VariantId(10)),
        numeric_variant: (ProductId(2), VariantId(20)),
        identifier_variant: (ProductId(3), VariantId(30)),
        lexical_only_variant: (ProductId(4), VariantId(40)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_is_deterministic() {
        let a = build();
        let b = build();
        assert_eq!(a.catalog.products, b.catalog.products);
    }

    #[test]
    fn every_interpretation_is_actually_present_in_the_catalog() {
        let fixture = build();
        let by_id = |pid: ProductId| {
            fixture
                .catalog
                .products
                .iter()
                .find(|p| p.id == pid)
                .expect("product must exist")
        };

        let enum_product = by_id(fixture.enum_variant.0);
        assert_eq!(
            enum_product.variants[0].attributes.get("size"),
            Some(&AttributeValue::Enum(SAME_TOKEN.to_string()))
        );

        let numeric_product = by_id(fixture.numeric_variant.0);
        assert_eq!(
            numeric_product.attributes.get("size"),
            Some(&AttributeValue::Numeric(34.0))
        );

        let identifier_product = by_id(fixture.identifier_variant.0);
        assert_eq!(
            identifier_product.variants[0].attributes.get("part_number"),
            Some(&AttributeValue::Text(SAME_TOKEN.to_string()))
        );

        let lexical_only_product = by_id(fixture.lexical_only_variant.0);
        assert!(lexical_only_product.title.contains(SAME_TOKEN));
        assert!(lexical_only_product.attributes.is_empty());
        assert!(lexical_only_product.variants[0].attributes.is_empty());
    }
}
