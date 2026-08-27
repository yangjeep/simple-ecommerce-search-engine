//! The lookup-outcome contract every comparator backend in this crate
//! promises: a legitimate empty result is a `Success(vec![])`, never
//! indistinguishable from a failure to answer at all.

/// The outcome of asking a comparator backend (Solr today; Elasticsearch/
/// Havenask under the same trait for Issue #57) to answer one query.
///
/// Kept as four variants, not the two `issue35_eval::eval::SolrLookup`
/// originally used, because that type's own `TransportError` conflated
/// two materially different failures: the request never reaching the
/// server (or the server never replying) versus the server replying with
/// a well-formed rejection of the query itself (`responseHeader.status`
/// non-zero for Solr -- a malformed filter query, for instance). A caller
/// diagnosing "why did this comparator fail" benefits from knowing which
/// of the two happened; a caller only asking "may I trust this as a
/// relevance signal" can treat every non-`Success` variant identically.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EngineLookup {
    /// The request round-tripped and the backend answered successfully.
    /// `Success(vec![])` is a real, scoreable zero-result query -- it
    /// must never be produced by any failure path in this crate.
    Success(Vec<String>),
    /// The request itself did not complete: connection refused, timed
    /// out, DNS failure, or a non-2xx/3xx HTTP status with no
    /// interpretable body.
    TransportError(String),
    /// The request round-tripped and the backend replied, but the
    /// backend itself reported the query as invalid or erroring (e.g.
    /// Solr's `responseHeader.status != 0`: a malformed `fq`/`q`, an
    /// unknown field, etc.). This is evidence of a comparator-
    /// construction bug in the harness, not a relevance signal.
    QueryError(String),
    /// The response round-tripped with an outwardly successful status,
    /// but the body was not valid JSON, or valid JSON missing the
    /// expected result shape. A harness/response-format bug, not a
    /// relevance signal.
    ParseError(String),
}

impl EngineLookup {
    /// The matched document ids, only for a genuine success. Every other
    /// variant returns `None` -- deliberately not `Some(&[])`, so a
    /// caller cannot accidentally treat a failure as an empty success by
    /// reaching for this accessor instead of matching explicitly.
    pub fn ids(&self) -> Option<&[String]> {
        match self {
            EngineLookup::Success(ids) => Some(ids),
            _ => None,
        }
    }

    pub fn is_success(&self) -> bool {
        matches!(self, EngineLookup::Success(_))
    }

    /// A short, human-readable failure description, empty for `Success`.
    /// Used for failure logs/reports rather than `{:?}` so callers get a
    /// consistent "kind: detail" shape regardless of which variant fired.
    pub fn failure_description(&self) -> Option<String> {
        match self {
            EngineLookup::Success(_) => None,
            EngineLookup::TransportError(detail) => Some(format!("transport_error: {detail}")),
            EngineLookup::QueryError(detail) => Some(format!("query_error: {detail}")),
            EngineLookup::ParseError(detail) => Some(format!("parse_error: {detail}")),
        }
    }
}
