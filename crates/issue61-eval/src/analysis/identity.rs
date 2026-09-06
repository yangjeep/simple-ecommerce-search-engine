use crate::{
    campaign_plan, check_calibration, CampaignCalibrations, CampaignCycle, CampaignPhase,
    CampaignSeries, Dataset, Engine, EngineOrder, PairedBlock, RawRecord, SessionMode, SessionSpec,
    WorkloadProjection, ALPHA, RAW_SCHEMA_VERSION,
};
use std::error::Error;
use std::fmt;
use std::str::FromStr;

const EXPERIMENT_ID: &str = "I61-E1";
const RUN_ID: &str = "seed-61";
const EXPECTED_CALIBRATION_RATIO: f64 = 1.25;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdentityField {
    Engine,
    Dataset,
    QueryClass,
    Regime,
    EngineOrder,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RawSessionKey {
    pub calibration: bool,
    pub engine: Engine,
    pub dataset: Dataset,
    pub query_class: WorkloadProjection,
    pub regime: SessionMode,
    pub rep: usize,
    pub engine_order: EngineOrder,
}

impl RawSessionKey {
    fn from_record(record: &RawRecord, index: usize) -> Result<Self, AnalysisError> {
        let engine =
            Engine::from_str(&record.engine).map_err(|_| AnalysisError::MalformedIdentity {
                record: index,
                field: IdentityField::Engine,
            })?;
        let dataset =
            Dataset::parse(&record.dataset).map_err(|_| AnalysisError::MalformedIdentity {
                record: index,
                field: IdentityField::Dataset,
            })?;
        let query_class = WorkloadProjection::from_str(&record.query_class).map_err(|_| {
            AnalysisError::MalformedIdentity {
                record: index,
                field: IdentityField::QueryClass,
            }
        })?;
        let regime = SessionMode::from_str(&record.regime).map_err(|_| {
            AnalysisError::MalformedIdentity {
                record: index,
                field: IdentityField::Regime,
            }
        })?;
        let engine_order = match record.engine_order {
            0 => EngineOrder::First,
            1 => EngineOrder::Second,
            _ => {
                return Err(AnalysisError::MalformedIdentity {
                    record: index,
                    field: IdentityField::EngineOrder,
                });
            }
        };
        Ok(Self {
            calibration: record.calibration,
            engine,
            dataset,
            query_class,
            regime,
            rep: record.rep,
            engine_order,
        })
    }

    const fn from_spec(spec: SessionSpec) -> Self {
        Self {
            calibration: spec.plan().mode().is_calibration(),
            engine: spec.engine(),
            dataset: spec.series().dataset(),
            query_class: spec.series().projection(),
            regime: spec.plan().mode(),
            rep: spec.block_index().get(),
            engine_order: spec.slot(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnalysisError {
    UnsupportedSchemaVersion { record: usize, found: u32 },
    WrongExperimentId { record: usize },
    WrongRunId { record: usize },
    ExcludedRecord { record: usize },
    UnexpectedExclusionReason { record: usize },
    MalformedIdentity { record: usize, field: IdentityField },
    UnexpectedIdentity { record: usize, key: RawSessionKey },
    DuplicateIdentity { record: usize, key: RawSessionKey },
    MissingIdentity { key: RawSessionKey },
    InvalidCalibrationPlan { engine: Engine, block: usize },
}

impl fmt::Display for AnalysisError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedSchemaVersion { record, found } => write!(
                formatter,
                "record {record} has schema version {found}; expected {RAW_SCHEMA_VERSION}"
            ),
            Self::WrongExperimentId { record } => {
                write!(
                    formatter,
                    "record {record} does not belong to {EXPERIMENT_ID}"
                )
            }
            Self::WrongRunId { record } => {
                write!(formatter, "record {record} does not belong to {RUN_ID}")
            }
            Self::ExcludedRecord { record } => write!(formatter, "record {record} is excluded"),
            Self::UnexpectedExclusionReason { record } => {
                write!(formatter, "record {record} has an exclusion reason")
            }
            Self::MalformedIdentity { record, field } => {
                write!(formatter, "record {record} has malformed {field:?}")
            }
            Self::UnexpectedIdentity { record, key } => {
                write!(formatter, "record {record} has unexpected identity {key:?}")
            }
            Self::DuplicateIdentity { record, key } => {
                write!(formatter, "record {record} duplicates identity {key:?}")
            }
            Self::MissingIdentity { key } => write!(formatter, "missing identity {key:?}"),
            Self::InvalidCalibrationPlan { engine, block } => write!(
                formatter,
                "calibration plan has no four/five pair for {engine:?} block {block}"
            ),
        }
    }
}

impl Error for AnalysisError {}

#[derive(Clone, Copy)]
struct ExpectedSession {
    key: RawSessionKey,
    spec: SessionSpec,
}

#[derive(Clone, Copy)]
struct ValidatedRecord<'a> {
    spec: SessionSpec,
    raw: &'a RawRecord,
}

fn validate_records<'a>(
    cycle: CampaignCycle,
    records: &'a [RawRecord],
) -> Result<Vec<ValidatedRecord<'a>>, AnalysisError> {
    let expected: Vec<_> = campaign_plan(cycle)
        .sessions()
        .filter(|spec| spec.series().phase() == CampaignPhase::Calibration)
        .map(|spec| ExpectedSession {
            key: RawSessionKey::from_spec(spec),
            spec,
        })
        .collect();
    let mut matched = vec![None; expected.len()];
    for (index, record) in records.iter().enumerate() {
        validate_envelope(record, index)?;
        let key = RawSessionKey::from_record(record, index)?;
        let Some(position) = expected.iter().position(|session| session.key == key) else {
            return Err(AnalysisError::UnexpectedIdentity { record: index, key });
        };
        if matched[position].replace(record).is_some() {
            return Err(AnalysisError::DuplicateIdentity { record: index, key });
        }
    }
    expected
        .into_iter()
        .zip(matched)
        .map(|(session, raw)| {
            raw.map(|raw| ValidatedRecord {
                spec: session.spec,
                raw,
            })
            .ok_or(AnalysisError::MissingIdentity { key: session.key })
        })
        .collect()
}

fn validate_envelope(record: &RawRecord, index: usize) -> Result<(), AnalysisError> {
    if record.schema_version != RAW_SCHEMA_VERSION {
        return Err(AnalysisError::UnsupportedSchemaVersion {
            record: index,
            found: record.schema_version,
        });
    }
    if record.experiment_id != EXPERIMENT_ID {
        return Err(AnalysisError::WrongExperimentId { record: index });
    }
    if record.run_id != RUN_ID {
        return Err(AnalysisError::WrongRunId { record: index });
    }
    if record.excluded {
        return Err(AnalysisError::ExcludedRecord { record: index });
    }
    if record.exclusion_reason.is_some() {
        return Err(AnalysisError::UnexpectedExclusionReason { record: index });
    }
    Ok(())
}

fn paired_block(
    engine: Engine,
    block: usize,
    records: &[ValidatedRecord<'_>],
) -> Result<PairedBlock, AnalysisError> {
    let mut baseline = None;
    let mut treatment = None;
    for record in records
        .iter()
        .filter(|record| record.spec.engine() == engine && record.spec.block_index().get() == block)
    {
        match record.spec.plan().mode() {
            SessionMode::CalibrationFour => baseline = Some(record.raw.cpu_usage_usec as f64),
            SessionMode::CalibrationFive => treatment = Some(record.raw.cpu_usage_usec as f64),
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

pub fn analyze_calibration(
    cycle: CampaignCycle,
    records: &[RawRecord],
) -> Result<CampaignCalibrations, AnalysisError> {
    let records = validate_records(cycle, records)?;
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
        let pair = paired_block(engine, block.index().get(), &records)?;
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

#[cfg(test)]
mod tests;
