//! The hardened Elasticsearch/OpenSearch transport. Sibling of
//! [`crate::solr::SolrComparator`], sharing its wire-protocol subset
//! (both engines fork the same Lucene-based `_search` DSL and `_bulk`
//! API at the level used here) behind two distinct types
//! ([`ElasticsearchComparator`], [`OpenSearchComparator`]) so per-engine
//! reporting/labeling stays unambiguous even though the request/response
//! handling is identical.
//!
//! `fq` here is a `&[String]` of JSON-serialized ES `bool` filter clauses
//! produced by [`crate::translate_es::translate_all_es`] -- kept as
//! `Vec<String>`, not loosened to `Vec<serde_json::Value>`, so the
//! [`crate::solr::EngineComparator`] trait signature stays byte-identical
//! across every backend (a caller holding a `&dyn EngineComparator` never
//! needs to know which concrete engine it is talking to).

use crate::outcome::EngineLookup;
use crate::solr::EngineComparator;

/// A real standalone Elasticsearch server backend (not the
/// embedded-test-framework route earlier phases used -- see
/// `docs/experiments/FULL_MATRIX_PROTOCOL.md` §3 for why a real server
/// process is required for Issue #57's fairness contract: an embedded
/// in-process route has no network/IPC/process-boundary cost, which would
/// silently advantage ES/OpenSearch over Solr and Havenask, both of which
/// always pay a real HTTP round trip here).
pub struct ElasticsearchComparator {
    pub base_url: String,
    pub index: String,
    /// Fields `q` (free text) is matched against via `multi_match`,
    /// analogous to Solr's `qf` parameter.
    pub text_fields: Vec<String>,
    pub timeout: std::time::Duration,
}

impl ElasticsearchComparator {
    pub fn new(
        base_url: impl Into<String>,
        index: impl Into<String>,
        text_fields: Vec<String>,
    ) -> Self {
        Self {
            base_url: base_url.into(),
            index: index.into(),
            text_fields,
            timeout: std::time::Duration::from_secs(30),
        }
    }
}

impl EngineComparator for ElasticsearchComparator {
    fn search(&self, q: &str, fq: &[String], rows: usize) -> EngineLookup {
        es_family_search(
            &self.base_url,
            &self.index,
            &self.text_fields,
            q,
            fq,
            rows,
            self.timeout,
        )
    }
}

/// Identical wire protocol to [`ElasticsearchComparator`] -- see this
/// module's doc comment for why these are kept as distinct types.
pub struct OpenSearchComparator {
    pub base_url: String,
    pub index: String,
    pub text_fields: Vec<String>,
    pub timeout: std::time::Duration,
}

impl OpenSearchComparator {
    pub fn new(
        base_url: impl Into<String>,
        index: impl Into<String>,
        text_fields: Vec<String>,
    ) -> Self {
        Self {
            base_url: base_url.into(),
            index: index.into(),
            text_fields,
            timeout: std::time::Duration::from_secs(30),
        }
    }
}

impl EngineComparator for OpenSearchComparator {
    fn search(&self, q: &str, fq: &[String], rows: usize) -> EngineLookup {
        es_family_search(
            &self.base_url,
            &self.index,
            &self.text_fields,
            q,
            fq,
            rows,
            self.timeout,
        )
    }
}

/// The hardened transport function, shared by both wrapper types. `q ==
/// ""` (or `"*"`, mirroring Solr's own `q=*:*` convention used
/// throughout this workspace for filter-only requests) omits the
/// `multi_match` clause entirely rather than sending a query ES would
/// reject or silently no-op on.
fn es_family_search(
    base_url: &str,
    index: &str,
    text_fields: &[String],
    q: &str,
    fq: &[String],
    rows: usize,
    timeout: std::time::Duration,
) -> EngineLookup {
    let mut filter: Vec<serde_json::Value> = Vec::with_capacity(fq.len());
    for clause in fq {
        match serde_json::from_str::<serde_json::Value>(clause) {
            Ok(v) => filter.push(v),
            Err(e) => {
                return EngineLookup::ParseError(format!(
                    "fq clause was not valid JSON (harness bug, not an ES failure): {e}: {clause}"
                ))
            }
        }
    }
    let mut bool_query = serde_json::json!({"filter": filter});
    if !q.is_empty() && q != "*" && q != "*:*" {
        bool_query["must"] = serde_json::json!([{
            "multi_match": {"query": q, "fields": text_fields}
        }]);
    }
    let body = serde_json::json!({
        "query": {"bool": bool_query},
        "size": rows,
        "_source": false,
    });

    let url = format!("{base_url}/{index}/_search");
    let resp = ureq::post(&url)
        .timeout(timeout)
        .set("Content-Type", "application/json")
        .send_string(&body.to_string());
    let resp = match resp {
        Ok(resp) => resp,
        Err(ureq::Error::Status(_code, resp)) => {
            // A non-2xx status with a real body is a query-side rejection
            // (malformed filter/mapping mismatch), not a transport
            // failure -- distinguished the same way solr.rs distinguishes
            // Solr's `responseHeader.status != 0`.
            let text = resp.into_string().unwrap_or_default();
            return EngineLookup::QueryError(format!(
                "Elasticsearch/OpenSearch rejected the query: {text}"
            ));
        }
        Err(e) => return EngineLookup::TransportError(format!("HTTP request failed: {e}")),
    };
    let body = match resp.into_json::<serde_json::Value>() {
        Ok(body) => body,
        Err(e) => {
            return EngineLookup::ParseError(format!("response body was not valid JSON: {e}"))
        }
    };
    let Some(hits) = body["hits"]["hits"].as_array() else {
        return EngineLookup::ParseError(format!(
            "response JSON parsed but had no hits.hits array: {body}"
        ));
    };
    EngineLookup::Success(
        hits.iter()
            .filter_map(|h| h["_id"].as_str().map(str::to_string))
            .collect(),
    )
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

    fn fake_es_base_url(status_line: &'static str, body: &'static str) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
        let addr = listener.local_addr().expect("local_addr");
        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(5)));
                let mut buf = [0u8; 8192];
                let _ = stream.read(&mut buf);
                let response = format!(
                    "{status_line}\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{body}",
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
        let comparator = ElasticsearchComparator::new(
            closed_port_base_url(),
            "wands_bench",
            vec!["title".into()],
        );
        match comparator.search("widget", &[], 10) {
            EngineLookup::TransportError(_) => {}
            other => panic!("expected TransportError, got {other:?}"),
        }
    }

    #[test]
    fn invalid_json_body_is_parse_error_not_empty_success() {
        let url = fake_es_base_url("HTTP/1.1 200 OK", "not json {{{");
        let comparator = ElasticsearchComparator::new(url, "wands_bench", vec!["title".into()]);
        match comparator.search("widget", &[], 10) {
            EngineLookup::ParseError(_) => {}
            other => panic!("expected ParseError, got {other:?}"),
        }
    }

    #[test]
    fn missing_hits_shape_is_parse_error_not_empty_success() {
        let url = fake_es_base_url("HTTP/1.1 200 OK", r#"{"took":1}"#);
        let comparator = ElasticsearchComparator::new(url, "wands_bench", vec!["title".into()]);
        match comparator.search("widget", &[], 10) {
            EngineLookup::ParseError(_) => {}
            other => panic!("expected ParseError, got {other:?}"),
        }
    }

    #[test]
    fn nonzero_status_is_query_error() {
        let url = fake_es_base_url(
            "HTTP/1.1 400 Bad Request",
            r#"{"error":{"type":"parsing_exception"}}"#,
        );
        let comparator = ElasticsearchComparator::new(url, "wands_bench", vec!["title".into()]);
        match comparator.search("widget", &[], 10) {
            EngineLookup::QueryError(_) => {}
            other => panic!("expected QueryError, got {other:?}"),
        }
    }

    #[test]
    fn valid_empty_hits_is_a_legitimate_success_not_an_error() {
        let url = fake_es_base_url(
            "HTTP/1.1 200 OK",
            r#"{"hits":{"total":{"value":0},"hits":[]}}"#,
        );
        let comparator = ElasticsearchComparator::new(url, "wands_bench", vec!["title".into()]);
        match comparator.search("widget", &[], 10) {
            EngineLookup::Success(ids) => assert!(ids.is_empty()),
            other => panic!("expected Success(empty), got {other:?}"),
        }
    }

    #[test]
    fn valid_hits_round_trip_the_ids() {
        let url = fake_es_base_url(
            "HTTP/1.1 200 OK",
            r#"{"hits":{"total":{"value":2},"hits":[{"_id":"B001"},{"_id":"B002"}]}}"#,
        );
        let comparator = ElasticsearchComparator::new(url, "wands_bench", vec!["title".into()]);
        match comparator.search("widget", &[], 10) {
            EngineLookup::Success(ids) => {
                assert_eq!(ids, vec!["B001".to_string(), "B002".to_string()])
            }
            other => panic!("expected Success, got {other:?}"),
        }
    }

    #[test]
    fn malformed_fq_clause_is_a_harness_parse_error_not_a_transport_failure() {
        let url = fake_es_base_url("HTTP/1.1 200 OK", r#"{"hits":{"hits":[]}}"#);
        let comparator = ElasticsearchComparator::new(url, "wands_bench", vec!["title".into()]);
        match comparator.search("widget", &["not json".to_string()], 10) {
            EngineLookup::ParseError(_) => {}
            other => panic!("expected ParseError, got {other:?}"),
        }
    }

    #[test]
    fn opensearch_comparator_uses_the_identical_transport() {
        let url = fake_es_base_url(
            "HTTP/1.1 200 OK",
            r#"{"hits":{"total":{"value":1},"hits":[{"_id":"B001"}]}}"#,
        );
        let comparator = OpenSearchComparator::new(url, "wands_bench", vec!["title".into()]);
        let lookup = comparator.search("widget", &[], 10);
        assert_eq!(lookup.ids(), Some(&["B001".to_string()][..]));
    }
}
