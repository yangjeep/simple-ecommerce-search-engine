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

/// Student-t confidence interval for a sample mean. Preferred over the
/// percentile bootstrap at small n: the percentile interval is
/// anti-conservative below roughly n=30 (about 18% too narrow at n=10,
/// giving ~90% actual coverage for a nominal 95% interval), because
/// resampling cannot add information the sample does not contain.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MeanCi {
    pub mean: f64,
    pub ci_low: f64,
    pub ci_high: f64,
    pub n: usize,
    pub alpha: f64,
}

const T_CRITICAL_95: [f64; 30] = [
    12.706_204_736,
    4.302_652_730,
    3.182_446_305,
    2.776_445_105,
    2.570_581_836,
    2.446_911_851,
    2.364_624_252,
    2.306_004_135,
    2.262_157_163,
    2.228_138_852,
    2.200_985_160,
    2.178_812_830,
    2.160_368_656,
    2.144_786_688,
    2.131_449_546,
    2.119_905_299,
    2.109_815_578,
    2.100_922_040,
    2.093_024_054,
    2.085_963_447,
    2.079_613_845,
    2.073_873_068,
    2.068_657_610,
    2.063_898_562,
    2.059_538_553,
    2.055_529_439,
    2.051_830_516,
    2.048_407_142,
    2.045_229_642,
    2.042_272_456,
];

fn t_critical(df: usize, alpha: f64) -> f64 {
    if df == 0 || alpha.to_bits() != 0.05_f64.to_bits() {
        return f64::NAN;
    }
    if let Some(critical) = T_CRITICAL_95.get(df - 1) {
        return *critical;
    }

    let degrees_of_freedom = match u32::try_from(df) {
        Ok(value) => f64::from(value),
        Err(_) => return 1.959_963_984_540_054,
    };
    let z = 1.959_963_984_540_054_f64;
    let z2 = z * z;
    let z3 = z2 * z;
    let z5 = z3 * z2;
    let z7 = z5 * z2;
    z + (z3 + z) / (4.0 * degrees_of_freedom)
        + (5.0 * z5 + 16.0 * z3 + 3.0 * z) / (96.0 * degrees_of_freedom.powi(2))
        + (3.0 * z7 + 19.0 * z5 + 17.0 * z3 - 15.0 * z) / (384.0 * degrees_of_freedom.powi(3))
}

pub fn t_ci_mean(samples: &[f64], alpha: f64) -> MeanCi {
    let mut count = 0.0;
    let mut mean = 0.0;
    let mut squared_deviations = 0.0;
    for sample in samples {
        count += 1.0;
        let delta = sample - mean;
        mean += delta / count;
        squared_deviations += delta * (sample - mean);
    }

    let n = samples.len();
    if n == 0 {
        return MeanCi {
            mean: f64::NAN,
            ci_low: f64::NAN,
            ci_high: f64::NAN,
            n,
            alpha,
        };
    }
    if n == 1 {
        return MeanCi {
            mean,
            ci_low: mean,
            ci_high: mean,
            n,
            alpha,
        };
    }

    let standard_error = (squared_deviations / (count - 1.0) / count).sqrt();
    let halfwidth = t_critical(n - 1, alpha) * standard_error;
    MeanCi {
        mean,
        ci_low: mean - halfwidth,
        ci_high: mean + halfwidth,
        n,
        alpha,
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

/// Percentile bootstrap confidence interval for a single sample's mean.
/// Deterministic given `seed`, with the same preconditions as the
/// difference-of-means bootstrap.
pub fn bootstrap_ci_mean(samples: &[f64], resamples: usize, alpha: f64, seed: u64) -> BootstrapCi {
    assert!(!samples.is_empty(), "samples must be non-empty");
    assert!(
        resamples >= 2,
        "need at least 2 resamples to form an interval"
    );
    let mut rng = ChaCha8Rng::seed_from_u64(seed);
    let mut means: Vec<f64> = (0..resamples)
        .map(|_| resample_mean(&mut rng, samples))
        .collect();
    means.sort_by(f64::total_cmp);

    let point_mean = samples.iter().sum::<f64>() / samples.len() as f64;
    let lo_rank = ((alpha / 2.0) * (resamples - 1) as f64).round() as usize;
    let hi_rank = ((1.0 - alpha / 2.0) * (resamples - 1) as f64).round() as usize;

    BootstrapCi {
        diff: point_mean,
        ci_low: means[lo_rank],
        ci_high: means[hi_rank.min(resamples - 1)],
        alpha,
        resamples,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t_critical_matches_published_table_values() {
        // Given: published two-sided 95% Student-t critical values.
        let published = [(1, 12.706), (9, 2.262), (29, 2.045)];

        // When/Then: the dependency-free lookup reproduces each value.
        for (df, expected) in published {
            assert!((t_critical(df, 0.05) - expected).abs() <= 0.005);
        }
    }

    #[test]
    fn t_interval_is_wider_than_the_percentile_bootstrap_at_n_ten() {
        // Given: ten approximately normal scores scaled to mean 100 and
        // sample CV 10%, matching the adversarial review's small-n case.
        let normal_scores = [
            -1.5466, -1.0005, -0.6554, -0.3755, -0.1226, 0.1226, 0.3755, 0.6554, 1.0005, 1.5466,
        ];
        let sum_of_squares: f64 = normal_scores.iter().map(|value| value * value).sum();
        let scale = 10.0 / (sum_of_squares / 9.0).sqrt();
        let samples: Vec<f64> = normal_scores
            .iter()
            .map(|value| 100.0 + value * scale)
            .collect();

        // When: both nominal 95% intervals are computed from the same sample.
        let student = t_ci_mean(&samples, 0.05);
        let percentile = bootstrap_ci_mean(&samples, 100_000, 0.05, 61);
        let student_halfwidth = (student.ci_high - student.ci_low) / 2.0;
        let percentile_halfwidth = (percentile.ci_high - percentile.ci_low) / 2.0;

        // Then: resampling ten observations is anti-conservatively narrower.
        eprintln!(
            "n=10 CV=10%: Student-t half-width={student_halfwidth:.6}% percentile-bootstrap half-width={percentile_halfwidth:.6}%"
        );
        assert!((student_halfwidth - 7.153_690).abs() < 0.001);
        assert!(student_halfwidth > percentile_halfwidth);
    }

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

    #[test]
    fn bootstrap_ci_mean_brackets_a_known_constant_sample() {
        let ci = bootstrap_ci_mean(&[7.0; 10], 1_000, 0.05, 61);

        assert_eq!(ci.diff, 7.0);
        assert_eq!(ci.ci_low, 7.0);
        assert_eq!(ci.ci_high, 7.0);
    }

    #[test]
    fn bootstrap_ci_mean_is_deterministic_given_seed() {
        let samples = [1.0, 2.0, 3.0, 4.0, 5.0];

        let first = bootstrap_ci_mean(&samples, 1_000, 0.05, 61);
        let second = bootstrap_ci_mean(&samples, 1_000, 0.05, 61);

        assert_eq!(first, second);
    }

    #[test]
    fn bootstrap_ci_mean_widens_with_dispersion() {
        let low_variance = [9.9, 10.0, 10.1, 9.9, 10.1, 10.0];
        let high_variance = [0.0, 20.0, 0.0, 20.0, 0.0, 20.0];

        let low = bootstrap_ci_mean(&low_variance, 5_000, 0.05, 61);
        let high = bootstrap_ci_mean(&high_variance, 5_000, 0.05, 61);

        assert!(high.ci_high - high.ci_low > low.ci_high - low.ci_low);
    }
}
