use crate::ratio::CalibrationCheck;
use crate::{Dataset, Engine, MetricIdentity, WorkloadProjection, STABILITY_CELLS};
use bench_harness::{t_ci_mean, Distribution};

mod warm_cell;
pub(crate) use warm_cell::{DatasetCellStability, WarmCellKey};

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
    Refine {
        passing_dataset: Dataset,
        blocked_dataset: Dataset,
    },
    FixMeasurement,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct GlobalGateChecks {
    pub(crate) equivalence_passed: bool,
    pub(crate) process_reconciliation_passed: bool,
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
            (Some(_), None) | (None, Some(_)) | (None, None) => false,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct GateReport {
    verdict: GateVerdict,
    cells: Vec<CellStability>,
    calibrations: CampaignCalibrations,
    equivalence_passed: bool,
    process_reconciliation_passed: bool,
}

impl GateReport {
    pub const fn verdict(&self) -> GateVerdict {
        self.verdict
    }

    pub fn cells(&self) -> &[CellStability] {
        &self.cells
    }

    pub const fn calibrations(&self) -> &CampaignCalibrations {
        &self.calibrations
    }

    pub const fn equivalence_passed(&self) -> bool {
        self.equivalence_passed
    }

    pub const fn process_reconciliation_passed(&self) -> bool {
        self.process_reconciliation_passed
    }

    pub fn failing_cells(&self) -> impl Iterator<Item = &CellStability> {
        self.cells
            .iter()
            .filter(|cell| !cell.passes || cell.underpowered)
    }

    pub const fn exit_code(&self) -> i32 {
        match self.verdict {
            GateVerdict::Keep => 0,
            GateVerdict::Refine { .. } | GateVerdict::FixMeasurement => 1,
        }
    }
}

pub(crate) fn evaluate_campaign_gate(
    dataset_cells: Vec<DatasetCellStability>,
    calibrations: CampaignCalibrations,
    global: GlobalGateChecks,
) -> GateReport {
    let calibration_passed = calibrations.passed();
    let wands_passed = dataset_passed(&dataset_cells, Dataset::Wands);
    let esci_passed = dataset_passed(&dataset_cells, Dataset::EsciElectronics);
    let identities_passed = dataset_cells.len() == STABILITY_CELLS
        && dataset_cells.iter().enumerate().all(|(index, cell)| {
            dataset_cells[..index]
                .iter()
                .all(|prior| prior.key != cell.key)
        });
    let global_passed = calibration_passed
        && identities_passed
        && global.equivalence_passed
        && global.process_reconciliation_passed;
    let verdict = match (global_passed, wands_passed, esci_passed) {
        (false, _, _) | (true, false, false) => GateVerdict::FixMeasurement,
        (true, true, true) => GateVerdict::Keep,
        (true, true, false) => GateVerdict::Refine {
            passing_dataset: Dataset::Wands,
            blocked_dataset: Dataset::EsciElectronics,
        },
        (true, false, true) => GateVerdict::Refine {
            passing_dataset: Dataset::EsciElectronics,
            blocked_dataset: Dataset::Wands,
        },
    };
    let cells = dataset_cells.into_iter().map(|cell| cell.cell).collect();
    GateReport {
        verdict,
        cells,
        calibrations,
        equivalence_passed: global.equivalence_passed,
        process_reconciliation_passed: global.process_reconciliation_passed,
    }
}

fn dataset_passed(cells: &[DatasetCellStability], dataset: Dataset) -> bool {
    for engine in [Engine::Native, Engine::Solr] {
        for projection in [
            WorkloadProjection::All,
            WorkloadProjection::FastPath,
            WorkloadProjection::Hybrid,
            WorkloadProjection::Punt,
        ] {
            for metric in [
                MetricIdentity::CpuUsPerQuery,
                MetricIdentity::LatencyP50Us,
                MetricIdentity::MemoryCurrentMedianBytes,
            ] {
                let key = WarmCellKey {
                    engine,
                    dataset,
                    projection,
                    metric,
                };
                let mut matches = cells.iter().filter(|cell| cell.key == key);
                let Some(cell) = matches.next() else {
                    return false;
                };
                if matches.next().is_some() || !cell.cell.passes || cell.cell.underpowered {
                    return false;
                }
            }
        }
    }
    true
}

#[cfg(test)]
mod tests;
