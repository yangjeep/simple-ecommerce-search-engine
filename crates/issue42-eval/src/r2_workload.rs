//! Small, purpose-built fixture for R2 (`docs/experiments/ISSUE42_PROTOCOL.md`'s
//! R2 section: residual lexical semantics). Fully hand-specified, like
//! `r1_workload`'s fixtures -- nothing is sampled, so nothing needs an RNG
//! to be deterministic.
//!
//! **A design constraint learned the hard way during R1 and re-applied
//! here from the start**: `commerce_core::ir::query::compile`'s phrase
//! lexicon auto-resolves *any* token matching a registered Enum/MultiEnum/
//! Boolean-attribute-name value directly into a hard `Constraint` (or, with
//! no corroborating entity present, a demoted `Preference` via the
//! existing P9-E05 rule) -- it never stays as `residual_lexical` text.
//! Every word this fixture uses to test R2's residual-lexical mechanism is
//! therefore deliberately a plain **title word**, never an `Enum`/
//! `MultiEnum`/`Boolean` attribute value or name -- otherwise the query
//! would never reach the `Hybrid`/`Punt` residual-veto code path R2 exists
//! to test at all, and would silently exercise a completely different
//! mechanism instead. `phase9_eval::bitmap_delegate::build_index` only
//! indexes `Product::title` and product-level `AttributeValue::Text`
//! values (confirmed by direct source read); this fixture has no `Text`
//! attributes at all, so only title contents matter for delegate search.
//!
//! Four "benign" marketing/descriptive words (`furniture`, `waterproof`,
//! `bestseller`, `clearance`) each appear in >=2 *other* product types'
//! titles (Coffee Tables, Bookshelves) but never in Sofas/Boots titles or
//! as any registered attribute value anywhere -- a real, catalog-observable
//! "broadly used, not type-specific" signal a deterministic classifier can
//! discover. One "adversarial" word (`velvet`) appears *only* in a single
//! Sofas product's title -- a real, catalog-observable "narrowly used, for
//! a different type" signal. `banana` appears nowhere at all.

use commerce_core::domain::{
    attributes, AttributeValue, Brand, BrandId, Catalog, Category, CategoryId, Inventory, Price,
    Product, ProductId, ProductType, ProductTypeId, Variant, VariantId,
};

pub struct ResidualWorkloadCatalog {
    pub catalog: Catalog,
    pub product_types: Vec<ProductType>,
    pub brands: Vec<Brand>,
    pub categories: Vec<Category>,
    pub sofas_products: Vec<ProductId>,
    pub boots_products: Vec<ProductId>,
}

const SOFAS: u32 = 1;
const BOOTS: u32 = 2;
const COFFEE_TABLES: u32 = 3;
const BOOKSHELVES: u32 = 4;

/// Builds R2's shared fixture. Like `r1_workload`'s fixtures, this is
/// fully hand-specified -- nothing is sampled, so nothing needs an RNG to
/// be deterministic.
pub fn build() -> ResidualWorkloadCatalog {
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
        name: "R2 Test Brand".to_string(),
    }];
    let categories = vec![Category {
        id: CategoryId(1),
        name: "R2 Test Category".to_string(),
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

    // Sofas: real Enum color/material values (deliberately NOT used as R2
    // test residuals -- they would auto-resolve as hard constraints).
    // P2's title contains "velvet", which appears NOWHERE else in this
    // fixture -- the adversarial, type-exclusive signal row 6 needs.
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

    let p3 = make(
        3,
        BOOTS,
        "Rugged Hiking Boots",
        vec![("color", AttributeValue::Enum("Brown".to_string()))],
        30,
    );
    let p4 = make(
        4,
        BOOTS,
        "Steel Toe Work Boots",
        vec![("color", AttributeValue::Enum("Black".to_string()))],
        40,
    );

    // Coffee Tables and Bookshelves exist solely to give the four benign
    // marketing words a real, catalog-observable, cross-type (but never
    // Sofas/Boots) title presence.
    let p5 = make(
        5,
        COFFEE_TABLES,
        "Oak Coffee Table - Fine Furniture, Waterproof Finish, Bestseller Pick, Clearance Item",
        vec![],
        50,
    );
    let p6 = make(
        6,
        BOOKSHELVES,
        "Walnut Bookshelf - Solid Furniture, Waterproof Coating, Bestseller Choice, Clearance Sale",
        vec![],
        60,
    );

    // Filler: `commerce_core::plan::plan`'s own routing (`plan/mod.rs`)
    // sends a query with a non-empty structural constraint to `Hybrid`
    // only when `structural_candidates / catalog_size <=
    // policy.selectivity_threshold` (0.05 throughout this protocol,
    // matching R1/E1-E3); otherwise it routes to `Punt`, where the
    // delegate searches the *whole* corpus with no `restrict_to` at all
    // and a structural constraint is applied only as a post-hoc filter on
    // whatever the delegate happens to return. With only 6 real products,
    // Sofas/Boots' own 2-product share (33%) is far above that threshold,
    // so every R2 row would route to `Punt` -- and since the four benign
    // words are deliberately *also* present elsewhere in the catalog (P5/
    // P6's titles, the same catalog-observable "broadly used" signal
    // `ResidualPolicy` itself needs), a whole-corpus `Punt` search for
    // e.g. "furniture" would find P5/P6 even when restricted to Sofas is
    // what the row is meant to test -- silently replacing R2's intended
    // "delegate found nothing" scenario with an unrelated one ("delegate
    // found something, but it belongs to the wrong product type").
    // `Hybrid`'s `restrict_to`-scoped delegate search (confirmed by direct
    // read of `phase9_eval::bitmap_delegate`'s `BitmapRestrictQuery`,
    // which filters *inside* the delegate's own scorer, not merely
    // afterward) is the actual mechanism R2's protocol describes ("an
    // effective hard AND against the delegate's index"), so this filler
    // pads the catalog until Sofas/Boots' selectivity clears the
    // threshold with margin, forcing the intended routing rather than
    // silently exercising a different one. Plain, distinct titles with no
    // relation to any test word -- deliberately not adding new
    // occurrences of the test words themselves, so `r2_workload`'s own
    // catalog-invariant tests continue to describe the fixture completely.
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

    ResidualWorkloadCatalog {
        catalog: Catalog { products },
        product_types,
        brands,
        categories,
        sofas_products: vec![ProductId(1), ProductId(2)],
        boots_products: vec![ProductId(3), ProductId(4)],
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
    fn benign_words_never_appear_in_sofas_or_boots_titles_but_do_appear_elsewhere() {
        let fixture = build();
        for word in ["furniture", "waterproof", "bestseller", "clearance"] {
            for product in &fixture.catalog.products {
                let in_sofas_or_boots = fixture.sofas_products.contains(&product.id)
                    || fixture.boots_products.contains(&product.id);
                let title_lower = product.title.to_lowercase();
                if in_sofas_or_boots {
                    assert!(
                        !title_lower.contains(word),
                        "{word:?} must not appear in a Sofas/Boots title ({:?}), or Treatment A's \
                         delegate would already find it, defeating the zero-raw-hit test row this \
                         word is used for",
                        product.title
                    );
                }
            }
            let appears_elsewhere = fixture
                .catalog
                .products
                .iter()
                .filter(|p| {
                    !fixture.sofas_products.contains(&p.id)
                        && !fixture.boots_products.contains(&p.id)
                })
                .filter(|p| p.title.to_lowercase().contains(word))
                .count();
            assert!(
                appears_elsewhere >= 2,
                "{word:?} must appear in at least 2 non-Sofas/non-Boots titles to be a real, \
                 catalog-observable 'broadly used' signal, found {appears_elsewhere}"
            );
        }
    }

    #[test]
    fn velvet_is_exclusively_a_sofas_title_word() {
        let fixture = build();
        let mut sofas_hit = false;
        for product in &fixture.catalog.products {
            let has_velvet = product.title.to_lowercase().contains("velvet");
            if has_velvet {
                assert!(
                    fixture.sofas_products.contains(&product.id),
                    "'velvet' must never appear outside a Sofas title, found in {:?}",
                    product.title
                );
                sofas_hit = true;
            }
        }
        assert!(
            sofas_hit,
            "'velvet' must appear in at least one Sofas title"
        );
    }

    #[test]
    fn banana_appears_nowhere_at_all() {
        let fixture = build();
        for product in &fixture.catalog.products {
            assert!(!product.title.to_lowercase().contains("banana"));
        }
    }

    #[test]
    fn no_test_residual_word_is_also_a_registered_enum_value() {
        // The whole point of using title words: none of them may
        // coincide with a real Enum/MultiEnum attribute value anywhere,
        // or compile()'s own lexicon would auto-resolve them to a hard
        // constraint instead of leaving them as residual text.
        let fixture = build();
        let test_words = [
            "furniture",
            "waterproof",
            "bestseller",
            "clearance",
            "velvet",
            "banana",
        ];
        for product in &fixture.catalog.products {
            for value in product.attributes.values() {
                if let AttributeValue::Enum(v) = value {
                    let v_lower = v.to_lowercase();
                    assert!(
                        !test_words.contains(&v_lower.as_str()),
                        "test residual word {v_lower:?} must not coincide with a registered Enum value"
                    );
                }
            }
        }
    }
}
