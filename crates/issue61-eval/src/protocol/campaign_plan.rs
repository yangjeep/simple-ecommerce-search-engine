mod model;

use super::{calibration_schedule, campaign_schedule, Engine, CAMPAIGN_BLOCKS};
use crate::{Dataset, WorkloadProjection};
use std::str::FromStr;

pub use model::{
    BlockSpec, CampaignBlockIndex, CampaignPhase, CampaignPlan, CampaignSeries, PairOrder,
    SessionSpec,
};
use model::{EngineBlockSpec, EngineSeries};

pub const WARM_BLOCKS_PER_SERIES: usize = CAMPAIGN_BLOCKS;
pub const CALIBRATION_BLOCKS_PER_ENGINE: usize = CAMPAIGN_BLOCKS;
pub const COLD_BLOCKS_PER_DATASET: usize = 5;
pub const STABILITY_CELLS: usize = 48;
pub const EXACT_INDEX_CELLS: usize = 4;

const WARM_SERIES: usize = 8;
const CALIBRATION_SERIES: usize = 2;
const COLD_SERIES: usize = 2;
const CAMPAIGN_PAIRS: usize = WARM_SERIES * WARM_BLOCKS_PER_SERIES
    + CALIBRATION_SERIES * CALIBRATION_BLOCKS_PER_ENGINE
    + COLD_SERIES * COLD_BLOCKS_PER_DATASET;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CampaignCycle {
    Run1,
    Rerun1,
    Rerun2,
}

impl CampaignCycle {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Run1 => "run1",
            Self::Rerun1 => "rerun1",
            Self::Rerun2 => "rerun2",
        }
    }
}

impl FromStr for CampaignCycle {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "run1" => Ok(Self::Run1),
            "rerun1" => Ok(Self::Rerun1),
            "rerun2" => Ok(Self::Rerun2),
            other => Err(format!(
                "invalid campaign cycle {other:?}; expected run1, rerun1, or rerun2"
            )),
        }
    }
}

#[must_use]
pub fn campaign_plan(cycle: CampaignCycle) -> CampaignPlan {
    let engine_schedule = campaign_schedule();
    let pass_schedule = calibration_schedule();
    let mut blocks = Vec::with_capacity(CAMPAIGN_PAIRS);

    for engine in [Engine::Native, Engine::Solr] {
        for (index, order) in pass_schedule.iter().copied().enumerate() {
            blocks.push(BlockSpec::calibration(engine, index, order));
        }
    }
    for dataset in [Dataset::Wands, Dataset::EsciElectronics] {
        for projection in [
            WorkloadProjection::All,
            WorkloadProjection::FastPath,
            WorkloadProjection::Hybrid,
            WorkloadProjection::Punt,
        ] {
            let series = EngineSeries::Warm {
                dataset,
                projection,
            };
            for (index, pair) in engine_schedule.iter().copied().enumerate() {
                blocks.push(BlockSpec::engine_pair(EngineBlockSpec {
                    series,
                    index,
                    pair,
                }));
            }
        }
    }
    for dataset in [Dataset::Wands, Dataset::EsciElectronics] {
        let series = EngineSeries::Cold { dataset };
        for (index, pair) in engine_schedule
            .iter()
            .copied()
            .take(COLD_BLOCKS_PER_DATASET)
            .enumerate()
        {
            blocks.push(BlockSpec::engine_pair(EngineBlockSpec {
                series,
                index,
                pair,
            }));
        }
    }

    CampaignPlan { cycle, blocks }
}
