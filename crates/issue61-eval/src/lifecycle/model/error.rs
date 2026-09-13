use super::{EvidenceFile, IndexError, LoggedText};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LifecycleError {
    StaticValidation,
    Initialization,
    UnclassifiedInput,
    InvalidEnvironment,
    Serialization,
    CommandEvidence,
    EventEvidence,
    EquivalenceAudit,
    IndexCapture,
    Index(IndexError),
    Calibration(crate::AnalysisError),
    RejectionLimit { series: crate::CampaignSeries },
    SlotFailed,
    TeardownFailed,
    Flush(EvidenceFile),
    Sync(EvidenceFile),
    Close(EvidenceFile),
    SealCreateNew,
    SealWriteAll,
    SealFlush,
    SealSync,
    SealClose,
    InvalidSeal,
    EnvironmentLaunch,
    AnalyzerFailed,
}

impl LifecycleError {
    pub(crate) fn classified_reason(&self) -> LoggedText {
        LoggedText::Public(match self {
            Self::StaticValidation => "static_validation_failed".to_owned(),
            Self::Initialization => "initialization_failed".to_owned(),
            Self::UnclassifiedInput => "unclassified_input".to_owned(),
            Self::InvalidEnvironment => "invalid_environment".to_owned(),
            Self::Serialization => "serialization_failed".to_owned(),
            Self::CommandEvidence => "command_evidence_failed".to_owned(),
            Self::EventEvidence => "event_evidence_failed".to_owned(),
            Self::EquivalenceAudit => "equivalence_audit_failed".to_owned(),
            Self::IndexCapture => "index_capture_failed".to_owned(),
            Self::Index(error) => format!("index_validation_failed:{error:?}"),
            Self::Calibration(error) => format!("calibration_failed:{error}"),
            Self::RejectionLimit { series } => format!("rejection_limit:{series:?}"),
            Self::SlotFailed => "slot_failed".to_owned(),
            Self::TeardownFailed => "teardown_failed".to_owned(),
            Self::Flush(file) => format!("durability_flush_failed:{file:?}"),
            Self::Sync(file) => format!("durability_sync_failed:{file:?}"),
            Self::Close(file) => format!("durability_close_failed:{file:?}"),
            Self::SealCreateNew => "seal_create_new_failed".to_owned(),
            Self::SealWriteAll => "seal_write_all_failed".to_owned(),
            Self::SealFlush => "seal_flush_failed".to_owned(),
            Self::SealSync => "seal_sync_failed".to_owned(),
            Self::SealClose => "seal_close_failed".to_owned(),
            Self::InvalidSeal => "invalid_seal".to_owned(),
            Self::EnvironmentLaunch => "environment_launch_failed".to_owned(),
            Self::AnalyzerFailed => "analyzer_failed".to_owned(),
        })
    }
}
