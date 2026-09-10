use super::super::{CalibrationOrder, Engine, EngineOrder, EnginePair, SessionMode, SessionPlan};
use crate::{Dataset, WorkloadProjection};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CampaignPhase {
    Calibration,
    Warm,
    Cold,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CampaignSeries {
    Calibration {
        engine: Engine,
    },
    Warm {
        dataset: Dataset,
        projection: WorkloadProjection,
    },
    Cold {
        dataset: Dataset,
    },
}

impl CampaignSeries {
    #[must_use]
    pub const fn phase(self) -> CampaignPhase {
        match self {
            Self::Calibration { engine: _ } => CampaignPhase::Calibration,
            Self::Warm {
                dataset: _,
                projection: _,
            } => CampaignPhase::Warm,
            Self::Cold { dataset: _ } => CampaignPhase::Cold,
        }
    }

    #[must_use]
    pub const fn dataset(self) -> Dataset {
        match self {
            Self::Calibration { engine: _ } => Dataset::Wands,
            Self::Warm {
                dataset,
                projection: _,
            }
            | Self::Cold { dataset } => dataset,
        }
    }

    #[must_use]
    pub const fn projection(self) -> WorkloadProjection {
        match self {
            Self::Warm {
                dataset: _,
                projection,
            } => projection,
            Self::Calibration { engine: _ } | Self::Cold { dataset: _ } => WorkloadProjection::All,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CampaignBlockIndex(usize);

impl CampaignBlockIndex {
    pub(super) const fn new(value: usize) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> usize {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PairOrder {
    Engine(EnginePair),
    Calibration(CalibrationOrder),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SessionIdentity {
    series: CampaignSeries,
    block_index: CampaignBlockIndex,
    slot: EngineOrder,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionSpec {
    identity: SessionIdentity,
    engine: Engine,
    plan: SessionPlan,
}

impl SessionSpec {
    const fn new(identity: SessionIdentity, engine: Engine, plan: SessionPlan) -> Self {
        Self {
            identity,
            engine,
            plan,
        }
    }

    #[must_use]
    pub const fn series(self) -> CampaignSeries {
        self.identity.series
    }

    #[must_use]
    pub const fn block_index(self) -> CampaignBlockIndex {
        self.identity.block_index
    }

    #[must_use]
    pub const fn slot(self) -> EngineOrder {
        self.identity.slot
    }

    #[must_use]
    pub const fn engine(self) -> Engine {
        self.engine
    }

    #[must_use]
    pub const fn plan(self) -> SessionPlan {
        self.plan
    }
}

#[derive(Clone, Copy)]
pub(super) enum EngineSeries {
    Warm {
        dataset: Dataset,
        projection: WorkloadProjection,
    },
    Cold {
        dataset: Dataset,
    },
}

pub(super) struct EngineBlockSpec {
    pub(super) series: EngineSeries,
    pub(super) index: usize,
    pub(super) pair: EnginePair,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockSpec {
    series: CampaignSeries,
    index: CampaignBlockIndex,
    order: PairOrder,
    sessions: [SessionSpec; 2],
}

impl BlockSpec {
    pub(super) fn engine_pair(spec: EngineBlockSpec) -> Self {
        let block_index = CampaignBlockIndex::new(spec.index);
        let (series, mode) = match spec.series {
            EngineSeries::Warm {
                dataset,
                projection,
            } => (
                CampaignSeries::Warm {
                    dataset,
                    projection,
                },
                SessionMode::Warm,
            ),
            EngineSeries::Cold { dataset } => (CampaignSeries::Cold { dataset }, SessionMode::Cold),
        };
        Self {
            series,
            index: block_index,
            order: PairOrder::Engine(spec.pair),
            sessions: [
                SessionSpec::new(
                    SessionIdentity {
                        series,
                        block_index,
                        slot: EngineOrder::First,
                    },
                    spec.pair.first(),
                    mode.plan(),
                ),
                SessionSpec::new(
                    SessionIdentity {
                        series,
                        block_index,
                        slot: EngineOrder::Second,
                    },
                    spec.pair.second(),
                    mode.plan(),
                ),
            ],
        }
    }

    pub(super) fn calibration(engine: Engine, index: usize, order: CalibrationOrder) -> Self {
        let series = CampaignSeries::Calibration { engine };
        let block_index = CampaignBlockIndex::new(index);
        let (first, second) = match order {
            CalibrationOrder::FourFirst => {
                (SessionMode::CalibrationFour, SessionMode::CalibrationFive)
            }
            CalibrationOrder::FiveFirst => {
                (SessionMode::CalibrationFive, SessionMode::CalibrationFour)
            }
        };
        Self {
            series,
            index: block_index,
            order: PairOrder::Calibration(order),
            sessions: [
                SessionSpec::new(
                    SessionIdentity {
                        series,
                        block_index,
                        slot: EngineOrder::First,
                    },
                    engine,
                    first.plan(),
                ),
                SessionSpec::new(
                    SessionIdentity {
                        series,
                        block_index,
                        slot: EngineOrder::Second,
                    },
                    engine,
                    second.plan(),
                ),
            ],
        }
    }

    #[must_use]
    pub const fn series(&self) -> CampaignSeries {
        self.series
    }

    #[must_use]
    pub const fn index(&self) -> CampaignBlockIndex {
        self.index
    }

    #[must_use]
    pub const fn order(&self) -> PairOrder {
        self.order
    }

    #[must_use]
    pub const fn sessions(&self) -> &[SessionSpec; 2] {
        &self.sessions
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CampaignPlan {
    pub(super) cycle: super::CampaignCycle,
    pub(super) blocks: Vec<BlockSpec>,
}

impl CampaignPlan {
    #[must_use]
    pub const fn cycle(&self) -> super::CampaignCycle {
        self.cycle
    }

    #[must_use]
    pub fn blocks(&self) -> &[BlockSpec] {
        &self.blocks
    }

    pub fn sessions(&self) -> impl Iterator<Item = SessionSpec> + '_ {
        self.blocks
            .iter()
            .flat_map(|block| block.sessions.iter().copied())
    }
}
