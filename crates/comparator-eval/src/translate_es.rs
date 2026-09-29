//! One exhaustive, symmetric translation from every
//! `commerce_core::ir::ResolvedConstraint` shape to an Elasticsearch/
//! OpenSearch `bool` query filter clause.
//!
//! Sibling of [`crate::translate`] (Solr's regex-`fq` translator), not a
//! replacement -- reuses that module's [`crate::translate::SolrFieldMap`]
//! and [`crate::translate::StructuralNames`] unchanged (this crate's
//! per-dataset field maps use the same field *names* across every engine
//! by construction: every `scripts/datasets/*_index_*.py` indexer in this
//! workspace ingests the identical `dataset_cache/*/catalog.jsonl` source
//! and maps each commerce dimension to the same field name regardless of
//! backend, so one `SolrFieldMap` value is the correct field-name source
//! of truth for every engine). Only the *clause syntax* is engine-
//! specific, not the field-name resolution.
//!
//! Case-insensitive whole-field matching, which Solr achieves with a
//! `field:/[cC][aA]...[eE]/` regex (see [`crate::solr::case_insensitive_field_regex`]),
//! is achieved here by lower-casing both the indexed value (at index
//! time, in `scripts/datasets/es_family_index_wands.py` et al. -- see
//! each indexer's own doc comment) and the query value (here), then using
//! a plain `term`/`terms` query. This is the ES/OpenSearch-idiomatic
//! equivalent of the same case-insensitive-whole-match semantic, not a
//! weaker approximation of it: both constructions match exactly the same
//! document set for the same real-world input, which is what the
//! Issue #57 fairness contract requires (equivalent semantics, not
//! byte-identical query syntax).

use commerce_core::domain::{Constraint, NumericOp};
use commerce_core::ir::{ResolvedConstraint, StructuralConstraint};
use serde_json::{json, Value};

use crate::translate::{SolrFieldMap, StructuralNames};

/// The result of translating one [`ResolvedConstraint`] into an ES/
/// OpenSearch filter clause. Mirrors [`crate::translate::Translation`]
/// exactly (same three-way shape, same caller obligations) -- see that
/// type's doc comments for what each variant means and requires.
#[derive(Debug, Clone, PartialEq)]
pub enum EsTranslation {
    Filter(Value),
    NotApplicable,
    Unresolvable(String),
}

/// Translates one resolved constraint. No wildcard arm, for the identical
/// reason [`crate::translate::translate_constraint`] has none: a new
/// `ResolvedConstraint`/`StructuralConstraint`/`Constraint` variant must
/// fail to *compile* here, not silently under-filter Elasticsearch
/// relative to native.
pub fn translate_constraint_es(
    c: &ResolvedConstraint,
    fields: &SolrFieldMap,
    names: &dyn StructuralNames,
) -> EsTranslation {
    match c {
        ResolvedConstraint::Structural(s) => translate_structural(s, fields, names),
        ResolvedConstraint::Attribute(a) => translate_attribute(a),
    }
}

fn translate_structural(
    c: &StructuralConstraint,
    fields: &SolrFieldMap,
    names: &dyn StructuralNames,
) -> EsTranslation {
    match c {
        StructuralConstraint::Brand(id) => {
            let Some(field) = fields.brand else {
                return EsTranslation::NotApplicable;
            };
            match names.brand_name(*id) {
                Some(name) => EsTranslation::Filter(json!({"term": {field: name.to_lowercase()}})),
                None => EsTranslation::Unresolvable(format!("no brand name registered for {id:?}")),
            }
        }
        StructuralConstraint::BrandAny(ids) => {
            let Some(field) = fields.brand else {
                return EsTranslation::NotApplicable;
            };
            translate_any(field, ids, |id| names.brand_name(*id))
        }
        StructuralConstraint::ProductType(id) => {
            let Some(field) = fields.product_type else {
                return EsTranslation::NotApplicable;
            };
            match names.product_type_name(*id) {
                Some(name) => EsTranslation::Filter(json!({"term": {field: name.to_lowercase()}})),
                None => EsTranslation::Unresolvable(format!(
                    "no product_type name registered for {id:?}"
                )),
            }
        }
        StructuralConstraint::ProductTypeAny(ids) => {
            let Some(field) = fields.product_type else {
                return EsTranslation::NotApplicable;
            };
            translate_any(field, ids, |id| names.product_type_name(*id))
        }
        StructuralConstraint::Category(id) => {
            let Some(field) = fields.category else {
                return EsTranslation::NotApplicable;
            };
            match names.category_name(*id) {
                Some(name) => EsTranslation::Filter(json!({"term": {field: name.to_lowercase()}})),
                None => {
                    EsTranslation::Unresolvable(format!("no category name registered for {id:?}"))
                }
            }
        }
        StructuralConstraint::PriceUnderCents(cents) => {
            let Some(field) = fields.price_cents else {
                return EsTranslation::NotApplicable;
            };
            EsTranslation::Filter(json!({"range": {field: {"lt": cents}}}))
        }
        StructuralConstraint::PriceOverCents(cents) => {
            let Some(field) = fields.price_cents else {
                return EsTranslation::NotApplicable;
            };
            EsTranslation::Filter(json!({"range": {field: {"gt": cents}}}))
        }
    }
}

/// Shared terms-query construction for `BrandAny`/`ProductTypeAny`: every
/// id must resolve to a name -- a partial resolution would silently
/// narrow the filter relative to native's own `ids.contains(&product.brand)`,
/// exactly the failure mode [`crate::translate::translate_any`] guards
/// against for Solr.
fn translate_any<'a, Id: std::fmt::Debug>(
    field: &str,
    ids: &[Id],
    resolve: impl Fn(&Id) -> Option<&'a str>,
) -> EsTranslation {
    if ids.is_empty() {
        return EsTranslation::Unresolvable(format!("empty id group for field {field}"));
    }
    let mut names = Vec::with_capacity(ids.len());
    for id in ids {
        match resolve(id) {
            Some(name) => names.push(name.to_lowercase()),
            None => {
                return EsTranslation::Unresolvable(format!(
                    "no name registered for {id:?} (field {field}), {}/{} ids resolved so far",
                    names.len(),
                    ids.len()
                ))
            }
        }
    }
    EsTranslation::Filter(json!({"terms": {field: names}}))
}

fn translate_attribute(c: &Constraint) -> EsTranslation {
    match c {
        Constraint::Enum { attribute, value } => {
            EsTranslation::Filter(json!({"term": {attribute.as_str(): value.to_lowercase()}}))
        }
        Constraint::MultiEnumContains { attribute, value } => {
            // A `term` query against a multi-valued keyword field matches
            // if ANY element equals the term -- exactly native's
            // `values.contains(value)` semantics, no special OR needed.
            EsTranslation::Filter(json!({"term": {attribute.as_str(): value.to_lowercase()}}))
        }
        Constraint::Boolean { attribute, value } => {
            EsTranslation::Filter(json!({"term": {attribute.as_str(): *value}}))
        }
        Constraint::Numeric {
            attribute,
            op,
            value,
        } => {
            let filter = match op {
                NumericOp::Eq => json!({"term": {attribute.as_str(): value}}),
                NumericOp::Lt => json!({"range": {attribute.as_str(): {"lt": value}}}),
                NumericOp::Lte => json!({"range": {attribute.as_str(): {"lte": value}}}),
                NumericOp::Gt => json!({"range": {attribute.as_str(): {"gt": value}}}),
                NumericOp::Gte => json!({"range": {attribute.as_str(): {"gte": value}}}),
            };
            EsTranslation::Filter(filter)
        }
        Constraint::Text {
            attribute,
            contains,
        } => EsTranslation::Filter(json!({
            "wildcard": {attribute.as_str(): {"value": format!("*{}*", contains.to_lowercase())}}
        })),
    }
}

/// Translates every constraint, returning the built filter-clause list
/// (each a JSON-serialized string -- see [`crate::elasticsearch`]'s doc
/// comment for why [`crate::solr::EngineComparator::search`]'s `fq: &[String]`
/// shape is kept literally, not loosened to `Vec<Value>`) and a *separate*
/// list of [`EsTranslation::Unresolvable`] failures, mirroring
/// [`crate::translate::translate_all`]'s contract exactly.
pub fn translate_all_es(
    constraints: &[ResolvedConstraint],
    fields: &SolrFieldMap,
    names: &dyn StructuralNames,
) -> (Vec<String>, Vec<String>) {
    let mut filters = Vec::new();
    let mut failures = Vec::new();
    for c in constraints {
        match translate_constraint_es(c, fields, names) {
            EsTranslation::Filter(clause) => filters.push(clause.to_string()),
            EsTranslation::NotApplicable => {}
            EsTranslation::Unresolvable(reason) => failures.push(reason),
        }
    }
    (filters, failures)
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
    fn brand_translates_to_lowercased_term() {
        let c = ResolvedConstraint::Structural(StructuralConstraint::Brand(BrandId(1)));
        match translate_constraint_es(&c, &full_fields(), &FakeNames) {
            EsTranslation::Filter(v) => assert_eq!(v, json!({"term": {"brand": "nike"}})),
            other => panic!("expected Filter, got {other:?}"),
        }
    }

    #[test]
    fn brand_any_becomes_a_terms_query_of_every_resolved_name() {
        let c = ResolvedConstraint::Structural(StructuralConstraint::BrandAny(vec![
            BrandId(1),
            BrandId(2),
        ]));
        match translate_constraint_es(&c, &full_fields(), &FakeNames) {
            EsTranslation::Filter(v) => {
                assert_eq!(v, json!({"terms": {"brand": ["nike", "adidas"]}}))
            }
            other => panic!("expected Filter, got {other:?}"),
        }
    }

    #[test]
    fn brand_any_is_unresolvable_not_partial_when_one_id_is_unnamed() {
        let c = ResolvedConstraint::Structural(StructuralConstraint::BrandAny(vec![
            BrandId(1),
            BrandId(999),
        ]));
        match translate_constraint_es(&c, &full_fields(), &FakeNames) {
            EsTranslation::Unresolvable(_) => {}
            other => panic!("expected Unresolvable, got {other:?}"),
        }
    }

    #[test]
    fn product_type_any_translates_the_bug_class_translate_rs_guards_against() {
        let c = ResolvedConstraint::Structural(StructuralConstraint::ProductTypeAny(vec![
            ProductTypeId(1),
        ]));
        match translate_constraint_es(&c, &full_fields(), &FakeNames) {
            EsTranslation::Filter(v) => {
                assert_eq!(v, json!({"terms": {"product_class": ["beds"]}}))
            }
            other => panic!("expected Filter, got {other:?}"),
        }
    }

    #[test]
    fn not_applicable_when_no_field_configured() {
        let c = ResolvedConstraint::Structural(StructuralConstraint::Brand(BrandId(1)));
        let empty = SolrFieldMap::default();
        assert_eq!(
            translate_constraint_es(&c, &empty, &FakeNames),
            EsTranslation::NotApplicable
        );
    }

    #[test]
    fn price_under_is_strict_less_than() {
        let c = ResolvedConstraint::Structural(StructuralConstraint::PriceUnderCents(500));
        match translate_constraint_es(&c, &full_fields(), &FakeNames) {
            EsTranslation::Filter(v) => {
                assert_eq!(v, json!({"range": {"price_cents": {"lt": 500}}}))
            }
            other => panic!("expected Filter, got {other:?}"),
        }
    }

    #[test]
    fn enum_attribute_translates_by_attribute_name_lowercased() {
        let c = ResolvedConstraint::Attribute(Constraint::Enum {
            attribute: "color".to_string(),
            value: "Black".to_string(),
        });
        match translate_constraint_es(&c, &full_fields(), &FakeNames) {
            EsTranslation::Filter(v) => assert_eq!(v, json!({"term": {"color": "black"}})),
            other => panic!("expected Filter, got {other:?}"),
        }
    }

    #[test]
    fn numeric_gte_translates_to_lower_inclusive_range() {
        let c = ResolvedConstraint::Attribute(Constraint::Numeric {
            attribute: "voltage".to_string(),
            op: NumericOp::Gte,
            value: 12.0,
        });
        match translate_constraint_es(&c, &full_fields(), &FakeNames) {
            EsTranslation::Filter(v) => {
                assert_eq!(v, json!({"range": {"voltage": {"gte": 12.0}}}))
            }
            other => panic!("expected Filter, got {other:?}"),
        }
    }

    #[test]
    fn text_contains_translates_to_a_lowercased_wildcard() {
        let c = ResolvedConstraint::Attribute(Constraint::Text {
            attribute: "description".to_string(),
            contains: "Waterproof".to_string(),
        });
        match translate_constraint_es(&c, &full_fields(), &FakeNames) {
            EsTranslation::Filter(v) => assert_eq!(
                v,
                json!({"wildcard": {"description": {"value": "*waterproof*"}}})
            ),
            other => panic!("expected Filter, got {other:?}"),
        }
    }

    #[test]
    fn translate_all_es_separates_filters_from_unresolvable_failures() {
        let constraints = vec![
            ResolvedConstraint::Structural(StructuralConstraint::Brand(BrandId(1))),
            ResolvedConstraint::Structural(StructuralConstraint::Brand(BrandId(999))),
            ResolvedConstraint::Attribute(Constraint::Enum {
                attribute: "color".to_string(),
                value: "Black".to_string(),
            }),
        ];
        let (filters, failures) = translate_all_es(&constraints, &full_fields(), &FakeNames);
        assert_eq!(
            filters.len(),
            2,
            "the two resolvable constraints: {filters:?}"
        );
        assert_eq!(
            failures.len(),
            1,
            "the one unresolvable Brand(999): {failures:?}"
        );
    }
}
