use super::RawSessionKey;
use super::{
    candidate_audit::CandidateAuditError, process_reconciliation::ProcessReconciliationError,
};
use crate::{CampaignCycle, Dataset, Engine, RAW_SCHEMA_VERSION};
use std::error::Error;
use std::fmt;

const EXPERIMENT_ID: &str = "I61-E1";
const RUN_ID: &str = "seed-61";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdentityField {
    Engine,
    Dataset,
    QueryClass,
    Regime,
    EngineOrder,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetricIdentity {
    CpuUsPerQuery,
    LatencyP50Us,
    MemoryCurrentMedianBytes,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnalysisError {
    UnsupportedSchemaVersion {
        record: usize,
        found: u32,
    },
    WrongExperimentId {
        record: usize,
    },
    WrongRunId {
        record: usize,
    },
    ExcludedRecord {
        record: usize,
    },
    UnexpectedExclusionReason {
        record: usize,
    },
    MalformedIdentity {
        record: usize,
        field: IdentityField,
    },
    UnexpectedIdentity {
        record: usize,
        key: RawSessionKey,
    },
    DuplicateIdentity {
        record: usize,
        key: RawSessionKey,
    },
    MissingIdentity {
        key: RawSessionKey,
    },
    InvalidCalibrationPlan {
        engine: Engine,
        block: usize,
    },
    MissingMetric {
        record: usize,
        metric: MetricIdentity,
    },
    InvalidMetric {
        record: usize,
        metric: MetricIdentity,
    },
    MissingExactIndex {
        engine: Engine,
        dataset: Dataset,
    },
    DuplicateExactIndex {
        observation: usize,
        engine: Engine,
        dataset: Dataset,
    },
    UnexpectedExactIndex {
        observation: usize,
        cycle: CampaignCycle,
    },
    InvalidExactIndexBytes {
        observation: usize,
        engine: Engine,
        dataset: Dataset,
    },
    CandidateAudit(CandidateAuditError),
    ProcessReconciliation(ProcessReconciliationError),
}

impl fmt::Display for AnalysisError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedSchemaVersion { record, found } => write!(
                formatter,
                "record {record} has schema version {found}; expected {RAW_SCHEMA_VERSION}"
            ),
            Self::WrongExperimentId { record } => {
                write!(
                    formatter,
                    "record {record} does not belong to {EXPERIMENT_ID}"
                )
            }
            Self::WrongRunId { record } => {
                write!(formatter, "record {record} does not belong to {RUN_ID}")
            }
            Self::ExcludedRecord { record } => write!(formatter, "record {record} is excluded"),
            Self::UnexpectedExclusionReason { record } => {
                write!(formatter, "record {record} has an exclusion reason")
            }
            Self::MalformedIdentity { record, field } => {
                write!(formatter, "record {record} has malformed {field:?}")
            }
            Self::UnexpectedIdentity { record, key } => {
                write!(formatter, "record {record} has unexpected identity {key:?}")
            }
            Self::DuplicateIdentity { record, key } => {
                write!(formatter, "record {record} duplicates identity {key:?}")
            }
            Self::MissingIdentity { key } => write!(formatter, "missing identity {key:?}"),
            Self::InvalidCalibrationPlan { engine, block } => write!(
                formatter,
                "calibration plan has no four/five pair for {engine:?} block {block}"
            ),
            Self::MissingMetric { record, metric } => {
                write!(formatter, "record {record} has no {metric:?} sample")
            }
            Self::InvalidMetric { record, metric } => {
                write!(
                    formatter,
                    "record {record} has an invalid {metric:?} sample"
                )
            }
            Self::MissingExactIndex { engine, dataset } => {
                write!(
                    formatter,
                    "missing exact index observation for {engine:?}/{dataset:?}"
                )
            }
            Self::DuplicateExactIndex {
                observation,
                engine,
                dataset,
            } => write!(
                formatter,
                "exact index observation {observation} duplicates {engine:?}/{dataset:?}"
            ),
            Self::UnexpectedExactIndex { observation, cycle } => write!(
                formatter,
                "exact index observation {observation} belongs to unexpected cycle {cycle:?}"
            ),
            Self::InvalidExactIndexBytes {
                observation,
                engine,
                dataset,
            } => write!(
                formatter,
                "exact index observation {observation} has invalid bytes for {engine:?}/{dataset:?}"
            ),
            Self::CandidateAudit(source) => write!(formatter, "{source}"),
            Self::ProcessReconciliation(source) => write!(formatter, "{source}"),
        }
    }
}

impl Error for AnalysisError {}

impl From<CandidateAuditError> for AnalysisError {
    fn from(source: CandidateAuditError) -> Self {
        Self::CandidateAudit(source)
    }
}

impl From<ProcessReconciliationError> for AnalysisError {
    fn from(source: ProcessReconciliationError) -> Self {
        Self::ProcessReconciliation(source)
    }
}
