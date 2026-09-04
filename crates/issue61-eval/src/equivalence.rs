use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq)]
pub enum EngineOutcome {
    Ids(Vec<String>),
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
