use super::*;
use commerce_core::domain::{BrandId, CategoryId, ProductTypeId};
use commerce_core::ir::{ResolvedConstraint, StructuralConstraint};

struct TestNames;

impl StructuralNames for TestNames {
    fn brand_name(&self, id: BrandId) -> Option<&str> {
        (id == BrandId(1)).then_some("Acme")
    }

    fn product_type_name(&self, id: ProductTypeId) -> Option<&str> {
        (id == ProductTypeId(1)).then_some("Beds")
    }

    fn category_name(&self, id: CategoryId) -> Option<&str> {
        (id == CategoryId(1)).then_some("Bedroom")
    }
}

fn freeze<'a>(query_id: &'a str, text: &'a str, compiled: &'a CommerceQuery) -> FreezeRequest<'a> {
    FreezeRequest {
        query_id,
        text,
        compiled,
        dataset: Dataset::Wands,
        names: &TestNames,
    }
}

#[test]
fn wands_solr_request_freezes_the_complete_e1_parameter_map() {
    let compiled = CommerceQuery {
        residual_lexical: vec!["desk".to_string()],
        ..CommerceQuery::default()
    };

    let requests = freeze_engine_requests(freeze("q-contract", "desk", &compiled))
        .expect("valid lexical request");

    let expected = serde_json::from_str(r#"{"defType":"edismax","fl":"id","lowercaseOperators":"false","mm":"100%","mm.autoRelax":"false","ps":"0","ps2":"0","ps3":"0","q.op":"AND","qf":"title description","qs":"0","rows":"10","sort":"id asc","sow":"true","tie":"0.0","wt":"json"}"#)
        .expect("valid expected parameter map");
    assert_eq!(requests.solr.params, expected);
    assert_eq!(requests.solr.params.len(), 16);
}

#[test]
fn esci_solr_request_uses_the_dataset_lexical_field_scope() {
    let compiled = CommerceQuery {
        residual_lexical: vec!["desk".to_string()],
        ..CommerceQuery::default()
    };
    let input = FreezeRequest {
        dataset: Dataset::EsciElectronics,
        ..freeze("q-esci", "desk", &compiled)
    };

    let requests = freeze_engine_requests(input).expect("valid ESCI request");

    assert_eq!(
        requests.solr.params.get("qf").map(String::as_str),
        Some("title description bullet_point")
    );
}

#[test]
fn residual_edismax_text_escapes_backslash_and_every_lucene_metacharacter() {
    assert_eq!(
        escape_edismax_literal(r#"a\b+c-d&&e||f!g(h)i{j}k[l]m^n"o~p*q?r:s/t"#),
        r#"a\\b\+c\-d\&\&e\|\|f\!g\(h\)i\{j\}k\[l\]m\^n\"o\~p\*q\?r\:s\/t"#
    );
}

#[test]
fn structural_constraints_remain_lowercase_companion_filters() {
    let compiled = CommerceQuery {
        constraints: vec![
            ResolvedConstraint::Structural(StructuralConstraint::ProductType(ProductTypeId(1))),
            ResolvedConstraint::Structural(StructuralConstraint::Category(CategoryId(1))),
        ],
        residual_lexical: vec!["red".to_string()],
        ..CommerceQuery::default()
    };

    let requests = freeze_engine_requests(freeze("q1", "red beds", &compiled))
        .expect("resolvable constraints");

    assert_eq!(
        requests.solr.fq,
        [
            "product_class_lc:\"beds\"".to_string(),
            "category_leaf_lc:\"bedroom\"".to_string()
        ]
    );
}

#[test]
fn empty_residual_uses_match_all_only_with_a_structural_filter() {
    let compiled = CommerceQuery {
        constraints: vec![ResolvedConstraint::Structural(
            StructuralConstraint::ProductType(ProductTypeId(1)),
        )],
        ..CommerceQuery::default()
    };

    let requests =
        freeze_engine_requests(freeze("q3", "beds", &compiled)).expect("resolvable constraint");

    assert_eq!(requests.solr.q, "*:*");
}

#[test]
fn nonempty_query_without_executable_terms_uses_match_all() {
    let requests =
        freeze_engine_requests(freeze("q-ambiguous", "marble", &CommerceQuery::default()))
            .expect("native executes the unconstrained candidate set");

    assert_eq!(requests.solr.q, "*:*");
    assert!(requests.solr.fq.is_empty());
}

#[test]
fn query_without_residual_terms_or_structural_filters_fails_the_freeze() {
    let error = freeze_engine_requests(freeze("q-empty", "", &CommerceQuery::default()))
        .expect_err("empty semantic query must fail");

    assert!(error.contains("q-empty"));
}

#[test]
fn unresolvable_constraint_fails_instead_of_emitting_partial_filters() {
    let compiled = CommerceQuery {
        constraints: vec![
            ResolvedConstraint::Structural(StructuralConstraint::ProductType(ProductTypeId(1))),
            ResolvedConstraint::Structural(StructuralConstraint::Category(CategoryId(99))),
        ],
        ..CommerceQuery::default()
    };

    let error = freeze_engine_requests(freeze("q-unresolvable", "beds", &compiled))
        .expect_err("partial translation must fail");

    assert!(error.contains("q-unresolvable"));
}
