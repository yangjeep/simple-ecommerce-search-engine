use crate::ratio::CalibrationCheck;
use bench_harness::{t_ci_mean, Distribution};

pub const MIN_BLOCKS: usize = 30;
pub const MAX_CPU_LATENCY_REL_HALFWIDTH: f64 = 0.075;
pub const MAX_FOOTPRINT_REL_HALFWIDTH: f64 = 0.02;
pub const MAX_CV: f64 = 0.10;
pub const ALPHA: f64 = 0.05;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetricKind {
    CpuOrLatency,
    Footprint,
    ExactArtifact,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CellStability {
    pub cell: String,
    pub metric: String,
    pub kind: MetricKind,
    pub n: usize,
    pub n_blocks: usize,
    pub mean: f64,
    pub ci_low: f64,
    pub ci_high: f64,
    pub rel_halfwidth: f64,
    pub max_relative_halfwidth: f64,
    pub cv: f64,
    pub underpowered: bool,
    pub passes: bool,
}

pub fn evaluate_cell(cell: &str, metric: &str, kind: MetricKind, samples: &[f64]) -> CellStability {
    let max_relative_halfwidth = match kind {
        MetricKind::CpuOrLatency => MAX_CPU_LATENCY_REL_HALFWIDTH,
        MetricKind::Footprint => MAX_FOOTPRINT_REL_HALFWIDTH,
        MetricKind::ExactArtifact => return evaluate_exact_artifact(cell, metric, samples, 0.0),
    };
    let interval = t_ci_mean(samples, ALPHA);
    let distribution = if samples.is_empty() {
        None
    } else {
        Some(Distribution::compute(samples))
    };
    let (rel_halfwidth, cv) = match distribution {
        Some(distribution) if distribution.mean > 0.0 => (
            (interval.ci_high - interval.ci_low) / (2.0 * distribution.mean),
            distribution.stddev / distribution.mean,
        ),
        Some(_) | None => (f64::INFINITY, f64::INFINITY),
    };
    CellStability {
        cell: cell.to_owned(),
        metric: metric.to_owned(),
        kind,
        n: interval.n,
        n_blocks: interval.n,
        mean: interval.mean,
        ci_low: interval.ci_low,
        ci_high: interval.ci_high,
        rel_halfwidth,
        max_relative_halfwidth,
        cv,
        underpowered: interval.n < MIN_BLOCKS,
        passes: interval.mean.is_finite()
            && rel_halfwidth <= max_relative_halfwidth
            && cv <= MAX_CV,
    }
}

/// Checks deterministic artifact observations directly; no resampling or
/// inferential interval is meaningful for exact index-byte measurements.
pub fn evaluate_exact_artifact(
    cell: &str,
    metric: &str,
    samples: &[f64],
    tolerance: f64,
) -> CellStability {
    let distribution = if samples.is_empty() {
        None
    } else {
        Some(Distribution::compute(samples))
    };
    let (n, mean, cv, passes) = match distribution {
        Some(distribution) => {
            let cv = if distribution.mean == 0.0 {
                f64::INFINITY
            } else {
                distribution.stddev / distribution.mean.abs()
            };
            let passes = tolerance.is_finite()
                && tolerance >= 0.0
                && samples.iter().copied().all(f64::is_finite)
                && distribution.max - distribution.min <= tolerance;
            (distribution.n, distribution.mean, cv, passes)
        }
        None => (0, f64::NAN, f64::INFINITY, false),
    };
    CellStability {
        cell: cell.to_owned(),
        metric: metric.to_owned(),
        kind: MetricKind::ExactArtifact,
        n,
        n_blocks: n,
        mean,
        ci_low: mean,
        ci_high: mean,
        rel_halfwidth: 0.0,
        max_relative_halfwidth: 0.0,
        cv,
        underpowered: false,
        passes,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateVerdict {
    Keep,
    FixMeasurement,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CampaignCalibrations {
    pub native: Option<CalibrationCheck>,
    pub solr: Option<CalibrationCheck>,
}

impl CampaignCalibrations {
    const fn passed(&self) -> bool {
        match (&self.native, &self.solr) {
            (Some(native), Some(solr)) => native.passed && solr.passed,
            (Some(_), None) => false,
            (None, Some(_)) => false,
            (None, None) => false,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct GateReport {
    pub verdict: GateVerdict,
    pub cells: Vec<CellStability>,
    pub calibrations: CampaignCalibrations,
    pub equivalence_passed: bool,
}

impl GateReport {
    pub fn failing_cells(&self) -> impl Iterator<Item = &CellStability> {
        self.cells
            .iter()
            .filter(|cell| !cell.passes || cell.underpowered)
    }

    pub const fn exit_code(&self) -> i32 {
        match self.verdict {
            GateVerdict::Keep => 0,
            GateVerdict::FixMeasurement => 1,
        }
    }
}

pub fn evaluate(
    cells: Vec<CellStability>,
    calibrations: CampaignCalibrations,
    equivalence_passed: bool,
) -> GateReport {
    let all_cells_passed = cells.iter().all(|cell| cell.passes && !cell.underpowered);
    let calibration_passed = calibrations.passed();
    let verdict = if equivalence_passed && all_cells_passed && calibration_passed {
        GateVerdict::Keep
    } else {
        GateVerdict::FixMeasurement
    };
    GateReport {
        verdict,
        cells,
        calibrations,
        equivalence_passed,
    }
}
