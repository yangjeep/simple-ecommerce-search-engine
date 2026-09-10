use super::{AnalysisError, IdentityField};
use crate::{
    campaign_plan, CampaignCycle, CampaignPhase, Dataset, Engine, EngineOrder, RawRecord,
    SessionMode, SessionSpec, WorkloadProjection, RAW_SCHEMA_VERSION,
};
use std::str::FromStr;

const EXPERIMENT_ID: &str = "I61-E1";
const RUN_ID: &str = "seed-61";

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

#[derive(Clone, Copy)]
struct ExpectedSession {
    key: RawSessionKey,
    spec: SessionSpec,
}

#[derive(Clone, Copy)]
pub(super) struct ValidatedRecord<'a> {
    source_index: usize,
    spec: SessionSpec,
    raw: &'a RawRecord,
}

impl<'a> ValidatedRecord<'a> {
    pub(super) const fn source_index(self) -> usize {
        self.source_index
    }

    pub(super) const fn spec(self) -> SessionSpec {
        self.spec
    }

    pub(super) const fn raw(self) -> &'a RawRecord {
        self.raw
    }
}

#[derive(Clone, Copy)]
enum EvidenceScope {
    Calibration,
    FullCampaign,
}

impl EvidenceScope {
    const fn includes(self, phase: CampaignPhase) -> bool {
        match (self, phase) {
            (Self::Calibration, CampaignPhase::Calibration) | (Self::FullCampaign, _) => true,
            (Self::Calibration, CampaignPhase::Warm | CampaignPhase::Cold) => false,
        }
    }
}

pub(super) fn validate_calibration_records(
    cycle: CampaignCycle,
    records: &[RawRecord],
) -> Result<Vec<ValidatedRecord<'_>>, AnalysisError> {
    validate_records(cycle, records, EvidenceScope::Calibration)
}

pub(super) fn validate_campaign_records(
    cycle: CampaignCycle,
    records: &[RawRecord],
) -> Result<Vec<ValidatedRecord<'_>>, AnalysisError> {
    validate_records(cycle, records, EvidenceScope::FullCampaign)
}

fn validate_records(
    cycle: CampaignCycle,
    records: &[RawRecord],
    scope: EvidenceScope,
) -> Result<Vec<ValidatedRecord<'_>>, AnalysisError> {
    let expected: Vec<_> = campaign_plan(cycle)
        .sessions()
        .filter(|spec| scope.includes(spec.series().phase()))
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
        if matched[position].replace((index, record)).is_some() {
            return Err(AnalysisError::DuplicateIdentity { record: index, key });
        }
    }
    expected
        .into_iter()
        .zip(matched)
        .map(|(session, matched)| {
            matched
                .map(|(source_index, raw)| ValidatedRecord {
                    source_index,
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

#[cfg(test)]
mod tests;
