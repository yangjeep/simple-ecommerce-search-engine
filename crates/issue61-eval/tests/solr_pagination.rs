use issue61_eval::{
    prepare_solr_request, AdmissionClass, CursorCollector, CursorDecision, FrozenQuery, SolrPage,
    SolrRequest,
};
use std::collections::BTreeMap;

fn frozen_with_params(values: &[(&str, &str)]) -> FrozenQuery {
    FrozenQuery {
        query_id: "q1".to_string(),
        text: "chair".to_string(),
        admission_class: AdmissionClass::Hybrid,
        structural_constraint_count: 1,
        has_residual_lexical: true,
        rows: 10,
        native: None,
        solr: Some(SolrRequest {
            q: "chair".to_string(),
            fq: vec!["brand_lc:nike".to_string()],
            params: values
                .iter()
                .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
                .collect::<BTreeMap<_, _>>(),
        }),
    }
}

fn page(num_found: usize, ids: &[&str], next: &str) -> SolrPage {
    SolrPage {
        num_found,
        ids: ids.iter().map(|id| (*id).to_string()).collect(),
        next_cursor_mark: next.to_string(),
    }
}

#[test]
fn audit_request_replaces_only_rows_and_preserves_frozen_contract() {
    let query = frozen_with_params(&[
        ("defType", "edismax"),
        ("fl", "id"),
        ("rows", "10"),
        ("sort", "id asc"),
        ("wt", "json"),
    ]);

    let prepared = prepare_solr_request(&query).expect("valid frozen request");

    assert_eq!(prepared.params["rows"], "5000");
    assert_eq!(prepared.params["defType"], "edismax");
    assert_eq!(prepared.fq, vec!["brand_lc:nike"]);
}

#[test]
fn audit_request_rejects_non_id_sort() {
    let query = frozen_with_params(&[
        ("fl", "id"),
        ("rows", "10"),
        ("sort", "score desc,id asc"),
        ("wt", "json"),
    ]);

    let error = prepare_solr_request(&query).expect_err("sort must be fail-closed");

    assert!(error.contains("sort"));
}

#[test]
fn changing_num_found_across_pages_is_rejected() {
    let mut collector = CursorCollector::default();
    let first = collector
        .accept_page("*", page(2, &["a"], "next"))
        .expect("first page");
    assert_eq!(first, CursorDecision::Continue("next".to_string()));

    let error = collector
        .accept_page("next", page(3, &["b"], "next"))
        .expect_err("changed numFound must fail");

    assert!(error.contains("numFound changed"));
}

#[test]
fn duplicate_ids_across_pages_are_rejected() {
    let mut collector = CursorCollector::default();
    collector
        .accept_page("*", page(2, &["a"], "next"))
        .expect("first page");

    let error = collector
        .accept_page("next", page(2, &["a"], "next"))
        .expect_err("duplicate ids must fail");

    assert!(error.contains("duplicate id"));
}

#[test]
fn terminal_page_requires_collected_count_to_equal_num_found() {
    let mut collector = CursorCollector::default();

    let error = collector
        .accept_page("*", page(2, &["a"], "*"))
        .expect_err("incomplete terminal page must fail");

    assert!(error.contains("terminal count"));
}

#[test]
fn empty_non_terminal_page_is_rejected() {
    let mut collector = CursorCollector::default();

    let error = collector
        .accept_page("*", page(1, &[], "next"))
        .expect_err("empty advancing page must fail");

    assert!(error.contains("empty non-terminal"));
}

#[test]
fn empty_non_terminal_page_is_rejected_after_prior_results() {
    let mut collector = CursorCollector::default();
    collector
        .accept_page("*", page(2, &["a"], "next"))
        .expect("first page");

    let error = collector
        .accept_page("next", page(2, &[], "later"))
        .expect_err("every empty advancing page must fail");

    assert!(error.contains("empty non-terminal"));
}
