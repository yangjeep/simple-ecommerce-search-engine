//! One exhaustive, symmetric translation from every
//! `commerce_core::ir::ResolvedConstraint` shape to a Havenask SQL `WHERE`
//! clause fragment. Sibling of [`crate::translate`] (Solr) and
//! [`crate::translate_es`] (Elasticsearch/OpenSearch) -- same field-name
//! reuse via [`crate::translate::SolrFieldMap`]/[`crate::translate::StructuralNames`],
//! same no-wildcard-arm discipline, same case-insensitivity strategy
//! (lower-case both the indexed value and the query value -- see
//! `translate_es`'s doc comment for why this is a faithful equivalent,
//! not a weaker approximation, of Solr's case-insensitive regex).
//!
//! Unlike the ES translator, Havenask's `fq`-equivalent clauses are
//! already the wire-native form (a SQL text fragment), so no JSON
//! serialize/reparse round trip is needed -- [`translate_all_havenask`]'s
//! output strings are used by [`crate::havenask::HavenaskComparator`]
//! directly, joined with `AND`.

use commerce_core::domain::{Constraint, NumericOp};
use commerce_core::ir::{ResolvedConstraint, StructuralConstraint};

use crate::translate::{SolrFieldMap, StructuralNames};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HavenaskTranslation {
    Where(String),
    NotApplicable,
    Unresolvable(String),
}

/// Escapes a value for embedding in a Havenask SQL string literal.
/// Havenask's SQL layer follows standard SQL single-quote doubling; this
/// is the same escaping discipline `commerce-core`'s own callers use
/// nowhere else in this crate (Solr/ES use regex/JSON escaping instead),
/// so it is defined here rather than shared.
fn escape_sql_literal(s: &str) -> String {
    s.replace('\'', "''")
}

pub fn translate_constraint_havenask(
    c: &ResolvedConstraint,
    fields: &SolrFieldMap,
    names: &dyn StructuralNames,
) -> HavenaskTranslation {
    match c {
        ResolvedConstraint::Structural(s) => translate_structural(s, fields, names),
        ResolvedConstraint::Attribute(a) => translate_attribute(a),
    }
}

fn translate_structural(
    c: &StructuralConstraint,
    fields: &SolrFieldMap,
    names: &dyn StructuralNames,
) -> HavenaskTranslation {
    match c {
        StructuralConstraint::Brand(id) => {
            let Some(field) = fields.brand else {
                return HavenaskTranslation::NotApplicable;
            };
            match names.brand_name(*id) {
                Some(name) => HavenaskTranslation::Where(format!(
                    "{field} = '{}'",
                    escape_sql_literal(&name.to_lowercase())
                )),
                None => HavenaskTranslation::Unresolvable(format!(
                    "no brand name registered for {id:?}"
                )),
            }
        }
        StructuralConstraint::BrandAny(ids) => {
            let Some(field) = fields.brand else {
                return HavenaskTranslation::NotApplicable;
            };
            translate_any(field, ids, |id| names.brand_name(*id))
        }
        StructuralConstraint::ProductType(id) => {
            let Some(field) = fields.product_type else {
                return HavenaskTranslation::NotApplicable;
            };
            match names.product_type_name(*id) {
                Some(name) => HavenaskTranslation::Where(format!(
                    "{field} = '{}'",
                    escape_sql_literal(&name.to_lowercase())
                )),
                None => HavenaskTranslation::Unresolvable(format!(
                    "no product_type name registered for {id:?}"
                )),
            }
        }
        StructuralConstraint::ProductTypeAny(ids) => {
            let Some(field) = fields.product_type else {
                return HavenaskTranslation::NotApplicable;
            };
            translate_any(field, ids, |id| names.product_type_name(*id))
        }
        StructuralConstraint::Category(id) => {
            let Some(field) = fields.category else {
                return HavenaskTranslation::NotApplicable;
            };
            match names.category_name(*id) {
                Some(name) => HavenaskTranslation::Where(format!(
                    "{field} = '{}'",
                    escape_sql_literal(&name.to_lowercase())
                )),
                None => HavenaskTranslation::Unresolvable(format!(
                    "no category name registered for {id:?}"
                )),
            }
        }
        StructuralConstraint::PriceUnderCents(cents) => {
            let Some(field) = fields.price_cents else {
                return HavenaskTranslation::NotApplicable;
            };
            HavenaskTranslation::Where(format!("{field} < {cents}"))
        }
        StructuralConstraint::PriceOverCents(cents) => {
            let Some(field) = fields.price_cents else {
                return HavenaskTranslation::NotApplicable;
            };
            HavenaskTranslation::Where(format!("{field} > {cents}"))
        }
    }
}

fn translate_any<'a, Id: std::fmt::Debug>(
    field: &str,
    ids: &[Id],
    resolve: impl Fn(&Id) -> Option<&'a str>,
) -> HavenaskTranslation {
    if ids.is_empty() {
        return HavenaskTranslation::Unresolvable(format!("empty id group for field {field}"));
    }
    let mut names = Vec::with_capacity(ids.len());
    for id in ids {
        match resolve(id) {
            Some(name) => names.push(format!("'{}'", escape_sql_literal(&name.to_lowercase()))),
            None => {
                return HavenaskTranslation::Unresolvable(format!(
                    "no name registered for {id:?} (field {field}), {}/{} ids resolved so far",
                    names.len(),
                    ids.len()
                ))
            }
        }
    }
    HavenaskTranslation::Where(format!("{field} IN ({})", names.join(", ")))
}

fn translate_attribute(c: &Constraint) -> HavenaskTranslation {
    match c {
        Constraint::Enum { attribute, value } => HavenaskTranslation::Where(format!(
            "{attribute} = '{}'",
            escape_sql_literal(&value.to_lowercase())
        )),
        Constraint::MultiEnumContains { attribute, value } => {
            // Havenask MULTI_STRING attribute columns support `=` against
            // any element of a multi-value column identically to a
            // scalar column in its SQL layer (documented Havenask
            // behavior for MULTI_* attribute types) -- same semantics as
            // ES's `term` against a multi-valued keyword field.
            HavenaskTranslation::Where(format!(
                "{attribute} = '{}'",
                escape_sql_literal(&value.to_lowercase())
            ))
        }
        Constraint::Boolean { attribute, value } => {
            HavenaskTranslation::Where(format!("{attribute} = {value}"))
        }
        Constraint::Numeric {
            attribute,
            op,
            value,
        } => {
            let clause = match op {
                NumericOp::Eq => format!("{attribute} = {value}"),
                NumericOp::Lt => format!("{attribute} < {value}"),
                NumericOp::Lte => format!("{attribute} <= {value}"),
                NumericOp::Gt => format!("{attribute} > {value}"),
                NumericOp::Gte => format!("{attribute} >= {value}"),
            };
            HavenaskTranslation::Where(clause)
        }
        Constraint::Text {
            attribute,
            contains,
        } => HavenaskTranslation::Where(format!(
            "{attribute} LIKE '%{}%'",
            escape_sql_literal(&contains.to_lowercase())
        )),
    }
}

pub fn translate_all_havenask(
    constraints: &[ResolvedConstraint],
    fields: &SolrFieldMap,
    names: &dyn StructuralNames,
) -> (Vec<String>, Vec<String>) {
    let mut clauses = Vec::new();
    let mut failures = Vec::new();
    for c in constraints {
        match translate_constraint_havenask(c, fields, names) {
            HavenaskTranslation::Where(clause) => clauses.push(clause),
            HavenaskTranslation::NotApplicable => {}
            HavenaskTranslation::Unresolvable(reason) => failures.push(reason),
        }
    }
    (clauses, failures)
}

#[cfg(test)]
mod tests {
    use super::*;
    use commerce_core::domain::{BrandId, CategoryId, ProductTypeId};

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
                1 => Some("Beds"),
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

    #[test]
    fn brand_translates_to_lowercased_equality() {
        let c = ResolvedConstraint::Structural(StructuralConstraint::Brand(BrandId(1)));
        match translate_constraint_havenask(&c, &full_fields(), &FakeNames) {
            HavenaskTranslation::Where(w) => assert_eq!(w, "brand = 'nike'"),
            other => panic!("expected Where, got {other:?}"),
        }
    }

    #[test]
    fn brand_any_becomes_an_in_list() {
        let c = ResolvedConstraint::Structural(StructuralConstraint::BrandAny(vec![
            BrandId(1),
            BrandId(2),
        ]));
        match translate_constraint_havenask(&c, &full_fields(), &FakeNames) {
            HavenaskTranslation::Where(w) => assert_eq!(w, "brand IN ('nike', 'adidas')"),
            other => panic!("expected Where, got {other:?}"),
        }
    }

    #[test]
    fn product_type_any_translates_the_bug_class_translate_rs_guards_against() {
        let c = ResolvedConstraint::Structural(StructuralConstraint::ProductTypeAny(vec![
            ProductTypeId(1),
        ]));
        match translate_constraint_havenask(&c, &full_fields(), &FakeNames) {
            HavenaskTranslation::Where(w) => assert_eq!(w, "product_class IN ('beds')"),
            other => panic!("expected Where, got {other:?}"),
        }
    }

    #[test]
    fn not_applicable_when_no_field_configured() {
        let c = ResolvedConstraint::Structural(StructuralConstraint::Brand(BrandId(1)));
        let empty = SolrFieldMap::default();
        assert_eq!(
            translate_constraint_havenask(&c, &empty, &FakeNames),
            HavenaskTranslation::NotApplicable
        );
    }

    #[test]
    fn unresolvable_when_field_configured_but_name_missing() {
        let c = ResolvedConstraint::Structural(StructuralConstraint::Brand(BrandId(999)));
        match translate_constraint_havenask(&c, &full_fields(), &FakeNames) {
            HavenaskTranslation::Unresolvable(_) => {}
            other => panic!("expected Unresolvable, got {other:?}"),
        }
    }

    #[test]
    fn price_under_is_strict_less_than() {
        let c = ResolvedConstraint::Structural(StructuralConstraint::PriceUnderCents(500));
        match translate_constraint_havenask(&c, &full_fields(), &FakeNames) {
            HavenaskTranslation::Where(w) => assert_eq!(w, "price_cents < 500"),
            other => panic!("expected Where, got {other:?}"),
        }
    }

    #[test]
    fn enum_attribute_translates_by_attribute_name_lowercased() {
        let c = ResolvedConstraint::Attribute(Constraint::Enum {
            attribute: "color".to_string(),
            value: "Black".to_string(),
        });
        match translate_constraint_havenask(&c, &full_fields(), &FakeNames) {
            HavenaskTranslation::Where(w) => assert_eq!(w, "color = 'black'"),
            other => panic!("expected Where, got {other:?}"),
        }
    }

    #[test]
    fn single_quotes_in_values_are_escaped_by_doubling() {
        let c = ResolvedConstraint::Attribute(Constraint::Enum {
            attribute: "style".to_string(),
            value: "Farmer's Market".to_string(),
        });
        match translate_constraint_havenask(&c, &full_fields(), &FakeNames) {
            HavenaskTranslation::Where(w) => assert_eq!(w, "style = 'farmer''s market'"),
            other => panic!("expected Where, got {other:?}"),
        }
    }

    #[test]
    fn text_contains_translates_to_a_lowercased_like() {
        let c = ResolvedConstraint::Attribute(Constraint::Text {
            attribute: "description".to_string(),
            contains: "Waterproof".to_string(),
        });
        match translate_constraint_havenask(&c, &full_fields(), &FakeNames) {
            HavenaskTranslation::Where(w) => assert_eq!(w, "description LIKE '%waterproof%'"),
            other => panic!("expected Where, got {other:?}"),
        }
    }

    #[test]
    fn translate_all_havenask_separates_clauses_from_unresolvable_failures() {
        let constraints = vec![
            ResolvedConstraint::Structural(StructuralConstraint::Brand(BrandId(1))),
            ResolvedConstraint::Structural(StructuralConstraint::Brand(BrandId(999))),
            ResolvedConstraint::Attribute(Constraint::Enum {
                attribute: "color".to_string(),
                value: "Black".to_string(),
            }),
        ];
        let (clauses, failures) = translate_all_havenask(&constraints, &full_fields(), &FakeNames);
        assert_eq!(
            clauses.len(),
            2,
            "the two resolvable constraints: {clauses:?}"
        );
        assert_eq!(
            failures.len(),
            1,
            "the one unresolvable Brand(999): {failures:?}"
        );
    }
}
