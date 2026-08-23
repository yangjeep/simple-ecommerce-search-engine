//! Independent ground-truth oracle (Issue #42 shared infrastructure,
//! `docs/experiments/ISSUE42_PROTOCOL.md`'s "Ground-truth oracle" section).
//!
//! Deliberately has zero dependency on any `issue38_e2e3_eval` generator's
//! own `generate_workload`/`ground_truth` judgment maps, or on any
//! per-family `attr_str`/`attr_numeric` helper closure defined there. A
//! [`QueryIntent`] is hand-written in domain terms (e.g. "the apparel
//! Jeans product type, Enum `size` = \"34\""), never in a generator's own
//! internal representation of "the query I happened to build." Every
//! relevance label this module produces is re-derived directly from
//! `commerce_core::domain::{Product, Variant, AttributeValue}` fields on
//! the catalog actually materialized for the current run -- so a bug
//! shared between a catalog generator and its own judgment map cannot hide
//! behind this oracle, because this oracle never reads that judgment map
//! at all.

use std::collections::BTreeSet;

use commerce_core::domain::{
    AttributeValue, Catalog, Product, ProductId, ProductType, ProductTypeId, Variant, VariantId,
};

/// Resolves a human-readable product-type name (e.g. `"Jeans"`) to the
/// `ProductTypeId` the *actually materialized* catalog assigned it this
/// run. Panics on an unknown name: a [`QueryIntent`] naming a product type
/// absent from the registry is a test-construction bug and must fail
/// loudly, rather than this oracle silently classifying every candidate as
/// a type mismatch and reporting a falsely-low recall number.
pub fn resolve_product_type(product_types: &[ProductType], name: &str) -> ProductTypeId {
    product_types
        .iter()
        .find(|pt| pt.name == name)
        .unwrap_or_else(|| {
            let known: Vec<&str> = product_types.iter().map(|pt| pt.name.as_str()).collect();
            panic!(
                "no ProductType named {name:?} in this catalog's registry (known: {known:?}) -- \
                 this is a QueryIntent construction bug, not a real oracle finding"
            )
        })
        .id
}

/// One requirement a [`QueryIntent`] places on a candidate variant's
/// *effective* attributes -- variant-level overriding product-level,
/// exactly as `commerce_core::domain::catalog::effective_attributes`
/// defines the merge (reimplemented locally as `effective_value` below,
/// rather than imported, so this module touches only raw
/// `Product`/`Variant` fields, per this module's own independence
/// discipline).
#[derive(Debug, Clone, PartialEq)]
pub enum AttrRequirement {
    EnumEquals {
        attribute: String,
        value: String,
    },
    MultiEnumContains {
        attribute: String,
        value: String,
    },
    Boolean {
        attribute: String,
        value: bool,
    },
    NumericEquals {
        attribute: String,
        value: f64,
        epsilon: f64,
    },
    NumericRange {
        attribute: String,
        min: Option<f64>,
        max: Option<f64>,
    },
    /// Case/punctuation-insensitive exact text match -- R3's identifier
    /// normalization case (`"ia-1234-bp"` vs `"IA-1234-BP"` vs
    /// `"IA 1234 BP"` must all be the same identifier).
    TextEqualsNormalized {
        attribute: String,
        value: String,
    },
}

fn normalize_identifier(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

fn effective_value<'a>(
    name: &str,
    product: &'a Product,
    variant: &'a Variant,
) -> Option<&'a AttributeValue> {
    variant
        .attributes
        .get(name)
        .or_else(|| product.attributes.get(name))
}

impl AttrRequirement {
    fn matches(&self, product: &Product, variant: &Variant) -> bool {
        match self {
            AttrRequirement::EnumEquals { attribute, value } => matches!(
                effective_value(attribute, product, variant),
                Some(AttributeValue::Enum(have)) if have == value
            ),
            AttrRequirement::MultiEnumContains { attribute, value } => matches!(
                effective_value(attribute, product, variant),
                Some(AttributeValue::MultiEnum(have)) if have.iter().any(|v| v == value)
            ),
            AttrRequirement::Boolean { attribute, value } => matches!(
                effective_value(attribute, product, variant),
                Some(AttributeValue::Boolean(have)) if have == value
            ),
            AttrRequirement::NumericEquals {
                attribute,
                value,
                epsilon,
            } => matches!(
                effective_value(attribute, product, variant),
                Some(AttributeValue::Numeric(have)) if (have - value).abs() <= *epsilon
            ),
            AttrRequirement::NumericRange {
                attribute,
                min,
                max,
            } => match effective_value(attribute, product, variant) {
                Some(AttributeValue::Numeric(have)) => {
                    min.is_none_or(|m| *have >= m) && max.is_none_or(|m| *have <= m)
                }
                _ => false,
            },
            AttrRequirement::TextEqualsNormalized { attribute, value } => {
                match effective_value(attribute, product, variant) {
                    Some(AttributeValue::Text(have)) => {
                        normalize_identifier(have) == normalize_identifier(value)
                    }
                    _ => false,
                }
            }
        }
    }
}

/// What a query *should* mean in domain terms, independent of however a
/// generator happened to build the query string or its own internal
/// judgment map. `product_type`, if set, must be resolved via
/// [`resolve_product_type`] against the catalog registry for the current
/// run -- never hard-coded as a numeric literal.
#[derive(Debug, Clone, Default)]
pub struct QueryIntent {
    pub product_type: Option<ProductTypeId>,
    pub attributes: Vec<AttrRequirement>,
}

/// The oracle's verdict for every `(ProductId, VariantId)` pair in a
/// catalog against one [`QueryIntent`]. Anything not present in `exact` or
/// `partial` is `Irrelevant` by omission (`docs/experiments/ISSUE42_PROTOCOL.md`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OracleResult {
    pub exact: BTreeSet<(ProductId, VariantId)>,
    pub partial: BTreeSet<(ProductId, VariantId)>,
}

impl OracleResult {
    pub fn is_relevant(&self, key: &(ProductId, VariantId)) -> bool {
        self.exact.contains(key) || self.partial.contains(key)
    }
}

/// Classifies every variant in `catalog` against `intent`:
/// - `Exact`: the product type (if named) matches AND every
///   [`AttrRequirement`] matches.
/// - `Partial`: the product type matches but at least one attribute
///   requirement fails, OR the product type does not match but at least
///   one attribute requirement still does.
/// - Otherwise: irrelevant (absent from both sets).
pub fn classify(catalog: &Catalog, intent: &QueryIntent) -> OracleResult {
    let mut result = OracleResult::default();
    let type_specified = intent.product_type.is_some();
    for product in &catalog.products {
        // Only a *specified* product type is a real signal. When none is
        // named, "type matches" must not vacuously become true for every
        // product -- otherwise every attribute-only miss would be
        // mislabeled Partial instead of Irrelevant (a real bug this
        // module's own tests caught: see
        // `variant_level_attribute_overrides_product_level_of_the_same_name`).
        let type_matches = intent
            .product_type
            .is_some_and(|wanted| product.product_type == wanted);
        for variant in &product.variants {
            let satisfied = intent
                .attributes
                .iter()
                .filter(|req| req.matches(product, variant))
                .count();
            let all_attrs_match = satisfied == intent.attributes.len();
            let type_signal_ok = !type_specified || type_matches;
            let key = (product.id, variant.id);
            if type_signal_ok && all_attrs_match {
                result.exact.insert(key);
            } else if (type_specified && type_matches) || satisfied > 0 {
                result.partial.insert(key);
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use commerce_core::domain::{attributes, BrandId, CategoryId, Inventory, Price};

    // Hand-built fixtures only -- no synthetic-catalog generator is
    // imported anywhere in this test module, per this module's own
    // independence discipline.

    fn product(
        id: u64,
        product_type: u32,
        title: &str,
        product_attrs: Vec<(&'static str, AttributeValue)>,
        variants: Vec<Variant>,
    ) -> Product {
        Product {
            id: ProductId(id),
            product_type: ProductTypeId(product_type),
            brand: BrandId(1),
            category: CategoryId(1),
            title: title.to_string(),
            attributes: attributes(product_attrs),
            variants,
        }
    }

    fn variant(id: u64, attrs: Vec<(&'static str, AttributeValue)>) -> Variant {
        Variant {
            id: VariantId(id),
            attributes: attributes(attrs),
            price: Price::usd(1000),
            inventory: Inventory::in_stock(5),
        }
    }

    fn fixture_catalog() -> (Catalog, Vec<ProductType>) {
        let product_types = vec![
            ProductType {
                id: ProductTypeId(1),
                name: "Jeans".to_string(),
            },
            ProductType {
                id: ProductTypeId(2),
                name: "Wiper Blades".to_string(),
            },
        ];
        let catalog = Catalog {
            products: vec![
                product(
                    1,
                    1,
                    "Classic Jeans",
                    vec![("material", AttributeValue::Enum("Denim".to_string()))],
                    vec![
                        variant(
                            10,
                            vec![
                                ("size", AttributeValue::Enum("34".to_string())),
                                ("color", AttributeValue::Enum("Blue".to_string())),
                            ],
                        ),
                        variant(
                            11,
                            vec![
                                ("size", AttributeValue::Enum("36".to_string())),
                                ("color", AttributeValue::Enum("Black".to_string())),
                            ],
                        ),
                    ],
                ),
                product(
                    2,
                    2,
                    "All-Season Wiper Blade",
                    vec![],
                    vec![variant(20, vec![("size", AttributeValue::Numeric(34.0))])],
                ),
            ],
        };
        (catalog, product_types)
    }

    #[test]
    fn exact_match_requires_both_product_type_and_attribute() {
        let (catalog, product_types) = fixture_catalog();
        let intent = QueryIntent {
            product_type: Some(resolve_product_type(&product_types, "Jeans")),
            attributes: vec![AttrRequirement::EnumEquals {
                attribute: "size".to_string(),
                value: "34".to_string(),
            }],
        };
        let result = classify(&catalog, &intent);
        assert_eq!(
            result.exact,
            BTreeSet::from([(ProductId(1), VariantId(10))])
        );
        assert!(!result.exact.contains(&(ProductId(1), VariantId(11))));
    }

    #[test]
    fn right_product_type_wrong_attribute_value_is_partial_not_exact() {
        let (catalog, product_types) = fixture_catalog();
        let intent = QueryIntent {
            product_type: Some(resolve_product_type(&product_types, "Jeans")),
            attributes: vec![AttrRequirement::EnumEquals {
                attribute: "size".to_string(),
                value: "99".to_string(),
            }],
        };
        let result = classify(&catalog, &intent);
        assert!(result.exact.is_empty());
        assert_eq!(
            result.partial,
            BTreeSet::from([(ProductId(1), VariantId(10)), (ProductId(1), VariantId(11))])
        );
    }

    #[test]
    fn wrong_product_type_numeric_size_never_matches_the_enum_interpretation() {
        // The exact size-schema conflict E3/issue#40 measured: apparel's
        // Enum size="34" and automotive's Numeric size=34.0 must never be
        // conflated by this oracle, even though both are "34" in prose.
        let (catalog, product_types) = fixture_catalog();
        let intent = QueryIntent {
            product_type: Some(resolve_product_type(&product_types, "Jeans")),
            attributes: vec![AttrRequirement::EnumEquals {
                attribute: "size".to_string(),
                value: "34".to_string(),
            }],
        };
        let result = classify(&catalog, &intent);
        assert!(
            !result.is_relevant(&(ProductId(2), VariantId(20))),
            "the Numeric size=34.0 wiper blade must not satisfy an Enum size=\"34\" intent"
        );
    }

    #[test]
    fn numeric_range_and_epsilon_work_on_the_automotive_side() {
        let (catalog, product_types) = fixture_catalog();
        let exact_intent = QueryIntent {
            product_type: Some(resolve_product_type(&product_types, "Wiper Blades")),
            attributes: vec![AttrRequirement::NumericEquals {
                attribute: "size".to_string(),
                value: 34.0,
                epsilon: 1e-9,
            }],
        };
        assert!(classify(&catalog, &exact_intent)
            .exact
            .contains(&(ProductId(2), VariantId(20))));

        let range_intent = QueryIntent {
            product_type: Some(resolve_product_type(&product_types, "Wiper Blades")),
            attributes: vec![AttrRequirement::NumericRange {
                attribute: "size".to_string(),
                min: Some(30.0),
                max: Some(40.0),
            }],
        };
        assert!(classify(&catalog, &range_intent)
            .exact
            .contains(&(ProductId(2), VariantId(20))));

        let out_of_range = QueryIntent {
            product_type: Some(resolve_product_type(&product_types, "Wiper Blades")),
            attributes: vec![AttrRequirement::NumericRange {
                attribute: "size".to_string(),
                min: Some(35.0),
                max: None,
            }],
        };
        // Right product type (it genuinely is a Wiper Blades product),
        // wrong attribute value (34.0 is not >= 35.0) -- this is exactly
        // the protocol's own definition of Partial, not Irrelevant.
        let out_of_range_result = classify(&catalog, &out_of_range);
        assert!(!out_of_range_result
            .exact
            .contains(&(ProductId(2), VariantId(20))));
        assert!(out_of_range_result
            .partial
            .contains(&(ProductId(2), VariantId(20))));
    }

    #[test]
    fn variant_level_attribute_overrides_product_level_of_the_same_name() {
        let catalog = Catalog {
            products: vec![product(
                3,
                1,
                "Overridable",
                vec![("color", AttributeValue::Enum("Red".to_string()))],
                vec![variant(
                    30,
                    vec![("color", AttributeValue::Enum("Green".to_string()))],
                )],
            )],
        };
        let wants_green = QueryIntent {
            product_type: None,
            attributes: vec![AttrRequirement::EnumEquals {
                attribute: "color".to_string(),
                value: "Green".to_string(),
            }],
        };
        assert!(classify(&catalog, &wants_green)
            .exact
            .contains(&(ProductId(3), VariantId(30))));

        let wants_red = QueryIntent {
            product_type: None,
            attributes: vec![AttrRequirement::EnumEquals {
                attribute: "color".to_string(),
                value: "Red".to_string(),
            }],
        };
        assert!(
            !classify(&catalog, &wants_red).is_relevant(&(ProductId(3), VariantId(30))),
            "the variant's own color must shadow the product-level color entirely, not merge with it"
        );
    }

    #[test]
    fn multi_enum_contains_and_absent_attribute_is_irrelevant_not_a_panic() {
        let catalog = Catalog {
            products: vec![product(
                4,
                2,
                "Brake Pads",
                vec![],
                vec![variant(
                    40,
                    vec![(
                        "compatible_fitment",
                        AttributeValue::MultiEnum(vec![
                            "2015 honda civic".to_string(),
                            "2016 honda civic".to_string(),
                        ]),
                    )],
                )],
            )],
        };
        let fits = QueryIntent {
            product_type: None,
            attributes: vec![AttrRequirement::MultiEnumContains {
                attribute: "compatible_fitment".to_string(),
                value: "2015 honda civic".to_string(),
            }],
        };
        assert!(classify(&catalog, &fits)
            .exact
            .contains(&(ProductId(4), VariantId(40))));

        let missing_attr = QueryIntent {
            product_type: None,
            attributes: vec![AttrRequirement::EnumEquals {
                attribute: "nonexistent".to_string(),
                value: "anything".to_string(),
            }],
        };
        let result = classify(&catalog, &missing_attr);
        assert!(!result.is_relevant(&(ProductId(4), VariantId(40))));
    }

    #[test]
    fn text_equals_normalized_ignores_case_and_punctuation() {
        let catalog = Catalog {
            products: vec![product(
                5,
                2,
                "Spark Plug",
                vec![],
                vec![variant(
                    50,
                    vec![(
                        "part_number",
                        AttributeValue::Text("IA-1234-BP".to_string()),
                    )],
                )],
            )],
        };
        for query_form in ["IA-1234-BP", "ia-1234-bp", "IA 1234 BP", "ia1234bp"] {
            let intent = QueryIntent {
                product_type: None,
                attributes: vec![AttrRequirement::TextEqualsNormalized {
                    attribute: "part_number".to_string(),
                    value: query_form.to_string(),
                }],
            };
            assert!(
                classify(&catalog, &intent)
                    .exact
                    .contains(&(ProductId(5), VariantId(50))),
                "normalized form {query_form:?} should match"
            );
        }

        let prefix_only = QueryIntent {
            product_type: None,
            attributes: vec![AttrRequirement::TextEqualsNormalized {
                attribute: "part_number".to_string(),
                value: "IA-1234".to_string(),
            }],
        };
        assert!(
            !classify(&catalog, &prefix_only).is_relevant(&(ProductId(5), VariantId(50))),
            "a partial prefix must not be treated as an exact identifier match"
        );
    }

    #[test]
    #[should_panic(expected = "no ProductType named")]
    fn resolve_product_type_panics_loudly_on_an_unknown_name() {
        let (_catalog, product_types) = fixture_catalog();
        resolve_product_type(&product_types, "Sofas");
    }
}
