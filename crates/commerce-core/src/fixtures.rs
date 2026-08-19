//! Deterministic, versioned test fixtures (Gate 0 requirement). No randomness,
//! no file I/O: every fixture is a plain Rust function so it type-checks and
//! stays in lockstep with the domain model it exercises.

use crate::domain::{
    attributes, AttributeValue, Brand, BrandId, Catalog, Category, CategoryId, Constraint,
    Currency, Inventory, Price, Product, ProductId, ProductType, ProductTypeId, Variant, VariantId,
};
use crate::ir::{Candidate, ResolvedConstraint, SemanticLexicon, StructuralConstraint};

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

/// The Gate 2 representative query from Issue #2.
pub const REPRESENTATIVE_QUERY: &str = "black Nike waterproof running shoes size 9 under $150";

/// A small, deterministic semantic lexicon for the shoe catalog fixtures.
/// This is the Gate 2 compiler prototype's semantic context, not the
/// versioned/compiled FIB Gate 4 requires (no version, no promotion gate).
pub fn shoe_lexicon() -> SemanticLexicon {
    let mut lex = SemanticLexicon::new();
    lex.insert(
        "nike",
        vec![Candidate::constraint(
            ResolvedConstraint::Structural(StructuralConstraint::Brand(BrandId(1))),
            1.0,
        )],
    );
    lex.insert(
        "black",
        vec![Candidate::constraint(
            ResolvedConstraint::Attribute(Constraint::Enum {
                attribute: "color".to_string(),
                value: "Black".to_string(),
            }),
            1.0,
        )],
    );
    lex.insert(
        "red",
        vec![Candidate::constraint(
            ResolvedConstraint::Attribute(Constraint::Enum {
                attribute: "color".to_string(),
                value: "Red".to_string(),
            }),
            1.0,
        )],
    );
    lex.insert(
        "waterproof",
        vec![Candidate::constraint(
            ResolvedConstraint::Attribute(Constraint::Boolean {
                attribute: "waterproof".to_string(),
                value: true,
            }),
            1.0,
        )],
    );
    lex.insert(
        "running shoes",
        vec![Candidate::constraint(
            ResolvedConstraint::Structural(StructuralConstraint::ProductType(ProductTypeId(1))),
            1.0,
        )],
    );
    lex.insert(
        "cushioned",
        vec![Candidate::preference(
            crate::ir::Preference::Boost {
                attribute: "features".to_string(),
                value: "cushioned".to_string(),
                weight: 0.5,
            },
            1.0,
        )],
    );
    lex.insert(
        "breathable",
        vec![Candidate::preference(
            crate::ir::Preference::Boost {
                attribute: "features".to_string(),
                value: "breathable".to_string(),
                weight: 0.5,
            },
            1.0,
        )],
    );
    // Deliberately ambiguous: "leather" could describe the shoe's material
    // or be a standalone feature tag. The lexicon offers both readings at
    // equal confidence instead of guessing.
    lex.insert(
        "leather",
        vec![
            Candidate::constraint(
                ResolvedConstraint::Attribute(Constraint::Text {
                    attribute: "material".to_string(),
                    contains: "leather".to_string(),
                }),
                0.5,
            ),
            Candidate::constraint(
                ResolvedConstraint::Attribute(Constraint::MultiEnumContains {
                    attribute: "features".to_string(),
                    value: "leather-trim".to_string(),
                }),
                0.5,
            ),
        ],
    );
    lex
}

/// A catalog containing a product that genuinely satisfies
/// [`REPRESENTATIVE_QUERY`], to prove positive end-to-end compilation +
/// execution (as opposed to [`variant_safety_catalog`], which proves the
/// query correctly matches *nothing* there despite each individual clause
/// matching a different variant).
pub fn representative_query_catalog() -> Catalog {
    let product = Product {
        id: ProductId(2),
        product_type: ProductTypeId(1),
        brand: BrandId(1),
        category: CategoryId(1),
        title: "Nike Air Zoom Runner Waterproof".to_string(),
        attributes: attributes([
            ("waterproof", AttributeValue::Boolean(true)),
            (
                "material",
                AttributeValue::Text("synthetic mesh".to_string()),
            ),
            (
                "features",
                AttributeValue::MultiEnum(vec!["cushioned".to_string()]),
            ),
        ]),
        variants: vec![Variant {
            id: VariantId(201),
            attributes: attributes([
                ("color", AttributeValue::Enum("Black".to_string())),
                ("size", AttributeValue::Numeric(9.0)),
            ]),
            price: Price::usd(13_999),
            inventory: Inventory::in_stock(7),
        }],
    };
    Catalog {
        products: vec![product],
    }
}
