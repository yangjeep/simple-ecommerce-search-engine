//! One exhaustive, symmetric translation from every
//! `commerce_core::ir::ResolvedConstraint` shape to a Solr `fq` clause.
//!
//! Every prior fq-translation function in this workspace (`round1_eval`,
//! `issue35_eval::eval`, `phase9_eval`'s `wands_solr_query_for` and its
//! `i55_e14`/`p9_e07` copies) matched only the constraint kinds its
//! author happened to need, leaving the rest to an implicit `_ => {}`.
//! That is exactly how the `ProductTypeAny` omission shipped twice
//! independently (`docs/decisions/ISSUE55_PAIRED_COMPARATOR_DECISION.md`,
//! and again in `p9_e07_ambiguous_routing_diagnostic.rs`, which drifted
//! out of sync with its own claimed-identical twin). [`translate_constraint`]
//! matches every `ResolvedConstraint` variant explicitly with no wildcard
//! arm, so a new variant added to `commerce_core::ir` fails to *compile*
//! here rather than silently under-filtering Solr relative to native.

use commerce_core::domain::{BrandId, CategoryId, Constraint, NumericOp, ProductTypeId};
use commerce_core::ir::ResolvedConstraint;
#[cfg(test)]
use commerce_core::ir::StructuralConstraint;

use crate::solr::{case_insensitive_contains_regex, case_insensitive_field_regex};

mod structural;
use structural::translate_structural;

/// Which Solr field (if any) this dataset's Solr core uses for each
/// structural dimension. `None` means the dataset genuinely has no such
/// field -- e.g. WANDS has no `brand` field, ESCI has no `product_type`/
/// `category` field. That must produce [`Translation::NotApplicable`],
/// an explicit, auditable "this dataset can't express that constraint,"
/// never a silently-omitted `fq`.
///
/// Attribute-level constraints (`Constraint::Enum`/`MultiEnumContains`/
/// `Boolean`/`Numeric`/`Text`) are not listed here: every dataset in this
/// workspace indexes an attribute under its own attribute name as the
/// Solr field name directly (confirmed in every existing translator --
/// `color` attribute -> `color` field), so no separate map is needed for
/// them.
#[derive(Debug, Clone, Default)]
pub struct SolrFieldMap {
    pub brand: Option<&'static str>,
    pub product_type: Option<&'static str>,
    pub category: Option<&'static str>,
    pub price_cents: Option<&'static str>,
}

/// Opt-in translation configuration for production-style exact structured
/// string filters. The default preserves the historical regex wire format.
#[derive(Debug, Clone, Default)]
pub struct SolrTranslationConfig {
    pub fields: SolrFieldMap,
    /// When set, structured string filters target a lowercased companion
    /// field populated at index time, replacing a regex automaton with an
    /// exact term lookup while preserving case-insensitive semantics.
    pub lowercase_companion_suffix: Option<&'static str>,
}

/// Resolves the compiler-internal typed ids a [`StructuralConstraint`]
/// carries (`BrandId`/`ProductTypeId`/`CategoryId`) to the display-text
/// name a Solr field actually stores. Structural constraints never carry
/// catalog text directly, so this is always needed alongside
/// [`SolrFieldMap`].
pub trait StructuralNames {
    fn brand_name(&self, id: BrandId) -> Option<&str>;
    fn product_type_name(&self, id: ProductTypeId) -> Option<&str>;
    fn category_name(&self, id: CategoryId) -> Option<&str>;
}

/// The result of translating one [`ResolvedConstraint`] into a Solr `fq`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Translation {
    /// A concrete `fq` clause native's equivalent hard constraint maps to.
    Fq(String),
    /// This dataset's [`SolrFieldMap`] declares no field for this
    /// constraint's dimension -- a deliberate, disclosed omission (the
    /// dataset has no such data at all), not a translation bug. Safe to
    /// drop from `fq` *only* because it is structurally impossible for
    /// native to have matched on this dimension either, for the same
    /// reason (no data to have compiled the constraint from in the first
    /// place, in the normal case) -- see each call site's own comment
    /// for the disclosed exception where a generic compiler could still
    /// emit one anyway.
    NotApplicable,
    /// The dataset's `SolrFieldMap` DOES declare a field for this
    /// dimension, but the id inside the constraint could not be resolved
    /// to a display name via `StructuralNames` (e.g. a stale/missing
    /// catalog entry). Unlike `NotApplicable`, this is a hard failure a
    /// caller must not silently swallow -- native did have a name to
    /// evaluate `matches()` against; Solr would silently receive an
    /// easier, unfiltered query if this were dropped instead of surfaced.
    Unresolvable(String),
}

/// Translates one resolved constraint. No wildcard arm: every
/// `StructuralConstraint` and `Constraint` variant is matched explicitly.
pub fn translate_constraint(
    c: &ResolvedConstraint,
    fields: &SolrFieldMap,
    names: &dyn StructuralNames,
) -> Translation {
    let context = TranslationContext {
        fields,
        names,
        lowercase_companion_suffix: None,
    };
    translate_constraint_with_context(c, &context)
}

/// Translates one resolved constraint with an explicit, default-off physical
/// representation for structured string filters.
pub fn translate_constraint_with_config(
    c: &ResolvedConstraint,
    names: &dyn StructuralNames,
    config: &SolrTranslationConfig,
) -> Translation {
    let context = TranslationContext {
        fields: &config.fields,
        names,
        lowercase_companion_suffix: config.lowercase_companion_suffix,
    };
    translate_constraint_with_context(c, &context)
}

pub(super) struct TranslationContext<'a> {
    pub(super) fields: &'a SolrFieldMap,
    pub(super) names: &'a dyn StructuralNames,
    pub(super) lowercase_companion_suffix: Option<&'static str>,
}

fn translate_constraint_with_context(
    c: &ResolvedConstraint,
    context: &TranslationContext<'_>,
) -> Translation {
    match c {
        ResolvedConstraint::Structural(s) => translate_structural(s, context),
        ResolvedConstraint::Attribute(a) => translate_attribute(a),
    }
}

fn translate_attribute(c: &Constraint) -> Translation {
    match c {
        Constraint::Enum { attribute, value } => Translation::Fq(format!(
            "{attribute}:/{}/",
            case_insensitive_field_regex(value)
        )),
        Constraint::MultiEnumContains { attribute, value } => Translation::Fq(format!(
            "{attribute}:/{}/",
            case_insensitive_field_regex(value)
        )),
        Constraint::Boolean { attribute, value } => Translation::Fq(format!("{attribute}:{value}")),
        Constraint::Numeric {
            attribute,
            op,
            value,
        } => {
            let clause = match op {
                NumericOp::Eq => format!("{attribute}:{value}"),
                NumericOp::Lt => format!("{attribute}:[* TO {value}}}"),
                NumericOp::Lte => format!("{attribute}:[* TO {value}]"),
                NumericOp::Gt => format!("{attribute}:{{{value} TO *]"),
                NumericOp::Gte => format!("{attribute}:[{value} TO *]"),
            };
            Translation::Fq(clause)
        }
        Constraint::Text {
            attribute,
            contains,
        } => Translation::Fq(format!(
            "{attribute}:/{}/",
            case_insensitive_contains_regex(contains)
        )),
    }
}

/// Translates every constraint in `constraints`, returning the built `fq`
/// list and a *separate* list of any [`Translation::Unresolvable`]
/// failures. A caller MUST treat a non-empty failure list as a hard
/// comparator failure for that query (do not send a partial `fq` to
/// Solr and score the result as if it were symmetric with native) --
/// see [`crate::compare`].
pub fn translate_all(
    constraints: &[ResolvedConstraint],
    fields: &SolrFieldMap,
    names: &dyn StructuralNames,
) -> (Vec<String>, Vec<String>) {
    let context = TranslationContext {
        fields,
        names,
        lowercase_companion_suffix: None,
    };
    translate_all_with_context(constraints, &context)
}

/// Translates all constraints with an explicit, default-off structured-string
/// representation while preserving separate unresolvable failures.
pub fn translate_all_with_config(
    constraints: &[ResolvedConstraint],
    names: &dyn StructuralNames,
    config: &SolrTranslationConfig,
) -> (Vec<String>, Vec<String>) {
    let context = TranslationContext {
        fields: &config.fields,
        names,
        lowercase_companion_suffix: config.lowercase_companion_suffix,
    };
    translate_all_with_context(constraints, &context)
}

fn translate_all_with_context(
    constraints: &[ResolvedConstraint],
    context: &TranslationContext<'_>,
) -> (Vec<String>, Vec<String>) {
    let mut fq = Vec::new();
    let mut failures = Vec::new();
    for c in constraints {
        match translate_constraint_with_context(c, context) {
            Translation::Fq(clause) => fq.push(clause),
            Translation::NotApplicable => {}
            Translation::Unresolvable(reason) => failures.push(reason),
        }
    }
    (fq, failures)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct NoNames;
    impl StructuralNames for NoNames {
        fn brand_name(&self, _id: BrandId) -> Option<&str> {
            None
        }
        fn product_type_name(&self, _id: ProductTypeId) -> Option<&str> {
            None
        }
        fn category_name(&self, _id: CategoryId) -> Option<&str> {
            None
        }
    }

    struct FakeNames;
    impl StructuralNames for FakeNames {
        fn brand_name(&self, id: BrandId) -> Option<&str> {
            match id.0 {
                1 => Some("Nike"),
                2 => Some("Adidas"),
                _ => None,
            }
        }
        fn product_type_name(&self, id: ProductTypeId) -> Option<&str> {
            match id.0 {
                1 => Some("beds"),
                2 => Some("kids beds"),
                _ => None,
            }
        }
        fn category_name(&self, id: CategoryId) -> Option<&str> {
            match id.0 {
                1 => Some("Furniture"),
                _ => None,
            }
        }
    }

    fn full_fields() -> SolrFieldMap {
        SolrFieldMap {
            brand: Some("brand"),
            product_type: Some("product_class"),
            category: Some("category_leaf"),
            price_cents: Some("price_cents"),
        }
    }

    struct SpecialCharacterNames;
    impl StructuralNames for SpecialCharacterNames {
        fn brand_name(&self, id: BrandId) -> Option<&str> {
            match id.0 {
                1 => Some("ACME \"Pro\"\\ Gear"),
                _ => None,
            }
        }

        fn product_type_name(&self, _id: ProductTypeId) -> Option<&str> {
            None
        }

        fn category_name(&self, _id: CategoryId) -> Option<&str> {
            None
        }
    }

    fn lowercase_companion_config() -> SolrTranslationConfig {
        SolrTranslationConfig {
            fields: full_fields(),
            lowercase_companion_suffix: Some("_lc"),
        }
    }

    #[test]
    fn default_field_map_still_emits_the_historical_regex_form() {
        // Given
        let constraint = ResolvedConstraint::Structural(StructuralConstraint::Brand(BrandId(1)));

        // When
        let translated = translate_constraint(&constraint, &full_fields(), &FakeNames);

        // Then
        assert_eq!(
            translated,
            Translation::Fq("brand:/[nN][iI][kK][eE]/".to_string())
        );
    }

    #[test]
    fn lowercase_companion_mode_emits_an_exact_term_query_not_a_regex() {
        // Given
        let constraint = ResolvedConstraint::Structural(StructuralConstraint::Brand(BrandId(1)));

        // When
        let translated = translate_constraint_with_config(
            &constraint,
            &FakeNames,
            &lowercase_companion_config(),
        );

        // Then
        match translated {
            Translation::Fq(fq) => {
                assert!(!fq.contains('/'));
                assert!(fq.contains("brand_lc:\"nike\""));
            }
            other => panic!("expected Fq, got {other:?}"),
        }
    }

    #[test]
    fn lowercase_companion_mode_lowercases_the_value() {
        // Given
        let constraint = ResolvedConstraint::Structural(StructuralConstraint::Brand(BrandId(1)));

        // When
        let translated = translate_constraint_with_config(
            &constraint,
            &FakeNames,
            &lowercase_companion_config(),
        );

        // Then
        assert_eq!(translated, Translation::Fq("brand_lc:\"nike\"".to_string()));
    }

    #[test]
    fn lowercase_companion_mode_escapes_solr_special_characters() {
        // Given
        let constraint = ResolvedConstraint::Structural(StructuralConstraint::Brand(BrandId(1)));

        // When
        let translated = translate_constraint_with_config(
            &constraint,
            &SpecialCharacterNames,
            &lowercase_companion_config(),
        );

        // Then
        assert_eq!(
            translated,
            Translation::Fq("brand_lc:\"acme \\\"pro\\\"\\\\ gear\"".to_string())
        );
    }

    #[test]
    fn lowercase_companion_multi_value_any_emits_a_single_term_disjunction() {
        // Given
        let constraint = ResolvedConstraint::Structural(StructuralConstraint::BrandAny(vec![
            BrandId(1),
            BrandId(2),
        ]));

        // When
        let translated = translate_constraint_with_config(
            &constraint,
            &FakeNames,
            &lowercase_companion_config(),
        );

        // Then
        assert_eq!(
            translated,
            Translation::Fq("brand_lc:(\"nike\" OR \"adidas\")".to_string())
        );
    }

    #[test]
    fn both_modes_select_the_same_logical_value() {
        // Given: the stored source value and its index-time lowercase companion.
        let constraint = ResolvedConstraint::Structural(StructuralConstraint::Brand(BrandId(1)));

        // When
        let historical = translate_constraint(&constraint, &full_fields(), &FakeNames);
        let exact = translate_constraint_with_config(
            &constraint,
            &FakeNames,
            &lowercase_companion_config(),
        );

        // Then: both forms select "Nike"; only the physical lookup differs.
        assert_eq!(
            historical,
            Translation::Fq("brand:/[nN][iI][kK][eE]/".to_string())
        );
        assert_eq!(exact, Translation::Fq("brand_lc:\"nike\"".to_string()));
    }

    #[test]
    fn brand_not_applicable_when_no_field_configured() {
        let c = ResolvedConstraint::Structural(StructuralConstraint::Brand(BrandId(1)));
        let empty = SolrFieldMap::default();
        assert_eq!(
            translate_constraint(&c, &empty, &FakeNames),
            Translation::NotApplicable
        );
    }

    #[test]
    fn brand_unresolvable_when_field_configured_but_name_missing() {
        let c = ResolvedConstraint::Structural(StructuralConstraint::Brand(BrandId(99)));
        match translate_constraint(&c, &full_fields(), &FakeNames) {
            Translation::Unresolvable(_) => {}
            other => panic!("expected Unresolvable, got {other:?}"),
        }
    }

    #[test]
    fn brand_translates_when_resolvable() {
        let c = ResolvedConstraint::Structural(StructuralConstraint::Brand(BrandId(1)));
        match translate_constraint(&c, &full_fields(), &FakeNames) {
            Translation::Fq(fq) => assert_eq!(fq, "brand:/[nN][iI][kK][eE]/"),
            other => panic!("expected Fq, got {other:?}"),
        }
    }

    #[test]
    fn brand_any_ors_every_resolved_name() {
        let c = ResolvedConstraint::Structural(StructuralConstraint::BrandAny(vec![
            BrandId(1),
            BrandId(2),
        ]));
        match translate_constraint(&c, &full_fields(), &FakeNames) {
            Translation::Fq(fq) => {
                assert!(fq.starts_with("brand:/("));
                assert!(fq.contains('|'));
            }
            other => panic!("expected Fq, got {other:?}"),
        }
    }

    #[test]
    fn brand_any_is_unresolvable_not_partial_when_one_id_is_unnamed() {
        // The exact bug class this exists to prevent: a partial OR would
        // silently narrow the filter relative to native's own
        // `ids.contains(&product.brand)`, which admits every id.
        let c = ResolvedConstraint::Structural(StructuralConstraint::BrandAny(vec![
            BrandId(1),
            BrandId(999),
        ]));
        match translate_constraint(&c, &full_fields(), &FakeNames) {
            Translation::Unresolvable(_) => {}
            other => panic!("expected Unresolvable, got {other:?}"),
        }
    }

    #[test]
    fn product_type_any_translates_this_is_the_bug_that_shipped_twice() {
        // docs/decisions/ISSUE55_PAIRED_COMPARATOR_DECISION.md: a missing
        // match arm for exactly this variant shipped to production twice
        // independently. This test exists so it cannot regress silently.
        let c = ResolvedConstraint::Structural(StructuralConstraint::ProductTypeAny(vec![
            ProductTypeId(1),
            ProductTypeId(2),
        ]));
        match translate_constraint(&c, &full_fields(), &FakeNames) {
            Translation::Fq(fq) => assert!(fq.starts_with("product_class:/(")),
            other => panic!("expected Fq, got {other:?}"),
        }
    }

    #[test]
    fn category_not_applicable_when_no_field_configured() {
        let c = ResolvedConstraint::Structural(StructuralConstraint::Category(CategoryId(1)));
        let mut fields = full_fields();
        fields.category = None;
        assert_eq!(
            translate_constraint(&c, &fields, &FakeNames),
            Translation::NotApplicable
        );
    }

    #[test]
    fn price_under_is_upper_exclusive_range() {
        let c = ResolvedConstraint::Structural(StructuralConstraint::PriceUnderCents(500));
        match translate_constraint(&c, &full_fields(), &NoNames) {
            Translation::Fq(fq) => assert_eq!(fq, "price_cents:[* TO 500}"),
            other => panic!("expected Fq, got {other:?}"),
        }
    }

    #[test]
    fn price_over_is_lower_exclusive_range() {
        let c = ResolvedConstraint::Structural(StructuralConstraint::PriceOverCents(500));
        match translate_constraint(&c, &full_fields(), &NoNames) {
            Translation::Fq(fq) => assert_eq!(fq, "price_cents:{500 TO *]"),
            other => panic!("expected Fq, got {other:?}"),
        }
    }

    #[test]
    fn enum_attribute_translates_by_attribute_name() {
        let c = ResolvedConstraint::Attribute(Constraint::Enum {
            attribute: "color".to_string(),
            value: "Black".to_string(),
        });
        match translate_constraint(&c, &full_fields(), &NoNames) {
            Translation::Fq(fq) => assert_eq!(fq, "color:/[bB][lL][aA][cC][kK]/"),
            other => panic!("expected Fq, got {other:?}"),
        }
    }

    #[test]
    fn multi_enum_contains_translates_by_attribute_name() {
        let c = ResolvedConstraint::Attribute(Constraint::MultiEnumContains {
            attribute: "materials".to_string(),
            value: "Oak".to_string(),
        });
        match translate_constraint(&c, &full_fields(), &NoNames) {
            Translation::Fq(fq) => assert_eq!(fq, "materials:/[oO][aA][kK]/"),
            other => panic!("expected Fq, got {other:?}"),
        }
    }

    #[test]
    fn boolean_attribute_translates_literally() {
        let c = ResolvedConstraint::Attribute(Constraint::Boolean {
            attribute: "waterproof".to_string(),
            value: true,
        });
        match translate_constraint(&c, &full_fields(), &NoNames) {
            Translation::Fq(fq) => assert_eq!(fq, "waterproof:true"),
            other => panic!("expected Fq, got {other:?}"),
        }
    }

    #[test]
    fn numeric_gte_translates_to_lower_inclusive_range() {
        let c = ResolvedConstraint::Attribute(Constraint::Numeric {
            attribute: "voltage".to_string(),
            op: NumericOp::Gte,
            value: 12.0,
        });
        match translate_constraint(&c, &full_fields(), &NoNames) {
            Translation::Fq(fq) => assert_eq!(fq, "voltage:[12 TO *]"),
            other => panic!("expected Fq, got {other:?}"),
        }
    }

    #[test]
    fn numeric_lt_translates_to_upper_exclusive_range() {
        let c = ResolvedConstraint::Attribute(Constraint::Numeric {
            attribute: "voltage".to_string(),
            op: NumericOp::Lt,
            value: 12.0,
        });
        match translate_constraint(&c, &full_fields(), &NoNames) {
            Translation::Fq(fq) => assert_eq!(fq, "voltage:[* TO 12}"),
            other => panic!("expected Fq, got {other:?}"),
        }
    }

    #[test]
    fn text_contains_translates_to_a_substring_regex() {
        let c = ResolvedConstraint::Attribute(Constraint::Text {
            attribute: "description".to_string(),
            contains: "waterproof".to_string(),
        });
        match translate_constraint(&c, &full_fields(), &NoNames) {
            Translation::Fq(fq) => {
                assert!(fq.starts_with("description:/.*"));
                assert!(fq.ends_with(".*/"));
            }
            other => panic!("expected Fq, got {other:?}"),
        }
    }

    #[test]
    fn translate_all_separates_fq_from_unresolvable_failures() {
        let constraints = vec![
            ResolvedConstraint::Structural(StructuralConstraint::Brand(BrandId(1))),
            ResolvedConstraint::Structural(StructuralConstraint::Brand(BrandId(999))),
            ResolvedConstraint::Attribute(Constraint::Enum {
                attribute: "color".to_string(),
                value: "Black".to_string(),
            }),
        ];
        let (fq, failures) = translate_all(&constraints, &full_fields(), &FakeNames);
        assert_eq!(fq.len(), 2, "the two resolvable constraints: {fq:?}");
        assert_eq!(
            failures.len(),
            1,
            "the one unresolvable Brand(999): {failures:?}"
        );
    }
}
