//! The hardened Havenask SQL/QRS transport. Sibling of
//! [`crate::solr::SolrComparator`]/[`crate::elasticsearch::ElasticsearchComparator`],
//! talking to Havenask's `QrsService/searchSql` HTTP endpoint (the same
//! endpoint `docs/experiments/FULL_MATRIX_PROTOCOL.md` §3.2 proved
//! correct against the official quickstart's known-expected-output
//! `JOIN`/`MATCHINDEX` queries, before this dataset-specific comparator
//! was written).
//!
//! `fq` here is a `&[String]` of Havenask SQL `WHERE`-clause fragments
//! produced by [`crate::translate_havenask::translate_all_havenask`],
//! joined with `AND` -- unlike the Elasticsearch comparator, no
//! serialize/reparse round trip is needed since Havenask's clause form
//! is already wire-native text.

use crate::outcome::EngineLookup;
use crate::solr::EngineComparator;

/// A real Havenask `proc`-domain (or `default`-domain, unchanged wire
/// protocol either way) single-node cluster backend.
pub struct HavenaskComparator {
    pub base_url: String,
    pub table: String,
    /// The Havenask `MATCHINDEX` index name free-text `q` is matched
    /// against (Havenask's analog of Solr's `qf`/ES's `multi_match`
    /// fields -- a single named full-text index, not a field list, since
    /// that is how Havenask's own schema/query model works: see the
    /// official quickstart's `in0_schema.json`, whose only full-text
    /// index is named `default`).
    pub text_index: String,
    pub timeout: std::time::Duration,
}

impl HavenaskComparator {
    pub fn new(
        base_url: impl Into<String>,
        table: impl Into<String>,
        text_index: impl Into<String>,
    ) -> Self {
        Self {
            base_url: base_url.into(),
            table: table.into(),
            text_index: text_index.into(),
            timeout: std::time::Duration::from_secs(30),
        }
    }
}

impl EngineComparator for HavenaskComparator {
    fn search(&self, q: &str, fq: &[String], rows: usize) -> EngineLookup {
        havenask_search(
            &self.base_url,
            &self.table,
            &self.text_index,
            q,
            fq,
            rows,
            self.timeout,
        )
    }
}

fn havenask_search(
    base_url: &str,
    table: &str,
    text_index: &str,
    q: &str,
    fq: &[String],
    rows: usize,
    timeout: std::time::Duration,
) -> EngineLookup {
    let mut clauses: Vec<String> = fq.to_vec();
    if !q.is_empty() && q != "*" && q != "*:*" {
        // Havenask SQL string literals use doubled-single-quote escaping,
        // same as translate_havenask.rs's `escape_sql_literal` -- inlined
        // here rather than imported to keep this transport module
        // independent of the translator module (mirrors solr.rs, which
        // also does its own escaping rather than depending on
        // translate.rs).
        let escaped = q.replace('\'', "''");
        clauses.push(format!("MATCHINDEX('{text_index}', '{escaped}')"));
    }
    let where_clause = if clauses.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", clauses.join(" AND "))
    };
    let sql = format!(
        "select id from {table}{where_clause} limit {rows}&&kvpair=databaseName:database;formatType:json"
    );

    let url = format!("{base_url}/QrsService/searchSql");
    let resp = ureq::post(&url)
        .timeout(timeout)
        .set("Content-Type", "application/json")
        .send_json(serde_json::json!({"assemblyQuery": sql}));
    let resp = match resp {
        Ok(resp) => resp,
        Err(e) => return EngineLookup::TransportError(format!("HTTP request failed: {e}")),
    };
    let body = match resp.into_json::<serde_json::Value>() {
        Ok(body) => body,
        Err(e) => {
            return EngineLookup::ParseError(format!("response body was not valid JSON: {e}"))
        }
    };

    // Havenask's own `error_info` field is not itself strictly valid JSON
    // on success (`"errorMsg": ERROR_NONE` -- an unquoted bare
    // identifier, confirmed live against the real QRS service), so it
    // cannot be reparsed as JSON. Mirrored from the official quickstart's
    // own check (`example/common/case.py`: `data["error_info"].find('ERROR_NONE') != -1`)
    // rather than inventing a different, unverified parsing strategy.
    let error_info = body["error_info"].as_str().unwrap_or_default();
    if !error_info.contains("ERROR_NONE") {
        return EngineLookup::QueryError(format!(
            "Havenask rejected the query: {error_info}: sql={sql}"
        ));
    }

    let Some(sql_result_str) = body["sql_result"].as_str() else {
        return EngineLookup::ParseError(format!(
            "response JSON parsed and error_info was ERROR_NONE, but sql_result was missing/not a string: {body}"
        ));
    };
    if sql_result_str.is_empty() {
        // A genuinely empty result set still carries a real
        // `{"data":[],...}` sql_result in Havenask's own response shape;
        // a truly empty string here means the row-count-0/error-8020
        // "init sql request failed" shape this session found live during
        // setup (see FULL_MATRIX_PROTOCOL.md §3) slipped past the
        // ERROR_NONE check above -- treat it as a parse failure, not a
        // silent empty success.
        return EngineLookup::ParseError(format!(
            "error_info reported ERROR_NONE but sql_result was an empty string: {body}"
        ));
    }
    let inner: serde_json::Value = match serde_json::from_str(sql_result_str) {
        Ok(v) => v,
        Err(e) => {
            return EngineLookup::ParseError(format!(
                "sql_result was not valid nested JSON: {e}: {sql_result_str}"
            ))
        }
    };
    let Some(column_names) = inner["column_name"].as_array() else {
        return EngineLookup::ParseError(format!(
            "sql_result had no column_name array: {sql_result_str}"
        ));
    };
    let Some(id_col) = column_names.iter().position(|c| c.as_str() == Some("id")) else {
        return EngineLookup::ParseError(format!(
            "sql_result's column_name did not contain \"id\" (query must select it): {sql_result_str}"
        ));
    };
    let Some(rows_data) = inner["data"].as_array() else {
        return EngineLookup::ParseError(format!("sql_result had no data array: {sql_result_str}"));
    };
    let mut ids = Vec::with_capacity(rows_data.len());
    for row in rows_data {
        let Some(row) = row.as_array() else {
            return EngineLookup::ParseError(format!(
                "sql_result data row was not an array: {sql_result_str}"
            ));
        };
        let Some(id_value) = row.get(id_col) else {
            return EngineLookup::ParseError(format!(
                "sql_result data row shorter than its own column_name: {sql_result_str}"
            ));
        };
        let id_string = match id_value {
            serde_json::Value::String(s) => s.clone(),
            serde_json::Value::Number(n) => n.to_string(),
            other => {
                return EngineLookup::ParseError(format!(
                    "sql_result id column had an unexpected JSON type: {other:?}"
                ))
            }
        };
        ids.push(id_string);
    }
    EngineLookup::Success(ids)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    fn closed_port_base_url() -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
        let addr = listener.local_addr().expect("local_addr");
        drop(listener);
        format!("http://{addr}")
    }

    fn fake_havenask_base_url(body: &'static str) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
        let addr = listener.local_addr().expect("local_addr");
        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(5)));
                let mut buf = [0u8; 8192];
                let _ = stream.read(&mut buf);
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.flush();
            }
        });
        format!("http://{addr}")
    }

    #[test]
    fn connection_refused_is_transport_error_not_empty_success() {
        let comparator = HavenaskComparator::new(closed_port_base_url(), "wands_bench", "default");
        match comparator.search("widget", &[], 10) {
            EngineLookup::TransportError(_) => {}
            other => panic!("expected TransportError, got {other:?}"),
        }
    }

    #[test]
    fn real_query_init_failure_shape_is_query_error_not_empty_success() {
        // The exact response shape this session hit live before fixing
        // the assemblyQuery field name -- see FULL_MATRIX_PROTOCOL.md §3.
        let body = r#"{"row_count":0,"sql_result":"","error_info":"{\n\"errorCode\": 8020,\n\"errorMsg\": run sql graph failed. init sql request failed []\n}\n"}"#;
        let url = fake_havenask_base_url(body);
        let comparator = HavenaskComparator::new(url, "wands_bench", "default");
        match comparator.search("widget", &[], 10) {
            EngineLookup::QueryError(_) => {}
            other => panic!("expected QueryError, got {other:?}"),
        }
    }

    #[test]
    fn error_none_with_empty_sql_result_is_a_parse_error_not_a_silent_empty_success() {
        let body = r#"{"row_count":0,"sql_result":"","error_info":"{\n\"errorCode\": 0,\n\"errorMsg\": ERROR_NONE\n}\n"}"#;
        let url = fake_havenask_base_url(body);
        let comparator = HavenaskComparator::new(url, "wands_bench", "default");
        match comparator.search("widget", &[], 10) {
            EngineLookup::ParseError(_) => {}
            other => panic!("expected ParseError, got {other:?}"),
        }
    }

    #[test]
    fn valid_empty_result_set_is_a_legitimate_success() {
        let body = r#"{"row_count":0,"sql_result":"{\"data\":[],\"column_name\":[\"id\"],\"column_type\":[\"uint32\"]}","error_info":"{\n\"errorCode\": 0,\n\"errorMsg\": ERROR_NONE\n}\n"}"#;
        let url = fake_havenask_base_url(body);
        let comparator = HavenaskComparator::new(url, "wands_bench", "default");
        match comparator.search("widget", &[], 10) {
            EngineLookup::Success(ids) => assert!(ids.is_empty()),
            other => panic!("expected Success(empty), got {other:?}"),
        }
    }

    #[test]
    fn valid_rows_round_trip_the_ids_matching_the_real_response_shape() {
        // Mirrors the exact shape returned by a live `select id, title
        // from in0 limit 10&&...formatType:json` query against the real
        // quickstart cluster this session ran.
        let body = r#"{"row_count":2,"sql_result":"{\"data\":[[1,\"null\"],[2,\"null\"]],\"column_name\":[\"id\",\"title\"],\"column_type\":[\"uint32\",\"multi_char\"]}","error_info":"{\n\"errorCode\": 0,\n\"errorMsg\": ERROR_NONE\n}\n"}"#;
        let url = fake_havenask_base_url(body);
        let comparator = HavenaskComparator::new(url, "wands_bench", "default");
        match comparator.search("widget", &[], 10) {
            EngineLookup::Success(ids) => assert_eq!(ids, vec!["1".to_string(), "2".to_string()]),
            other => panic!("expected Success, got {other:?}"),
        }
    }

    #[test]
    fn missing_id_column_is_a_parse_error() {
        let body = r#"{"row_count":1,"sql_result":"{\"data\":[[\"x\"]],\"column_name\":[\"title\"],\"column_type\":[\"multi_char\"]}","error_info":"{\n\"errorCode\": 0,\n\"errorMsg\": ERROR_NONE\n}\n"}"#;
        let url = fake_havenask_base_url(body);
        let comparator = HavenaskComparator::new(url, "wands_bench", "default");
        match comparator.search("widget", &[], 10) {
            EngineLookup::ParseError(_) => {}
            other => panic!("expected ParseError, got {other:?}"),
        }
    }
}
