use super::*;
use std::collections::BTreeMap;

fn frozen_record() -> FrozenQuery {
    serde_json::from_str(
        r#"{
            "query_id":"q-structural",
            "text":"red beds",
            "admission_class":"Hybrid",
            "structural_constraint_count":2,
            "has_residual_lexical":true,
            "rows":99,
            "native":{"q":"red beds","params":{"fl":"id","rows":"7"}},
            "solr":{
                "q":"red",
                "fq":["product_class_lc:\"beds\"","color_lc:\"red\""],
                "params":{"defType":"edismax","qf":"title","fl":"id","rows":"10"}
            }
        }"#,
    )
    .expect("valid frozen record")
}

#[test]
fn solr_request_includes_every_frozen_fq_as_a_repeated_parameter() {
    let request = build_request(&frozen_record(), RequestEngine::Solr).expect("complete record");

    assert_eq!(
        request.fq,
        [
            "product_class_lc:\"beds\"".to_string(),
            "color_lc:\"red\"".to_string()
        ]
    );
}

#[test]
fn solr_request_includes_deftype_qf_and_fl_id_from_the_frozen_params() {
    let request = build_request(&frozen_record(), RequestEngine::Solr).expect("complete record");

    assert_eq!(
        request.params.get("defType").map(String::as_str),
        Some("edismax")
    );
    assert_eq!(request.params.get("qf").map(String::as_str), Some("title"));
    assert_eq!(request.params.get("fl").map(String::as_str), Some("id"));
}

#[test]
fn native_request_replays_its_frozen_params_verbatim() {
    let request = build_request(&frozen_record(), RequestEngine::Native).expect("complete record");

    assert_eq!(request.q, "red beds");
    assert!(request.fq.is_empty());
    assert_eq!(
        request.params,
        BTreeMap::from([
            ("fl".to_string(), "id".to_string()),
            ("rows".to_string(), "7".to_string()),
        ])
    );
}

#[test]
fn a_record_missing_its_per_engine_block_is_an_error_not_a_bare_q_fallback() {
    let record: FrozenQuery = serde_json::from_str(
        r#"{"query_id":"q-missing","text":"beds","admission_class":"FastPath","structural_constraint_count":1,"has_residual_lexical":false,"rows":10,"native":{"q":"beds","params":{"rows":"10"}}}"#,
    )
    .expect("missing engine blocks remain parseable for a query-specific error");

    let error = build_request(&record, RequestEngine::Solr).expect_err("missing Solr request");

    assert!(error.contains("q-missing"));
}

#[test]
fn validate_response_records_num_found() {
    let body = r#"{"responseHeader":{"status":0},"response":{"numFound":12,"docs":[{"id":"P1"}]}}"#;

    let validated = validate_response(body).expect("valid response");

    assert_eq!(validated.num_found, 12);
}

#[test]
fn response_validation_rejects_nonzero_status_missing_docs_and_non_string_ids() {
    assert!(
        validate_response(r#"{"responseHeader":{"status":500},"error":{"msg":"bad"}}"#).is_err()
    );
    assert!(
        validate_response(r#"{"responseHeader":{"status":0},"response":{"numFound":0}}"#).is_err()
    );
    assert!(validate_response(
        r#"{"responseHeader":{"status":0},"response":{"numFound":1,"docs":[{"id":7}]}}"#
    )
    .is_err());
}

#[test]
fn response_validation_accepts_solr_shape_and_returns_total_count() {
    assert_eq!(
        validate_response(
            r#"{"responseHeader":{"status":0},"response":{"numFound":12,"docs":[{"id":"P1"}]}}"#
        )
        .expect("valid response")
        .num_found,
        12
    );
}
