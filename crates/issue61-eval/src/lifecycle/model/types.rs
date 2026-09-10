use crate::{CampaignSeries, Dataset, Engine, WorkloadProjection};
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub(crate) enum Phase {
    StaticValidation,
    Initialization,
    EquivalenceAudit,
    IndexCapture,
    EquivalenceGate,
    Calibration,
    CalibrationGate,
    Warm,
    Cold,
    EvidenceFinalization,
}

impl Phase {
    pub(crate) const ALL: [Self; 10] = [
        Self::StaticValidation,
        Self::Initialization,
        Self::EquivalenceAudit,
        Self::IndexCapture,
        Self::EquivalenceGate,
        Self::Calibration,
        Self::CalibrationGate,
        Self::Warm,
        Self::Cold,
        Self::EvidenceFinalization,
    ];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum SeriesIdentity {
    Calibration,
    Warm,
    Cold,
}

impl From<CampaignSeries> for SeriesIdentity {
    fn from(value: CampaignSeries) -> Self {
        match value {
            CampaignSeries::Calibration { engine: _ } => Self::Calibration,
            CampaignSeries::Warm {
                dataset: _,
                projection: _,
            } => Self::Warm,
            CampaignSeries::Cold { dataset: _ } => Self::Cold,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum EngineIdentity {
    Native,
    Solr,
}

impl From<Engine> for EngineIdentity {
    fn from(value: Engine) -> Self {
        match value {
            Engine::Native => Self::Native,
            Engine::Solr => Self::Solr,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DatasetIdentity {
    Wands,
    EsciElectronics,
}

impl DatasetIdentity {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Wands => "wands",
            Self::EsciElectronics => "esci_electronics",
        }
    }
}

impl From<Dataset> for DatasetIdentity {
    fn from(value: Dataset) -> Self {
        match value {
            Dataset::Wands => Self::Wands,
            Dataset::EsciElectronics => Self::EsciElectronics,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub(crate) enum Projection {
    #[serde(rename = "all")]
    All,
    #[serde(rename = "fast-path")]
    FastPath,
    #[serde(rename = "hybrid")]
    Hybrid,
    #[serde(rename = "punt")]
    Punt,
}

impl From<WorkloadProjection> for Projection {
    fn from(value: WorkloadProjection) -> Self {
        match value {
            WorkloadProjection::All => Self::All,
            WorkloadProjection::FastPath => Self::FastPath,
            WorkloadProjection::Hybrid => Self::Hybrid,
            WorkloadProjection::Punt => Self::Punt,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Sequence(u64);

impl Sequence {
    pub(crate) const fn new(value: u64) -> Self {
        Self(value)
    }

    pub(crate) const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Attempt(u32);

impl Attempt {
    pub(crate) const fn new(value: u32) -> Self {
        Self(value)
    }

    pub(crate) const fn get(self) -> u32 {
        self.0
    }

    pub(crate) const fn next(self) -> Self {
        Self(self.0 + 1)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SlotIndex(u8);

impl SlotIndex {
    pub(crate) const fn zero() -> Self {
        Self(0)
    }

    pub(crate) const fn one() -> Self {
        Self(1)
    }

    pub(crate) const fn get(self) -> u8 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum EvidenceFile {
    CandidateAuditEsci,
    CandidateAuditWands,
    Commands,
    Events,
    IndexArtifacts,
    Raw,
}

impl EvidenceFile {
    pub(crate) const ALL: [Self; 6] = [
        Self::CandidateAuditEsci,
        Self::CandidateAuditWands,
        Self::Commands,
        Self::Events,
        Self::IndexArtifacts,
        Self::Raw,
    ];

    pub(crate) const NON_EVENT_CLOSE_ORDER: [Self; 5] = [
        Self::CandidateAuditEsci,
        Self::CandidateAuditWands,
        Self::Commands,
        Self::IndexArtifacts,
        Self::Raw,
    ];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Terminal {
    Completed,
    GateFailed(Phase),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BlockContext {
    pub(crate) phase: Phase,
    pub(crate) series: SeriesIdentity,
    pub(crate) campaign_series: crate::CampaignSeries,
    pub(crate) block_index: usize,
    pub(crate) attempt: Attempt,
    pub(crate) dataset: DatasetIdentity,
    pub(crate) projection: Option<Projection>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SlotContext {
    pub(crate) block: BlockContext,
    pub(crate) slot: SlotIndex,
    pub(crate) engine: EngineIdentity,
    pub(crate) session: crate::SessionSpec,
}
