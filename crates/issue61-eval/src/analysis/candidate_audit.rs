mod error;

use crate::{AuditVerdict, CandidateAuditRecord, Dataset, FrozenQuery};
pub use error::{CandidateAuditError, CandidateEvidenceKind, CandidateSetSide};
use std::collections::{BTreeMap, BTreeSet};

const WANDS_RECORDS: usize = 480;
const ESCI_RECORDS: usize = 600;

#[derive(Debug, Clone, Copy)]
pub struct CandidateAuditEvidence<'a> {
    pub dataset: Dataset,
    pub workload: &'a [FrozenQuery],
    pub records: &'a [CandidateAuditRecord],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CandidateAuditSummary {
    pub total_records: usize,
    pub matched_records: usize,
    pub mismatched_records: usize,
    pub native_failure_records: usize,
    pub engine_failure_records: usize,
    pub equivalence_passed: bool,
}

#[derive(Clone, Copy)]
struct AuditLocation {
    dataset: Dataset,
    record: usize,
}

#[derive(Clone, Copy)]
struct CandidatePair<'a> {
    count: Option<usize>,
    digest: Option<&'a str>,
}

pub(super) fn validate_candidate_audits(
    evidence: &[CandidateAuditEvidence<'_>],
) -> Result<CandidateAuditSummary, CandidateAuditError> {
    let mut total_records = 0;
    let mut matched_records = 0;
    let mut mismatched_records = 0;
    let mut native_failure_records = 0;
    let mut engine_failure_records = 0;
    for dataset in [Dataset::Wands, Dataset::EsciElectronics] {
        let evidence = evidence_for_dataset(evidence, dataset)?;
        let verdicts = validate_dataset(evidence)?;
        total_records += verdicts.len();
        for verdict in verdicts {
            match verdict {
                AuditVerdict::Match => matched_records += 1,
                AuditVerdict::Mismatch => mismatched_records += 1,
                AuditVerdict::NativeFailure => native_failure_records += 1,
                AuditVerdict::EngineFailure => engine_failure_records += 1,
            }
        }
    }
    Ok(CandidateAuditSummary {
        total_records,
        matched_records,
        mismatched_records,
        native_failure_records,
        engine_failure_records,
        equivalence_passed: matched_records == total_records,
    })
}

fn evidence_for_dataset<'a>(
    evidence: &[CandidateAuditEvidence<'a>],
    dataset: Dataset,
) -> Result<CandidateAuditEvidence<'a>, CandidateAuditError> {
    let matching: Vec<_> = evidence
        .iter()
        .copied()
        .filter(|item| item.dataset == dataset)
        .collect();
    match matching.as_slice() {
        [item] => Ok(*item),
        items => Err(CandidateAuditError::DatasetMultiplicity {
            dataset,
            found: items.len(),
        }),
    }
}

fn validate_dataset(
    evidence: CandidateAuditEvidence<'_>,
) -> Result<Vec<AuditVerdict>, CandidateAuditError> {
    let expected = match evidence.dataset {
        Dataset::Wands => WANDS_RECORDS,
        Dataset::EsciElectronics => ESCI_RECORDS,
    };
    if evidence.workload.len() != expected {
        return Err(CandidateAuditError::WrongRecordCount {
            dataset: evidence.dataset,
            kind: CandidateEvidenceKind::Workload,
            expected,
            found: evidence.workload.len(),
        });
    }
    if evidence.records.len() != expected {
        return Err(CandidateAuditError::WrongRecordCount {
            dataset: evidence.dataset,
            kind: CandidateEvidenceKind::Audit,
            expected,
            found: evidence.records.len(),
        });
    }
    let mut workload = BTreeMap::new();
    for (query, frozen) in evidence.workload.iter().enumerate() {
        if frozen.query_id.trim().is_empty() {
            return Err(CandidateAuditError::BlankWorkloadQueryId {
                dataset: evidence.dataset,
                query,
            });
        }
        if workload
            .insert(frozen.query_id.as_str(), (query, frozen.admission_class))
            .is_some()
        {
            return Err(CandidateAuditError::DuplicateWorkloadQueryId {
                dataset: evidence.dataset,
                query,
            });
        }
    }
    let mut seen = BTreeSet::new();
    let mut verdicts = Vec::with_capacity(expected);
    for (record_index, record) in evidence.records.iter().enumerate() {
        let location = AuditLocation {
            dataset: evidence.dataset,
            record: record_index,
        };
        if record.dataset != evidence.dataset.as_str() {
            return Err(CandidateAuditError::AuditDatasetMismatch {
                dataset: evidence.dataset,
                record: record_index,
            });
        }
        let Some((_, admission_class)) = workload.get(record.query_id.as_str()).copied() else {
            return Err(CandidateAuditError::UnexpectedAuditQueryId {
                dataset: evidence.dataset,
                record: record_index,
            });
        };
        if !seen.insert(record.query_id.as_str()) {
            return Err(CandidateAuditError::DuplicateAuditQueryId {
                dataset: evidence.dataset,
                record: record_index,
            });
        }
        if record.admission_class != admission_class {
            return Err(CandidateAuditError::AdmissionClassMismatch {
                dataset: evidence.dataset,
                record: record_index,
            });
        }
        validate_record_shape(location, record)?;
        verdicts.push(record.verdict);
    }
    for (query_id, (query, _)) in workload {
        if !seen.contains(query_id) {
            return Err(CandidateAuditError::MissingAuditQueryId {
                dataset: evidence.dataset,
                query,
            });
        }
    }
    Ok(verdicts)
}

fn validate_record_shape(
    location: AuditLocation,
    record: &CandidateAuditRecord,
) -> Result<(), CandidateAuditError> {
    let native = validate_pair(
        location,
        CandidateSetSide::Native,
        CandidatePair {
            count: record.native_count,
            digest: record.native_digest.as_deref(),
        },
    )?;
    let engine = validate_pair(
        location,
        CandidateSetSide::Engine,
        CandidatePair {
            count: record.engine_count,
            digest: record.engine_digest.as_deref(),
        },
    )?;
    let differences_empty = record.only_native.is_empty() && record.only_engine.is_empty();
    let no_failure = record.failure_reason.is_none();
    let failure = record
        .failure_reason
        .as_deref()
        .is_some_and(|reason| !reason.trim().is_empty());
    let valid = match record.verdict {
        AuditVerdict::Match => {
            native.is_some()
                && engine.is_some()
                && native == engine
                && differences_empty
                && no_failure
        }
        AuditVerdict::Mismatch => {
            native.zip(engine).is_some_and(
                |((native_count, native_digest), (engine_count, engine_digest))| {
                    let digest_differs = native_digest != engine_digest;
                    (native_count != engine_count || digest_differs)
                        && (!digest_differs || !differences_empty)
                },
            ) && no_failure
        }
        AuditVerdict::NativeFailure => native.is_none() && differences_empty && failure,
        AuditVerdict::EngineFailure => engine.is_none() && differences_empty && failure,
    };
    if valid {
        Ok(())
    } else {
        Err(CandidateAuditError::InvalidVerdictShape {
            dataset: location.dataset,
            record: location.record,
            verdict: record.verdict,
        })
    }
}

fn validate_pair<'a>(
    location: AuditLocation,
    side: CandidateSetSide,
    pair: CandidatePair<'a>,
) -> Result<Option<(usize, &'a str)>, CandidateAuditError> {
    match (pair.count, pair.digest) {
        (None, None) => Ok(None),
        (Some(count), Some(digest)) if valid_digest(digest) => Ok(Some((count, digest))),
        (Some(_), Some(_)) => Err(CandidateAuditError::InvalidCandidateDigest {
            dataset: location.dataset,
            record: location.record,
            side,
        }),
        (Some(_), None) | (None, Some(_)) => Err(CandidateAuditError::UnpairedCandidateFields {
            dataset: location.dataset,
            record: location.record,
            side,
        }),
    }
}

fn valid_digest(digest: &str) -> bool {
    digest.len() == 64
        && digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
