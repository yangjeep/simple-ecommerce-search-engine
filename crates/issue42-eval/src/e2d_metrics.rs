//! Issue #47 Phase A metrics: per-key adaptive-controller depth
//! distribution, abstention rate, stable-but-wrong rate, and the two
//! cost-accounting units `ISSUE47_PROTOCOL.md` section 10 requires (raw
//! batched-call count vs. per-key equivalent depth). Reuses
//! `e2c_metrics::pairwise_stability` directly for A2/A3's own
//! repeated-run (5-rotation) stability numbers -- it already operates on
//! `&[CanonicalOutcome]`, the same shape `e2d_controller::run_controller`
//! traces produce, so no reimplementation is needed.

use std::collections::BTreeMap;

use crate::e2b_schema::SemanticRole;
use crate::e2c_metrics::{pairwise_stability, StabilityCounts};
use crate::e2c_schema::CanonicalOutcome;
use crate::e2d_controller::ControllerTrace;

#[derive(Debug, Clone, Default)]
pub struct DepthStats {
    pub depths: Vec<u32>,
}

impl DepthStats {
    pub fn push(&mut self, n: u32) {
        self.depths.push(n);
    }

    pub fn mean(&self) -> f64 {
        if self.depths.is_empty() {
            return 0.0;
        }
        self.depths.iter().sum::<u32>() as f64 / self.depths.len() as f64
    }

    pub fn median(&self) -> f64 {
        percentile(&self.depths, 0.5)
    }

    pub fn p95(&self) -> f64 {
        percentile(&self.depths, 0.95)
    }
}

fn percentile(values: &[u32], p: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    let rank = (p * (sorted.len() as f64 - 1.0)).round() as usize;
    sorted[rank.min(sorted.len() - 1)] as f64
}

/// `1 - n_promoted/n_total`, matching `e2c_canonicalization_eval.rs`'s
/// own inline computation (`e2c_metrics.rs` has no standalone function
/// for this either -- this factors the same arithmetic into a reusable
/// function instead of a third copy-pasted inline computation).
pub fn abstention_rate(outcomes: &[CanonicalOutcome]) -> f64 {
    if outcomes.is_empty() {
        return 0.0;
    }
    let n_promoted = outcomes.iter().filter(|o| o.is_promoted()).count();
    1.0 - (n_promoted as f64 / outcomes.len() as f64)
}

/// Of the outcomes that are canonically *stable* (checked by the
/// caller, e.g. via `pairwise_stability`'s own full-agreement
/// accounting), how many disagree with the oracle's own hand-authored
/// role -- the same "stable-but-wrong" concept `ISSUE45_LOG.md` reports,
/// factored into a reusable function.
pub fn stable_but_wrong_count(
    stable_promoted_keys_and_roles: &[(String, SemanticRole)],
    oracle_by_key: &BTreeMap<String, SemanticRole>,
) -> usize {
    stable_promoted_keys_and_roles
        .iter()
        .filter(|(k, role)| {
            oracle_by_key
                .get(k)
                .map(|oracle_role| oracle_role != role)
                .unwrap_or(false)
        })
        .count()
}

/// A2/A3's own repeated-run stability, computed exactly like A1's
/// leave-one-out design but over the 5 cyclic-rotation traces
/// (`ISSUE47_PROTOCOL.md` section 8) instead of 5 leave-one-out draws --
/// same `C(5,2)=10` pairwise count, same underlying
/// `e2c_metrics::pairwise_stability` function.
pub fn rotation_stability(traces: &[ControllerTrace]) -> StabilityCounts {
    let outcomes: Vec<CanonicalOutcome> = traces.iter().map(|t| t.final_outcome.clone()).collect();
    pairwise_stability(&outcomes)
}

/// Cost accounting (`ISSUE47_PROTOCOL.md` section 10): raw batched-call
/// count per configuration is the max `n_used` across every key in that
/// configuration's traces (a straggler key forces another full-config
/// draw); per-key equivalent depth is each key's own `n_used`, reported
/// separately.
pub fn raw_batched_call_count(n_used_per_key: &[u32]) -> u32 {
    n_used_per_key.iter().copied().max().unwrap_or(0)
}

/// Per-key token estimate: `total_batch_tokens_for_that_call /
/// n_keys_in_batch`, summed over the calls actually used for a key's own
/// resolved depth (`ISSUE47_PROTOCOL.md` section 10, unit 2).
/// `tokens_per_call` is the real measured token count for each of the
/// pool's draws, in the same order as the pool; `n_used` is how many of
/// them this key actually needed.
pub fn per_key_token_estimate(tokens_per_call: &[u64], n_keys_in_batch: usize, n_used: u32) -> f64 {
    if n_keys_in_batch == 0 {
        return 0.0;
    }
    tokens_per_call
        .iter()
        .take(n_used as usize)
        .map(|&t| t as f64 / n_keys_in_batch as f64)
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn depth_stats_mean_median_p95() {
        let mut d = DepthStats::default();
        for n in [1, 1, 1, 2, 5] {
            d.push(n);
        }
        assert!((d.mean() - 2.0).abs() < 1e-9);
        assert_eq!(d.median(), 1.0);
        assert_eq!(d.p95(), 5.0);
    }

    #[test]
    fn raw_batched_call_count_is_the_max_not_the_sum() {
        assert_eq!(raw_batched_call_count(&[1, 1, 3, 2]), 3);
        assert_eq!(raw_batched_call_count(&[]), 0);
    }

    #[test]
    fn per_key_token_estimate_scales_by_batch_size_and_depth_used() {
        let tokens = vec![3600, 3600, 3600, 3600, 3600];
        // 36-key wands_baseline batch, key needed only the first 2 draws.
        let estimate = per_key_token_estimate(&tokens, 36, 2);
        assert!((estimate - 200.0).abs() < 1e-9);
    }

    #[test]
    fn stable_but_wrong_counts_only_real_oracle_disagreements() {
        let oracle: BTreeMap<String, SemanticRole> = [
            ("a".to_string(), SemanticRole::Enum),
            ("b".to_string(), SemanticRole::Numeric),
        ]
        .into_iter()
        .collect();
        let stable = vec![
            ("a".to_string(), SemanticRole::Enum),    // agrees
            ("b".to_string(), SemanticRole::Boolean), // disagrees
            ("c".to_string(), SemanticRole::Enum),    // not in oracle, ignored
        ];
        assert_eq!(stable_but_wrong_count(&stable, &oracle), 1);
    }
}
