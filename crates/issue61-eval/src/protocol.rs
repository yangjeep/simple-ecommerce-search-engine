use std::str::FromStr;

mod campaign_plan;
mod schedule;
mod session_plan;

pub use campaign_plan::{
    campaign_plan, BlockSpec, CampaignBlockIndex, CampaignCycle, CampaignPhase, CampaignPlan,
    CampaignSeries, PairOrder, SessionSpec, CALIBRATION_BLOCKS_PER_ENGINE, COLD_BLOCKS_PER_DATASET,
    EXACT_INDEX_CELLS, STABILITY_CELLS, WARM_BLOCKS_PER_SERIES,
};
pub use schedule::{
    calibration_schedule, campaign_schedule, BlockIndex, CalibrationOrder, CampaignSeed,
    EngineOrder, EnginePair, CAMPAIGN_BLOCKS,
};
pub use session_plan::{SessionMode, SessionPlan, SessionStep};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Engine {
    Native,
    Solr,
}

impl Engine {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Native => "native",
            Self::Solr => "solr",
        }
    }
}

impl FromStr for Engine {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "native" => Ok(Self::Native),
            "solr" => Ok(Self::Solr),
            other => Err(format!("invalid engine {other:?}; expected native or solr")),
        }
    }
}
