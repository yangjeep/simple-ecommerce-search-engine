use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;

/// A point estimate plus a bootstrap confidence interval, for a headline
/// relative-improvement claim ("method B is Nx faster than method A").
/// Issue #6: "Use bootstrap/confidence intervals for headline relative
/// improvements where practical." `diff` is `mean(b) - mean(a)` (negative
/// means b is faster, for a latency metric); `ci_low`/`ci_high` bound the
/// `(1 - alpha)` confidence interval for that same quantity.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BootstrapCi {
    pub diff: f64,
    pub ci_low: f64,
    pub ci_high: f64,
    pub alpha: f64,
    pub resamples: usize,
}

impl BootstrapCi {
    /// Whether the interval excludes zero -- the standard bootstrap
    /// significance check. Does not by itself mean the difference is
    /// *practically* meaningful (Issue #6's 5-10x bar is a magnitude
    /// question, not a p<0.05 question) -- report both.
    pub fn excludes_zero(&self) -> bool {
        self.ci_low > 0.0 || self.ci_high < 0.0
    }
}

fn resample_mean(rng: &mut ChaCha8Rng, samples: &[f64]) -> f64 {
    let n = samples.len();
    let sum: f64 = (0..n).map(|_| samples[rng.gen_range(0..n)]).sum();
    sum / n as f64
}

/// Percentile bootstrap CI for `mean(b) - mean(a)`, via independent
/// resampling of each sample set. Deterministic given `seed`, matching
/// this project's existing "seeded, not truly random" convention
/// (`realtime-eval`'s R-E01/R-E02, Round 1's various seeded samplers) --
/// a bootstrap run must be reproducible, not just statistically valid.
/// `resamples` should be >= 2000 for a stable interval at `alpha=0.05`;
/// lower is fine for quick exploration.
pub fn bootstrap_ci_diff_of_means(
    a: &[f64],
    b: &[f64],
    resamples: usize,
    alpha: f64,
    seed: u64,
) -> BootstrapCi {
    assert!(
        !a.is_empty() && !b.is_empty(),
        "both samples must be non-empty"
    );
    assert!(
        resamples >= 2,
        "need at least 2 resamples to form an interval"
    );
    let mut rng = ChaCha8Rng::seed_from_u64(seed);
    let mut diffs: Vec<f64> = (0..resamples)
        .map(|_| resample_mean(&mut rng, b) - resample_mean(&mut rng, a))
        .collect();
    diffs.sort_by(f64::total_cmp);

    let point_diff =
        (b.iter().sum::<f64>() / b.len() as f64) - (a.iter().sum::<f64>() / a.len() as f64);
    let lo_rank = ((alpha / 2.0) * (resamples - 1) as f64).round() as usize;
    let hi_rank = ((1.0 - alpha / 2.0) * (resamples - 1) as f64).round() as usize;

    BootstrapCi {
        diff: point_diff,
        ci_low: diffs[lo_rank],
        ci_high: diffs[hi_rank.min(resamples - 1)],
        alpha,
        resamples,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_distributions_produce_a_ci_that_contains_zero() {
        let a = vec![10.0, 11.0, 9.0, 10.5, 9.5, 10.0, 10.2, 9.8, 10.1, 9.9];
        let b = a.clone();
        let ci = bootstrap_ci_diff_of_means(&a, &b, 5000, 0.05, 1);
        assert!(
            !ci.excludes_zero(),
            "identical samples must not show a significant difference: {ci:?}"
        );
        assert!(ci.diff.abs() < 1e-9);
    }

    #[test]
    fn a_large_real_gap_is_detected_as_significant() {
        // a ~ 10ms, b ~ 2ms: a real, large, low-variance gap must not be
        // swallowed by resampling noise.
        let a: Vec<f64> = (0..50).map(|i| 10.0 + (i % 5) as f64 * 0.1).collect();
        let b: Vec<f64> = (0..50).map(|i| 2.0 + (i % 5) as f64 * 0.1).collect();
        let ci = bootstrap_ci_diff_of_means(&a, &b, 5000, 0.05, 1);
        assert!(
            ci.excludes_zero(),
            "a real ~5x gap must be significant: {ci:?}"
        );
        assert!(ci.diff < 0.0, "b is faster, diff (b-a) must be negative");
        assert!(
            ci.ci_high < 0.0,
            "even the upper CI bound should stay negative for this gap"
        );
    }

    #[test]
    fn deterministic_given_the_same_seed() {
        let a = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let b = vec![2.0, 3.0, 4.0, 5.0, 6.0];
        let ci1 = bootstrap_ci_diff_of_means(&a, &b, 1000, 0.05, 99);
        let ci2 = bootstrap_ci_diff_of_means(&a, &b, 1000, 0.05, 99);
        assert_eq!(ci1, ci2);
    }
}
