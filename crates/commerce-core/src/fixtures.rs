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

/// Issue #42 R2 (`docs/experiments/ISSUE42_LOG.md#i42-r2`) residual-lexical-
/// policy fixture, used by `plan`'s own regression tests
/// (`docs/adr/0012-residual-lexical-policy.md`). Ported directly from the
/// already-validated `issue42-eval::r2_workload::build()` fixture shape —
/// not a dependency, since `commerce-core` cannot depend on an eval crate.
///
/// Two real product types under test (Sofas, Boots) plus two decoy types
/// (Coffee Tables, Bookshelves) whose titles alone give the benign
/// marketing word "furniture" a real, catalog-observable presence under
/// two *other* product types — the cross-type-breadth signal
/// [`crate::plan::ResidualPolicy`] needs to classify it
/// [`crate::plan::ResidualClass::Preferred`]. "velvet" appears in exactly
/// one Sofas product's title and nowhere else — observed under only one
/// product type, below the cross-type-breadth threshold, so it classifies
/// [`crate::plan::ResidualClass::Required`] regardless of which product
/// type a query names (the genuinely disqualifying, adversarial case).
/// Every product shares one brand ("Acme"), so a `Brand` constraint alone
/// matches the whole catalog (unselective — routes `Punt`, not `Hybrid`)
/// without ever corroborating a `ProductType`. 50 filler products pad the
/// catalog so Sofas/Boots' own selectivity clears `plan::plan`'s
/// `Hybrid`-routing threshold (`<= 0.05`) with real margin — with only the
/// 6 hand-authored products, Sofas/Boots' 2-product share (33%) would stay
/// far above that threshold and every row would route to `Punt` instead
/// (see `issue42-eval::r2_workload`'s own doc comment for the full
/// Punt-vs-Hybrid routing trap this fixture is built to avoid).
pub struct ResidualPolicyCatalog {
    pub catalog: Catalog,
    pub product_types: Vec<ProductType>,
    pub brands: Vec<Brand>,
    pub categories: Vec<Category>,
    pub sofas: ProductTypeId,
    pub boots: ProductTypeId,
}

pub fn residual_policy_catalog() -> ResidualPolicyCatalog {
    const SOFAS: u32 = 1;
    const BOOTS: u32 = 2;
    const COFFEE_TABLES: u32 = 3;
    const BOOKSHELVES: u32 = 4;

    let product_types = vec![
        ProductType {
            id: ProductTypeId(SOFAS),
            name: "Sofas".to_string(),
        },
        ProductType {
            id: ProductTypeId(BOOTS),
            name: "Boots".to_string(),
        },
        ProductType {
            id: ProductTypeId(COFFEE_TABLES),
            name: "Coffee Tables".to_string(),
        },
        ProductType {
            id: ProductTypeId(BOOKSHELVES),
            name: "Bookshelves".to_string(),
        },
    ];
    let brands = vec![Brand {
        id: BrandId(1),
        name: "Acme".to_string(),
    }];
    let categories = vec![Category {
        id: CategoryId(1),
        name: "Homeware".to_string(),
    }];

    let make = |id: u64,
                product_type: u32,
                title: &str,
                product_attrs: Vec<(&'static str, AttributeValue)>,
                variant_id: u64|
     -> Product {
        Product {
            id: ProductId(id),
            product_type: ProductTypeId(product_type),
            brand: BrandId(1),
            category: CategoryId(1),
            title: title.to_string(),
            attributes: attributes(product_attrs),
            variants: vec![Variant {
                id: VariantId(variant_id),
                attributes: attributes([]),
                price: Price::usd(2999),
                inventory: Inventory::in_stock(5),
            }],
        }
    };

    // Sofas: real Enum color/material values, deliberately NOT used as
    // residual-policy test words (they auto-resolve to hard constraints,
    // exactly what a compound-constraint regression test needs). P2's
    // title contains "velvet", which appears nowhere else in this fixture
    // -- the adversarial, single-type-exclusive signal. Second correction
    // round (a fresh adversarial review of this production merge): the
    // original version of this fixture gave every product `attributes([])`,
    // so `commerce_core::ir::compile` could never produce a *compound*
    // `ProductType(Sofas) AND Enum(color=...)` constraint from it at all --
    // exactly the shape Issue #42 R2's own second correction round needed
    // to catch a real false positive in `ResidualPolicy::classify` (see
    // `docs/experiments/ISSUE42_LOG.md#i42-r2`). P1/P2 now carry the same
    // real `color`/`material` values `issue42-eval::r2_workload::build()`
    // uses, so `commerce-core`'s own test suite can reproduce that exact
    // scenario directly (see `plan::tests::execute_planned_does_not_recover_the_wrong_variant_for_a_compound_constraint_query`),
    // not merely rely on `classify`'s signature no longer accepting a
    // `ProductTypeId` parameter as a structural guarantee against a
    // *different* future regression in the same area.
    let p1 = make(
        1,
        SOFAS,
        "Blue Leather Sofa",
        vec![
            ("color", AttributeValue::Enum("Blue".to_string())),
            ("material", AttributeValue::Enum("Leather".to_string())),
        ],
        10,
    );
    let p2 = make(
        2,
        SOFAS,
        "Purple Velvet Sofa",
        vec![("color", AttributeValue::Enum("Purple".to_string()))],
        20,
    );
    let p3 = make(3, BOOTS, "Rugged Hiking Boots", vec![], 30);
    let p4 = make(4, BOOTS, "Steel Toe Work Boots", vec![], 40);

    // Coffee Tables and Bookshelves exist solely to give "furniture" a
    // real, catalog-observable, cross-type (but never Sofas/Boots) title
    // presence.
    let p5 = make(
        5,
        COFFEE_TABLES,
        "Oak Coffee Table - Fine Furniture Piece",
        vec![],
        50,
    );
    let p6 = make(
        6,
        BOOKSHELVES,
        "Walnut Bookshelf - Solid Furniture Design",
        vec![],
        60,
    );

    // Filler: pads Sofas/Boots' own selectivity below plan::plan's
    // Hybrid-routing threshold; plain, distinct titles unrelated to any
    // test word.
    let filler: Vec<Product> = (0..50)
        .map(|i| {
            let product_type = if i % 2 == 0 {
                COFFEE_TABLES
            } else {
                BOOKSHELVES
            };
            make(
                100 + i,
                product_type,
                &format!("Assorted Home Good {i}"),
                vec![],
                1000 + i,
            )
        })
        .collect();

    let mut products = vec![p1, p2, p3, p4, p5, p6];
    products.extend(filler);

    ResidualPolicyCatalog {
        catalog: Catalog { products },
        product_types,
        brands,
        categories,
        sofas: ProductTypeId(SOFAS),
        boots: ProductTypeId(BOOTS),
    }
}

/// A second brand and product type, used only by [`cold_start_catalog`] to
/// give Gate 6's profiler more than one brand/product-type to derive.
pub fn aerowalk_brand() -> Brand {
    Brand {
        id: BrandId(2),
        name: "Aerowalk".to_string(),
    }
}

pub fn hiking_boots_type() -> ProductType {
    ProductType {
        id: ProductTypeId(2),
        name: "Hiking Boots".to_string(),
    }
}

pub fn cold_start_brands() -> Vec<Brand> {
    vec![nike_brand(), aerowalk_brand()]
}

pub fn cold_start_product_types() -> Vec<ProductType> {
    vec![running_shoes_type(), hiking_boots_type()]
}

pub fn cold_start_categories() -> Vec<Category> {
    vec![footwear_category()]
}

/// Gate 6's catalog fixture: two brands x two product types, hand-authored
/// (not randomly generated — this is a relevance/coverage fixture, not a
/// performance one, so `docs/EXPERIMENT_LOOP.md`'s "synthetic expansion...
/// clearly separated from relevance claims" rule keeps it off a random
/// generator). Deliberately contains one genuine cross-attribute value
/// collision: "green" is both a `color` (the Nike hiking boot variant) and
/// a `features` tag meaning eco-friendly material (the Aerowalk hiking
/// boot) — a cold-start profiler that indexes raw attribute values without
/// knowing their semantics cannot tell these apart and must surface the
/// collision as ambiguity rather than silently picking one meaning.
pub fn cold_start_catalog() -> Catalog {
    let running_shoes_nike = Product {
        id: ProductId(10),
        product_type: ProductTypeId(1),
        brand: BrandId(1),
        category: CategoryId(1),
        title: "Nike Trail Runner".to_string(),
        attributes: attributes([
            ("waterproof", AttributeValue::Boolean(false)),
            ("material", AttributeValue::Text("mesh".to_string())),
            (
                "features",
                AttributeValue::MultiEnum(vec!["cushioned".to_string()]),
            ),
        ]),
        variants: vec![
            Variant {
                id: VariantId(1001),
                attributes: attributes([
                    ("color", AttributeValue::Enum("Black".to_string())),
                    ("size", AttributeValue::Numeric(8.0)),
                ]),
                price: Price::usd(9_999),
                inventory: Inventory::in_stock(12),
            },
            Variant {
                id: VariantId(1002),
                attributes: attributes([
                    ("color", AttributeValue::Enum("Red".to_string())),
                    ("size", AttributeValue::Numeric(9.0)),
                ]),
                price: Price::usd(9_999),
                inventory: Inventory::in_stock(5),
            },
        ],
    };
    let hiking_boots_nike = Product {
        id: ProductId(11),
        product_type: ProductTypeId(2),
        brand: BrandId(1),
        category: CategoryId(1),
        title: "Nike Summit Boot".to_string(),
        attributes: attributes([
            ("waterproof", AttributeValue::Boolean(true)),
            ("material", AttributeValue::Text("leather".to_string())),
            (
                "features",
                AttributeValue::MultiEnum(vec!["insulated".to_string()]),
            ),
        ]),
        variants: vec![
            Variant {
                id: VariantId(1011),
                attributes: attributes([
                    ("color", AttributeValue::Enum("Brown".to_string())),
                    ("size", AttributeValue::Numeric(10.0)),
                ]),
                price: Price::usd(15_900),
                inventory: Inventory::in_stock(8),
            },
            Variant {
                id: VariantId(1012),
                attributes: attributes([
                    ("color", AttributeValue::Enum("Green".to_string())),
                    ("size", AttributeValue::Numeric(9.0)),
                ]),
                price: Price::usd(15_900),
                inventory: Inventory::in_stock(3),
            },
        ],
    };
    let running_shoes_aerowalk = Product {
        id: ProductId(12),
        product_type: ProductTypeId(1),
        brand: BrandId(2),
        category: CategoryId(1),
        title: "Aerowalk Sprint".to_string(),
        attributes: attributes([
            ("waterproof", AttributeValue::Boolean(true)),
            ("material", AttributeValue::Text("synthetic".to_string())),
            (
                "features",
                AttributeValue::MultiEnum(vec!["breathable".to_string()]),
            ),
        ]),
        variants: vec![
            Variant {
                id: VariantId(1021),
                attributes: attributes([
                    ("color", AttributeValue::Enum("Blue".to_string())),
                    ("size", AttributeValue::Numeric(8.0)),
                ]),
                price: Price::usd(7_900),
                inventory: Inventory::in_stock(20),
            },
            Variant {
                id: VariantId(1022),
                attributes: attributes([
                    ("color", AttributeValue::Enum("Black".to_string())),
                    ("size", AttributeValue::Numeric(10.0)),
                ]),
                price: Price::usd(7_900),
                inventory: Inventory::in_stock(6),
            },
        ],
    };
    let hiking_boots_aerowalk = Product {
        id: ProductId(13),
        product_type: ProductTypeId(2),
        brand: BrandId(2),
        category: CategoryId(1),
        title: "Aerowalk Summit Pro".to_string(),
        attributes: attributes([
            ("waterproof", AttributeValue::Boolean(true)),
            ("material", AttributeValue::Text("leather".to_string())),
            (
                "features",
                AttributeValue::MultiEnum(vec![
                    "insulated".to_string(),
                    "cushioned".to_string(),
                    "green".to_string(),
                ]),
            ),
        ]),
        variants: vec![Variant {
            id: VariantId(1031),
            attributes: attributes([
                ("color", AttributeValue::Enum("Brown".to_string())),
                ("size", AttributeValue::Numeric(9.0)),
            ]),
            price: Price::usd(18_900),
            inventory: Inventory::in_stock(4),
        }],
    };
    Catalog {
        products: vec![
            running_shoes_nike,
            hiking_boots_nike,
            running_shoes_aerowalk,
            hiking_boots_aerowalk,
        ],
    }
}
