use super::checksum::SealedInput;
use super::error::PostRunError;
use crate::{
    CalibrationCheck, CampaignAnalysis, CellStability, GateVerdict, MetricKind, RatioResult,
};
use serde::Serialize;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

#[derive(Serialize)]
struct AnalysisReport<'a> {
    schema_version: u32,
    experiment_id: &'static str,
    cycle: &'static str,
    inputs: Vec<InputReport<'a>>,
    decision: DecisionReport,
    global_gates: GlobalGatesReport,
    calibrations: CalibrationsReport,
    candidate_audit: CandidateAuditReport,
    process_cpu: ProcessCpuReport,
    warm_cells: Vec<CellReport<'a>>,
    exact_index_cells: Vec<CellReport<'a>>,
    cold: ColdReport,
}

#[derive(Serialize)]
struct InputReport<'a> {
    path: &'a str,
    sha256: &'a str,
}

#[derive(Serialize)]
struct DecisionReport {
    verdict: &'static str,
    passing_dataset: Option<&'static str>,
    blocked_dataset: Option<&'static str>,
    exit_code: i32,
}

#[derive(Serialize)]
struct GlobalGatesReport {
    calibration_passed: bool,
    equivalence_passed: bool,
    process_cpu_reconciliation_passed: bool,
}

#[derive(Serialize)]
struct CalibrationsReport {
    passed: bool,
    arms: [CalibrationArmReport; 2],
}

#[derive(Serialize)]
struct CalibrationArmReport {
    engine: &'static str,
    expected_ratio: f64,
    observed: Option<RatioReport>,
    passed: bool,
}

#[derive(Serialize)]
struct RatioReport {
    n_blocks: usize,
    point_ratio: f64,
    ci_low: f64,
    ci_high: f64,
}

#[derive(Serialize)]
struct CandidateAuditReport {
    total_records: usize,
    matched_records: usize,
    mismatched_records: usize,
    native_failure_records: usize,
    engine_failure_records: usize,
    equivalence_passed: bool,
}

#[derive(Serialize)]
struct ProcessCpuReport {
    native_records: usize,
    solr_records: usize,
    max_disagreement_pct: f64,
    reconciliation_passed: bool,
}

#[derive(Serialize)]
struct CellReport<'a> {
    cell: &'a str,
    metric: &'a str,
    kind: &'static str,
    n: usize,
    n_blocks: usize,
    mean: f64,
    ci_low: f64,
    ci_high: f64,
    rel_halfwidth: f64,
    max_relative_halfwidth: f64,
    cv: f64,
    underpowered: bool,
    passes: bool,
}

#[derive(Serialize)]
struct ColdReport {
    session_count: usize,
    gated: bool,
}

pub fn serialize_report(
    analysis: &CampaignAnalysis,
    inputs: &[SealedInput],
) -> Result<Vec<u8>, PostRunError> {
    let calibrations = analysis.gate_report.calibrations();
    let native = calibration_arm("native", calibrations.native.as_ref());
    let solr = calibration_arm("solr", calibrations.solr.as_ref());
    let report = AnalysisReport {
        schema_version: 1,
        experiment_id: "I61-E1",
        cycle: analysis.cycle.as_str(),
        inputs: inputs
            .iter()
            .map(|input| InputReport {
                path: &input.path,
                sha256: &input.sha256,
            })
            .collect(),
        decision: decision(analysis.gate_report.verdict()),
        global_gates: GlobalGatesReport {
            calibration_passed: native.passed && solr.passed,
            equivalence_passed: analysis.gate_report.equivalence_passed(),
            process_cpu_reconciliation_passed: analysis.gate_report.process_reconciliation_passed(),
        },
        calibrations: CalibrationsReport {
            passed: native.passed && solr.passed,
            arms: [native, solr],
        },
        candidate_audit: CandidateAuditReport {
            total_records: analysis.candidate_audit_summary.total_records,
            matched_records: analysis.candidate_audit_summary.matched_records,
            mismatched_records: analysis.candidate_audit_summary.mismatched_records,
            native_failure_records: analysis.candidate_audit_summary.native_failure_records,
            engine_failure_records: analysis.candidate_audit_summary.engine_failure_records,
            equivalence_passed: analysis.candidate_audit_summary.equivalence_passed,
        },
        process_cpu: ProcessCpuReport {
            native_records: analysis.process_cpu_summary.native_records,
            solr_records: analysis.process_cpu_summary.solr_records,
            max_disagreement_pct: analysis.process_cpu_summary.max_disagreement_pct,
            reconciliation_passed: analysis.process_cpu_summary.reconciliation_passed,
        },
        warm_cells: analysis
            .gate_report
            .cells()
            .iter()
            .map(CellReport::from)
            .collect(),
        exact_index_cells: analysis
            .exact_index_cells
            .iter()
            .map(CellReport::from)
            .collect(),
        cold: ColdReport {
            session_count: analysis.cold_session_count,
            gated: false,
        },
    };
    let mut bytes = serde_json::to_vec_pretty(&report).map_err(PostRunError::Serialization)?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn calibration_arm(engine: &'static str, check: Option<&CalibrationCheck>) -> CalibrationArmReport {
    CalibrationArmReport {
        engine,
        expected_ratio: check.map_or(1.25, |value| value.expected_ratio),
        observed: check.map(|value| RatioReport::from(value.observed)),
        passed: check.is_some_and(|value| value.passed),
    }
}

impl From<RatioResult> for RatioReport {
    fn from(value: RatioResult) -> Self {
        Self {
            n_blocks: value.n_blocks,
            point_ratio: value.point_ratio,
            ci_low: value.ci_low,
            ci_high: value.ci_high,
        }
    }
}

impl<'a> From<&'a CellStability> for CellReport<'a> {
    fn from(value: &'a CellStability) -> Self {
        Self {
            cell: &value.cell,
            metric: &value.metric,
            kind: match value.kind {
                MetricKind::CpuOrLatency => "CPU_OR_LATENCY",
                MetricKind::Footprint => "FOOTPRINT",
                MetricKind::ExactArtifact => "EXACT_ARTIFACT",
            },
            n: value.n,
            n_blocks: value.n_blocks,
            mean: value.mean,
            ci_low: value.ci_low,
            ci_high: value.ci_high,
            rel_halfwidth: value.rel_halfwidth,
            max_relative_halfwidth: value.max_relative_halfwidth,
            cv: value.cv,
            underpowered: value.underpowered,
            passes: value.passes,
        }
    }
}

fn decision(verdict: GateVerdict) -> DecisionReport {
    match verdict {
        GateVerdict::Keep => DecisionReport {
            verdict: "KEEP",
            passing_dataset: None,
            blocked_dataset: None,
            exit_code: 0,
        },
        GateVerdict::Refine {
            passing_dataset,
            blocked_dataset,
        } => DecisionReport {
            verdict: "REFINE",
            passing_dataset: Some(passing_dataset.as_str()),
            blocked_dataset: Some(blocked_dataset.as_str()),
            exit_code: 1,
        },
        GateVerdict::FixMeasurement => DecisionReport {
            verdict: "FIX MEASUREMENT",
            passing_dataset: None,
            blocked_dataset: None,
            exit_code: 1,
        },
    }
}

pub fn publish_report(path: &Path, bytes: &[u8]) -> Result<(), PostRunError> {
    let mut file = match OpenOptions::new().write(true).create_new(true).open(path) {
        Ok(file) => file,
        Err(source) if source.kind() == std::io::ErrorKind::AlreadyExists => {
            return Err(PostRunError::AnalysisCollision);
        }
        Err(source) => return Err(PostRunError::Publish { source }),
    };
    let result = file.write_all(bytes).and_then(|()| file.sync_all());
    drop(file);
    if let Err(source) = result {
        return match fs::remove_file(path) {
            Ok(()) => Err(PostRunError::Publish { source }),
            Err(cleanup) => Err(PostRunError::PublishResidue { source, cleanup }),
        };
    }
    Ok(())
}
