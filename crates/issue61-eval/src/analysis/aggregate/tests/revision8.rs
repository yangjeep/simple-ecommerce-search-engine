use super::{analyze_with_candidate_audits, campaign_records, exact_observations};
use crate::analysis::test_support::{set_native_process_total, CompleteCandidateAuditFixture};
use crate::{
    campaign_plan, AdmissionClass, AuditVerdict, CampaignCycle, CampaignPhase, Dataset, Engine,
    GateVerdict, RawRecord, SessionMode, WorkloadProjection,
};
use std::collections::BTreeSet;

fn block_wands_warm_cell(records: &mut [RawRecord]) {
    records
        .iter_mut()
        .find(|record| {
            record.engine == Engine::Native.as_str()
                && record.dataset == Dataset::Wands.as_str()
                && record.regime == SessionMode::Warm.as_str()
                && record.query_class == WorkloadProjection::All.as_str()
        })
        .expect("campaign fixture must contain a native WANDS all-query warm record")
        .latency_p50_us = 2_000.0;
}

fn native_cold_record(records: &mut [RawRecord]) -> &mut RawRecord {
    records
        .iter_mut()
        .find(|record| {
            record.engine == Engine::Native.as_str() && record.regime == SessionMode::Cold.as_str()
        })
        .expect("campaign fixture must contain a native cold record")
}

#[test]
fn complete_typed_candidate_audits_replace_boolean_equivalence() {
    // Given
    let records = campaign_records();
    let exact_index = exact_observations();
    let candidate_audits = CompleteCandidateAuditFixture::complete();
    let query_ids: BTreeSet<_> = candidate_audits
        .wands_workload
        .iter()
        .chain(&candidate_audits.esci_workload)
        .map(|query| query.query_id.as_str())
        .collect();

    // When
    let analysis = analyze_with_candidate_audits(&records, &exact_index, &candidate_audits)
        .unwrap_or_else(|error| panic!("complete candidate audits must analyze: {error}"));

    // Then
    assert_eq!(candidate_audits.wands_workload.len(), 480);
    assert_eq!(candidate_audits.wands_records.len(), 480);
    assert_eq!(candidate_audits.esci_workload.len(), 600);
    assert_eq!(candidate_audits.esci_records.len(), 600);
    assert_eq!(query_ids.len(), 1_080);
    assert_eq!(analysis.candidate_audit_summary.total_records, 1_080);
    assert_eq!(analysis.candidate_audit_summary.matched_records, 1_080);
    assert!(analysis.candidate_audit_summary.equivalence_passed);
}

#[test]
fn missing_candidate_audit_record_is_an_analysis_error() {
    // Given
    let records = campaign_records();
    let exact_index = exact_observations();
    let mut candidate_audits = CompleteCandidateAuditFixture::complete();
    candidate_audits.wands_records.pop();

    // When
    let result = analyze_with_candidate_audits(&records, &exact_index, &candidate_audits);

    // Then
    assert!(result.is_err());
}

#[test]
fn structurally_malformed_match_is_an_analysis_error() {
    // Given
    let records = campaign_records();
    let exact_index = exact_observations();
    let mut candidate_audits = CompleteCandidateAuditFixture::complete();
    candidate_audits.wands_records[0].only_native = vec!["unexpected-id".to_owned()];

    // When
    let result = analyze_with_candidate_audits(&records, &exact_index, &candidate_audits);

    // Then
    assert!(result.is_err());
}

#[test]
fn candidate_admission_class_must_match_the_frozen_workload() {
    // Given
    let records = campaign_records();
    let exact_index = exact_observations();
    let mut candidate_audits = CompleteCandidateAuditFixture::complete();
    candidate_audits.wands_records[0].admission_class = AdmissionClass::Hybrid;

    // When
    let result = analyze_with_candidate_audits(&records, &exact_index, &candidate_audits);

    // Then
    assert!(result.is_err());
}

#[test]
fn exactly_two_percent_process_disagreement_passes() {
    // Given
    let mut records = campaign_records();
    set_native_process_total(native_cold_record(&mut records), 2_040);
    let exact_index = exact_observations();
    let candidate_audits = CompleteCandidateAuditFixture::complete();

    // When
    let analysis = analyze_with_candidate_audits(&records, &exact_index, &candidate_audits)
        .unwrap_or_else(|error| panic!("two percent is valid process evidence: {error}"));

    // Then
    assert_eq!(analysis.process_cpu_summary.native_records, 310);
    assert_eq!(analysis.process_cpu_summary.max_disagreement_pct, 2.0);
    assert!(analysis.process_cpu_summary.reconciliation_passed);
    assert_eq!(analysis.gate_report.verdict(), GateVerdict::Keep);
}

#[test]
fn process_disagreement_above_two_percent_overrides_refine() {
    // Given
    let mut records = campaign_records();
    block_wands_warm_cell(&mut records);
    set_native_process_total(native_cold_record(&mut records), 2_041);
    let exact_index = exact_observations();
    let candidate_audits = CompleteCandidateAuditFixture::complete();

    // When
    let analysis = analyze_with_candidate_audits(&records, &exact_index, &candidate_audits)
        .unwrap_or_else(|error| panic!("valid negative process evidence must analyze: {error}"));

    // Then
    assert!(analysis.process_cpu_summary.max_disagreement_pct > 2.0);
    assert!(!analysis.process_cpu_summary.reconciliation_passed);
    assert_eq!(analysis.gate_report.verdict(), GateVerdict::FixMeasurement);
}

#[test]
fn one_passing_dataset_yields_typed_refine() {
    // Given
    let mut records = campaign_records();
    block_wands_warm_cell(&mut records);
    let exact_index = exact_observations();
    let candidate_audits = CompleteCandidateAuditFixture::complete();

    // When
    let analysis = analyze_with_candidate_audits(&records, &exact_index, &candidate_audits)
        .unwrap_or_else(|error| panic!("dataset-scoped warm evidence must analyze: {error}"));

    // Then
    for dataset in [Dataset::Wands, Dataset::EsciElectronics] {
        assert_eq!(
            analysis
                .gate_report
                .cells()
                .iter()
                .filter(|cell| cell.cell.contains(dataset.as_str()))
                .count(),
            24
        );
    }
    assert_eq!(
        analysis.gate_report.verdict(),
        GateVerdict::Refine {
            passing_dataset: Dataset::EsciElectronics,
            blocked_dataset: Dataset::Wands,
        }
    );
}

#[test]
fn failed_equivalence_overrides_refine() {
    // Given
    let mut records = campaign_records();
    block_wands_warm_cell(&mut records);
    let exact_index = exact_observations();
    let mut candidate_audits = CompleteCandidateAuditFixture::complete();
    candidate_audits.wands_records[0].verdict = AuditVerdict::Mismatch;
    candidate_audits.wands_records[0].engine_count = Some(2);

    // When
    let analysis = analyze_with_candidate_audits(&records, &exact_index, &candidate_audits)
        .unwrap_or_else(|error| panic!("valid mismatch evidence must analyze: {error}"));

    // Then
    assert!(!analysis.candidate_audit_summary.equivalence_passed);
    assert_eq!(analysis.gate_report.verdict(), GateVerdict::FixMeasurement);
}

#[test]
fn failed_calibration_overrides_refine() {
    // Given
    let mut records = campaign_records();
    block_wands_warm_cell(&mut records);
    for record in &mut records {
        if record.engine == Engine::Native.as_str()
            && record.regime == SessionMode::CalibrationFive.as_str()
        {
            record.cpu_usage_usec = 400;
            set_native_process_total(record, 400);
        }
    }
    let exact_index = exact_observations();
    let candidate_audits = CompleteCandidateAuditFixture::complete();

    // When
    let analysis = analyze_with_candidate_audits(&records, &exact_index, &candidate_audits)
        .unwrap_or_else(|error| panic!("complete failed calibration must analyze: {error}"));

    // Then
    assert_eq!(analysis.gate_report.verdict(), GateVerdict::FixMeasurement);
}

#[test]
fn fixture_covers_the_full_frozen_campaign() {
    // Given / When
    let records = campaign_records();

    // Then
    assert_eq!(records.len(), 620);
    assert_eq!(
        records
            .iter()
            .filter(|record| record.regime == SessionMode::Cold.as_str())
            .count(),
        20
    );
    assert_eq!(
        campaign_plan(CampaignCycle::Run1)
            .sessions()
            .filter(|spec| spec.series().phase() == CampaignPhase::Warm)
            .count(),
        480
    );
}
