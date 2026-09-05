use super::*;
use issue61_eval::{Engine, FrozenQuery, SessionMode, SessionStep, WorkloadProjection};
use std::collections::BTreeMap;
use std::path::PathBuf;

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
    let request = build_request(&frozen_record(), Engine::Solr).expect("complete record");

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
    let request = build_request(&frozen_record(), Engine::Solr).expect("complete record");

    assert_eq!(
        request.params.get("defType").map(String::as_str),
        Some("edismax")
    );
    assert_eq!(request.params.get("qf").map(String::as_str), Some("title"));
    assert_eq!(request.params.get("fl").map(String::as_str), Some("id"));
}

#[test]
fn native_request_replays_its_frozen_params_verbatim() {
    let request = build_request(&frozen_record(), Engine::Native).expect("complete record");

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

    let error = build_request(&record, Engine::Solr).expect_err("missing Solr request");

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

#[test]
fn warm_session_executes_frozen_pass_and_counter_order() {
    // Given
    let mut observed = Vec::new();

    // When
    session::execute_session(SessionMode::Warm.plan(), |step| {
        observed.push(step);
        Ok::<(), String>(())
    })
    .expect("session steps succeed");

    // Then
    assert_eq!(
        observed,
        [
            SessionStep::WarmupPass,
            SessionStep::WarmupPass,
            SessionStep::WarmupPass,
            SessionStep::OpenCounters,
            SessionStep::MeasuredPass,
            SessionStep::MeasuredPass,
            SessionStep::CloseCounters,
        ]
    );
}

#[test]
fn native_fetches_process_snapshots_while_solr_never_calls_control_endpoint() {
    // Given
    let mut calls = 0;

    // When
    let native = session::process_snapshot_for(Engine::Native, || {
        calls += 1;
        Ok::<_, String>(issue61_eval::ProcessCpuSnapshot::new(41, 10, 5))
    })
    .expect("native snapshot");
    let solr = session::process_snapshot_for(Engine::Solr, || {
        calls += 1;
        Ok::<_, String>(issue61_eval::ProcessCpuSnapshot::new(42, 20, 10))
    })
    .expect("Solr branch");

    // Then
    assert_eq!(calls, 1);
    assert!(native.is_some());
    assert!(solr.is_none());
}

#[test]
fn config_parses_exactly_one_typed_engine_session() {
    // Given
    let args = vec![
        "i61_bench",
        "--workload",
        "workload.jsonl",
        "--dataset",
        "wands",
        "--query-class",
        "all",
        "--engine",
        "solr",
        "--session-mode",
        "warm",
        "--engine-url",
        "http://solr:8983",
        "--engine-cgroup",
        "/sys/fs/cgroup/solr",
        "--block",
        "4",
        "--engine-order",
        "1",
        "--seed",
        "61",
        "--out",
        "raw.jsonl",
    ]
    .into_iter()
    .map(str::to_string)
    .collect::<Vec<_>>();

    // When
    let config = parse_config(&args).expect("single-session config");

    // Then
    assert_eq!(config.engine, Engine::Solr);
    assert_eq!(config.plan, SessionMode::Warm.plan());
    assert_eq!(config.projection, WorkloadProjection::All);
    assert_eq!(config.engine_url, "http://solr:8983");
    assert_eq!(config.engine_cgroup, PathBuf::from("/sys/fs/cgroup/solr"));
    assert_eq!(config.block.get(), 4);
    assert_eq!(config.order.index(), 1);
}

#[test]
fn config_rejects_the_legacy_two_engine_campaign_shape() {
    // Given
    let args = vec![
        "i61_bench",
        "--workload",
        "workload.jsonl",
        "--dataset",
        "wands",
        "--query-class",
        "all",
        "--regime",
        "warm",
        "--baseline-url",
        "http://solr:8983",
        "--baseline-cgroup",
        "/sys/fs/cgroup/solr",
        "--treatment-url",
        "http://native:3000",
        "--treatment-cgroup",
        "/sys/fs/cgroup/native",
        "--blocks",
        "30",
        "--warmup-passes",
        "3",
        "--seed",
        "61",
        "--out",
        "raw.jsonl",
    ]
    .into_iter()
    .map(str::to_string)
    .collect::<Vec<_>>();

    // When
    let result = parse_config(&args);

    // Then
    assert!(result.is_err());
}

#[test]
fn config_rejects_a_second_engine_url() {
    // Given
    let args = vec![
        "i61_bench",
        "--workload",
        "workload.jsonl",
        "--dataset",
        "wands",
        "--engine",
        "solr",
        "--session-mode",
        "warm",
        "--engine-url",
        "http://solr-a:8983",
        "--engine-url",
        "http://solr-b:8983",
        "--engine-cgroup",
        "/sys/fs/cgroup/solr",
        "--block",
        "4",
        "--engine-order",
        "0",
        "--seed",
        "61",
        "--out",
        "raw.jsonl",
    ]
    .into_iter()
    .map(str::to_string)
    .collect::<Vec<_>>();

    // When
    let result = parse_config(&args);

    // Then
    assert!(result.is_err());
}

#[path = "tests/cli_tests.rs"]
mod cli_tests;
#[path = "../../../tests/support/http_capture.rs"]
mod http_capture;
#[path = "tests/http_contract.rs"]
mod http_contract;
#[path = "tests/projection_tests.rs"]
mod projection_tests;
#[path = "tests/protocol_tests.rs"]
mod protocol_tests;
