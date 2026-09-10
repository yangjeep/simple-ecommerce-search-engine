use super::calibration::calibrations_from_records;
use super::candidate_audit::{
    validate_candidate_audits, CandidateAuditEvidence, CandidateAuditSummary,
};
use super::identity::{validate_campaign_records, ValidatedRecord};
use super::process_reconciliation::{validate_process_reconciliation, ProcessCpuSummary};
use super::{AnalysisError, MetricIdentity};
use crate::gate::{evaluate_campaign_gate, DatasetCellStability, GlobalGateChecks, WarmCellKey};
use crate::{
    evaluate_cell, evaluate_exact_artifact, CampaignCycle, CampaignPhase, CellStability, Dataset,
    Engine, GateReport, MetricKind, RawRecord, WorkloadProjection,
};

const ENGINES: [Engine; 2] = [Engine::Native, Engine::Solr];
const DATASETS: [Dataset; 2] = [Dataset::Wands, Dataset::EsciElectronics];
const PROJECTIONS: [WorkloadProjection; 4] = [
    WorkloadProjection::All,
    WorkloadProjection::FastPath,
    WorkloadProjection::Hybrid,
    WorkloadProjection::Punt,
];
const METRICS: [MetricIdentity; 3] = [
    MetricIdentity::CpuUsPerQuery,
    MetricIdentity::LatencyP50Us,
    MetricIdentity::MemoryCurrentMedianBytes,
];
const MAX_EXACT_INTEGER_BYTES: u64 = 1 << 53;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExactIndexObservation {
    pub cycle: CampaignCycle,
    pub engine: Engine,
    pub dataset: Dataset,
    pub bytes: u64,
}

#[derive(Clone, Copy)]
pub struct CampaignEvidence<'a> {
    pub cycle: CampaignCycle,
    pub records: &'a [RawRecord],
    pub exact_index: &'a [ExactIndexObservation],
    pub candidate_audits: &'a [CandidateAuditEvidence<'a>],
}

#[derive(Debug, Clone, PartialEq)]
pub struct CampaignAnalysis {
    pub cycle: CampaignCycle,
    pub gate_report: GateReport,
    pub exact_index_cells: Vec<CellStability>,
    pub cold_session_count: usize,
    pub candidate_audit_summary: CandidateAuditSummary,
    pub process_cpu_summary: ProcessCpuSummary,
}

impl MetricIdentity {
    const fn as_str(self) -> &'static str {
        match self {
            Self::CpuUsPerQuery => "cpu_us_per_query",
            Self::LatencyP50Us => "latency_p50_us",
            Self::MemoryCurrentMedianBytes => "cgroup_memory_current_median_bytes",
        }
    }

    const fn kind(self) -> MetricKind {
        match self {
            Self::CpuUsPerQuery | Self::LatencyP50Us => MetricKind::CpuOrLatency,
            Self::MemoryCurrentMedianBytes => MetricKind::Footprint,
        }
    }
}

fn metric_sample(
    record: ValidatedRecord<'_>,
    metric: MetricIdentity,
) -> Result<f64, AnalysisError> {
    let raw = record.raw();
    let value = match metric {
        MetricIdentity::CpuUsPerQuery => {
            raw.cpu_us_per_query().ok_or(AnalysisError::MissingMetric {
                record: record.source_index(),
                metric,
            })?
        }
        MetricIdentity::LatencyP50Us => raw.latency_p50_us,
        MetricIdentity::MemoryCurrentMedianBytes => raw.cgroup_memory_current_median_bytes as f64,
    };
    if value.is_finite() && value > 0.0 {
        Ok(value)
    } else {
        Err(AnalysisError::InvalidMetric {
            record: record.source_index(),
            metric,
        })
    }
}

fn warm_cells(records: &[ValidatedRecord<'_>]) -> Result<Vec<DatasetCellStability>, AnalysisError> {
    let mut cells = Vec::with_capacity(48);
    for engine in ENGINES {
        for dataset in DATASETS {
            for projection in PROJECTIONS {
                for metric in METRICS {
                    let samples: Result<Vec<_>, _> = records
                        .iter()
                        .copied()
                        .filter(|record| {
                            record.spec().series().phase() == CampaignPhase::Warm
                                && record.spec().engine() == engine
                                && record.spec().series().dataset() == dataset
                                && record.spec().series().projection() == projection
                        })
                        .map(|record| metric_sample(record, metric))
                        .collect();
                    let cell = format!(
                        "{}/{}/{}/warm",
                        engine.as_str(),
                        dataset.as_str(),
                        projection.as_str()
                    );
                    cells.push(DatasetCellStability {
                        key: WarmCellKey {
                            engine,
                            dataset,
                            projection,
                            metric,
                        },
                        cell: evaluate_cell(&cell, metric.as_str(), metric.kind(), &samples?),
                    });
                }
            }
        }
    }
    Ok(cells)
}

fn exact_index_cells(
    cycle: CampaignCycle,
    observations: &[ExactIndexObservation],
) -> Result<Vec<CellStability>, AnalysisError> {
    let expected: Vec<_> = ENGINES
        .into_iter()
        .flat_map(|engine| DATASETS.into_iter().map(move |dataset| (engine, dataset)))
        .collect();
    let mut matched = vec![None; expected.len()];
    for (index, observation) in observations.iter().copied().enumerate() {
        if observation.cycle != cycle {
            return Err(AnalysisError::UnexpectedExactIndex {
                observation: index,
                cycle: observation.cycle,
            });
        }
        if observation.bytes == 0 || observation.bytes > MAX_EXACT_INTEGER_BYTES {
            return Err(AnalysisError::InvalidExactIndexBytes {
                observation: index,
                engine: observation.engine,
                dataset: observation.dataset,
            });
        }
        let position = expected
            .iter()
            .position(|key| *key == (observation.engine, observation.dataset));
        let Some(position) = position else {
            return Err(AnalysisError::UnexpectedExactIndex {
                observation: index,
                cycle: observation.cycle,
            });
        };
        if matched[position].replace(observation).is_some() {
            return Err(AnalysisError::DuplicateExactIndex {
                observation: index,
                engine: observation.engine,
                dataset: observation.dataset,
            });
        }
    }
    expected
        .into_iter()
        .zip(matched)
        .map(|((engine, dataset), observation)| {
            let observation =
                observation.ok_or(AnalysisError::MissingExactIndex { engine, dataset })?;
            Ok(evaluate_exact_artifact(
                &format!("{}/{}/exact-index", engine.as_str(), dataset.as_str()),
                "index_serialized_bytes",
                &[observation.bytes as f64],
                0.0,
            ))
        })
        .collect()
}

pub fn analyze_campaign(evidence: CampaignEvidence<'_>) -> Result<CampaignAnalysis, AnalysisError> {
    let records = validate_campaign_records(evidence.cycle, evidence.records)?;
    let candidate_audit_summary = validate_candidate_audits(evidence.candidate_audits)?;
    let process_cpu_summary = validate_process_reconciliation(&records)?;
    let calibrations = calibrations_from_records(evidence.cycle, &records)?;
    let cells = warm_cells(&records)?;
    let exact_index_cells = exact_index_cells(evidence.cycle, evidence.exact_index)?;
    let cold_session_count = records
        .iter()
        .filter(|record| record.spec().series().phase() == CampaignPhase::Cold)
        .count();
    Ok(CampaignAnalysis {
        cycle: evidence.cycle,
        gate_report: evaluate_campaign_gate(
            cells,
            calibrations,
            GlobalGateChecks {
                equivalence_passed: candidate_audit_summary.equivalence_passed,
                process_reconciliation_passed: process_cpu_summary.reconciliation_passed,
            },
        ),
        exact_index_cells,
        cold_session_count,
        candidate_audit_summary,
        process_cpu_summary,
    })
}

#[cfg(test)]
mod tests;
