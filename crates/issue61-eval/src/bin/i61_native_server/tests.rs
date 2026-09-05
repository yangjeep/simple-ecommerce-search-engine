use super::*;

#[test]
fn request_line_parses_select_when_http11_get() {
    let request =
        parse_select_request("GET /select?q=red%20chair&rows=5 HTTP/1.1").expect("valid request");
    assert_eq!(
        request,
        SelectRequest {
            query: "red chair".to_string(),
            rows: 5
        }
    );
}

#[test]
fn query_string_decodes_plus_and_percent_when_present() {
    let request =
        parse_select_request("GET /select?q=a%2Bb+chair HTTP/1.1").expect("valid encoded query");
    assert_eq!(request.query, "a+b chair");
}

#[test]
fn rows_defaults_and_clamps_when_out_of_range() {
    let defaulted = parse_select_request("GET /select?q=chair HTTP/1.1").expect("default rows");
    let clamped =
        parse_select_request("GET /select?q=chair&rows=999999 HTTP/1.1").expect("clamped rows");
    assert_eq!((defaulted.rows, clamped.rows), (10, 1000));
}

#[test]
fn response_rendering_matches_solr_contract_when_ids_need_escaping() {
    let body =
        render_success(12, &["P1".to_string(), "P\"2".to_string()]).expect("serializable response");
    let value: serde_json::Value = serde_json::from_str(&body).expect("valid JSON");
    assert_eq!(value["responseHeader"]["status"], 0);
    assert_eq!(value["response"]["numFound"], 12);
    assert_eq!(value["response"]["docs"][1]["id"], "P\"2");
}

#[test]
fn select_response_byte_shape_remains_unchanged() {
    // Given / When
    let body = render_success(1, &["P1".to_string()]).expect("serializable response");

    // Then
    assert_eq!(
        body,
        r#"{"responseHeader":{"status":0},"response":{"numFound":1,"docs":[{"id":"P1"}]}}"#
    );
}

#[test]
fn rusage_response_is_a_typed_pid_and_cumulative_cpu_payload() {
    // Given
    let snapshot = ProcessCpuSnapshot::new(41, 12, 7);

    // When
    let body = render_rusage(&snapshot).expect("serializable rusage response");
    let decoded: ProcessCpuSnapshot = serde_json::from_str(&body).expect("typed response");

    // Then
    assert_eq!(decoded, snapshot);
    assert_eq!(body, r#"{"pid":41,"user_usec":12,"system_usec":7}"#);
}
