#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Engine {
    Baseline,
    Treatment,
}

impl Engine {
    pub(super) const fn name(self) -> &'static str {
        match self {
            Self::Baseline => "baseline",
            Self::Treatment => "treatment",
        }
    }
}

struct SplitMix64(u64);

impl SplitMix64 {
    const fn new(seed: u64) -> Self {
        Self(seed)
    }

    const fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e3779b97f4a7c15);
        let mut value = self.0;
        value = (value ^ (value >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        value = (value ^ (value >> 27)).wrapping_mul(0x94d049bb133111eb);
        value ^ (value >> 31)
    }
}

pub(super) fn engine_order(seed: u64, blocks: usize) -> Vec<[Engine; 2]> {
    let mut random = SplitMix64::new(seed);
    let mut baseline_first = blocks / 2;
    let mut treatment_first = blocks - baseline_first;
    let mut orders = Vec::with_capacity(blocks);
    for remaining in (1..=blocks).rev() {
        let choose_baseline = if baseline_first == 0 {
            false
        } else if treatment_first == 0 {
            true
        } else {
            random.next() % u64::try_from(remaining).unwrap_or(1)
                < u64::try_from(baseline_first).unwrap_or(0)
        };
        if choose_baseline {
            orders.push([Engine::Baseline, Engine::Treatment]);
            baseline_first -= 1;
        } else {
            orders.push([Engine::Treatment, Engine::Baseline]);
            treatment_first -= 1;
        }
    }
    orders
}

pub(super) fn calibration_pass_counts(passes: usize) -> Result<(usize, usize), String> {
    passes
        .checked_sub(1)
        .filter(|baseline| *baseline > 0)
        .map(|baseline| (baseline, passes))
        .ok_or_else(|| "calibration passes must be at least 2".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seeded_scheduler_is_deterministic_and_balanced_across_blocks() {
        let first = engine_order(61, 30);
        assert_eq!(first, engine_order(61, 30));
        assert_eq!(
            first
                .iter()
                .filter(|order| order[0] == Engine::Baseline)
                .count(),
            15
        );
    }

    #[test]
    fn calibration_block_construction_yields_expected_pass_counts() {
        assert_eq!(calibration_pass_counts(5).expect("valid passes"), (4, 5));
        assert!(calibration_pass_counts(1).is_err());
    }
}
