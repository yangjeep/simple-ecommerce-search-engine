//! Issue #47 Phase A: reproducible entry point
//! (`docs/experiments/ISSUE47_PROTOCOL.md` sections 9-13). Loads a
//! frozen pool of 5 independent `LlmPassOutput` draws per configuration,
//! computes A0/A1/A2/A3 per real key, and reports Phase A's own GO-gate
//! criteria (section 11).
//!
//! Two modes, selected by the first CLI argument:
//! - `calibration`: loads the already-frozen, already-analyzed E2b
//!   artifacts (`dataset_cache/export/e2b_llm_proposals_automotive_run{1..5}.json`)
//!   as a pre-held-out sanity/consistency check (section 7) -- zero new
//!   calls, not part of any GO-gate number.
//! - `heldout` (default): loads the genuinely new E2d artifacts
//!   (`dataset_cache/export/e2d_llm_proposals_{wands_baseline,automotive}_run{1..5}.json`),
//!   the only source for every reported Phase A GO-gate number.
//!
//! Usage: `e2d_adaptive_consensus_eval [calibration|heldout] [out.json]`

use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;

use issue42_eval::e2b_oracle::{automotive_oracle, wands_oracle};
use issue42_eval::e2b_schema::{Descriptor, LlmPassOutput, SemanticRole};
use issue42_eval::e2b_validator::wands_query_texts;
use issue42_eval::e2b_workload::{automotive_unified_stats, load_wands_feed, UnifiedFieldStats};
use issue42_eval::e2c_canonicalizer::canonicalize;
use issue42_eval::e2c_metrics::{
    group_by_real_key, leave_one_out_outcomes, pairwise_stability, retrieval_significant_recall,
    Treatment,
};
use issue42_eval::e2c_schema::CanonicalOutcome;
use issue42_eval::e2d_controller::{cyclic_rotations, run_controller, ControllerTrace};
use issue42_eval::e2d_metrics::{
    abstention_rate, raw_batched_call_count, rotation_stability, DepthStats,
};

fn load_pool(prefix: &str, config: &str) -> Vec<LlmPassOutput> {
    let mut pool = Vec::new();
    for run in 1..=5u32 {
        let path = format!("dataset_cache/export/{prefix}_llm_proposals_{config}_run{run}.json");
        match fs::read_to_string(&path) {
            Ok(content) => match serde_json::from_str::<LlmPassOutput>(&content) {
                Ok(v) => pool.push(v),
                Err(e) => eprintln!("warning: failed to parse {path}: {e}"),
            },
            Err(_) => eprintln!("warning: missing {path}"),
        }
    }
    pool
}

fn promoted_role(outcome: &CanonicalOutcome) -> Option<SemanticRole> {
    outcome.promoted().map(|d| d.semantic_role)
}

/// Phase A GO-gate criterion 1 (`ISSUE47_PROTOCOL.md` section 11):
/// zero confirmed unsafe accepted structural classifications. Reuses
/// `e2c_metrics::unsafe_accepted_count` -- E2b/E2c's own corrected
/// definition (a promoted descriptor whose oracle-confirmed real role is
/// Identifier or Relationship) -- rather than a generic oracle-role
/// mismatch count, since disagreement and unsafety are not the same
/// thing (e.g. `productwarranty`/`heat_range` in E2c's own precedent are
/// disagreements, not unsafe promotions).
struct SafetyBreakdown {
    unsafe_count: usize,
    unsafe_keys: Vec<String>,
    disagreement_count: usize,
    disagreeing_keys: Vec<String>,
}

fn safety_breakdown(
    promoted_keys_and_roles: &[(String, SemanticRole)],
    oracle: &BTreeMap<String, SemanticRole>,
) -> SafetyBreakdown {
    let unsafe_count =
        issue42_eval::e2c_metrics::unsafe_accepted_count(promoted_keys_and_roles, oracle);
    // Mirrors e2c_metrics::unsafe_accepted_count's own (fixed) definition
    // exactly -- oracle says Identifier/Relationship AND the promoted
    // role does not match -- so this list is never inconsistent with
    // `unsafe_count` above.
    let unsafe_keys: Vec<String> = promoted_keys_and_roles
        .iter()
        .filter(|(k, role)| {
            matches!(
                oracle.get(k),
                Some(SemanticRole::Identifier) | Some(SemanticRole::Relationship)
            ) && oracle.get(k) != Some(role)
        })
        .map(|(k, _)| k.clone())
        .collect();
    let disagreeing_keys: Vec<String> = promoted_keys_and_roles
        .iter()
        .filter(|(k, role)| oracle.get(k).map(|o| o != role).unwrap_or(false))
        .map(|(k, _)| k.clone())
        .collect();
    SafetyBreakdown {
        unsafe_count,
        disagreement_count: disagreeing_keys.len(),
        unsafe_keys,
        disagreeing_keys,
    }
}

struct KeyResult {
    real_key: String,
    a0: CanonicalOutcome,
    a1_full: CanonicalOutcome,
    a1_leave_one_out: Vec<CanonicalOutcome>,
    a2_rotations: Vec<ControllerTrace>,
    a3_rotations: Vec<ControllerTrace>,
}

#[allow(clippy::too_many_arguments)]
fn evaluate_config(
    config: &str,
    runs: &[LlmPassOutput],
    stats_by_key: &BTreeMap<String, UnifiedFieldStats>,
    wands_queries: &[String],
) -> Vec<KeyResult> {
    let empty: BTreeMap<String, String> = BTreeMap::new();
    let grouped = group_by_real_key(config, runs, &empty, &empty);
    let mut results = Vec::new();
    for (real_key, draws) in grouped {
        if draws.len() < 5 {
            eprintln!(
                "skipping {real_key}: only {} runs available (need 5)",
                draws.len()
            );
            continue;
        }
        let stats = match stats_by_key.get(&real_key) {
            Some(s) => s,
            None => {
                eprintln!("skipping {real_key}: no UnifiedFieldStats");
                continue;
            }
        };
        // Draws are already in run-index order from group_by_real_key's
        // own iteration (run1..run5); sort explicitly to be certain.
        let mut ordered = draws.clone();
        ordered.sort_by_key(|(run_index, _)| *run_index);

        let a0 = canonicalize(
            &ordered[0..1],
            &real_key,
            stats,
            wands_queries,
            false,
            false,
        );
        let a1_full = canonicalize(&ordered, &real_key, stats, wands_queries, false, false);
        let a1_leave_one_out = leave_one_out_outcomes(
            Treatment::C,
            &ordered,
            &real_key,
            Some(stats),
            wands_queries,
            false,
        );

        let rotations = cyclic_rotations(&ordered);
        let a2_rotations: Vec<ControllerTrace> = rotations
            .iter()
            .map(|r| run_controller(r, &real_key, stats, wands_queries, false, false))
            .collect();
        let a3_rotations: Vec<ControllerTrace> = rotations
            .iter()
            .map(|r| run_controller(r, &real_key, stats, wands_queries, false, true))
            .collect();

        results.push(KeyResult {
            real_key,
            a0,
            a1_full,
            a1_leave_one_out,
            a2_rotations,
            a3_rotations,
        });
    }
    results
}

fn oracle_by_key() -> BTreeMap<String, SemanticRole> {
    let mut oracle: BTreeMap<String, SemanticRole> = BTreeMap::new();
    for d in wands_oracle().into_iter().chain(automotive_oracle()) {
        oracle.insert(d.key, d.semantic_role);
    }
    oracle
}

/// Full per-key promoted-role map -- transparency for adversarial review
/// (so "does treatment X's promoted set actually equal treatment Y's"
/// can be checked directly, key by key, not inferred from aggregate
/// counts alone) and the basis this checkpoint's own log uses to verify
/// A2/A3 promote an identical set to A1 on the held-out data before
/// inheriting A1's own end-to-end relevance result rather than
/// recomputing it independently.
fn promoted_role_map(
    promoted_keys_and_roles: &[(String, SemanticRole)],
) -> BTreeMap<String, SemanticRole> {
    promoted_keys_and_roles.iter().cloned().collect()
}

fn oracle_all() -> Vec<Descriptor> {
    wands_oracle()
        .into_iter()
        .chain(automotive_oracle())
        .collect()
}

fn summarize_treatment_a2_or_a3(
    label: &str,
    results: &[KeyResult],
    pick: impl Fn(&KeyResult) -> &Vec<ControllerTrace>,
    oracle: &BTreeMap<String, SemanticRole>,
    oracle_all: &[Descriptor],
) -> serde_json::Value {
    let mut depth = DepthStats::default();
    let mut n_used_per_key = Vec::new();
    let mut outcomes_primary_order = Vec::new();
    let mut promoted_keys_and_roles: Vec<(String, SemanticRole)> = Vec::new();
    let mut certified_count = 0usize;
    let mut agg_stability = issue42_eval::e2c_metrics::StabilityCounts::default();

    for r in results {
        let traces = pick(r);
        // "Primary order" trace = rotation index 0 (natural run order
        // 1,2,3,4,5), used for the headline per-key depth/outcome.
        let primary = &traces[0];
        depth.push(primary.n_used);
        n_used_per_key.push(primary.n_used);
        outcomes_primary_order.push(primary.final_outcome.clone());
        if primary.certified_robust_at_stop {
            certified_count += 1;
        }
        if let Some(role) = promoted_role(&primary.final_outcome) {
            promoted_keys_and_roles.push((r.real_key.clone(), role));
        }
        let stab = rotation_stability(traces);
        agg_stability.add(&stab);
    }

    let abstention = abstention_rate(&outcomes_primary_order);
    let mean_depth = depth.mean();
    let reduction_vs_fixed5 = 1.0 - (mean_depth / 5.0);
    let safety = safety_breakdown(&promoted_keys_and_roles, oracle);
    let promoted_set: BTreeSet<String> = promoted_keys_and_roles
        .iter()
        .map(|(k, _)| k.clone())
        .collect();
    let rs_recall = retrieval_significant_recall(&promoted_set, oracle_all);

    serde_json::json!({
        "label": label,
        "n_keys": results.len(),
        "depth_mean": mean_depth,
        "depth_median": depth.median(),
        "depth_p95": depth.p95(),
        "raw_batched_call_count": raw_batched_call_count(&n_used_per_key),
        "reduction_vs_fixed5_pct": reduction_vs_fixed5 * 100.0,
        "certified_robust_rate_pct": (certified_count as f64 / results.len().max(1) as f64) * 100.0,
        "abstention_rate_pct": abstention * 100.0,
        "role_stability_pct": issue42_eval::e2c_metrics::StabilityCounts::rate(agg_stability.role_agree, agg_stability.total_pairs) * 100.0,
        "primitive_stability_pct": issue42_eval::e2c_metrics::StabilityCounts::rate(agg_stability.primitive_agree, agg_stability.total_pairs) * 100.0,
        "full_stability_pct": issue42_eval::e2c_metrics::StabilityCounts::rate(agg_stability.full_agree, agg_stability.total_pairs) * 100.0,
        "unsafe_accepted_count": safety.unsafe_count,
        "unsafe_accepted_keys": safety.unsafe_keys,
        "oracle_disagreements_among_promoted": safety.disagreement_count,
        "oracle_disagreeing_keys": safety.disagreeing_keys,
        "retrieval_significant_recall_pct": rs_recall * 100.0,
        "n_promoted": promoted_keys_and_roles.len(),
        "promoted_role_by_key": promoted_role_map(&promoted_keys_and_roles),
    })
}

fn summarize_a1(
    results: &[KeyResult],
    oracle: &BTreeMap<String, SemanticRole>,
    oracle_all: &[Descriptor],
) -> serde_json::Value {
    let mut agg_stability = issue42_eval::e2c_metrics::StabilityCounts::default();
    let mut outcomes_full = Vec::new();
    let mut promoted_keys_and_roles: Vec<(String, SemanticRole)> = Vec::new();
    for r in results {
        let stab = pairwise_stability(&r.a1_leave_one_out);
        agg_stability.add(&stab);
        outcomes_full.push(r.a1_full.clone());
        if let Some(role) = promoted_role(&r.a1_full) {
            promoted_keys_and_roles.push((r.real_key.clone(), role));
        }
    }
    let abstention = abstention_rate(&outcomes_full);
    let safety = safety_breakdown(&promoted_keys_and_roles, oracle);
    let promoted_set: BTreeSet<String> = promoted_keys_and_roles
        .iter()
        .map(|(k, _)| k.clone())
        .collect();
    let rs_recall = retrieval_significant_recall(&promoted_set, oracle_all);
    serde_json::json!({
        "label": "A1_fixed5",
        "n_keys": results.len(),
        "depth_mean": 5.0,
        "raw_batched_call_count": 5,
        "reduction_vs_fixed5_pct": 0.0,
        "abstention_rate_pct": abstention * 100.0,
        "role_stability_pct": issue42_eval::e2c_metrics::StabilityCounts::rate(agg_stability.role_agree, agg_stability.total_pairs) * 100.0,
        "primitive_stability_pct": issue42_eval::e2c_metrics::StabilityCounts::rate(agg_stability.primitive_agree, agg_stability.total_pairs) * 100.0,
        "full_stability_pct": issue42_eval::e2c_metrics::StabilityCounts::rate(agg_stability.full_agree, agg_stability.total_pairs) * 100.0,
        "unsafe_accepted_count": safety.unsafe_count,
        "unsafe_accepted_keys": safety.unsafe_keys,
        "oracle_disagreements_among_promoted": safety.disagreement_count,
        "oracle_disagreeing_keys": safety.disagreeing_keys,
        "retrieval_significant_recall_pct": rs_recall * 100.0,
        "n_promoted": promoted_keys_and_roles.len(),
        "promoted_role_by_key": promoted_role_map(&promoted_keys_and_roles),
    })
}

fn summarize_a0(
    results: &[KeyResult],
    oracle: &BTreeMap<String, SemanticRole>,
    oracle_all: &[Descriptor],
) -> serde_json::Value {
    let mut outcomes = Vec::new();
    let mut promoted_keys_and_roles: Vec<(String, SemanticRole)> = Vec::new();
    for r in results {
        outcomes.push(r.a0.clone());
        if let Some(role) = promoted_role(&r.a0) {
            promoted_keys_and_roles.push((r.real_key.clone(), role));
        }
    }
    let abstention = abstention_rate(&outcomes);
    let safety = safety_breakdown(&promoted_keys_and_roles, oracle);
    let promoted_set: BTreeSet<String> = promoted_keys_and_roles
        .iter()
        .map(|(k, _)| k.clone())
        .collect();
    let rs_recall = retrieval_significant_recall(&promoted_set, oracle_all);
    serde_json::json!({
        "label": "A0_single",
        "n_keys": results.len(),
        "depth_mean": 1.0,
        "raw_batched_call_count": 1,
        "reduction_vs_fixed5_pct": 80.0,
        "abstention_rate_pct": abstention * 100.0,
        "unsafe_accepted_count": safety.unsafe_count,
        "unsafe_accepted_keys": safety.unsafe_keys,
        "oracle_disagreements_among_promoted": safety.disagreement_count,
        "oracle_disagreeing_keys": safety.disagreeing_keys,
        "retrieval_significant_recall_pct": rs_recall * 100.0,
        "n_promoted": promoted_keys_and_roles.len(),
        "promoted_role_by_key": promoted_role_map(&promoted_keys_and_roles),
    })
}

fn main() {
    let mode = env::args().nth(1).unwrap_or_else(|| "heldout".to_string());
    let out_path = env::args().nth(2);
    let prefix = match mode.as_str() {
        "calibration" => "e2b",
        "heldout" => "e2d",
        other => {
            eprintln!("unknown mode {other}, expected calibration|heldout");
            std::process::exit(1);
        }
    };

    let wands_feed = load_wands_feed();
    let wands_stats: BTreeMap<String, UnifiedFieldStats> = wands_feed
        .stats
        .iter()
        .map(|(k, s)| (k.clone(), UnifiedFieldStats::from(s)))
        .collect();
    let automotive_stats = automotive_unified_stats(1500);
    let wands_queries = wands_query_texts();
    let oracle = oracle_by_key();
    let oracle_descriptors = oracle_all();

    let configs: &[&str] = if mode == "calibration" {
        &["automotive"]
    } else {
        &["wands_baseline", "automotive"]
    };

    let mut all_results: Vec<KeyResult> = Vec::new();
    let mut per_config_summary = serde_json::Map::new();
    for &config in configs {
        let runs = load_pool(prefix, config);
        if runs.len() < 5 {
            eprintln!(
                "warning: {config} has only {} of 5 expected runs under prefix {prefix}",
                runs.len()
            );
        }
        let stats_by_key = if config == "automotive" {
            automotive_stats.clone()
        } else {
            wands_stats.clone()
        };
        let results = evaluate_config(config, &runs, &stats_by_key, &wands_queries);
        per_config_summary.insert(
            config.to_string(),
            serde_json::json!({
                "n_keys_evaluated": results.len(),
                "a0": summarize_a0(&results, &oracle, &oracle_descriptors),
                "a1": summarize_a1(&results, &oracle, &oracle_descriptors),
                "a2": summarize_treatment_a2_or_a3("A2_adaptive_C", &results, |r| &r.a2_rotations, &oracle, &oracle_descriptors),
                "a3": summarize_treatment_a2_or_a3("A3_conservative_D", &results, |r| &r.a3_rotations, &oracle, &oracle_descriptors),
            }),
        );
        all_results.extend(results);
    }

    let combined = serde_json::json!({
        "experiment_id": "I47-E2d-phase-a-adaptive-consensus",
        "mode": mode,
        "baseline_sha": "20db66a0016176b3c16c1566c4e0796584f5e243",
        "configs": configs,
        "n_total_keys": all_results.len(),
        "per_config": per_config_summary,
        "combined": {
            "a0": summarize_a0(&all_results, &oracle, &oracle_descriptors),
            "a1": summarize_a1(&all_results, &oracle, &oracle_descriptors),
            "a2": summarize_treatment_a2_or_a3("A2_adaptive_C", &all_results, |r| &r.a2_rotations, &oracle, &oracle_descriptors),
            "a3": summarize_treatment_a2_or_a3("A3_conservative_D", &all_results, |r| &r.a3_rotations, &oracle, &oracle_descriptors),
        },
    });

    let pretty = serde_json::to_string_pretty(&combined).unwrap();
    println!("{pretty}");
    if let Some(path) = out_path {
        if let Some(parent) = std::path::Path::new(&path).parent() {
            let _ = fs::create_dir_all(parent);
        }
        fs::write(&path, &pretty).unwrap_or_else(|e| panic!("write {path}: {e}"));
    }
}
