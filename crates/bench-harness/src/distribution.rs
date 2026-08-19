use serde::{Deserialize, Serialize};

/// Full percentile distribution over a set of samples, per Issue #6's
/// "report distributions, not single numbers" requirement. Percentiles use
/// linear interpolation between the two nearest ranks (the standard
/// "R-7"/Excel-style estimator) -- deterministic, no randomness, matches
/// what most plotting/analysis tools default to so figures generated from
/// these numbers are comparable to third-party tooling without a
/// footnote.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Distribution {
    pub n: usize,
    pub mean: f64,
    pub stddev: f64,
    pub min: f64,
    pub p10: f64,
    pub p25: f64,
    pub p50: f64,
    pub p75: f64,
    pub p90: f64,
    pub p95: f64,
    pub p99: f64,
    pub max: f64,
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    if sorted.len() == 1 {
        return sorted[0];
    }
    let rank = p * (sorted.len() - 1) as f64;
    let lo = rank.floor() as usize;
    let hi = rank.ceil() as usize;
    if lo == hi {
        return sorted[lo];
    }
    let frac = rank - lo as f64;
    sorted[lo] + (sorted[hi] - sorted[lo]) * frac
}

impl Distribution {
    /// Panics on an empty slice: a distribution over zero samples is not
    /// a distribution, and every caller in this codebase has real samples
    /// available before it ever needs one -- fail loud rather than return
    /// a silently-meaningless all-zero `Distribution`.
    pub fn compute(samples: &[f64]) -> Self {
        assert!(
            !samples.is_empty(),
            "Distribution::compute requires at least one sample"
        );
        let mut sorted: Vec<f64> = samples.to_vec();
        sorted.sort_by(f64::total_cmp);
        let n = sorted.len();
        let mean = sorted.iter().sum::<f64>() / n as f64;
        let variance = if n > 1 {
            sorted.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / (n - 1) as f64
        } else {
            0.0
        };
        Distribution {
            n,
            mean,
            stddev: variance.sqrt(),
            min: sorted[0],
            p10: percentile(&sorted, 0.10),
            p25: percentile(&sorted, 0.25),
            p50: percentile(&sorted, 0.50),
            p75: percentile(&sorted, 0.75),
            p90: percentile(&sorted, 0.90),
            p95: percentile(&sorted, 0.95),
            p99: percentile(&sorted, 0.99),
            max: sorted[n - 1],
        }
    }

    pub fn print(&self, label: &str, unit: &str) {
        println!(
            "  {label}: n={} mean={:.4}{unit} sd={:.4}{unit} | p10={:.4} p25={:.4} p50={:.4} p75={:.4} p90={:.4} p95={:.4} p99={:.4} | min={:.4} max={:.4}{unit}",
            self.n, self.mean, self.stddev,
            self.p10, self.p25, self.p50, self.p75, self.p90, self.p95, self.p99,
            self.min, self.max
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentiles_of_a_known_uniform_sequence() {
        // 0..=100 step 1 (101 samples): p50 must be exactly 50, p10
        // exactly 10.0, matching linear-interpolation-rank arithmetic by
        // construction (a sequence with no interpolation ambiguity).
        let samples: Vec<f64> = (0..=100).map(|i| i as f64).collect();
        let d = Distribution::compute(&samples);
        assert_eq!(d.n, 101);
        assert_eq!(d.min, 0.0);
        assert_eq!(d.max, 100.0);
        assert_eq!(d.p50, 50.0);
        assert_eq!(d.p10, 10.0);
        assert_eq!(d.p90, 90.0);
        assert_eq!(d.mean, 50.0);
    }

    #[test]
    fn single_sample_distribution_has_zero_spread() {
        let d = Distribution::compute(&[42.0]);
        assert_eq!(d.n, 1);
        assert_eq!(d.mean, 42.0);
        assert_eq!(d.stddev, 0.0);
        assert_eq!(d.p50, 42.0);
    }

    #[test]
    #[should_panic(expected = "at least one sample")]
    fn empty_samples_panics_rather_than_lying() {
        Distribution::compute(&[]);
    }

    #[test]
    fn stddev_matches_a_hand_computed_value() {
        // {2, 4, 4, 4, 5, 5, 7, 9}: well-known textbook example, sample
        // stddev (n-1 denominator) = 2.13809...
        let samples = [2.0, 4.0, 4.0, 4.0, 5.0, 5.0, 7.0, 9.0];
        let d = Distribution::compute(&samples);
        assert!((d.mean - 5.0).abs() < 1e-9);
        assert!((d.stddev - 2.138_089_935_299_395).abs() < 1e-9);
    }
}
