use crate::{sha256_hex, AdmissionClass, FrozenQuery};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq)]
pub enum EngineOutcome {
    Ids(Vec<String>),
    HttpError(String),
    TransportError(String),
    QueryError(String),
    ParseError(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum QueryVerdict {
    Match,
    Mismatch {
        only_native: Vec<String>,
        only_engine: Vec<String>,
    },
    ExcludedEngineFailure {
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct QueryAudit {
    pub query_id: String,
    pub verdict: QueryVerdict,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EquivalenceReport {
    pub total: usize,
    pub matched: usize,
    pub mismatched: usize,
    pub excluded: usize,
    pub audits: Vec<QueryAudit>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditVerdict {
    Match,
    Mismatch,
    NativeFailure,
    EngineFailure,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CandidateAuditRecord {
    pub dataset: String,
    pub query_id: String,
    pub admission_class: AdmissionClass,
    pub native_count: Option<usize>,
    pub native_digest: Option<String>,
    pub engine_count: Option<usize>,
    pub engine_digest: Option<String>,
    pub verdict: AuditVerdict,
    pub only_native: Vec<String>,
    pub only_engine: Vec<String>,
    pub failure_reason: Option<String>,
}

impl EquivalenceReport {
    pub fn match_rate(&self) -> Option<f64> {
        let comparable = self.total - self.excluded;
        (comparable > 0).then(|| self.matched as f64 / comparable as f64)
    }

    pub fn mismatches(&self) -> impl Iterator<Item = &QueryAudit> {
        self.audits
            .iter()
            .filter(|audit| matches!(audit.verdict, QueryVerdict::Mismatch { .. }))
    }

    pub fn passes(&self) -> bool {
        self.mismatched == 0 && self.excluded == 0
    }
}

pub fn audit_query(query_id: &str, native_ids: &[String], engine: &EngineOutcome) -> QueryAudit {
    let verdict = match engine {
        EngineOutcome::Ids(engine_ids) => compare_ids(native_ids, engine_ids),
        EngineOutcome::HttpError(reason) => QueryVerdict::ExcludedEngineFailure {
            reason: format!("http error: {reason}"),
        },
        EngineOutcome::TransportError(reason) => QueryVerdict::ExcludedEngineFailure {
            reason: format!("transport error: {reason}"),
        },
        EngineOutcome::QueryError(reason) => QueryVerdict::ExcludedEngineFailure {
            reason: format!("query error: {reason}"),
        },
        EngineOutcome::ParseError(reason) => QueryVerdict::ExcludedEngineFailure {
            reason: format!("parse error: {reason}"),
        },
    };
    QueryAudit {
        query_id: query_id.to_owned(),
        verdict,
    }
}

pub fn audit_all(pairs: &[(String, Vec<String>, EngineOutcome)]) -> EquivalenceReport {
    let audits: Vec<QueryAudit> = pairs
        .iter()
        .map(|(query_id, native_ids, engine)| audit_query(query_id, native_ids, engine))
        .collect();
    let mut matched = 0;
    let mut mismatched = 0;
    let mut excluded = 0;
    for audit in &audits {
        match audit.verdict {
            QueryVerdict::Match => matched += 1,
            QueryVerdict::Mismatch { .. } => mismatched += 1,
            QueryVerdict::ExcludedEngineFailure { .. } => excluded += 1,
        }
    }
    EquivalenceReport {
        total: audits.len(),
        matched,
        mismatched,
        excluded,
        audits,
    }
}

fn compare_ids(native_ids: &[String], engine_ids: &[String]) -> QueryVerdict {
    let native: BTreeSet<&str> = native_ids.iter().map(String::as_str).collect();
    let engine: BTreeSet<&str> = engine_ids.iter().map(String::as_str).collect();
    if native == engine {
        return QueryVerdict::Match;
    }
    QueryVerdict::Mismatch {
        only_native: native
            .difference(&engine)
            .map(|value| (*value).to_owned())
            .collect(),
        only_engine: engine
            .difference(&native)
            .map(|value| (*value).to_owned())
            .collect(),
    }
}

pub fn candidate_digest(ids: &[String]) -> String {
    let canonical: BTreeSet<&str> = ids.iter().map(String::as_str).collect();
    sha256_hex(
        canonical
            .into_iter()
            .collect::<Vec<_>>()
            .join("\n")
            .as_bytes(),
    )
}

pub fn audit_candidate_sets(
    dataset: &str,
    query: &FrozenQuery,
    native: Result<Vec<String>, String>,
    engine: EngineOutcome,
) -> CandidateAuditRecord {
    match (native, engine) {
        (Ok(native_ids), EngineOutcome::Ids(engine_ids)) => {
            let native_digest = candidate_digest(&native_ids);
            let engine_digest = candidate_digest(&engine_ids);
            let matches = native_ids.len() == engine_ids.len() && native_digest == engine_digest;
            let (only_native, only_engine) = if matches {
                (Vec::new(), Vec::new())
            } else {
                directional_differences(&native_ids, &engine_ids)
            };
            CandidateAuditRecord {
                dataset: dataset.to_owned(),
                query_id: query.query_id.clone(),
                admission_class: query.admission_class,
                native_count: Some(native_ids.len()),
                native_digest: Some(native_digest),
                engine_count: Some(engine_ids.len()),
                engine_digest: Some(engine_digest),
                verdict: if matches {
                    AuditVerdict::Match
                } else {
                    AuditVerdict::Mismatch
                },
                only_native,
                only_engine,
                failure_reason: None,
            }
        }
        (Err(reason), engine) => CandidateAuditRecord {
            dataset: dataset.to_owned(),
            query_id: query.query_id.clone(),
            admission_class: query.admission_class,
            native_count: None,
            native_digest: None,
            engine_count: engine_ids(&engine).map(Vec::len),
            engine_digest: engine_ids(&engine).map(|ids| candidate_digest(ids)),
            verdict: AuditVerdict::NativeFailure,
            only_native: Vec::new(),
            only_engine: Vec::new(),
            failure_reason: Some(format!("native error: {reason}")),
        },
        (Ok(native_ids), engine) => CandidateAuditRecord {
            dataset: dataset.to_owned(),
            query_id: query.query_id.clone(),
            admission_class: query.admission_class,
            native_count: Some(native_ids.len()),
            native_digest: Some(candidate_digest(&native_ids)),
            engine_count: None,
            engine_digest: None,
            verdict: AuditVerdict::EngineFailure,
            only_native: Vec::new(),
            only_engine: Vec::new(),
            failure_reason: Some(engine_failure_reason(&engine)),
        },
    }
}

fn engine_ids(outcome: &EngineOutcome) -> Option<&Vec<String>> {
    match outcome {
        EngineOutcome::Ids(ids) => Some(ids),
        EngineOutcome::HttpError(_)
        | EngineOutcome::TransportError(_)
        | EngineOutcome::QueryError(_)
        | EngineOutcome::ParseError(_) => None,
    }
}

fn engine_failure_reason(outcome: &EngineOutcome) -> String {
    match outcome {
        EngineOutcome::HttpError(reason) => format!("http error: {reason}"),
        EngineOutcome::TransportError(reason) => format!("transport error: {reason}"),
        EngineOutcome::QueryError(reason) => format!("query error: {reason}"),
        EngineOutcome::ParseError(reason) => format!("parse error: {reason}"),
        EngineOutcome::Ids(_) => "engine result unavailable".to_string(),
    }
}

fn directional_differences(
    native_ids: &[String],
    engine_ids: &[String],
) -> (Vec<String>, Vec<String>) {
    let native: BTreeSet<&str> = native_ids.iter().map(String::as_str).collect();
    let engine: BTreeSet<&str> = engine_ids.iter().map(String::as_str).collect();
    (
        native
            .difference(&engine)
            .map(|id| (*id).to_owned())
            .collect(),
        engine
            .difference(&native)
            .map(|id| (*id).to_owned())
            .collect(),
    )
}
