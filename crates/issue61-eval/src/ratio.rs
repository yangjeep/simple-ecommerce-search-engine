use bench_harness::t_ci_mean;

pub const MATERIALITY_RATIO: f64 = 0.75;

/// One randomized paired block: both engines measured under the same host
/// conditions. The block, not the individual repetition, is the unit of
/// analysis -- repetitions are serially dependent (page cache, engine caches,
/// host drift), so treating them as IID is unjustified.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PairedBlock {
    pub block: usize,
    pub baseline: f64,
    pub treatment: f64,
}

impl PairedBlock {
    /// Returns `None` when the baseline is non-positive, because that ratio is
    /// undefined and must never silently become zero or infinity.
    pub fn ratio(&self) -> Option<f64> {
        (self.baseline > 0.0).then_some(self.treatment / self.baseline)
    }

    pub fn log_ratio(&self) -> Option<f64> {
        self.ratio()
            .filter(|ratio| *ratio > 0.0 && ratio.is_finite())
            .map(f64::ln)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RatioVerdict {
    MateriallyBetter,
    Inconclusive,
    NotBetter,
}

/// Ratio interval computed on the log scale and exponentiated back because
/// ratios are multiplicative and a raw symmetric interval can cross zero.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RatioResult {
    pub n_blocks: usize,
    pub point_ratio: f64,
    pub ci_low: f64,
    pub ci_high: f64,
    pub verdict: RatioVerdict,
}

/// Returns `MateriallyBetter` only when the upper confidence bound clears the
/// materiality ratio; a point estimate on the threshold is not enough.
pub fn paired_ratio(blocks: &[PairedBlock], alpha: f64) -> Option<RatioResult> {
    let log_ratios: Option<Vec<f64>> = blocks.iter().map(PairedBlock::log_ratio).collect();
    let interval = t_ci_mean(&log_ratios?, alpha);
    if interval.n == 0
        || !interval.mean.is_finite()
        || !interval.ci_low.is_finite()
        || !interval.ci_high.is_finite()
    {
        return None;
    }

    let point_ratio = interval.mean.exp();
    let ci_low = interval.ci_low.exp();
    let ci_high = interval.ci_high.exp();
    let verdict = if ci_high <= MATERIALITY_RATIO {
        RatioVerdict::MateriallyBetter
    } else if ci_low > MATERIALITY_RATIO {
        RatioVerdict::NotBetter
    } else {
        RatioVerdict::Inconclusive
    };
    Some(RatioResult {
        n_blocks: interval.n,
        point_ratio,
        ci_low,
        ci_high,
        verdict,
    })
}

/// A deliberately injected, known effect. If the instrument cannot recover a
/// ratio it was told to expect, it cannot be trusted to measure an unknown one.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CalibrationCheck {
    pub expected_ratio: f64,
    pub observed: RatioResult,
    pub passed: bool,
}

/// Passes only when the observed interval contains the known ratio and
/// excludes the no-effect ratio of one.
pub fn check_calibration(
    blocks: &[PairedBlock],
    expected_ratio: f64,
    alpha: f64,
) -> Option<CalibrationCheck> {
    let observed = paired_ratio(blocks, alpha)?;
    let contains_expected = observed.ci_low <= expected_ratio && expected_ratio <= observed.ci_high;
    let excludes_one = observed.ci_high < 1.0 || observed.ci_low > 1.0;
    Some(CalibrationCheck {
        expected_ratio,
        observed,
        passed: contains_expected && excludes_one,
    })
}
