use super::identity::ValidatedRecord;
use crate::Engine;
use std::error::Error;
use std::fmt;

const MAX_DISAGREEMENT_PERCENT: u128 = 2;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProcessCpuSummary {
    pub native_records: usize,
    pub solr_records: usize,
    pub max_disagreement_pct: f64,
    pub reconciliation_passed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProcessReconciliationError {
    IncompleteNativeTuple { record: usize },
    UnexpectedSolrTuple { record: usize },
    CpuTotalOverflow { record: usize },
    CpuTotalMismatch { record: usize },
    ZeroCgroupCpu { record: usize },
    InvalidStoredDisagreement { record: usize },
    StoredDisagreementMismatch { record: usize },
}

impl fmt::Display for ProcessReconciliationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "invalid process CPU evidence: {self:?}")
    }
}

impl Error for ProcessReconciliationError {}

pub(super) fn validate_process_reconciliation(
    records: &[ValidatedRecord<'_>],
) -> Result<ProcessCpuSummary, ProcessReconciliationError> {
    let mut native_records = 0;
    let mut solr_records = 0;
    let mut max_disagreement_pct: f64 = 0.0;
    let mut reconciliation_passed = true;
    for record in records.iter().copied() {
        let raw = record.raw();
        match record.spec().engine() {
            Engine::Native => {
                native_records += 1;
                let (user, system, total, stored) = match (
                    raw.process_cpu_user_usec,
                    raw.process_cpu_system_usec,
                    raw.process_cpu_total_usec,
                    raw.process_cgroup_disagreement_pct,
                ) {
                    (Some(user), Some(system), Some(total), Some(stored)) => {
                        (user, system, total, stored)
                    }
                    (Some(_), Some(_), Some(_), None)
                    | (Some(_), Some(_), None, Some(_))
                    | (Some(_), Some(_), None, None)
                    | (Some(_), None, Some(_), Some(_))
                    | (Some(_), None, Some(_), None)
                    | (Some(_), None, None, Some(_))
                    | (Some(_), None, None, None)
                    | (None, Some(_), Some(_), Some(_))
                    | (None, Some(_), Some(_), None)
                    | (None, Some(_), None, Some(_))
                    | (None, Some(_), None, None)
                    | (None, None, Some(_), Some(_))
                    | (None, None, Some(_), None)
                    | (None, None, None, Some(_))
                    | (None, None, None, None) => {
                        return Err(ProcessReconciliationError::IncompleteNativeTuple {
                            record: record.source_index(),
                        });
                    }
                };
                let recomputed_total = user.checked_add(system).ok_or(
                    ProcessReconciliationError::CpuTotalOverflow {
                        record: record.source_index(),
                    },
                )?;
                if recomputed_total != total {
                    return Err(ProcessReconciliationError::CpuTotalMismatch {
                        record: record.source_index(),
                    });
                }
                if raw.cpu_usage_usec == 0 {
                    return Err(ProcessReconciliationError::ZeroCgroupCpu {
                        record: record.source_index(),
                    });
                }
                if !stored.is_finite() || stored < 0.0 {
                    return Err(ProcessReconciliationError::InvalidStoredDisagreement {
                        record: record.source_index(),
                    });
                }
                let difference = total.abs_diff(raw.cpu_usage_usec);
                let recomputed = difference as f64 * 100.0 / raw.cpu_usage_usec as f64;
                if stored != recomputed {
                    return Err(ProcessReconciliationError::StoredDisagreementMismatch {
                        record: record.source_index(),
                    });
                }
                max_disagreement_pct = max_disagreement_pct.max(recomputed);
                reconciliation_passed &= u128::from(difference) * 100
                    <= u128::from(raw.cpu_usage_usec) * MAX_DISAGREEMENT_PERCENT;
            }
            Engine::Solr => {
                solr_records += 1;
                match (
                    raw.process_cpu_user_usec,
                    raw.process_cpu_system_usec,
                    raw.process_cpu_total_usec,
                    raw.process_cgroup_disagreement_pct,
                ) {
                    (None, None, None, None) => {}
                    (Some(_), Some(_), Some(_), Some(_))
                    | (Some(_), Some(_), Some(_), None)
                    | (Some(_), Some(_), None, Some(_))
                    | (Some(_), Some(_), None, None)
                    | (Some(_), None, Some(_), Some(_))
                    | (Some(_), None, Some(_), None)
                    | (Some(_), None, None, Some(_))
                    | (Some(_), None, None, None)
                    | (None, Some(_), Some(_), Some(_))
                    | (None, Some(_), Some(_), None)
                    | (None, Some(_), None, Some(_))
                    | (None, Some(_), None, None)
                    | (None, None, Some(_), Some(_))
                    | (None, None, Some(_), None)
                    | (None, None, None, Some(_)) => {
                        return Err(ProcessReconciliationError::UnexpectedSolrTuple {
                            record: record.source_index(),
                        });
                    }
                }
            }
        }
    }
    Ok(ProcessCpuSummary {
        native_records,
        solr_records,
        max_disagreement_pct,
        reconciliation_passed,
    })
}
