//! Deterministic, versioned test fixtures (Gate 0 requirement). No randomness,
//! no file I/O: every fixture is a plain Rust function so it type-checks and
//! stays in lockstep with the domain model it exercises.

use crate::domain::{
    attributes, AttributeValue, Brand, BrandId, Catalog, Category, CategoryId, Currency, Inventory,
    Price, Product, ProductId, ProductType, ProductTypeId, Variant, VariantId,
};

/// The canonical Gate 1 correctness fixture: one product with two variants
/// whose color/size combinations are deliberately *not* interchangeable
/// (black is only sold in size 8, red only in size 9). Any matcher that
/// flattens variant attributes into a single per-product bag will wrongly
/// satisfy "black size 9" against this fixture; a correct matcher must not.
pub fn variant_safety_catalog() -> Catalog {
    let product = Product {
        id: ProductId(1),
        product_type: ProductTypeId(1),
        brand: BrandId(1),
        category: CategoryId(1),
        title: "Nike Air Zoom Runner".to_string(),
        attributes: attributes([
            ("waterproof", AttributeValue::Boolean(true)),
            ("material", AttributeValue::Text("mesh knit".to_string())),
            (
                "features",
                AttributeValue::MultiEnum(vec!["cushioned".to_string(), "breathable".to_string()]),
            ),
        ]),
        variants: vec![
            Variant {
                id: VariantId(101),
                attributes: attributes([
                    ("color", AttributeValue::Enum("Black".to_string())),
                    ("size", AttributeValue::Numeric(8.0)),
                ]),
                price: Price::usd(12_999),
                inventory: Inventory::in_stock(12),
            },
            Variant {
                id: VariantId(102),
                attributes: attributes([
                    ("color", AttributeValue::Enum("Red".to_string())),
                    ("size", AttributeValue::Numeric(9.0)),
                ]),
                price: Price::usd(12_999),
                inventory: Inventory::in_stock(5),
            },
        ],
    };
    Catalog {
        products: vec![product],
    }
}

pub fn nike_brand() -> Brand {
    Brand {
        id: BrandId(1),
        name: "Nike".to_string(),
    }
}

pub fn running_shoes_type() -> ProductType {
    ProductType {
        id: ProductTypeId(1),
        name: "Running Shoes".to_string(),
    }
}

pub fn footwear_category() -> Category {
    Category {
        id: CategoryId(1),
        name: "Footwear".to_string(),
    }
}

pub fn usd(cents: i64) -> Price {
    Price {
        amount_cents: cents,
        currency: Currency::Usd,
    }
}
