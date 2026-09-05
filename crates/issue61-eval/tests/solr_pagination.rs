use issue61_eval::{
    fetch_complete_solr, prepare_solr_request, AdmissionClass, CursorCollector, CursorDecision,
    EngineOutcome, FrozenQuery, SolrPage, SolrRequest,
};
use std::collections::BTreeMap;
use std::time::Duration;

#[path = "support/http_capture.rs"]
mod http_capture;

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
fn audit_solr_get_transmits_frozen_contract_with_only_pagination_overrides() {
    // Given
    let mut query = frozen_with_params(&[
        ("defType", "edismax"),
        ("qf", "title text"),
        ("fl", "id"),
        ("rows", "10"),
        ("sort", "id asc"),
        ("wt", "json"),
    ]);
    let frozen = query.solr.as_mut().expect("Solr request block");
    frozen.q = "red chair".to_string();
    frozen.fq = vec![
        "product_class_lc:\"chairs\"".to_string(),
        "color_lc:\"red\"".to_string(),
    ];
    let response = r#"{"responseHeader":{"status":0},"response":{"numFound":1,"docs":[{"id":"P1"}]},"nextCursorMark":"*"}"#;
    let (core_url, capture) = http_capture::spawn_capture_server(response);
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(2))
        .build();

    // When
    let outcome = fetch_complete_solr(&agent, &core_url, &query);
    let captured = capture.join().expect("capture server succeeds");

    // Then
    assert_eq!(outcome, EngineOutcome::Ids(vec!["P1".to_string()]));
    assert_eq!(captured.path, "/select");
    assert_eq!(
        captured.params,
        BTreeMap::from([
            ("cursorMark".to_string(), vec!["*".to_string()]),
            ("defType".to_string(), vec!["edismax".to_string()]),
            ("fl".to_string(), vec!["id".to_string()]),
            (
                "fq".to_string(),
                vec![
                    "product_class_lc:\"chairs\"".to_string(),
                    "color_lc:\"red\"".to_string(),
                ],
            ),
            ("q".to_string(), vec!["red chair".to_string()]),
            ("qf".to_string(), vec!["title text".to_string()]),
            ("rows".to_string(), vec!["5000".to_string()]),
            ("sort".to_string(), vec!["id asc".to_string()]),
            ("wt".to_string(), vec!["json".to_string()]),
        ])
    );
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
