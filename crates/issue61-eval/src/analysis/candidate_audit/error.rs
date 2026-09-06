use crate::{AuditVerdict, Dataset};
use std::error::Error;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CandidateEvidenceKind {
    Workload,
    Audit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CandidateSetSide {
    Native,
    Engine,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CandidateAuditError {
    DatasetMultiplicity {
        dataset: Dataset,
        found: usize,
    },
    WrongRecordCount {
        dataset: Dataset,
        kind: CandidateEvidenceKind,
        expected: usize,
        found: usize,
    },
    BlankWorkloadQueryId {
        dataset: Dataset,
        query: usize,
    },
    DuplicateWorkloadQueryId {
        dataset: Dataset,
        query: usize,
    },
    DuplicateAuditQueryId {
        dataset: Dataset,
        record: usize,
    },
    UnexpectedAuditQueryId {
        dataset: Dataset,
        record: usize,
    },
    MissingAuditQueryId {
        dataset: Dataset,
        query: usize,
    },
    AuditDatasetMismatch {
        dataset: Dataset,
        record: usize,
    },
    AdmissionClassMismatch {
        dataset: Dataset,
        record: usize,
    },
    UnpairedCandidateFields {
        dataset: Dataset,
        record: usize,
        side: CandidateSetSide,
    },
    InvalidCandidateDigest {
        dataset: Dataset,
        record: usize,
        side: CandidateSetSide,
    },
    InvalidVerdictShape {
        dataset: Dataset,
        record: usize,
        verdict: AuditVerdict,
    },
}

impl fmt::Display for CandidateAuditError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "invalid candidate audit evidence: {self:?}")
    }
}

impl Error for CandidateAuditError {}
