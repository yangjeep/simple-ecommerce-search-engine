use super::identity::{validate_calibration_records, ValidatedRecord};
use super::AnalysisError;
use crate::{
    campaign_plan, check_calibration, CampaignCalibrations, CampaignCycle, CampaignPhase,
    CampaignSeries, Engine, PairedBlock, RawRecord, SessionMode, ALPHA,
};

const EXPECTED_CALIBRATION_RATIO: f64 = 1.25;

fn paired_block(
    engine: Engine,
    block: usize,
    records: &[ValidatedRecord<'_>],
) -> Result<PairedBlock, AnalysisError> {
    let mut baseline = None;
    let mut treatment = None;
    for record in records.iter().copied().filter(|record| {
        record.spec().engine() == engine && record.spec().block_index().get() == block
    }) {
        match record.spec().plan().mode() {
            SessionMode::CalibrationFour => baseline = Some(record.raw().cpu_usage_usec as f64),
            SessionMode::CalibrationFive => treatment = Some(record.raw().cpu_usage_usec as f64),
            SessionMode::Warm | SessionMode::Cold => {}
        }
    }
    match (baseline, treatment) {
        (Some(baseline), Some(treatment)) => Ok(PairedBlock {
            block,
            baseline,
            treatment,
        }),
        (Some(_), None) | (None, Some(_)) | (None, None) => {
            Err(AnalysisError::InvalidCalibrationPlan { engine, block })
        }
    }
}

pub(super) fn calibrations_from_records(
    cycle: CampaignCycle,
    records: &[ValidatedRecord<'_>],
) -> Result<CampaignCalibrations, AnalysisError> {
    let mut native = Vec::new();
    let mut solr = Vec::new();
    for block in campaign_plan(cycle)
        .blocks()
        .iter()
        .filter(|block| block.series().phase() == CampaignPhase::Calibration)
    {
        let CampaignSeries::Calibration { engine } = block.series() else {
            continue;
        };
        let pair = paired_block(engine, block.index().get(), records)?;
        match engine {
            Engine::Native => native.push(pair),
            Engine::Solr => solr.push(pair),
        }
    }
    Ok(CampaignCalibrations {
        native: check_calibration(&native, EXPECTED_CALIBRATION_RATIO, ALPHA),
        solr: check_calibration(&solr, EXPECTED_CALIBRATION_RATIO, ALPHA),
    })
}

pub fn analyze_calibration(
    cycle: CampaignCycle,
    records: &[RawRecord],
) -> Result<CampaignCalibrations, AnalysisError> {
    let records = validate_calibration_records(cycle, records)?;
    calibrations_from_records(cycle, &records)
}
