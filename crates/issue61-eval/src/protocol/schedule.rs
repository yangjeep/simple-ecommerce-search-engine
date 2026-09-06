use super::Engine;
use std::str::FromStr;

pub const CAMPAIGN_BLOCKS: usize = 30;
const BALANCED_SLOTS: u64 = 15;
const TOTAL_SLOTS: u64 = 30;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CampaignSeed;

impl CampaignSeed {
    #[must_use]
    pub const fn get(self) -> u64 {
        61
    }
}

impl FromStr for CampaignSeed {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "61" => Ok(Self),
            other => Err(format!("invalid campaign seed {other:?}; expected 61")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockIndex(usize);

impl BlockIndex {
    #[must_use]
    pub const fn get(self) -> usize {
        self.0
    }
}

impl FromStr for BlockIndex {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let index = value
            .parse::<usize>()
            .map_err(|error| format!("invalid --block: {error}"))?;
        if index >= CAMPAIGN_BLOCKS {
            return Err(format!(
                "invalid --block {index}; expected 0..{CAMPAIGN_BLOCKS}"
            ));
        }
        Ok(Self(index))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineOrder {
    First,
    Second,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CalibrationOrder {
    FourFirst,
    FiveFirst,
}

impl EngineOrder {
    #[must_use]
    pub const fn index(self) -> usize {
        match self {
            Self::First => 0,
            Self::Second => 1,
        }
    }
}

impl FromStr for EngineOrder {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "0" => Ok(Self::First),
            "1" => Ok(Self::Second),
            other => Err(format!("invalid --engine-order {other:?}; expected 0 or 1")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnginePair {
    first: Engine,
    second: Engine,
}

impl EnginePair {
    const fn new(first: Engine, second: Engine) -> Self {
        Self { first, second }
    }

    #[must_use]
    pub const fn first(self) -> Engine {
        self.first
    }

    #[must_use]
    pub const fn second(self) -> Engine {
        self.second
    }

    #[must_use]
    pub const fn engine(self, order: EngineOrder) -> Engine {
        match order {
            EngineOrder::First => self.first(),
            EngineOrder::Second => self.second(),
        }
    }
}

struct SplitMix64(u64);

impl SplitMix64 {
    const fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut value = self.0;
        value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        value ^ (value >> 31)
    }
}

fn build_balanced_schedule<T: Copy>(selected: T, alternate: T) -> [T; CAMPAIGN_BLOCKS] {
    let mut random = SplitMix64(CampaignSeed.get());
    let mut selected_remaining = BALANCED_SLOTS;
    let mut remaining = TOTAL_SLOTS;
    std::array::from_fn(|_| {
        let value = if random.next() % remaining < selected_remaining {
            selected_remaining -= 1;
            selected
        } else {
            alternate
        };
        remaining -= 1;
        value
    })
}

#[must_use]
pub fn campaign_schedule() -> [EnginePair; CAMPAIGN_BLOCKS] {
    build_balanced_schedule(
        EnginePair::new(Engine::Solr, Engine::Native),
        EnginePair::new(Engine::Native, Engine::Solr),
    )
}

#[must_use]
pub fn calibration_schedule() -> [CalibrationOrder; CAMPAIGN_BLOCKS] {
    build_balanced_schedule(CalibrationOrder::FourFirst, CalibrationOrder::FiveFirst)
}

#[cfg(test)]
mod tests {
    use super::{calibration_schedule, CalibrationOrder};

    #[test]
    fn seed_61_calibration_schedule_is_deterministic_and_balanced() {
        let schedule = calibration_schedule();

        assert_eq!(schedule, calibration_schedule());
        assert_eq!(
            schedule
                .iter()
                .filter(|order| **order == CalibrationOrder::FourFirst)
                .count(),
            15
        );
        assert_eq!(
            schedule
                .iter()
                .filter(|order| **order == CalibrationOrder::FiveFirst)
                .count(),
            15
        );
    }
}
