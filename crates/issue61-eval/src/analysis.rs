mod aggregate;
mod calibration;
mod candidate_audit;
mod error;
mod identity;
mod process_reconciliation;

#[cfg(test)]
mod test_support;

pub use aggregate::{analyze_campaign, CampaignAnalysis, CampaignEvidence, ExactIndexObservation};
pub use calibration::analyze_calibration;
pub use candidate_audit::{
    CandidateAuditError, CandidateAuditEvidence, CandidateAuditSummary, CandidateEvidenceKind,
    CandidateSetSide,
};
pub use error::{AnalysisError, IdentityField, MetricIdentity};
pub use identity::RawSessionKey;
pub use process_reconciliation::{ProcessCpuSummary, ProcessReconciliationError};
