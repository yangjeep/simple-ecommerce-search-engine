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
/// Wrapped as a versioned [`crate::ir::SemanticContext`] by
/// [`shoe_semantic_context`] for Gate 4; on its own this is still just the
/// Gate 2 compiler prototype's raw lexicon, hand-curated rather than
/// compiled from catalog profiling (Gate 6) or promoted via replay
/// evidence (Gate 5).
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
    // Aliases resolving to the same canonical ProductTypeId (Gate 4:
    // "aliases/canonical IDs"). Confidence is lower than the canonical
    // phrase because these are informal synonyms, not the catalog's own
    // vocabulary; a single candidate still resolves deterministically
    // (see ir::query::compile's doc comment on how ambiguity is decided).
    lex.insert(
        "sneakers",
        vec![Candidate::constraint(
            ResolvedConstraint::Structural(StructuralConstraint::ProductType(ProductTypeId(1))),
            0.9,
        )],
    );
    lex.insert(
        "trainers",
        vec![Candidate::constraint(
            ResolvedConstraint::Structural(StructuralConstraint::ProductType(ProductTypeId(1))),
            0.9,
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

/// The Gate 2/4 lexicon wrapped as a versioned, compiled semantic context.
/// Version 1: hand-curated (not derived from catalog profiling — that is
/// Gate 6's cold-start job).
pub fn shoe_semantic_context() -> crate::ir::SemanticContext {
    crate::ir::SemanticContext::new(
        1,
        "hand-curated shoe lexicon (Gate 2/4 prototype)",
        shoe_lexicon(),
    )
}

/// A hand-authored, deterministic stand-in for a "representative ecommerce
/// query set" (Gate 4: "measure what fraction ... resolves without model
/// inference"). Every query uses only vocabulary [`shoe_lexicon`] does or
/// deliberately does not know about, split into three known-outcome
/// groups so the coverage measurement in `tests/coverage.rs` can assert
/// exact expected counts rather than an unverified fraction:
///
/// - queries 0-11 (12): fully resolvable (every token maps to a
///   constraint/preference/stopword, or is covered by the "sneakers"/
///   "trainers" aliases resolving to the same canonical product type);
/// - queries 12-13 (2): contain "leather", the lexicon's deliberately
///   ambiguous term (see [`shoe_lexicon`]);
/// - queries 14-19 (6): contain at least one token absent from the
///   lexicon entirely (an unmodeled brand, an unmodeled color, informal
///   phrasing), so they must fall back to `residual_lexical`.
pub const REPRESENTATIVE_QUERY_SET: &[&str] = &[
    // Fully resolvable.
    "black Nike waterproof running shoes size 9 under $150",
    "red running shoes size 8",
    "Nike running shoes under $100",
    "waterproof running shoes",
    "black running shoes over $50",
    "size 9 running shoes",
    "Nike black running shoes",
    "cushioned running shoes",
    "breathable waterproof running shoes",
    "red Nike sneakers under $200",
    "black trainers",
    "running shoes for the waterproof black size 9 nike",
    // Ambiguous ("leather").
    "leather running shoes",
    "black leather trainers",
    // Residual (out-of-vocabulary brand/color/informal terms).
    "Adidas running shoes",
    "blue running shoes",
    "New Balance waterproof shoes",
    "vegan running shoes",
    "wide fit running shoes",
    "trail running shoes size 10",
];
