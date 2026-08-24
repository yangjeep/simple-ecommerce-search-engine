//! Issue #47 Phase B: proposal-model capability/cost frontier
//! (`docs/experiments/ISSUE47_PROTOCOL.md` Phase B Addendum). Loads
//! frozen 5-draw pools for three model tiers (opus-5 strong, sonnet-5
//! mid -- reusing Phase A's own held-out draws verbatim -- and
//! haiku-4.5 small), computes B1-B5 per real key using the exact same
//! frozen `e2d_controller::run_controller`/`e2c_canonicalizer::canonicalize`
//! Phase A already adversarially reviewed and fixed, and reports the
//! Phase B GO-gate criteria plus the "preventing a fake cascade win"
//! breakdown Issue #47's own text requires.
//!
//! Usage: `e2d_phase_b_eval [out.json]`

use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;

use issue42_eval::e2b_oracle::{automotive_oracle, wands_oracle};
use issue42_eval::e2b_schema::{Descriptor, LlmPassOutput, SemanticRole};
use issue42_eval::e2b_validator::wands_query_texts;
use issue42_eval::e2b_workload::{automotive_unified_stats, load_wands_feed, UnifiedFieldStats};
use issue42_eval::e2c_canonicalizer::canonicalize;
use issue42_eval::e2c_metrics::{
    group_by_real_key, retrieval_significant_recall, unsafe_accepted_keys, StabilityCounts,
};
use issue42_eval::e2c_schema::{CandidateDescriptor, CanonicalOutcome};
use issue42_eval::e2d_controller::{cyclic_rotations, run_controller, ControllerTrace};
use issue42_eval::e2d_metrics::{abstention_rate, DepthStats};

/// `tier_prefix` is inserted between `llm_proposals_` and the config
/// name: `""` reuses Phase A's own sonnet-5 held-out files verbatim
/// (`e2d_llm_proposals_wands_baseline_run1.json`); `"opus_"`/`"haiku_"`
/// load this checkpoint's own new Phase B draws
/// (`e2d_llm_proposals_opus_wands_baseline_run1.json`).
fn load_pool(tier_prefix: &str, config: &str) -> Vec<LlmPassOutput> {
    let mut pool = Vec::new();
    for run in 1..=5u32 {
        let path =
            format!("dataset_cache/export/e2d_llm_proposals_{tier_prefix}{config}_run{run}.json");
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

type PromotedTriple = (
    SemanticRole,
    issue42_eval::e2b_schema::PhysicalPrimitive,
    issue42_eval::e2b_schema::Scope,
);

fn promoted_triple(outcome: &CanonicalOutcome) -> Option<PromotedTriple> {
    outcome
        .promoted()
        .map(|d| (d.semantic_role, d.canonical_physical_primitive, d.scope))
}

fn oracle_by_key() -> BTreeMap<String, SemanticRole> {
    let mut oracle: BTreeMap<String, SemanticRole> = BTreeMap::new();
    for d in wands_oracle().into_iter().chain(automotive_oracle()) {
        oracle.insert(d.key, d.semantic_role);
    }
    oracle
}

fn oracle_all() -> Vec<Descriptor> {
    wands_oracle()
        .into_iter()
        .chain(automotive_oracle())
        .collect()
}

struct KeyPools {
    real_key: String,
    opus: Vec<(u32, CandidateDescriptor)>,
    sonnet: Vec<(u32, CandidateDescriptor)>,
    haiku: Vec<(u32, CandidateDescriptor)>,
    stats: UnifiedFieldStats,
}

fn ordered_pool(
    config: &str,
    tier_prefix: &str,
    stats_by_key: &BTreeMap<String, UnifiedFieldStats>,
) -> BTreeMap<String, Vec<(u32, CandidateDescriptor)>> {
    let empty: BTreeMap<String, String> = BTreeMap::new();
    let runs = load_pool(tier_prefix, config);
    let grouped = group_by_real_key(config, &runs, &empty, &empty);
    grouped
        .into_iter()
        .filter(|(k, v)| v.len() == 5 && stats_by_key.contains_key(k))
        .map(|(k, mut v)| {
            v.sort_by_key(|(run_index, _)| *run_index);
            (k, v)
        })
        .collect()
}

fn load_all_pools(
    config: &str,
    stats_by_key: &BTreeMap<String, UnifiedFieldStats>,
) -> Vec<KeyPools> {
    let opus = ordered_pool(config, "opus_", stats_by_key);
    let sonnet = ordered_pool(config, "", stats_by_key);
    let haiku = ordered_pool(config, "haiku_", stats_by_key);

    let mut keys: BTreeSet<String> = BTreeSet::new();
    keys.extend(opus.keys().cloned());
    keys.extend(sonnet.keys().cloned());
    keys.extend(haiku.keys().cloned());

    keys.into_iter()
        .filter_map(|k| {
            let o = opus.get(&k)?.clone();
            let s = sonnet.get(&k)?.clone();
            let h = haiku.get(&k)?.clone();
            let stats = stats_by_key.get(&k)?.clone();
            Some(KeyPools {
                real_key: k,
                opus: o,
                sonnet: s,
                haiku: h,
                stats,
            })
        })
        .collect()
}

struct KeyResult {
    real_key: String,
    b1: CanonicalOutcome,
    b1_leave_one_out: Vec<CanonicalOutcome>,
    b2_rotations: Vec<ControllerTrace>,
    b3_rotations: Vec<ControllerTrace>,
    b4_rotations: Vec<ControllerTrace>,
    /// One cascade result per rotation index (haiku rotation i paired
    /// with opus rotation i) -- (final_outcome, escalated, tier_used_tokens_estimate_marker).
    b5_rotations: Vec<(CanonicalOutcome, bool)>,
}

fn evaluate_key(kp: &KeyPools, wands_queries: &[String]) -> KeyResult {
    let b1 = canonicalize(
        &kp.opus,
        &kp.real_key,
        &kp.stats,
        wands_queries,
        false,
        false,
    );
    // Reuses the same shared leave-one-out helper Phase A's own binary
    // uses (never hand-rolled a second time), matching this repo's own
    // "do not independently reimplement... logic that could silently
    // drift" discipline the Phase A adversarial review already enforced
    // elsewhere in this checkpoint.
    let b1_leave_one_out = issue42_eval::e2c_metrics::leave_one_out_outcomes(
        issue42_eval::e2c_metrics::Treatment::C,
        &kp.opus,
        &kp.real_key,
        Some(&kp.stats),
        wands_queries,
        false,
    );

    let opus_rotations = cyclic_rotations(&kp.opus);
    let sonnet_rotations = cyclic_rotations(&kp.sonnet);
    let haiku_rotations = cyclic_rotations(&kp.haiku);

    let b2_rotations: Vec<ControllerTrace> = opus_rotations
        .iter()
        .map(|r| run_controller(r, &kp.real_key, &kp.stats, wands_queries, false, false))
        .collect();
    let b3_rotations: Vec<ControllerTrace> = sonnet_rotations
        .iter()
        .map(|r| run_controller(r, &kp.real_key, &kp.stats, wands_queries, false, false))
        .collect();
    let b4_rotations: Vec<ControllerTrace> = haiku_rotations
        .iter()
        .map(|r| run_controller(r, &kp.real_key, &kp.stats, wands_queries, false, false))
        .collect();

    // B5 cascade: per rotation index, run haiku's own controller first;
    // escalate to opus's own controller (matched rotation index) only
    // if haiku did not produce a genuinely certified result.
    let b5_rotations: Vec<(CanonicalOutcome, bool)> = (0..5)
        .map(|i| {
            let cheap = &b4_rotations[i];
            if cheap.final_outcome.is_promoted() && cheap.certified_robust_at_stop {
                (cheap.final_outcome.clone(), false)
            } else {
                (b2_rotations[i].final_outcome.clone(), true)
            }
        })
        .collect();

    KeyResult {
        real_key: kp.real_key.clone(),
        b1,
        b1_leave_one_out,
        b2_rotations,
        b3_rotations,
        b4_rotations,
        b5_rotations,
    }
}

struct Summary {
    n_keys: usize,
    n_promoted: usize,
    depth_mean: f64,
    unsafe_count: usize,
    unsafe_keys: Vec<String>,
    abstention_pct: f64,
    role_stability_pct: f64,
    primitive_stability_pct: f64,
    full_stability_pct: f64,
    rs_recall_pct: f64,
    promoted_role_by_key: BTreeMap<String, SemanticRole>,
}

fn summarize(
    outcomes_primary: &[CanonicalOutcome],
    stability: &StabilityCounts,
    depths: &[u32],
    real_keys: &[String],
    oracle: &BTreeMap<String, SemanticRole>,
    oracle_all_descriptors: &[Descriptor],
) -> Summary {
    let mut promoted_keys_and_roles: Vec<(String, SemanticRole)> = Vec::new();
    let mut promoted_role_by_key = BTreeMap::new();
    for (k, o) in real_keys.iter().zip(outcomes_primary.iter()) {
        if let Some(role) = promoted_role(o) {
            promoted_keys_and_roles.push((k.clone(), role));
            promoted_role_by_key.insert(k.clone(), role);
        }
    }
    let unsafe_keys = unsafe_accepted_keys(&promoted_keys_and_roles, oracle);
    let promoted_set: BTreeSet<String> = promoted_keys_and_roles
        .iter()
        .map(|(k, _)| k.clone())
        .collect();
    let mut depth = DepthStats::default();
    for &d in depths {
        depth.push(d);
    }
    Summary {
        n_keys: real_keys.len(),
        n_promoted: promoted_keys_and_roles.len(),
        depth_mean: depth.mean(),
        unsafe_count: unsafe_keys.len(),
        unsafe_keys,
        abstention_pct: abstention_rate(outcomes_primary) * 100.0,
        role_stability_pct: StabilityCounts::rate(stability.role_agree, stability.total_pairs)
            * 100.0,
        primitive_stability_pct: StabilityCounts::rate(
            stability.primitive_agree,
            stability.total_pairs,
        ) * 100.0,
        full_stability_pct: StabilityCounts::rate(stability.full_agree, stability.total_pairs)
            * 100.0,
        rs_recall_pct: retrieval_significant_recall(&promoted_set, oracle_all_descriptors) * 100.0,
        promoted_role_by_key,
    }
}

fn summary_json(label: &str, s: &Summary) -> serde_json::Value {
    serde_json::json!({
        "label": label,
        "n_keys": s.n_keys,
        "n_promoted": s.n_promoted,
        "depth_mean": s.depth_mean,
        "unsafe_accepted_count": s.unsafe_count,
        "unsafe_accepted_keys": s.unsafe_keys,
        "abstention_rate_pct": s.abstention_pct,
        "role_stability_pct": s.role_stability_pct,
        "primitive_stability_pct": s.primitive_stability_pct,
        "full_stability_pct": s.full_stability_pct,
        "retrieval_significant_recall_pct": s.rs_recall_pct,
        "promoted_role_by_key": s.promoted_role_by_key,
    })
}

fn stability_from_rotations(traces_per_key: &[&Vec<ControllerTrace>]) -> StabilityCounts {
    let mut agg = StabilityCounts::default();
    for traces in traces_per_key {
        let outcomes: Vec<CanonicalOutcome> =
            traces.iter().map(|t| t.final_outcome.clone()).collect();
        agg.add(&issue42_eval::e2c_metrics::pairwise_stability(&outcomes));
    }
    agg
}

fn main() {
    let out_path = env::args().nth(1);

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

    let configs = ["wands_baseline", "automotive"];
    let mut all_results: Vec<KeyResult> = Vec::new();
    let mut n_keys_missing_a_tier = 0usize;

    for &config in &configs {
        let stats_by_key = if config == "automotive" {
            automotive_stats.clone()
        } else {
            wands_stats.clone()
        };
        let pools = load_all_pools(config, &stats_by_key);
        for kp in &pools {
            all_results.push(evaluate_key(kp, &wands_queries));
        }
        // Diagnostic: how many keys had a draw in at least one tier but
        // not all three (would be silently dropped by load_all_pools'
        // own intersection-by-key join) -- reported, never silently
        // hidden, per this repo's own "no silent caps" discipline.
        let opus_keys: BTreeSet<String> = ordered_pool(config, "opus_", &stats_by_key)
            .into_keys()
            .collect();
        let sonnet_keys: BTreeSet<String> = ordered_pool(config, "", &stats_by_key)
            .into_keys()
            .collect();
        let haiku_keys: BTreeSet<String> = ordered_pool(config, "haiku_", &stats_by_key)
            .into_keys()
            .collect();
        let union: BTreeSet<String> = opus_keys
            .union(&sonnet_keys)
            .cloned()
            .collect::<BTreeSet<_>>()
            .union(&haiku_keys)
            .cloned()
            .collect();
        n_keys_missing_a_tier += union.len().saturating_sub(pools.len());
    }

    if n_keys_missing_a_tier > 0 {
        eprintln!(
            "warning: {n_keys_missing_a_tier} keys had a draw in at least one tier but not all three -- dropped from every Phase B treatment"
        );
    }

    let real_keys: Vec<String> = all_results.iter().map(|r| r.real_key.clone()).collect();

    // B1: fixed-5 opus ensemble.
    let b1_outcomes: Vec<CanonicalOutcome> = all_results.iter().map(|r| r.b1.clone()).collect();
    let b1_stability = {
        let mut agg = StabilityCounts::default();
        for r in &all_results {
            agg.add(&issue42_eval::e2c_metrics::pairwise_stability(
                &r.b1_leave_one_out,
            ));
        }
        agg
    };
    let b1_depths: Vec<u32> = vec![5; all_results.len()];
    let b1_summary = summarize(
        &b1_outcomes,
        &b1_stability,
        &b1_depths,
        &real_keys,
        &oracle,
        &oracle_descriptors,
    );

    // B2/B3/B4: primary-rotation (rotation index 0) outcome + full
    // rotation-based stability, matching Phase A's own methodology.
    let make_summary = |pick: fn(&KeyResult) -> &Vec<ControllerTrace>| {
        let outcomes: Vec<CanonicalOutcome> = all_results
            .iter()
            .map(|r| pick(r)[0].final_outcome.clone())
            .collect();
        let depths: Vec<u32> = all_results.iter().map(|r| pick(r)[0].n_used).collect();
        let traces_per_key: Vec<&Vec<ControllerTrace>> = all_results.iter().map(pick).collect();
        let stability = stability_from_rotations(&traces_per_key);
        summarize(
            &outcomes,
            &stability,
            &depths,
            &real_keys,
            &oracle,
            &oracle_descriptors,
        )
    };
    let b2_summary = make_summary(|r| &r.b2_rotations);
    let b3_summary = make_summary(|r| &r.b3_rotations);
    let b4_summary = make_summary(|r| &r.b4_rotations);

    // B5 cascade: primary rotation for headline outcome; escalation
    // stats aggregated across ALL 5 rotations (more robust sample of
    // "how often does this cascade escalate" than one rotation alone).
    let b5_outcomes: Vec<CanonicalOutcome> = all_results
        .iter()
        .map(|r| r.b5_rotations[0].0.clone())
        .collect();
    let b5_stability = {
        let mut agg = StabilityCounts::default();
        for r in &all_results {
            let outcomes: Vec<CanonicalOutcome> =
                r.b5_rotations.iter().map(|(o, _)| o.clone()).collect();
            agg.add(&issue42_eval::e2c_metrics::pairwise_stability(&outcomes));
        }
        agg
    };
    let b5_depths: Vec<u32> = all_results
        .iter()
        .map(|r| {
            if r.b5_rotations[0].1 {
                r.b2_rotations[0].n_used
            } else {
                r.b4_rotations[0].n_used
            }
        })
        .collect();
    let b5_summary = summarize(
        &b5_outcomes,
        &b5_stability,
        &b5_depths,
        &real_keys,
        &oracle,
        &oracle_descriptors,
    );

    // Escalation accounting (Issue #47's own "preventing a fake cascade
    // win" list): fraction of keys/rotations escalated, overall and
    // among retrieval-significant keys; quality split by
    // escalated-vs-not.
    let rs_keys: BTreeSet<String> = oracle_descriptors
        .iter()
        .filter(|d| {
            matches!(
                d.retrieval_significance,
                issue42_eval::e2b_schema::Significance::RetrievalSignificant
            )
        })
        .map(|d| d.key.clone())
        .collect();

    let total_rotation_decisions = all_results.len() * 5;
    let escalated_rotation_decisions: usize = all_results
        .iter()
        .map(|r| r.b5_rotations.iter().filter(|(_, esc)| *esc).count())
        .sum();
    let escalation_rate_pct = if total_rotation_decisions == 0 {
        0.0
    } else {
        (escalated_rotation_decisions as f64 / total_rotation_decisions as f64) * 100.0
    };

    let rs_total: usize = all_results
        .iter()
        .filter(|r| rs_keys.contains(&r.real_key))
        .count()
        * 5;
    let rs_escalated: usize = all_results
        .iter()
        .filter(|r| rs_keys.contains(&r.real_key))
        .map(|r| r.b5_rotations.iter().filter(|(_, esc)| *esc).count())
        .sum();
    let rs_escalation_rate_pct = if rs_total == 0 {
        0.0
    } else {
        (rs_escalated as f64 / rs_total as f64) * 100.0
    };

    let mut oracle_disagreements_escalated = 0usize;
    let mut oracle_disagreements_not_escalated = 0usize;
    let mut n_escalated_keys = 0usize;
    let mut n_not_escalated_keys = 0usize;
    for r in &all_results {
        let (outcome, escalated) = &r.b5_rotations[0];
        if *escalated {
            n_escalated_keys += 1;
        } else {
            n_not_escalated_keys += 1;
        }
        if let Some(role) = promoted_role(outcome) {
            let mismatched = oracle.get(&r.real_key).map(|o| *o != role).unwrap_or(false);
            if mismatched {
                if *escalated {
                    oracle_disagreements_escalated += 1;
                } else {
                    oracle_disagreements_not_escalated += 1;
                }
            }
        }
    }

    // Cross-tier agreement vs B2 (opus reference) -- a direct answer to
    // "how often does model capability change the final answer,"
    // reported even though it is not one of Issue #47's own 8 named
    // criteria, because it is the most direct evidence for this issue's
    // own central question.
    let mut b3_vs_b2_agree = 0usize;
    let mut b4_vs_b2_agree = 0usize;
    let mut b5_vs_b2_agree = 0usize;
    for r in &all_results {
        let b2_triple = promoted_triple(&r.b2_rotations[0].final_outcome);
        if promoted_triple(&r.b3_rotations[0].final_outcome) == b2_triple {
            b3_vs_b2_agree += 1;
        }
        if promoted_triple(&r.b4_rotations[0].final_outcome) == b2_triple {
            b4_vs_b2_agree += 1;
        }
        if promoted_triple(&r.b5_rotations[0].0) == b2_triple {
            b5_vs_b2_agree += 1;
        }
    }
    let n = all_results.len().max(1) as f64;

    let combined = serde_json::json!({
        "experiment_id": "I47-E2d-phase-b-capability-cost-frontier",
        "baseline_sha": "20db66a0016176b3c16c1566c4e0796584f5e243",
        "n_total_keys": all_results.len(),
        "n_keys_missing_a_tier_dropped": n_keys_missing_a_tier,
        "b1_strong_fixed5": summary_json("B1_opus_fixed5", &b1_summary),
        "b2_strong_adaptive": summary_json("B2_opus_adaptive", &b2_summary),
        "b3_mid_adaptive": summary_json("B3_sonnet_adaptive_reused_from_phase_a", &b3_summary),
        "b4_small_adaptive": summary_json("B4_haiku_adaptive", &b4_summary),
        "b5_cascade": summary_json("B5_cascade_haiku_then_opus", &b5_summary),
        "cascade_escalation": {
            "rotation_decisions_total": total_rotation_decisions,
            "rotation_decisions_escalated": escalated_rotation_decisions,
            "escalation_rate_pct": escalation_rate_pct,
            "retrieval_significant_rotation_decisions_total": rs_total,
            "retrieval_significant_rotation_decisions_escalated": rs_escalated,
            "retrieval_significant_escalation_rate_pct": rs_escalation_rate_pct,
            "n_keys_escalated_primary_rotation": n_escalated_keys,
            "n_keys_not_escalated_primary_rotation": n_not_escalated_keys,
            "oracle_disagreements_among_escalated": oracle_disagreements_escalated,
            "oracle_disagreements_among_not_escalated": oracle_disagreements_not_escalated,
        },
        "cross_tier_agreement_vs_b2_opus_reference": {
            "b3_sonnet_agrees_with_b2_pct": (b3_vs_b2_agree as f64 / n) * 100.0,
            "b4_haiku_agrees_with_b2_pct": (b4_vs_b2_agree as f64 / n) * 100.0,
            "b5_cascade_agrees_with_b2_pct": (b5_vs_b2_agree as f64 / n) * 100.0,
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
