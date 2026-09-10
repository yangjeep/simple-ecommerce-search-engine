mod candidate_audit_coverage;
mod process_reconciliation_coverage;
mod revision8;

use crate::analysis::test_support::{
    record_for, set_native_process_total, CompleteCandidateAuditFixture,
};
use crate::{
    analyze_campaign, campaign_plan, AnalysisError, AuditVerdict, CampaignCycle, CampaignEvidence,
    Dataset, Engine, ExactIndexObservation, GateVerdict, MetricKind, RawRecord, SessionMode,
    EXACT_INDEX_CELLS, STABILITY_CELLS,
};

fn campaign_records() -> Vec<RawRecord> {
    campaign_plan(CampaignCycle::Run1)
        .sessions()
        .map(record_for)
        .collect()
}

fn exact_observations() -> Vec<ExactIndexObservation> {
    [Engine::Native, Engine::Solr]
        .into_iter()
        .flat_map(|engine| {
            [Dataset::Wands, Dataset::EsciElectronics]
                .into_iter()
                .map(move |dataset| ExactIndexObservation {
                    cycle: CampaignCycle::Run1,
                    engine,
                    dataset,
                    bytes: 1_000,
                })
        })
        .collect()
}

fn analyze(
    records: &[RawRecord],
    exact_index: &[ExactIndexObservation],
) -> Result<crate::CampaignAnalysis, AnalysisError> {
    let candidate_audits = CompleteCandidateAuditFixture::complete();
    analyze_with_candidate_audits(records, exact_index, &candidate_audits)
}

fn analyze_with_candidate_audits(
    records: &[RawRecord],
    exact_index: &[ExactIndexObservation],
    candidate_audits: &CompleteCandidateAuditFixture,
) -> Result<crate::CampaignAnalysis, AnalysisError> {
    let candidate_audits = candidate_audits.evidence();
    analyze_campaign(CampaignEvidence {
        cycle: CampaignCycle::Run1,
        records,
        exact_index,
        candidate_audits: &candidate_audits,
    })
}

#[test]
fn complete_campaign_produces_only_preregistered_warm_gate_cells() {
    // Given
    let mut records = campaign_records();
    records.reverse();
    let exact_index = exact_observations();

    // When
    let analysis = analyze(&records, &exact_index)
        .unwrap_or_else(|error| panic!("complete campaign must validate: {error}"));

    // Then
    assert_eq!(records.len(), 620);
    assert_eq!(analysis.cycle, CampaignCycle::Run1);
    assert_eq!(analysis.gate_report.cells().len(), STABILITY_CELLS);
    assert!(analysis
        .gate_report
        .cells()
        .iter()
        .all(|cell| cell.n_blocks == 30));
    assert_eq!(analysis.exact_index_cells.len(), EXACT_INDEX_CELLS);
    assert!(analysis.exact_index_cells.iter().all(|cell| {
        cell.kind == MetricKind::ExactArtifact && cell.n == 1 && !cell.underpowered && cell.passes
    }));
    assert_eq!(analysis.cold_session_count, 20);
    assert!(analysis
        .gate_report
        .cells()
        .iter()
        .all(|cell| !cell.cell.contains("cold")));
    for calibration in [
        analysis.gate_report.calibrations().native,
        analysis.gate_report.calibrations().solr,
    ] {
        let calibration = calibration.expect("both calibration arms must be present");
        assert_eq!(calibration.observed.n_blocks, 30);
        assert!(calibration.passed);
    }
    assert_eq!(analysis.gate_report.verdict(), GateVerdict::Keep);
}

#[test]
fn footprint_and_exact_index_use_only_their_separate_frozen_sources() {
    // Given
    let mut records = campaign_records();
    for record in &mut records {
        record.cgroup_memory_current_median_bytes = 321;
        record.cgroup_memory_footprint_bytes = 654;
        record.index_serialized_bytes = 987;
    }
    let exact_index = exact_observations();

    // When
    let analysis = analyze(&records, &exact_index)
        .unwrap_or_else(|error| panic!("valid evidence must analyze: {error}"));

    // Then
    let footprint_cells: Vec<_> = analysis
        .gate_report
        .cells()
        .iter()
        .filter(|cell| cell.kind == MetricKind::Footprint)
        .collect();
    assert_eq!(footprint_cells.len(), 16);
    assert!(footprint_cells.iter().all(|cell| cell.mean == 321.0));
    assert!(analysis
        .exact_index_cells
        .iter()
        .all(|cell| cell.mean == 1_000.0));
}

#[test]
fn failed_equivalence_or_calibration_forces_fix_measurement() {
    // Given
    let records = campaign_records();
    let exact_index = exact_observations();
    let mut failed_audits = CompleteCandidateAuditFixture::complete();
    failed_audits.wands_records[0].verdict = AuditVerdict::Mismatch;
    failed_audits.wands_records[0].engine_count = Some(2);

    // When / Then: failed equivalence
    let failed_equivalence = analyze_with_candidate_audits(&records, &exact_index, &failed_audits)
        .unwrap_or_else(|error| panic!("evidence must analyze: {error}"));
    assert_eq!(
        failed_equivalence.gate_report.verdict(),
        GateVerdict::FixMeasurement
    );

    // Given: one calibration arm cannot recover the known effect.
    let mut failed_records = records;
    for record in &mut failed_records {
        if record.engine == Engine::Native.as_str()
            && record.regime == SessionMode::CalibrationFive.as_str()
        {
            record.cpu_usage_usec = 400;
            set_native_process_total(record, 400);
        }
    }

    // When / Then: failed calibration
    let failed_calibration = analyze(&failed_records, &exact_index)
        .unwrap_or_else(|error| panic!("complete evidence must analyze: {error}"));
    assert_eq!(
        failed_calibration.gate_report.verdict(),
        GateVerdict::FixMeasurement
    );
    assert!(
        !failed_calibration
            .gate_report
            .calibrations()
            .native
            .expect("native calibration must exist")
            .passed
    );
}

#[test]
fn full_campaign_identity_mismatches_fail_closed() {
    // Given / When / Then: missing
    let mut missing = campaign_records();
    missing.pop();
    assert!(matches!(
        analyze(&missing, &exact_observations()),
        Err(AnalysisError::MissingIdentity { .. })
    ));

    // Given / When / Then: duplicate
    let mut duplicate = campaign_records();
    duplicate.push(duplicate[0].clone());
    assert!(matches!(
        analyze(&duplicate, &exact_observations()),
        Err(AnalysisError::DuplicateIdentity { .. })
    ));

    // Given / When / Then: unexpected
    let mut unexpected = campaign_records();
    unexpected[0].rep = usize::MAX;
    assert!(matches!(
        analyze(&unexpected, &exact_observations()),
        Err(AnalysisError::UnexpectedIdentity { .. })
    ));
}

#[test]
fn exact_index_identity_mismatches_fail_closed() {
    // Given / When / Then: missing
    let mut missing = exact_observations();
    missing.pop();
    assert!(matches!(
        analyze(&campaign_records(), &missing),
        Err(AnalysisError::MissingExactIndex { .. })
    ));

    // Given / When / Then: duplicate
    let mut duplicate = exact_observations();
    duplicate.push(duplicate[0]);
    assert!(matches!(
        analyze(&campaign_records(), &duplicate),
        Err(AnalysisError::DuplicateExactIndex { .. })
    ));

    // Given / When / Then: unexpected cycle
    let mut unexpected = exact_observations();
    unexpected[0].cycle = CampaignCycle::Rerun1;
    assert!(matches!(
        analyze(&campaign_records(), &unexpected),
        Err(AnalysisError::UnexpectedExactIndex { .. })
    ));
}

#[test]
fn invalid_warm_metrics_and_exact_bytes_are_rejected_not_skipped() {
    // Given / When / Then: missing CPU denominator
    let mut missing_cpu = campaign_records();
    let warm = missing_cpu
        .iter_mut()
        .find(|record| !record.calibration && record.regime == SessionMode::Warm.as_str())
        .expect("campaign has warm records");
    warm.queries = 0;
    assert!(matches!(
        analyze(&missing_cpu, &exact_observations()),
        Err(AnalysisError::MissingMetric { .. })
    ));

    // Given / When / Then: non-finite latency
    let mut invalid_latency = campaign_records();
    invalid_latency
        .iter_mut()
        .find(|record| record.regime == SessionMode::Warm.as_str())
        .expect("campaign has warm records")
        .latency_p50_us = f64::NAN;
    assert!(matches!(
        analyze(&invalid_latency, &exact_observations()),
        Err(AnalysisError::InvalidMetric { .. })
    ));

    // Given / When / Then: zero exact bytes
    let mut invalid_exact = exact_observations();
    invalid_exact[0].bytes = 0;
    assert!(matches!(
        analyze(&campaign_records(), &invalid_exact),
        Err(AnalysisError::InvalidExactIndexBytes { .. })
    ));

    // Given / When / Then: first integer not exactly representable as f64
    let mut inexact = exact_observations();
    inexact[0].bytes = super::MAX_EXACT_INTEGER_BYTES + 1;
    assert!(matches!(
        analyze(&campaign_records(), &inexact),
        Err(AnalysisError::InvalidExactIndexBytes { .. })
    ));
}

#[test]
fn cold_records_are_validated_but_never_evaluated_as_metrics() {
    // Given
    let mut records = campaign_records();
    let cold = records
        .iter_mut()
        .find(|record| record.regime == SessionMode::Cold.as_str())
        .expect("campaign has cold records");
    cold.queries = 0;
    cold.latency_p50_us = f64::NAN;
    cold.cgroup_memory_current_median_bytes = 0;

    // When
    let analysis = analyze(&records, &exact_observations())
        .unwrap_or_else(|error| panic!("cold metrics are descriptive only: {error}"));

    // Then
    assert_eq!(analysis.cold_session_count, 20);
    assert_eq!(analysis.gate_report.cells().len(), 48);
}
