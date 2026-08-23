//! Shared E2b pipeline: loading the 8 frozen LLM-proposal artifacts and
//! building Baselines 2 (LLM proposal, no validator) and 3 (LLM +
//! deterministic validator)'s headline per-real-key accepted-descriptor
//! maps. Factored out of `bin/e2b_feature_discovery_eval.rs` (where this
//! logic originally lived inline) so a second, independent consumer --
//! `bin/e2b_serving_overhead_eval.rs`, added for Issue #42's E2b serving-
//! overhead gate -- computes "what did the LLM+validator baseline
//! actually accept" through the exact same code path as the accuracy/GO-
//! gate evaluation, rather than a second, independently-written
//! reimplementation that could silently diverge from it (itself exactly
//! the kind of methodology risk Issue #42's own "do not trust the
//! experiment author" governance exists to catch). Behavior is
//! byte-for-byte unchanged by this extraction -- see this crate's own
//! `docs/experiments/ISSUE42_LOG.md`'s E2b-serving-contract-closure section
//! for the byte-identical reverification, and the "one disclosed
//! behavioral generalization" note below.
//!
//! **Disclosed generalization**: `build_baselines_2_and_3`'s repeated-run
//! agreement now compares every distinct PAIR of runs within a config
//! (`C(n,2)` pairs), not only "run 1 vs run 2" -- for `n=2` these are
//! identical (there is exactly one pair), so nothing changes for any
//! existing 2-run artifact; this is what lets the same function serve
//! the materially-larger-N stability re-run (Issue #42's own E2b
//! serving-contract-closure pass, item 3) without a second
//! implementation.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;

use crate::e2b_schema::{Descriptor, LlmPassOutput, PhysicalPrimitive, SemanticRole};
use crate::e2b_validator::cross_run_type_conflict;
use crate::e2b_workload::UnifiedFieldStats;

pub const EXPORT_DIR: &str = "dataset_cache/export";
pub const CONFIGS: &[&str] = &[
    "wands_baseline",
    "wands_anonymized",
    "wands_noisy",
    "automotive",
];
/// The two unperturbed configurations -- real key names visible, the
/// LLM's best fair shot at correct classification. Baselines 2/3's own
/// headline per-key maps (`no_validator_by_key`/`validated_by_key`) are
/// built ONLY from these two: a confirmed defect (found by a fresh
/// adversarial review) had them built from whichever of the 4 configs
/// happened to sort alphabetically first per real key -- for every one
/// of WANDS's 36 keys that was `wands_anonymized`, silently substituting
/// the hardest perturbation for the intended "real information visible"
/// baseline. The anonymized/noisy configs still fully participate in
/// `per_config_f1` (the degradation-under-perturbation analysis) and in
/// `stability_agreements` (repeated-run agreement across all 4
/// configurations) -- only the two headline per-key maps are restricted.
pub const CANONICAL_CONFIGS: &[&str] = &["wands_baseline", "automotive"];

pub fn export_path(config: &str, run: u32) -> String {
    format!("{EXPORT_DIR}/e2b_llm_proposals_{config}_run{run}.json")
}

pub fn load_llm_pass(config: &str, run: u32) -> Option<LlmPassOutput> {
    let path = export_path(config, run);
    let content = fs::read_to_string(&path).ok()?;
    match serde_json::from_str::<LlmPassOutput>(&content) {
        Ok(v) => Some(v),
        Err(e) => {
            eprintln!("warning: failed to parse {path}: {e}");
            None
        }
    }
}

/// Loads every available run (1 and 2) for every entry in `configs`,
/// skipping any config with zero artifacts on disk. Used by both the
/// accuracy/GO-gate binary (all 4 [`CONFIGS`]) and the stability re-run
/// tooling (which may pass a superset including newly-added run indices
/// beyond 2 -- see `load_llm_pass_range`).
pub fn load_all_runs(configs: &[&str]) -> BTreeMap<String, Vec<LlmPassOutput>> {
    let mut per_config_runs: BTreeMap<String, Vec<LlmPassOutput>> = BTreeMap::new();
    for &config in configs {
        let mut runs = Vec::new();
        for run in 1..=2 {
            if let Some(pass) = load_llm_pass(config, run) {
                runs.push(pass);
            }
        }
        if !runs.is_empty() {
            per_config_runs.insert(config.to_string(), runs);
        }
    }
    per_config_runs
}

pub fn resolve_real_key<'a>(
    config: &str,
    shown_key: &'a str,
    anon: &'a BTreeMap<String, String>,
    noisy: &'a BTreeMap<String, String>,
) -> &'a str {
    match config {
        "wands_anonymized" => anon.get(shown_key).map(String::as_str).unwrap_or(shown_key),
        "wands_noisy" => noisy
            .get(shown_key)
            .map(String::as_str)
            .unwrap_or(shown_key),
        _ => shown_key,
    }
}

pub fn macro_f1(
    predicted: &BTreeMap<String, SemanticRole>,
    oracle: &BTreeMap<String, SemanticRole>,
) -> f64 {
    let classes = [
        SemanticRole::Identifier,
        SemanticRole::Enum,
        SemanticRole::Numeric,
        SemanticRole::Boolean,
        SemanticRole::FreeText,
        SemanticRole::Relationship,
        SemanticRole::Ignore,
    ];
    let mut f1_sum = 0.0;
    let mut classes_present = 0;
    for class in classes {
        let tp = oracle
            .iter()
            .filter(|(k, &role)| role == class && predicted.get(*k) == Some(&class))
            .count();
        let predicted_positive = predicted.values().filter(|&&r| r == class).count();
        let actual_positive = oracle.values().filter(|&&r| r == class).count();
        if actual_positive == 0 {
            continue;
        }
        classes_present += 1;
        let precision = if predicted_positive == 0 {
            0.0
        } else {
            tp as f64 / predicted_positive as f64
        };
        let recall = tp as f64 / actual_positive as f64;
        let f1 = if precision + recall == 0.0 {
            0.0
        } else {
            2.0 * precision * recall / (precision + recall)
        };
        f1_sum += f1;
    }
    if classes_present == 0 {
        0.0
    } else {
        f1_sum / classes_present as f64
    }
}

pub fn is_structural(role: SemanticRole) -> bool {
    matches!(
        role,
        SemanticRole::Enum | SemanticRole::Numeric | SemanticRole::Boolean
    )
}

pub struct Baselines2And3 {
    pub no_validator_by_key: BTreeMap<String, Descriptor>,
    pub validated_by_key: BTreeMap<String, Descriptor>,
    pub stability_agreements: usize,
    pub stability_total: usize,
    pub per_config_f1: BTreeMap<String, f64>,
}

/// Builds Baseline 2 (LLM proposal, no validator) and Baseline 3 (LLM +
/// deterministic validator)'s headline per-real-key maps, plus the
/// cross-configuration diagnostics (repeated-run agreement,
/// per-configuration macro F1). See this module's own doc comment for
/// why this is a single, shared implementation rather than one per
/// consumer binary.
pub fn build_baselines_2_and_3(
    per_config_runs: &BTreeMap<String, Vec<LlmPassOutput>>,
    anon_mapping: &BTreeMap<String, String>,
    noisy_mapping: &BTreeMap<String, String>,
    all_unified: &BTreeMap<String, UnifiedFieldStats>,
    wands_queries_text: &[String],
    oracle_by_key: &BTreeMap<String, SemanticRole>,
) -> Baselines2And3 {
    let mut no_validator_by_key: BTreeMap<String, Descriptor> = BTreeMap::new();
    let mut validated_by_key: BTreeMap<String, Descriptor> = BTreeMap::new();
    let mut stability_agreements = 0usize;
    let mut stability_total = 0usize;
    let mut per_config_f1: BTreeMap<String, f64> = BTreeMap::new();
    let mut type_conflicted_keys: BTreeSet<String> = BTreeSet::new();

    for (config, runs) in per_config_runs {
        let is_canonical = CANONICAL_CONFIGS.contains(&config.as_str());

        // Repeated-run agreement (role + primitive): every distinct pair
        // of runs within this config, not just run1-vs-run2 -- the
        // natural generalization of the original 2-run pairwise
        // definition to N>2 runs (Issue #42's own E2b serving-contract
        // closure pass, item 3: "compute agreement exactly according to
        // the preregistered definition" -- the preregistered definition
        // IS pairwise; with N=2 runs this reduces to exactly the
        // original run1-vs-run2 comparison, byte-identical).
        for i in 0..runs.len() {
            for j in (i + 1)..runs.len() {
                let run_i: BTreeMap<&str, &Descriptor> = runs[i]
                    .descriptors
                    .iter()
                    .map(|d| (d.key.as_str(), d))
                    .collect();
                let run_j: BTreeMap<&str, &Descriptor> = runs[j]
                    .descriptors
                    .iter()
                    .map(|d| (d.key.as_str(), d))
                    .collect();
                for (k, d1) in &run_i {
                    if let Some(d2) = run_j.get(k) {
                        stability_total += 1;
                        if d1.semantic_role == d2.semantic_role
                            && d1.candidate_physical_primitive == d2.candidate_physical_primitive
                        {
                            stability_agreements += 1;
                        }
                        if is_canonical && cross_run_type_conflict(d1, d2) {
                            let real_key = resolve_real_key(config, k, anon_mapping, noisy_mapping);
                            type_conflicted_keys.insert(real_key.to_string());
                        }
                    }
                }
            }
        }

        let mut config_predicted_roles: BTreeMap<String, SemanticRole> = BTreeMap::new();
        for run in runs {
            for d in &run.descriptors {
                let real_key = resolve_real_key(config, &d.key, anon_mapping, noisy_mapping);
                let mut resolved = d.clone();
                resolved.real_key = Some(real_key.to_string());

                config_predicted_roles
                    .entry(real_key.to_string())
                    .or_insert(resolved.semantic_role);

                if !is_canonical {
                    continue;
                }

                no_validator_by_key
                    .entry(real_key.to_string())
                    .or_insert_with(|| resolved.clone());

                if let Some(stats) = all_unified.get(real_key) {
                    let validation =
                        crate::e2b_validator::validate(&resolved, stats, wands_queries_text);
                    validated_by_key
                        .entry(real_key.to_string())
                        .or_insert_with(|| {
                            if validation.accepted {
                                resolved.clone()
                            } else {
                                let mut abstained = resolved.clone();
                                abstained.abstain = true;
                                abstained.semantic_role = SemanticRole::Ignore;
                                abstained.candidate_physical_primitive = PhysicalPrimitive::None;
                                abstained
                            }
                        });
                }
            }
        }
        let config_f1 = macro_f1(&config_predicted_roles, oracle_by_key);
        per_config_f1.insert(config.clone(), config_f1);
    }

    for real_key in &type_conflicted_keys {
        if let Some(d) = validated_by_key.get_mut(real_key) {
            if !d.abstain {
                d.abstain = true;
                d.semantic_role = SemanticRole::Ignore;
                d.candidate_physical_primitive = PhysicalPrimitive::None;
            }
        }
    }

    Baselines2And3 {
        no_validator_by_key,
        validated_by_key,
        stability_agreements,
        stability_total,
        per_config_f1,
    }
}

/// The oracle's own accepted-structural (Enum/Numeric/Boolean,
/// non-abstain) descriptors for WANDS's 36-key sample specifically --
/// shared by any consumer that needs "what would the hand-authored
/// oracle compile," not only the accuracy/GO-gate binary.
pub fn oracle_wands_accepted() -> Vec<Descriptor> {
    crate::e2b_oracle::wands_oracle()
        .into_iter()
        .filter(|d| !d.abstain && is_structural(d.semantic_role))
        .collect()
}

/// The full E2b pipeline run end to end (loading the 8 frozen artifacts,
/// the committed key mappings, both real data sources' unified stats, and
/// running [`build_baselines_2_and_3`]), returning ONLY the LLM +
/// deterministic validator baseline's own accepted-structural descriptors
/// scoped to WANDS's 36-key sample -- i.e. exactly the descriptor set
/// `e2b_ingest::build_catalog` would materialize for Baseline 3 restricted
/// to WANDS, computed through the *same* code path
/// `bin/e2b_feature_discovery_eval.rs`'s own accuracy/GO-gate numbers use
/// (see this module's own doc comment for why that matters: a second,
/// independently-written "what did the validator accept" computation is
/// exactly the kind of silent-divergence risk Issue #42's own governance
/// exists to catch). `None` if no frozen LLM-proposal artifacts are found
/// under `dataset_cache/export/` yet.
pub fn validated_wands_accepted() -> Option<Vec<Descriptor>> {
    let per_config_runs = load_all_runs(CONFIGS);
    if per_config_runs.is_empty() {
        return None;
    }

    let (anon_mapping, noisy_mapping) = (
        crate::e2b_key_mapping::anonymized_mapping(),
        crate::e2b_key_mapping::noisy_mapping(),
    );

    let mut oracle_all: Vec<Descriptor> = Vec::new();
    oracle_all.extend(crate::e2b_oracle::wands_oracle());
    oracle_all.extend(crate::e2b_oracle::automotive_oracle());
    let oracle_by_key: BTreeMap<String, SemanticRole> = oracle_all
        .iter()
        .map(|d| (d.key.clone(), d.semantic_role))
        .collect();

    let wands_feed = crate::e2b_workload::load_wands_feed();
    let wands_unified: BTreeMap<String, UnifiedFieldStats> = wands_feed
        .stats
        .iter()
        .map(|(k, s)| (k.clone(), UnifiedFieldStats::from(s)))
        .collect();
    let automotive_unified = crate::e2b_workload::automotive_unified_stats(1500);
    let mut all_unified: BTreeMap<String, UnifiedFieldStats> = wands_unified.clone();
    all_unified.extend(automotive_unified);

    let wands_queries_text = crate::e2b_validator::wands_query_texts();

    let result = build_baselines_2_and_3(
        &per_config_runs,
        &anon_mapping,
        &noisy_mapping,
        &all_unified,
        &wands_queries_text,
        &oracle_by_key,
    );

    Some(
        result
            .validated_by_key
            .into_iter()
            .filter(|(k, _)| wands_unified.contains_key(k))
            .filter(|(_, d)| !d.abstain && is_structural(d.semantic_role))
            .map(|(_, d)| d)
            .collect(),
    )
}
