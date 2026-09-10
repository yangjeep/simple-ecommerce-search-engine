mod exact_cell_bijection;

use super::{
    evaluate_campaign_gate, CampaignCalibrations, GateVerdict, GlobalGateChecks, ALPHA, MIN_BLOCKS,
};
use crate::{check_calibration, CalibrationCheck, Dataset, PairedBlock};
use exact_cell_bijection::{complete_cells, mark_dataset_underpowered};

fn passed_calibration() -> CalibrationCheck {
    let blocks: Vec<PairedBlock> = (0..MIN_BLOCKS)
        .map(|block| PairedBlock {
            block,
            baseline: 100.0,
            treatment: 125.0,
        })
        .collect();
    check_calibration(&blocks, 1.25, ALPHA).expect("constant known effect should calibrate")
}

fn passed_calibrations() -> CampaignCalibrations {
    CampaignCalibrations {
        native: Some(passed_calibration()),
        solr: Some(passed_calibration()),
    }
}

fn failed_calibration() -> CalibrationCheck {
    CalibrationCheck {
        passed: false,
        ..passed_calibration()
    }
}

const fn passed_global_checks() -> GlobalGateChecks {
    GlobalGateChecks {
        equivalence_passed: true,
        process_reconciliation_passed: true,
    }
}

#[test]
fn one_underpowered_dataset_refines_to_the_passing_dataset() {
    // Given: an exact key bijection with one underpowered WANDS cell.
    let mut cells = complete_cells();
    mark_dataset_underpowered(&mut cells, Dataset::Wands);

    // When: all other gate requirements pass.
    let report = evaluate_campaign_gate(cells, passed_calibrations(), passed_global_checks());

    // Then: only the underpowered dataset is blocked.
    assert_eq!(
        report.verdict(),
        GateVerdict::Refine {
            passing_dataset: Dataset::EsciElectronics,
            blocked_dataset: Dataset::Wands,
        }
    );
}

#[test]
fn missing_native_calibration_forces_fix_measurement() {
    // Given: powered, stable, semantically equivalent measurements.
    let cells = complete_cells();

    // When: no known-effect calibration result is supplied.
    let calibrations = CampaignCalibrations {
        native: None,
        solr: Some(passed_calibration()),
    };
    let report = evaluate_campaign_gate(cells, calibrations, passed_global_checks());

    // Then: repeatability alone cannot pass an uncalibrated instrument.
    assert_eq!(report.verdict(), GateVerdict::FixMeasurement);
    assert!(report.calibrations().native.is_none());
}

#[test]
fn missing_solr_calibration_forces_fix_measurement() {
    // Given: powered, stable, equivalent measurements with native calibration only.
    let calibrations = CampaignCalibrations {
        native: Some(passed_calibration()),
        solr: None,
    };

    // When: the complete campaign gate is evaluated.
    let report = evaluate_campaign_gate(complete_cells(), calibrations, passed_global_checks());

    // Then: the absent Solr arm blocks the campaign.
    assert_eq!(report.verdict(), GateVerdict::FixMeasurement);
}

#[test]
fn failed_native_calibration_forces_fix_measurement() {
    // Given: both calibration arms are present but native failed.
    let calibrations = CampaignCalibrations {
        native: Some(failed_calibration()),
        solr: Some(passed_calibration()),
    };

    // When: the complete campaign gate is evaluated.
    let report = evaluate_campaign_gate(complete_cells(), calibrations, passed_global_checks());

    // Then: the failed native arm blocks the campaign.
    assert_eq!(report.verdict(), GateVerdict::FixMeasurement);
}

#[test]
fn failed_solr_calibration_forces_fix_measurement() {
    // Given: both calibration arms are present but Solr failed.
    let calibrations = CampaignCalibrations {
        native: Some(passed_calibration()),
        solr: Some(failed_calibration()),
    };

    // When: the complete campaign gate is evaluated.
    let report = evaluate_campaign_gate(complete_cells(), calibrations, passed_global_checks());

    // Then: the failed Solr arm blocks the campaign.
    assert_eq!(report.verdict(), GateVerdict::FixMeasurement);
}

#[test]
fn failed_equivalence_still_forces_fix_measurement() {
    // Given: stable, powered, calibrated resource measurements.
    let cells = complete_cells();

    // When: semantic equivalence fails.
    let report = evaluate_campaign_gate(
        cells,
        passed_calibrations(),
        GlobalGateChecks {
            equivalence_passed: false,
            process_reconciliation_passed: true,
        },
    );

    // Then: resource savings cannot excuse incorrect results.
    assert_eq!(report.verdict(), GateVerdict::FixMeasurement);
    assert!(!report.equivalence_passed());
}

#[test]
fn exit_code_is_nonzero_for_fix_measurement() {
    // Given/When: calibration is missing from an otherwise passing report.
    let report = evaluate_campaign_gate(
        complete_cells(),
        CampaignCalibrations {
            native: None,
            solr: None,
        },
        passed_global_checks(),
    );

    // Then: automation receives a blocking exit status.
    assert_eq!(report.exit_code(), 1);
}

#[test]
fn keep_requires_every_preregistered_condition() {
    // Given: stable, powered, equivalent, calibrated measurements.
    let calibrations = passed_calibrations();

    // When: the complete gate is evaluated.
    let report = evaluate_campaign_gate(complete_cells(), calibrations, passed_global_checks());

    // Then: and only then may the campaign proceed.
    assert_eq!(report.verdict(), GateVerdict::Keep);
    assert_eq!(report.cells().len(), 48);
    assert!(report.calibrations().native.is_some());
    assert!(report.calibrations().solr.is_some());
    assert!(report.equivalence_passed());
    assert!(report.process_reconciliation_passed());
    assert_eq!(report.exit_code(), 0);
    assert_eq!(report.failing_cells().count(), 0);
}
