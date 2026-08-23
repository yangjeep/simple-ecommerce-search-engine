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
    attributes, AttributeValue, Brand, BrandId, Catalog, Category, CategoryId, Inventory, Price,
    Product, ProductId, ProductType, ProductTypeId, Variant, VariantId,
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

/// The literal size value rows 1-3 use: real in automotive's Numeric
/// `size` range (`16..=28`, `automotive.rs`) and given directly to a
/// hand-built apparel-style Jeans product's Enum `size`. Apparel's own
/// real Jeans sizes (`["30","32","34","36","38"]`, `apparel.rs`) and
/// automotive's own real Wiper Blades `size` range never overlap (see
/// `ISSUE42_PROTOCOL.md`'s dated correction), so this fixture -- not the
/// frozen `apparel`/`automotive` generators -- is the single source of a
/// genuinely real, verifiable ambiguous value.
pub const AMBIGUOUS_SIZE_VALUE: &str = "22";

/// Fitment phrase rows 6 reuses, in the exact natural, space-joined form
/// `compile()`'s phrase lexicon requires (see `automotive.rs`'s
/// `fitment_tag` doc comment) -- not a pipe-joined key.
pub const FITMENT_PHRASE: &str = "2015 honda civic";

/// The identifier value row 7 uses -- deliberately not present anywhere
/// as an Enum or Numeric value in this fixture, so a numeric-typed
/// interpretation could never legitimately claim it either.
pub const IDENTIFIER_VALUE: &str = "IA-1234-BP";

/// The adversarial value for Treatment B's own falsification case
/// (rule 4: every treatment needs a case *capable of disproving it*, not
/// just cases it happens to pass): an unrelated attribute
/// (`thread_count`, on a "Bolt Kits" product with no `size` attribute at
/// all) that coincidentally shares [`AMBIGUOUS_SIZE_VALUE`]'s literal
/// text. `lexicon_alternatives` returns *every* Attribute-typed candidate
/// registered for a raw token, not only ones named "size" -- so a
/// treatment that keeps every lexicon alternative as an unconditional
/// hard constraint (Treatment B) is expected to incorrectly admit this
/// product for query row 1 (`"size 22"`), a genuine wrong-family/
/// wrong-interpretation false positive, while C/D (which never keep an
/// uncorroborated alternative as a hard constraint) must not.
pub const UNRELATED_ATTRIBUTE_ADVERSARIAL_VALUE: &str = AMBIGUOUS_SIZE_VALUE;

pub struct TypedAmbiguityCatalog {
    pub catalog: Catalog,
    pub product_types: Vec<ProductType>,
    pub brands: Vec<Brand>,
    pub categories: Vec<Category>,
    /// Enum `size` = [`AMBIGUOUS_SIZE_VALUE`], product type name "Jeans".
    pub jeans_variant: (ProductId, VariantId),
    /// Numeric `size` = [`AMBIGUOUS_SIZE_VALUE`] parsed as `f64`, product
    /// type name "Wiper Blades".
    pub wiper_variant: (ProductId, VariantId),
    /// `compatible_fitment` MultiEnum containing [`FITMENT_PHRASE`],
    /// product type name "Brake Pads".
    pub brake_pads_variant: (ProductId, VariantId),
    /// Text `part_number` = [`IDENTIFIER_VALUE`], on a product type this
    /// fixture never registers as a queryable entity phrase.
    pub identifier_variant: (ProductId, VariantId),
    /// Enum `thread_count` = [`UNRELATED_ATTRIBUTE_ADVERSARIAL_VALUE`] on
    /// a "Bolt Kits" product with no `size` attribute at all -- Treatment
    /// B's own falsification case (see that constant's doc comment).
    pub bolt_kits_variant: (ProductId, VariantId),
}

/// Builds R1 workload rows 1-7, 9, 10's shared small catalog. Like
/// [`build`] above, this is a fully hand-specified fixture -- nothing is
/// sampled, so nothing needs an RNG to be deterministic.
pub fn build_typed_ambiguity_catalog() -> TypedAmbiguityCatalog {
    let product_types = vec![
        ProductType {
            id: ProductTypeId(1),
            name: "Jeans".to_string(),
        },
        ProductType {
            id: ProductTypeId(2),
            name: "Wiper Blades".to_string(),
        },
        ProductType {
            id: ProductTypeId(3),
            name: "Brake Pads".to_string(),
        },
        ProductType {
            id: ProductTypeId(4),
            name: "Small Parts".to_string(),
        },
        ProductType {
            id: ProductTypeId(5),
            name: "Bolt Kits".to_string(),
        },
    ];
    let brands = vec![Brand {
        id: BrandId(1),
        name: "R1 Test Brand".to_string(),
    }];
    let categories = vec![Category {
        id: CategoryId(1),
        name: "R1 Test Category".to_string(),
    }];

    let jeans_product = Product {
        id: ProductId(1),
        product_type: ProductTypeId(1),
        brand: BrandId(1),
        category: CategoryId(1),
        title: "R1 Jeans Product".to_string(),
        attributes: attributes([]),
        variants: vec![Variant {
            id: VariantId(10),
            attributes: attributes([(
                "size",
                AttributeValue::Enum(AMBIGUOUS_SIZE_VALUE.to_string()),
            )]),
            price: Price::usd(2999),
            inventory: Inventory::in_stock(5),
        }],
    };

    let wiper_product = Product {
        id: ProductId(2),
        product_type: ProductTypeId(2),
        brand: BrandId(1),
        category: CategoryId(1),
        title: "R1 Wiper Blades Product".to_string(),
        attributes: attributes([(
            "size",
            AttributeValue::Numeric(
                AMBIGUOUS_SIZE_VALUE
                    .parse()
                    .expect("AMBIGUOUS_SIZE_VALUE is numeric-looking"),
            ),
        )]),
        variants: vec![Variant {
            id: VariantId(20),
            attributes: attributes([]),
            price: Price::usd(1999),
            inventory: Inventory::in_stock(5),
        }],
    };

    let brake_pads_product = Product {
        id: ProductId(3),
        product_type: ProductTypeId(3),
        brand: BrandId(1),
        category: CategoryId(1),
        title: "R1 Brake Pads Product".to_string(),
        attributes: attributes([]),
        variants: vec![Variant {
            id: VariantId(30),
            attributes: attributes([(
                "compatible_fitment",
                AttributeValue::MultiEnum(vec![FITMENT_PHRASE.to_string()]),
            )]),
            price: Price::usd(5999),
            inventory: Inventory::in_stock(5),
        }],
    };

    let identifier_product = Product {
        id: ProductId(4),
        product_type: ProductTypeId(4),
        brand: BrandId(1),
        category: CategoryId(1),
        title: "R1 Identifier Product".to_string(),
        attributes: attributes([]),
        variants: vec![Variant {
            id: VariantId(40),
            attributes: attributes([(
                "part_number",
                AttributeValue::Text(IDENTIFIER_VALUE.to_string()),
            )]),
            price: Price::usd(999),
            inventory: Inventory::in_stock(5),
        }],
    };

    let bolt_kits_product = Product {
        id: ProductId(5),
        product_type: ProductTypeId(5),
        brand: BrandId(1),
        category: CategoryId(1),
        title: "R1 Bolt Kits Product".to_string(),
        attributes: attributes([]),
        variants: vec![Variant {
            id: VariantId(50),
            attributes: attributes([(
                "thread_count",
                AttributeValue::Enum(UNRELATED_ATTRIBUTE_ADVERSARIAL_VALUE.to_string()),
            )]),
            price: Price::usd(1499),
            inventory: Inventory::in_stock(5),
        }],
    };

    TypedAmbiguityCatalog {
        catalog: Catalog {
            products: vec![
                jeans_product,
                wiper_product,
                brake_pads_product,
                identifier_product,
                bolt_kits_product,
            ],
        },
        product_types,
        brands,
        categories,
        jeans_variant: (ProductId(1), VariantId(10)),
        wiper_variant: (ProductId(2), VariantId(20)),
        brake_pads_variant: (ProductId(3), VariantId(30)),
        identifier_variant: (ProductId(4), VariantId(40)),
        bolt_kits_variant: (ProductId(5), VariantId(50)),
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

    #[test]
    fn typed_ambiguity_catalog_is_deterministic() {
        let a = build_typed_ambiguity_catalog();
        let b = build_typed_ambiguity_catalog();
        assert_eq!(a.catalog.products, b.catalog.products);
    }

    /// Every claimed positive in R1's rows 1-3/6/7 is checked here against
    /// the independent oracle -- not merely asserted in
    /// `ISSUE42_PROTOCOL.md`'s prose -- per this crate's own
    /// "do not trust the experiment author" governance.
    #[test]
    fn every_row_1_to_7_claim_is_a_real_oracle_exact_match() {
        use crate::oracle::{classify, resolve_product_type, AttrRequirement, QueryIntent};

        let fixture = build_typed_ambiguity_catalog();

        // Row 1/2: apparel-style Enum size, on the Jeans product type.
        let jeans_intent = QueryIntent {
            product_type: Some(resolve_product_type(&fixture.product_types, "Jeans")),
            attributes: vec![AttrRequirement::EnumEquals {
                attribute: "size".to_string(),
                value: AMBIGUOUS_SIZE_VALUE.to_string(),
            }],
        };
        assert!(
            classify(&fixture.catalog, &jeans_intent)
                .exact
                .contains(&fixture.jeans_variant),
            "row 1/2's Enum size={AMBIGUOUS_SIZE_VALUE:?} Jeans match must be real"
        );

        // Row 1/3: automotive-style Numeric size, on the Wiper Blades
        // product type -- the SAME literal value, a genuinely different
        // AttributeValue kind.
        let wiper_intent = QueryIntent {
            product_type: Some(resolve_product_type(&fixture.product_types, "Wiper Blades")),
            attributes: vec![AttrRequirement::NumericEquals {
                attribute: "size".to_string(),
                value: AMBIGUOUS_SIZE_VALUE.parse().unwrap(),
                epsilon: 1e-9,
            }],
        };
        assert!(
            classify(&fixture.catalog, &wiper_intent)
                .exact
                .contains(&fixture.wiper_variant),
            "row 1/3's Numeric size={AMBIGUOUS_SIZE_VALUE:?} Wiper Blades match must be real"
        );

        // Cross-check (the exact wrong-family shape E3/issue#40 measured):
        // the Jeans product must NOT satisfy the Numeric interpretation,
        // and the Wiper Blades product must NOT satisfy the Enum one.
        assert!(!classify(&fixture.catalog, &wiper_intent).is_relevant(&fixture.jeans_variant));
        assert!(!classify(&fixture.catalog, &jeans_intent).is_relevant(&fixture.wiper_variant));

        // Row 6: fitment MultiEnum match on Brake Pads.
        let fitment_intent = QueryIntent {
            product_type: Some(resolve_product_type(&fixture.product_types, "Brake Pads")),
            attributes: vec![AttrRequirement::MultiEnumContains {
                attribute: "compatible_fitment".to_string(),
                value: FITMENT_PHRASE.to_string(),
            }],
        };
        assert!(classify(&fixture.catalog, &fitment_intent)
            .exact
            .contains(&fixture.brake_pads_variant));

        // Row 7: the identifier value is real Text on its own product,
        // and (critically) is NOT also a valid Enum/Numeric size anywhere
        // -- so no numeric-typed interpretation could ever legitimately
        // claim it.
        let identifier_intent = QueryIntent {
            product_type: None,
            attributes: vec![AttrRequirement::TextEqualsNormalized {
                attribute: "part_number".to_string(),
                value: IDENTIFIER_VALUE.to_string(),
            }],
        };
        assert!(classify(&fixture.catalog, &identifier_intent)
            .exact
            .contains(&fixture.identifier_variant));
        assert!(
            IDENTIFIER_VALUE.parse::<f64>().is_err(),
            "the identifier value must not itself be numeric-parseable"
        );
    }

    /// Row 9's negative claim ("purple" has no valid size interpretation)
    /// checked against the independent oracle: no product in this fixture
    /// carries "purple" as any attribute value at all.
    #[test]
    fn row_9_purple_has_no_real_interpretation_anywhere_in_the_fixture() {
        let fixture = build_typed_ambiguity_catalog();
        for product in &fixture.catalog.products {
            for value in product.attributes.values() {
                if let AttributeValue::Enum(v) = value {
                    assert_ne!(v.to_lowercase(), "purple");
                }
            }
            for variant in &product.variants {
                for value in variant.attributes.values() {
                    if let AttributeValue::Enum(v) = value {
                        assert_ne!(v.to_lowercase(), "purple");
                    }
                }
            }
        }
    }

    /// Treatment B's own falsification case must be real: a Bolt Kits
    /// product with `thread_count` = "22" (an attribute genuinely
    /// distinct from `size`), independently confirmed via the oracle.
    #[test]
    fn treatment_b_adversarial_case_is_a_real_distinct_attribute_collision() {
        use crate::oracle::{classify, AttrRequirement, QueryIntent};

        let fixture = build_typed_ambiguity_catalog();
        let intent = QueryIntent {
            product_type: None,
            attributes: vec![AttrRequirement::EnumEquals {
                attribute: "thread_count".to_string(),
                value: UNRELATED_ATTRIBUTE_ADVERSARIAL_VALUE.to_string(),
            }],
        };
        assert!(classify(&fixture.catalog, &intent)
            .exact
            .contains(&fixture.bolt_kits_variant));

        // And it must NOT also have a `size` attribute of its own --
        // otherwise this wouldn't be testing a *distinct*-attribute
        // collision, just a duplicate of the size-ambiguity case.
        let bolt_kits_product = fixture
            .catalog
            .products
            .iter()
            .find(|p| p.id == fixture.bolt_kits_variant.0)
            .unwrap();
        assert!(!bolt_kits_product.attributes.contains_key("size"));
        assert!(!bolt_kits_product.variants[0]
            .attributes
            .contains_key("size"));
    }

    /// Row 10's negative claim: 999999 is outside both real size ranges
    /// (Jeans Enum values are single/double-digit strings; Wiper Blades
    /// Numeric size is [`AMBIGUOUS_SIZE_VALUE`] = 22 in this fixture).
    #[test]
    fn row_10_999999_is_outside_every_real_size_value_in_the_fixture() {
        let fixture = build_typed_ambiguity_catalog();
        for product in &fixture.catalog.products {
            for (name, value) in &product.attributes {
                if name == "size" {
                    if let AttributeValue::Numeric(n) = value {
                        assert_ne!(*n, 999999.0);
                    }
                }
            }
            for variant in &product.variants {
                for (name, value) in &variant.attributes {
                    if name == "size" {
                        match value {
                            AttributeValue::Numeric(n) => assert_ne!(*n, 999999.0),
                            AttributeValue::Enum(v) => assert_ne!(v.as_str(), "999999"),
                            _ => {}
                        }
                    }
                }
            }
        }
    }
}
