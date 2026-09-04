//! The hardened Solr transport. Generalized from
//! `issue35_eval::eval`'s crate-private `SolrLookup`/`solr_search` (the
//! only fully transport-hardened implementation this workspace had
//! before A3), made `pub` and reusable, and split its conflated
//! `TransportError` into [`EngineLookup::TransportError`] (the request
//! never got a real answer) vs. [`EngineLookup::QueryError`] (Solr
//! answered but rejected the query) -- see [`crate::outcome`].

use crate::outcome::{EngineHits, EngineLookup, EngineLookupHits};

/// The contract a comparator backend implements. Solr is the only
/// implementation today; the trait boundary exists so an Elasticsearch or
/// Havenask adapter (Issue #57) can implement `search` against its own
/// wire protocol and reuse [`crate::translate`] and [`crate::compare`]
/// unchanged -- those two modules depend only on this trait's output
/// shape, never on Solr specifically.
pub trait EngineComparator {
    /// `q` is the free-text query (already engine-specific, e.g. Solr's
    /// `{!edismax qf=...}` syntax); `fq` is a list of hard filter clauses,
    /// each produced by [`crate::translate::translate_constraint`] for
    /// this same backend. Returns at most `rows` document ids.
    fn search(&self, q: &str, fq: &[String], rows: usize) -> EngineLookup;

    /// Returns the result page plus the total number of matching documents,
    /// independent of `rows`. The default preserves source compatibility for
    /// existing implementors but refuses to fabricate a total from top-K ids;
    /// backends must override it to return a successful count.
    fn search_with_count(&self, q: &str, fq: &[String], rows: usize) -> EngineLookupHits {
        match self.search(q, fq, rows) {
            EngineLookup::Success(_) => EngineLookupHits::ParseError(
                "comparator backend does not implement total match counts".to_string(),
            ),
            EngineLookup::TransportError(detail) => EngineLookupHits::TransportError(detail),
            EngineLookup::QueryError(detail) => EngineLookupHits::QueryError(detail),
            EngineLookup::ParseError(detail) => EngineLookupHits::ParseError(detail),
        }
    }
}

fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Builds a Lucene `RegexpQuery` pattern that matches `s` case-
/// insensitively, anchored to the *entire* field value (Lucene's regex
/// queries on a `StrField`/`strings` field are implicitly whole-term-
/// anchored). Moved here verbatim from `round1_eval::solr` (itself
/// originally P2-E13's already-adversarially-reviewed construction);
/// `round1_eval::solr::case_insensitive_field_regex` now re-exports this
/// copy instead of maintaining a second one.
pub fn case_insensitive_field_regex(s: &str) -> String {
    const REGEX_METACHARS: &str = "\\.?+*|{}[]()\"#@&<>~^$/";
    let mut out = String::with_capacity(s.len() * 4);
    for c in s.chars() {
        if c.is_ascii_alphabetic() {
            out.push('[');
            out.push(c.to_ascii_lowercase());
            out.push(c.to_ascii_uppercase());
            out.push(']');
        } else if REGEX_METACHARS.contains(c) {
            out.push('\\');
            out.push(c);
        } else {
            out.push(c);
        }
    }
    out
}

/// Like [`case_insensitive_field_regex`], but for a *substring* match
/// (`Constraint::Text { contains, .. }`) rather than a whole-field match:
/// wraps the per-character case-alternation in `.*` on both sides.
pub fn case_insensitive_contains_regex(s: &str) -> String {
    format!(".*{}.*", case_insensitive_field_regex(s))
}

/// A real Solr `/select` backend, POSTing form-encoded (matching
/// `issue35_eval::eval`'s original construction, which -- unlike a GET
/// query string -- has no URL-length limit on a large `fq` list, e.g. a
/// wide `ProductTypeAny` group).
pub struct SolrComparator {
    pub base_url: String,
    /// The Solr `qf` (query fields) parameter for free-text `q`, e.g.
    /// `"title description bullet_point"`. Dataset-specific because
    /// different Solr cores index different free-text fields under
    /// different names.
    pub qf: String,
    pub timeout: std::time::Duration,
}

impl SolrComparator {
    pub fn new(base_url: impl Into<String>, qf: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            qf: qf.into(),
            timeout: std::time::Duration::from_secs(30),
        }
    }
}

impl EngineComparator for SolrComparator {
    fn search(&self, q: &str, fq: &[String], rows: usize) -> EngineLookup {
        solr_search(&self.base_url, &self.qf, q, fq, rows, self.timeout)
    }

    fn search_with_count(&self, q: &str, fq: &[String], rows: usize) -> EngineLookupHits {
        solr_search_with_count(&self.base_url, &self.qf, q, fq, rows, self.timeout)
    }
}

/// The hardened transport function itself, free of the `SolrComparator`
/// wrapper for callers (and tests) that want to call it directly.
pub fn solr_search(
    base_url: &str,
    qf: &str,
    q: &str,
    fq: &[String],
    rows: usize,
    timeout: std::time::Duration,
) -> EngineLookup {
    match solr_search_response(base_url, qf, q, fq, rows, timeout) {
        Ok(response) => EngineLookup::Success(response.ids),
        Err(failure) => failure.into_lookup(),
    }
}

/// Returns both the selected ids and Solr's full `response.numFound` count.
pub fn solr_search_with_count(
    base_url: &str,
    qf: &str,
    q: &str,
    fq: &[String],
    rows: usize,
    timeout: std::time::Duration,
) -> EngineLookupHits {
    match solr_search_response(base_url, qf, q, fq, rows, timeout) {
        Ok(response) => match response.num_found {
            Some(num_found) => EngineLookupHits::Success(EngineHits {
                ids: response.ids,
                num_found,
            }),
            None => EngineLookupHits::ParseError(
                "response JSON parsed but response.numFound was missing or not an unsigned number"
                    .to_string(),
            ),
        },
        Err(failure) => failure.into_lookup_hits(),
    }
}

struct ParsedSolrResponse {
    ids: Vec<String>,
    num_found: Option<u64>,
}

enum SolrFailure {
    Transport(String),
    Query(String),
    Parse(String),
}

impl SolrFailure {
    fn into_lookup(self) -> EngineLookup {
        match self {
            Self::Transport(detail) => EngineLookup::TransportError(detail),
            Self::Query(detail) => EngineLookup::QueryError(detail),
            Self::Parse(detail) => EngineLookup::ParseError(detail),
        }
    }

    fn into_lookup_hits(self) -> EngineLookupHits {
        match self {
            Self::Transport(detail) => EngineLookupHits::TransportError(detail),
            Self::Query(detail) => EngineLookupHits::QueryError(detail),
            Self::Parse(detail) => EngineLookupHits::ParseError(detail),
        }
    }
}

fn solr_search_response(
    base_url: &str,
    qf: &str,
    q: &str,
    fq: &[String],
    rows: usize,
    timeout: std::time::Duration,
) -> Result<ParsedSolrResponse, SolrFailure> {
    let url = format!("{base_url}/select");
    let rows_str = rows.to_string();
    let mut form: Vec<(&str, &str)> = vec![
        ("q", q),
        ("defType", "edismax"),
        ("qf", qf),
        ("rows", &rows_str),
        ("fl", "id"),
    ];
    for f in fq {
        form.push(("fq", f.as_str()));
    }
    let resp = ureq::post(&url).timeout(timeout).send_form(&form);
    let resp = match resp {
        Ok(resp) => resp,
        Err(error) => {
            return Err(SolrFailure::Transport(format!(
                "HTTP request failed: {error}"
            )))
        }
    };
    let body = match resp.into_json::<serde_json::Value>() {
        Ok(body) => body,
        Err(error) => {
            return Err(SolrFailure::Parse(format!(
                "response body was not valid JSON: {error}"
            )))
        }
    };
    if let Some(status) = body["responseHeader"]["status"].as_i64() {
        if status != 0 {
            return Err(SolrFailure::Query(format!(
                "Solr responseHeader.status={status} (Solr-side query error): {body}"
            )));
        }
    }
    let Some(docs) = body["response"]["docs"].as_array() else {
        return Err(SolrFailure::Parse(format!(
            "response JSON parsed but had no response.docs array: {body}"
        )));
    };
    match parse_document_ids(docs) {
        Ok(ids) => Ok(ParsedSolrResponse {
            ids,
            num_found: body["response"]["numFound"].as_u64(),
        }),
        Err(detail) => Err(SolrFailure::Parse(detail)),
    }
}

/// Percent-encoded GET variant, kept for callers that only ever send a
/// short `fq` list and prefer a single-round-trip GET (e.g. a startup
/// reachability ping with no `fq` at all). Uses the same hardened parsing
/// as [`solr_search`].
pub fn solr_search_get(
    base_url: &str,
    q: &str,
    fq: &[String],
    rows: usize,
    timeout: std::time::Duration,
) -> EngineLookup {
    let mut url = format!(
        "{base_url}/select?q={}&rows={rows}&fl=id",
        percent_encode(q)
    );
    for f in fq {
        url.push_str(&format!("&fq={}", percent_encode(f)));
    }
    let resp = ureq::get(&url).timeout(timeout).call();
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
    if let Some(status) = body["responseHeader"]["status"].as_i64() {
        if status != 0 {
            return EngineLookup::QueryError(format!(
                "Solr responseHeader.status={status} (Solr-side query error): {body}"
            ));
        }
    }
    let Some(docs) = body["response"]["docs"].as_array() else {
        return EngineLookup::ParseError(format!(
            "response JSON parsed but had no response.docs array: {body}"
        ));
    };
    match parse_document_ids(docs) {
        Ok(ids) => EngineLookup::Success(ids),
        Err(detail) => EngineLookup::ParseError(detail),
    }
}

fn parse_document_ids(docs: &[serde_json::Value]) -> Result<Vec<String>, String> {
    let mut ids = Vec::with_capacity(docs.len());
    for (index, document) in docs.iter().enumerate() {
        let Some(id) = document["id"].as_str() else {
            return Err(format!(
                "response document index {index} had no string id: {document}"
            ));
        };
        ids.push(id.to_string());
    }
    Ok(ids)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    const TEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

    fn closed_port_base_url() -> String {
        // Bind then immediately drop, freeing the port but leaving nothing
        // listening -- a reliable, fast connection-refused fixture.
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
        let addr = listener.local_addr().expect("local_addr");
        drop(listener);
        format!("http://{addr}/solr/fake_core")
    }

    fn fake_solr_base_url(status_line: &'static str, body: &'static str) -> String {
        let (url, _rx) = fake_solr_capturing_request(status_line, body);
        url
    }

    fn read_full_http_request(stream: &mut std::net::TcpStream) -> String {
        let mut buf = Vec::new();
        let mut chunk = [0u8; 4096];
        while let Ok(n) = stream.read(&mut chunk) {
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&chunk[..n]);
            let Some(header_end) = buf.windows(4).position(|w| w == b"\r\n\r\n") else {
                continue;
            };
            let headers = String::from_utf8_lossy(&buf[..header_end]);
            let content_length: usize = headers
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.trim()
                        .eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse().ok())
                        .flatten()
                })
                .unwrap_or(0);
            let body_len = buf.len() - (header_end + 4);
            if body_len >= content_length {
                break;
            }
        }
        String::from_utf8_lossy(&buf).into_owned()
    }

    fn fake_solr_capturing_request(
        status_line: &'static str,
        body: &'static str,
    ) -> (String, std::sync::mpsc::Receiver<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
        let addr = listener.local_addr().expect("local_addr");
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(5)));
                let request = read_full_http_request(&mut stream);
                let _ = tx.send(request);
                let response = format!(
                    "{status_line}\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.flush();
            }
        });
        (format!("http://{addr}/solr/fake_core"), rx)
    }

    #[test]
    fn connection_refused_is_transport_error_not_empty_success() {
        let url = closed_port_base_url();
        match solr_search(&url, "all_text", "widget", &[], 10, TEST_TIMEOUT) {
            EngineLookup::TransportError(_) => {}
            other => panic!("expected TransportError, got {other:?}"),
        }
    }

    #[test]
    fn invalid_json_body_is_parse_error_not_empty_success() {
        let url = fake_solr_base_url("HTTP/1.1 200 OK", "this is not json {{{");
        match solr_search(&url, "all_text", "widget", &[], 10, TEST_TIMEOUT) {
            EngineLookup::ParseError(_) => {}
            other => panic!("expected ParseError, got {other:?}"),
        }
    }

    #[test]
    fn missing_response_docs_shape_is_parse_error_not_empty_success() {
        let url = fake_solr_base_url("HTTP/1.1 200 OK", r#"{"responseHeader":{"status":0}}"#);
        match solr_search(&url, "all_text", "widget", &[], 10, TEST_TIMEOUT) {
            EngineLookup::ParseError(_) => {}
            other => panic!("expected ParseError, got {other:?}"),
        }
    }

    #[test]
    fn nonzero_solr_status_is_query_error_not_empty_success_and_not_transport_error() {
        let url = fake_solr_base_url(
            "HTTP/1.1 200 OK",
            r#"{"responseHeader":{"status":400},"error":{"msg":"bad query"}}"#,
        );
        match solr_search(&url, "all_text", "widget", &[], 10, TEST_TIMEOUT) {
            EngineLookup::QueryError(_) => {}
            other => panic!("expected QueryError, got {other:?}"),
        }
    }

    #[test]
    fn query_error_is_distinguishable_from_transport_error() {
        // The whole reason EngineLookup has 4 variants instead of
        // SolrLookup's 3: a caller must be able to tell "Solr rejected my
        // query" (a harness bug) apart from "Solr never answered" (an
        // infrastructure problem) -- conflating them was fine for
        // exclude-from-comparison purposes but not for diagnosis.
        let refused = solr_search(
            &closed_port_base_url(),
            "all_text",
            "widget",
            &[],
            10,
            TEST_TIMEOUT,
        );
        let rejected = solr_search(
            &fake_solr_base_url(
                "HTTP/1.1 200 OK",
                r#"{"responseHeader":{"status":400},"error":{"msg":"bad query"}}"#,
            ),
            "all_text",
            "widget",
            &[],
            10,
            TEST_TIMEOUT,
        );
        assert!(matches!(refused, EngineLookup::TransportError(_)));
        assert!(matches!(rejected, EngineLookup::QueryError(_)));
    }

    #[test]
    fn valid_empty_docs_is_a_legitimate_success_not_an_error() {
        let url = fake_solr_base_url(
            "HTTP/1.1 200 OK",
            r#"{"responseHeader":{"status":0},"response":{"numFound":0,"start":0,"docs":[]}}"#,
        );
        match solr_search(&url, "all_text", "widget", &[], 10, TEST_TIMEOUT) {
            EngineLookup::Success(ids) => assert!(ids.is_empty()),
            other => panic!("expected Success(empty), got {other:?}"),
        }
    }

    #[test]
    fn valid_docs_round_trip_the_ids() {
        let url = fake_solr_base_url(
            "HTTP/1.1 200 OK",
            r#"{"responseHeader":{"status":0},"response":{"numFound":2,"start":0,"docs":[{"id":"B001"},{"id":"B002"}]}}"#,
        );
        match solr_search(&url, "all_text", "widget", &[], 10, TEST_TIMEOUT) {
            EngineLookup::Success(ids) => {
                assert_eq!(ids, vec!["B001".to_string(), "B002".to_string()])
            }
            other => panic!("expected Success, got {other:?}"),
        }
    }

    #[test]
    fn a_document_missing_its_id_is_a_parse_error_not_a_short_success() {
        // Given
        let url = fake_solr_base_url(
            "HTTP/1.1 200 OK",
            r#"{"responseHeader":{"status":0},"response":{"numFound":2,"docs":[{"id":"B001"},{"title":"broken"}]}}"#,
        );

        // When
        let lookup = solr_search(&url, "all_text", "widget", &[], 10, TEST_TIMEOUT);

        // Then
        match lookup {
            EngineLookup::ParseError(detail) => {
                assert!(detail.contains("document index 1"));
                assert!(detail.contains(r#"{"title":"broken"}"#));
            }
            other => panic!("expected ParseError, got {other:?}"),
        }
    }

    #[test]
    fn a_document_with_a_non_string_id_is_a_parse_error() {
        // Given
        let url = fake_solr_base_url(
            "HTTP/1.1 200 OK",
            r#"{"responseHeader":{"status":0},"response":{"numFound":1,"docs":[{"id":42}]}}"#,
        );

        // When
        let lookup = solr_search(&url, "all_text", "widget", &[], 10, TEST_TIMEOUT);

        // Then
        match lookup {
            EngineLookup::ParseError(detail) => {
                assert!(detail.contains("document index 0"));
                assert!(detail.contains(r#"{"id":42}"#));
            }
            other => panic!("expected ParseError, got {other:?}"),
        }
    }

    #[test]
    fn all_documents_having_string_ids_still_succeeds() {
        // Given
        let url = fake_solr_base_url(
            "HTTP/1.1 200 OK",
            r#"{"responseHeader":{"status":0},"response":{"numFound":2,"docs":[{"id":"B001"},{"id":"B002"}]}}"#,
        );

        // When
        let lookup = solr_search(&url, "all_text", "widget", &[], 10, TEST_TIMEOUT);

        // Then
        assert_eq!(
            lookup,
            EngineLookup::Success(vec!["B001".to_string(), "B002".to_string()])
        );
    }

    #[test]
    fn fq_parameters_reach_the_wire_when_present() {
        let (url, rx) = fake_solr_capturing_request(
            "HTTP/1.1 200 OK",
            r#"{"responseHeader":{"status":0},"response":{"numFound":0,"start":0,"docs":[]}}"#,
        );
        let fq = vec![
            "brand:/[Nn][Ii][Kk][Ee]/".to_string(),
            "color:/[Bb][Ll][Aa][Cc][Kk]/".to_string(),
        ];
        let _ = solr_search(&url, "all_text", "widget", &fq, 10, TEST_TIMEOUT);
        let request = rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("fake Solr should have received a request");
        assert_eq!(
            request.matches("fq=").count(),
            2,
            "expected exactly 2 fq params on the wire, got request: {request}"
        );
    }

    #[test]
    fn no_fq_parameters_are_sent_when_the_query_has_no_structural_constraints() {
        let (url, rx) = fake_solr_capturing_request(
            "HTTP/1.1 200 OK",
            r#"{"responseHeader":{"status":0},"response":{"numFound":0,"start":0,"docs":[]}}"#,
        );
        let _ = solr_search(&url, "all_text", "widget", &[], 10, TEST_TIMEOUT);
        let request = rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("fake Solr should have received a request");
        assert_eq!(
            request.matches("fq=").count(),
            0,
            "expected no fq params on the wire, got request: {request}"
        );
    }

    #[test]
    fn engine_comparator_trait_matches_the_free_function() {
        let url = fake_solr_base_url(
            "HTTP/1.1 200 OK",
            r#"{"responseHeader":{"status":0},"response":{"numFound":1,"start":0,"docs":[{"id":"B001"}]}}"#,
        );
        let comparator = SolrComparator::new(url, "all_text");
        let lookup = comparator.search("widget", &[], 10);
        assert_eq!(lookup.ids(), Some(&["B001".to_string()][..]));
    }

    #[test]
    fn search_with_count_returns_num_found_from_the_response() {
        // Given
        let url = fake_solr_base_url(
            "HTTP/1.1 200 OK",
            r#"{"responseHeader":{"status":0},"response":{"numFound":2,"docs":[{"id":"B001"},{"id":"B002"}]}}"#,
        );
        let comparator = SolrComparator::new(url, "all_text");

        // When
        let lookup = comparator.search_with_count("widget", &[], 10);

        // Then
        assert_eq!(
            lookup,
            EngineLookupHits::Success(EngineHits {
                ids: vec!["B001".to_string(), "B002".to_string()],
                num_found: 2,
            })
        );
    }

    #[test]
    fn num_found_absent_is_a_parse_error() {
        // Given
        let url = fake_solr_base_url(
            "HTTP/1.1 200 OK",
            r#"{"responseHeader":{"status":0},"response":{"docs":[{"id":"B001"}]}}"#,
        );
        let comparator = SolrComparator::new(url, "all_text");

        // When
        let lookup = comparator.search_with_count("widget", &[], 10);

        // Then
        assert!(matches!(lookup, EngineLookupHits::ParseError(_)));
    }

    #[test]
    fn num_found_can_exceed_the_returned_id_count() {
        // Given
        let url = fake_solr_base_url(
            "HTTP/1.1 200 OK",
            r#"{"responseHeader":{"status":0},"response":{"numFound":1000,"docs":[{"id":"B001"}]}}"#,
        );
        let comparator = SolrComparator::new(url, "all_text");

        // When
        let lookup = comparator.search_with_count("widget", &[], 10);

        // Then
        match lookup {
            EngineLookupHits::Success(hits) => {
                assert_eq!(hits.ids.len(), 1);
                assert_eq!(hits.num_found, 1000);
            }
            other => panic!("expected Success, got {other:?}"),
        }
    }

    #[test]
    fn existing_search_behaviour_is_unchanged() {
        // Given
        let url = fake_solr_base_url(
            "HTTP/1.1 200 OK",
            r#"{"responseHeader":{"status":0},"response":{"numFound":1000,"docs":[{"id":"B001"}]}}"#,
        );
        let comparator = SolrComparator::new(url, "all_text");

        // When
        let lookup = comparator.search("widget", &[], 10);

        // Then
        assert_eq!(lookup, EngineLookup::Success(vec!["B001".to_string()]));
    }
}
