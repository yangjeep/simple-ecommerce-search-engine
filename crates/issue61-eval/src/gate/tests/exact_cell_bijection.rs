use super::super::{
    evaluate_campaign_gate, evaluate_cell, DatasetCellStability, GateVerdict, MetricKind,
    WarmCellKey, MIN_BLOCKS,
};
use super::{passed_calibrations, passed_global_checks};
use crate::{Dataset, Engine, MetricIdentity, WorkloadProjection};

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

pub(super) fn complete_cells() -> Vec<DatasetCellStability> {
    let mut cells = Vec::with_capacity(48);
    for engine in ENGINES {
        for dataset in DATASETS {
            for projection in PROJECTIONS {
                for metric in METRICS {
                    let (metric_name, kind) = match metric {
                        MetricIdentity::CpuUsPerQuery => {
                            ("cpu_us_per_query", MetricKind::CpuOrLatency)
                        }
                        MetricIdentity::LatencyP50Us => {
                            ("latency_p50_us", MetricKind::CpuOrLatency)
                        }
                        MetricIdentity::MemoryCurrentMedianBytes => {
                            ("cgroup_memory_current_median_bytes", MetricKind::Footprint)
                        }
                    };
                    let cell_name = format!(
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
                        cell: evaluate_cell(&cell_name, metric_name, kind, &[100.0; MIN_BLOCKS]),
                    });
                }
            }
        }
    }
    cells
}

pub(super) fn mark_dataset_underpowered(cells: &mut [DatasetCellStability], dataset: Dataset) {
    let cell = cells
        .iter_mut()
        .find(|cell| cell.key.dataset == dataset)
        .expect("complete fixture must contain each dataset");
    cell.cell = evaluate_cell(
        "underpowered",
        "cpu_us_per_query",
        MetricKind::CpuOrLatency,
        &[100.0; MIN_BLOCKS - 1],
    );
}

#[test]
fn exact_warm_key_bijection_keeps_when_every_expected_identity_appears_once() {
    // Given: every engine, dataset, projection, and metric identity exactly once.
    let cells = complete_cells();
    assert_eq!(cells.len(), 48);
    for dataset in DATASETS {
        assert_eq!(
            cells
                .iter()
                .filter(|cell| cell.key.dataset == dataset)
                .count(),
            24
        );
    }
    for (index, cell) in cells.iter().enumerate() {
        assert!(!cells[..index].iter().any(|prior| prior.key == cell.key));
    }

    // When: the campaign gate evaluates the exact 48-key fixture.
    let report = evaluate_campaign_gate(cells, passed_calibrations(), passed_global_checks());

    // Then: the complete bijection is eligible to keep.
    assert_eq!(report.verdict(), GateVerdict::Keep);
    assert_eq!(report.cells().len(), 48);
}

#[test]
fn wands_failure_refines_to_esci_and_returns_nonzero() {
    // Given: an exact key bijection with one underpowered WANDS cell.
    let mut cells = complete_cells();
    mark_dataset_underpowered(&mut cells, Dataset::Wands);

    // When: the campaign gate evaluates both datasets.
    let report = evaluate_campaign_gate(cells, passed_calibrations(), passed_global_checks());

    // Then: ESCI passes, WANDS is blocked, and automation must stop.
    assert_eq!(
        report.verdict(),
        GateVerdict::Refine {
            passing_dataset: Dataset::EsciElectronics,
            blocked_dataset: Dataset::Wands,
        }
    );
    assert_eq!(report.exit_code(), 1);
}

#[test]
fn esci_failure_refines_to_wands_and_returns_nonzero() {
    // Given: an exact key bijection with one underpowered ESCI cell.
    let mut cells = complete_cells();
    mark_dataset_underpowered(&mut cells, Dataset::EsciElectronics);

    // When: the campaign gate evaluates both datasets.
    let report = evaluate_campaign_gate(cells, passed_calibrations(), passed_global_checks());

    // Then: WANDS passes, ESCI is blocked, and automation must stop.
    assert_eq!(
        report.verdict(),
        GateVerdict::Refine {
            passing_dataset: Dataset::Wands,
            blocked_dataset: Dataset::EsciElectronics,
        }
    );
    assert_eq!(report.exit_code(), 1);
}

#[test]
fn both_dataset_failures_force_fix_measurement() {
    // Given: exact keys with one underpowered cell in each dataset.
    let mut cells = complete_cells();
    mark_dataset_underpowered(&mut cells, Dataset::Wands);
    mark_dataset_underpowered(&mut cells, Dataset::EsciElectronics);

    // When: the campaign gate evaluates both failures.
    let report = evaluate_campaign_gate(cells, passed_calibrations(), passed_global_checks());

    // Then: no single passing dataset remains eligible for refinement.
    assert_eq!(report.verdict(), GateVerdict::FixMeasurement);
}

#[test]
fn duplicate_passing_key_forces_fix_measurement_when_expected_key_is_missing() {
    // Given: one expected key replaced by a duplicate passing key.
    let mut cells = complete_cells();
    cells[1] = cells[0].clone();

    // When: the campaign still contains 48 individually stable observations.
    let report = evaluate_campaign_gate(cells, passed_calibrations(), passed_global_checks());

    // Then: count alone cannot satisfy the exact-cell bijection.
    assert_eq!(report.verdict(), GateVerdict::FixMeasurement);
}
