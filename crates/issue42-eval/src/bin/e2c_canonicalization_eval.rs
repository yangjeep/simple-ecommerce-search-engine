//! Issue #45's E2c: preregistered stability/safety/recall/abstention/
//! relevance measurement (`docs/experiments/ISSUE45_PROTOCOL.md`) --
//! GO-gate criteria 1-5 and 7. Criterion 6 (serving overhead) is
//! measured separately by `e2c_serving_overhead_eval`, matching E2b's
//! own precedent of splitting accuracy/stability from serving-latency
//! measurement into two binaries.
//!
//! Reuses the 20 already-frozen `dataset_cache/export/e2b_llm_proposals_*.json`
//! artifacts exactly as E2b left them -- no new LLM calls of any kind.
//!
//! Reproduction: `cargo build --release -p issue42-eval &&
//! ./target/release/e2c_canonicalization_eval [output_summary_json_path]`

use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;

use issue42_eval::e2b_ingest::{build_catalog, naive_constraints_for_query};
use issue42_eval::e2b_pipeline::{
    self, is_structural, oracle_wands_accepted, CANONICAL_CONFIGS, CONFIGS,
};
use issue42_eval::e2b_schema::{Descriptor, SemanticRole};
use issue42_eval::e2b_validator::wands_query_texts;
use issue42_eval::e2b_workload::{
    automotive_unified_stats, load_wands_feed, load_wands_labels, load_wands_queries,
    UnifiedFieldStats,
};
use issue42_eval::e2c_canonicalizer::canonicalize;
use issue42_eval::e2c_majority_vote::majority_vote;
use issue42_eval::e2c_metrics::{
    group_by_real_key, leave_one_out_outcomes, pairwise_stability, retrieval_significant_recall,
    unsafe_accepted_count, StabilityCounts, Treatment,
};
use issue42_eval::e2c_schema::CanonicalOutcome;

const BASELINE_SHA: &str = "d965b7444e1ae563707af987da1a55b98d939135";

/// Not observable for WANDS (`e2b_ingest::build_catalog`'s own doc
/// comment: exactly one Variant per Product) or automotive's own flat
/// generator as ingested here -- see `docs/experiments/ISSUE45_PROTOCOL.md`
/// R6.
const HAS_REAL_VARIANT_GROUPING: bool = false;

fn treatment_label(t: Treatment) -> &'static str {
    match t {
        Treatment::B => "B_majority_vote",
        Treatment::C => "C_canonicalizer",
        Treatment::D => "D_conservative",
    }
}

const RELEVANCE_K: usize = 10;

/// A minimal synthetic `e2b_schema::Descriptor` carrying only what
/// `e2b_ingest::build_catalog` and `naive_constraints_for_query` actually
/// read (`real_key`/`key`, `semantic_role`) -- lets GO-gate criterion 5
/// reuse E2b's own already-governed end-to-end relevance check exactly,
/// rather than writing a second, independently-implemented ingestion
/// path for `CanonicalDescriptor` (the same "do not trust a second,
/// independently-written computation" discipline
/// `docs/experiments/ISSUE42_LOG.md`'s own closure pass already applied
/// to `e2b_pipeline.rs`).
fn as_synthetic_e2b_descriptor(d: &issue42_eval::e2c_schema::CanonicalDescriptor) -> Descriptor {
    Descriptor {
        key: d.real_key.clone(),
        real_key: Some(d.real_key.clone()),
        semantic_role: d.semantic_role,
        value_type: d.value_type,
        scope: d.scope,
        supported_operators: d.supported_operators.clone(),
        aliases: d.aliases.clone(),
        relationship_semantics: None,
        retrieval_significance: d.retrieval_significance,
        candidate_physical_primitive: d.canonical_physical_primitive,
        confidence: d.confidence,
        evidence: d.decision_reasons.join("; "),
        abstain: false,
    }
}

struct EndToEndResult {
    ndcg: f64,
    recall: f64,
    n_scored: usize,
}

/// Byte-for-byte the same naive substring-matched check
/// `e2b_feature_discovery_eval.rs`'s own `end_to_end_ndcg_recall` uses
/// (same disclosed scope/reliability caveats: `e2b_ingest`'s own doc
/// comment), reused rather than reimplemented, applied here to
/// `accepted`'s own real keys instead of E2b's `Descriptor`s directly.
fn end_to_end_ndcg_recall(
    accepted: &[Descriptor],
    queries: &[issue42_eval::e2b_workload::WandsQuery],
    labels: &BTreeMap<String, BTreeMap<String, phase9_eval::wands_relevance::WandsLabel>>,
) -> EndToEndResult {
    let ingested = build_catalog(accepted);
    let mut ndcg_sum = 0.0;
    let mut recall_sum = 0.0;
    let mut n = 0usize;
    for query in queries {
        let Some(judged) = labels.get(&query.query_id) else {
            continue;
        };
        let constraints = naive_constraints_for_query(&query.text, &ingested.enum_like_values);
        if constraints.is_empty() {
            continue;
        }
        let matches = ingested.catalog.search(&constraints);
        let hits: Vec<String> = matches
            .iter()
            .filter_map(|(pid, _)| {
                ingested
                    .wands_id_to_product_id
                    .iter()
                    .find(|(_, v)| **v == *pid)
                    .map(|(k, _)| k.clone())
            })
            .take(RELEVANCE_K)
            .collect();
        let (ndcg, recall, _mrr) =
            phase9_eval::wands_relevance::ndcg_recall_mrr(&hits, judged, RELEVANCE_K);
        ndcg_sum += ndcg;
        recall_sum += recall;
        n += 1;
    }
    if n == 0 {
        EndToEndResult {
            ndcg: 0.0,
            recall: 0.0,
            n_scored: 0,
        }
    } else {
        EndToEndResult {
            ndcg: ndcg_sum / n as f64,
            recall: recall_sum / n as f64,
            n_scored: n,
        }
    }
}

fn main() {
    println!("=== Issue #45 E2c: canonicalization stability/safety/recall/abstention ===");
    println!("baseline_sha (PR #44 head this checkpoint builds on): {BASELINE_SHA}");

    let per_config_runs = e2b_pipeline::load_all_runs(CONFIGS);
    if per_config_runs.is_empty() {
        println!(
            "\nNo LLM proposal artifacts found under dataset_cache/export/ -- nothing to measure."
        );
        return;
    }
    for (config, runs) in &per_config_runs {
        println!("  {config}: {} runs loaded", runs.len());
    }

    let anon_mapping = issue42_eval::e2b_key_mapping::anonymized_mapping();
    let noisy_mapping = issue42_eval::e2b_key_mapping::noisy_mapping();

    let mut oracle_all: Vec<Descriptor> = Vec::new();
    oracle_all.extend(issue42_eval::e2b_oracle::wands_oracle());
    oracle_all.extend(issue42_eval::e2b_oracle::automotive_oracle());
    let oracle_by_key: BTreeMap<String, SemanticRole> = oracle_all
        .iter()
        .map(|d| (d.key.clone(), d.semantic_role))
        .collect();

    let wands_feed = load_wands_feed();
    let wands_unified: BTreeMap<String, UnifiedFieldStats> = wands_feed
        .stats
        .iter()
        .map(|(k, s)| (k.clone(), UnifiedFieldStats::from(s)))
        .collect();
    let automotive_unified = automotive_unified_stats(1500);
    let mut all_unified: BTreeMap<String, UnifiedFieldStats> = wands_unified.clone();
    all_unified.extend(automotive_unified);

    let wands_queries_text = wands_query_texts();

    // ==================== Stability (leave-one-out, all 4 configs) ====================
    println!("\n=== Stability (leave-one-out canonicalization, C(5,2)=10 pairs/config, matching E2b's own pairwise design) ===");

    let treatments = [Treatment::B, Treatment::C, Treatment::D];
    let mut stability_by_treatment: BTreeMap<&str, StabilityCounts> = BTreeMap::new();
    let mut stability_by_treatment_config: BTreeMap<(&str, String), StabilityCounts> =
        BTreeMap::new();
    // raw (Treatment A) stability, recomputed here to extend E2b's own
    // role+primitive-only metric with type/scope/full-descriptor -- see
    // docs/experiments/ISSUE45_PROTOCOL.md section 6.
    let mut raw_stability = StabilityCounts::default();
    let mut raw_stability_by_config: BTreeMap<String, StabilityCounts> = BTreeMap::new();
    let mut single_run_stability = StabilityCounts::default();

    for (config, runs) in &per_config_runs {
        let by_key = group_by_real_key(config, runs, &anon_mapping, &noisy_mapping);
        for (real_key, runs_for_key) in &by_key {
            if runs_for_key.len() < 2 {
                continue;
            }
            let stats = all_unified.get(real_key);

            // Raw (Treatment A extension): pairwise over the actual raw
            // runs directly (role/type/scope/primitive), not
            // leave-one-out.
            let raw_outcomes: Vec<CanonicalOutcome> = runs_for_key
                .iter()
                .map(|(idx, d)| {
                    CanonicalOutcome::Promoted(issue42_eval::e2c_schema::CanonicalDescriptor {
                        schema_version: 0,
                        real_key: real_key.clone(),
                        semantic_role: d.semantic_role,
                        value_type: d.value_type,
                        scope: d.scope,
                        supported_operators: d.supported_operators.clone(),
                        aliases: d.aliases.clone(),
                        retrieval_significance: d.retrieval_significance,
                        canonical_physical_primitive: d.candidate_physical_primitive,
                        confidence: d.confidence,
                        provenance: vec![],
                        decision_reasons: vec![format!("raw run {idx}, not canonicalized")],
                    })
                })
                .collect();
            let raw_r = pairwise_stability(&raw_outcomes);
            raw_stability.add(&raw_r);
            raw_stability_by_config
                .entry(config.clone())
                .or_default()
                .add(&raw_r);

            for &treatment in &treatments {
                let outcomes = leave_one_out_outcomes(
                    treatment,
                    runs_for_key,
                    real_key,
                    stats,
                    &wands_queries_text,
                    HAS_REAL_VARIANT_GROUPING,
                );
                let r = pairwise_stability(&outcomes);
                stability_by_treatment
                    .entry(treatment_label(treatment))
                    .or_default()
                    .add(&r);
                stability_by_treatment_config
                    .entry((treatment_label(treatment), config.clone()))
                    .or_default()
                    .add(&r);
            }

            // A stricter, adversarial-self-check design: leave-one-out
            // (4-of-5 runs feeding each canonicalization) is a WEAKER
            // test of instability-removal than it looks -- dropping one
            // of five runs rarely flips a plurality/majority vote, so
            // near-100% leave-one-out stability could partly reflect
            // that leniency rather than the rules genuinely resolving
            // disagreement. This canonicalizes each of the 5 raw runs
            // INDIVIDUALLY (N=1 input each, so R2's cross-run plurality
            // and R3's cross-run evidence arbitration can never fire --
            // only R1/R4/R5/R6/R7/R8's single-proposal deterministic
            // rules can act), then compares those 5 single-run
            // canonicalizations pairwise -- the closest apples-to-apples
            // analogue to E2b's own raw single-run-vs-single-run metric.
            if let Some(s) = stats {
                let single_run_outcomes: Vec<CanonicalOutcome> = runs_for_key
                    .iter()
                    .map(|(idx, d)| {
                        canonicalize(
                            std::slice::from_ref(&(*idx, d.clone())),
                            real_key,
                            s,
                            &wands_queries_text,
                            HAS_REAL_VARIANT_GROUPING,
                            false,
                        )
                    })
                    .collect();
                let r = pairwise_stability(&single_run_outcomes);
                single_run_stability.add(&r);
            }
        }
    }

    println!(
        "raw (Treatment A, extended beyond E2b's own role+primitive-only metric): role={:.2}% type={:.2}% scope={:.2}% primitive={:.2}% full={:.2}% ({} pairs)",
        StabilityCounts::rate(raw_stability.role_agree, raw_stability.total_pairs) * 100.0,
        StabilityCounts::rate(raw_stability.type_agree, raw_stability.total_pairs) * 100.0,
        StabilityCounts::rate(raw_stability.scope_agree, raw_stability.total_pairs) * 100.0,
        StabilityCounts::rate(raw_stability.primitive_agree, raw_stability.total_pairs) * 100.0,
        StabilityCounts::rate(raw_stability.full_agree, raw_stability.total_pairs) * 100.0,
        raw_stability.total_pairs,
    );
    println!(
        "C_canonicalizer, SINGLE-RUN comparison (stricter self-check: each raw run canonicalized alone, N=1, so only R1/R4/R5/R6/R7/R8 can act -- no cross-run voting or evidence arbitration): role={:.2}% type={:.2}% scope={:.2}% primitive={:.2}% full={:.2}% ({} pairs)",
        StabilityCounts::rate(single_run_stability.role_agree, single_run_stability.total_pairs) * 100.0,
        StabilityCounts::rate(single_run_stability.type_agree, single_run_stability.total_pairs) * 100.0,
        StabilityCounts::rate(single_run_stability.scope_agree, single_run_stability.total_pairs) * 100.0,
        StabilityCounts::rate(single_run_stability.primitive_agree, single_run_stability.total_pairs) * 100.0,
        StabilityCounts::rate(single_run_stability.full_agree, single_run_stability.total_pairs) * 100.0,
        single_run_stability.total_pairs,
    );
    for &treatment in &treatments {
        let r = &stability_by_treatment[treatment_label(treatment)];
        println!(
            "{}: role={:.2}% type={:.2}% scope={:.2}% primitive={:.2}% full={:.2}% ({} pairs)",
            treatment_label(treatment),
            StabilityCounts::rate(r.role_agree, r.total_pairs) * 100.0,
            StabilityCounts::rate(r.type_agree, r.total_pairs) * 100.0,
            StabilityCounts::rate(r.scope_agree, r.total_pairs) * 100.0,
            StabilityCounts::rate(r.primitive_agree, r.total_pairs) * 100.0,
            StabilityCounts::rate(r.full_agree, r.total_pairs) * 100.0,
            r.total_pairs,
        );
        for config in per_config_runs.keys() {
            if let Some(cr) =
                stability_by_treatment_config.get(&(treatment_label(treatment), config.clone()))
            {
                println!(
                    "    {config}: primitive={:.2}% full={:.2}% ({} pairs)",
                    StabilityCounts::rate(cr.primitive_agree, cr.total_pairs) * 100.0,
                    StabilityCounts::rate(cr.full_agree, cr.total_pairs) * 100.0,
                    cr.total_pairs,
                );
            }
        }
    }

    // ==================== Full canonicalization (CANONICAL_CONFIGS only, headline safety/recall/abstention) ====================
    println!("\n=== Safety/recall/abstention (full canonicalization, CANONICAL_CONFIGS = wands_baseline + automotive, matching E2b's own headline-map precedent) ===");

    struct FullResult {
        promoted_keys_and_roles: Vec<(String, SemanticRole)>,
        promoted_keys: BTreeSet<String>,
        n_total_keys: usize,
        n_promoted: usize,
    }

    let mut full_by_treatment: BTreeMap<&str, FullResult> = BTreeMap::new();
    for &treatment in &treatments {
        let mut promoted_keys_and_roles = Vec::new();
        let mut promoted_keys = BTreeSet::new();
        let mut n_total_keys = 0usize;

        for config in CANONICAL_CONFIGS {
            let Some(runs) = per_config_runs.get(*config) else {
                continue;
            };
            let by_key = group_by_real_key(config, runs, &anon_mapping, &noisy_mapping);
            for (real_key, runs_for_key) in &by_key {
                n_total_keys += 1;
                let outcome = match treatment {
                    Treatment::B => majority_vote(runs_for_key, real_key),
                    Treatment::C => match all_unified.get(real_key) {
                        Some(s) => canonicalize(
                            runs_for_key,
                            real_key,
                            s,
                            &wands_queries_text,
                            HAS_REAL_VARIANT_GROUPING,
                            false,
                        ),
                        None => continue,
                    },
                    Treatment::D => match all_unified.get(real_key) {
                        Some(s) => canonicalize(
                            runs_for_key,
                            real_key,
                            s,
                            &wands_queries_text,
                            HAS_REAL_VARIANT_GROUPING,
                            true,
                        ),
                        None => continue,
                    },
                };
                if let CanonicalOutcome::Promoted(d) = &outcome {
                    if is_structural(d.semantic_role) {
                        promoted_keys_and_roles.push((real_key.clone(), d.semantic_role));
                        promoted_keys.insert(real_key.clone());
                    }
                }
            }
        }

        let n_promoted = promoted_keys_and_roles.len();
        println!(
            "{}: {}/{} real keys promoted to a structural role",
            treatment_label(treatment),
            n_promoted,
            n_total_keys
        );
        full_by_treatment.insert(
            treatment_label(treatment),
            FullResult {
                promoted_keys_and_roles,
                promoted_keys,
                n_total_keys,
                n_promoted,
            },
        );
    }

    let mut unsafe_by_treatment: BTreeMap<&str, usize> = BTreeMap::new();
    let mut recall_by_treatment: BTreeMap<&str, f64> = BTreeMap::new();
    let mut abstention_by_treatment: BTreeMap<&str, f64> = BTreeMap::new();
    for &treatment in &treatments {
        let r = &full_by_treatment[treatment_label(treatment)];
        let unsafe_count = unsafe_accepted_count(&r.promoted_keys_and_roles, &oracle_by_key);
        let recall = retrieval_significant_recall(&r.promoted_keys, &oracle_all);
        let abstention_rate = if r.n_total_keys == 0 {
            0.0
        } else {
            1.0 - (r.n_promoted as f64 / r.n_total_keys as f64)
        };
        println!(
            "{}: unsafe_accepted={unsafe_count} recall={:.2}% abstention_rate={:.2}%",
            treatment_label(treatment),
            recall * 100.0,
            abstention_rate * 100.0
        );
        unsafe_by_treatment.insert(treatment_label(treatment), unsafe_count);
        recall_by_treatment.insert(treatment_label(treatment), recall);
        abstention_by_treatment.insert(treatment_label(treatment), abstention_rate);
    }

    // ==================== Single-run worst-case safety (stricter self-check companion) ====================
    // The single-run stability comparison above showed real disagreement
    // (95.20% full-descriptor agreement, not 100%) -- so it matters
    // whether ANY of the 5 single-run canonicalizations for a given real
    // key ever promotes an unsafe (Identifier/Relationship-conflated)
    // role, not only whether the ensemble-level (multi-run) canonical
    // answer is safe. Worst-case, not average-case, since a single-
    // proposal-per-field deployment could draw any one of the 5.
    println!("\n=== Single-run worst-case safety (CANONICAL_CONFIGS, Treatment C) ===");
    let mut single_run_any_promoted: Vec<(String, SemanticRole)> = Vec::new();
    for config in CANONICAL_CONFIGS {
        let Some(runs) = per_config_runs.get(*config) else {
            continue;
        };
        let by_key = group_by_real_key(config, runs, &anon_mapping, &noisy_mapping);
        for (real_key, runs_for_key) in &by_key {
            let Some(stats) = all_unified.get(real_key) else {
                continue;
            };
            for (idx, d) in runs_for_key {
                let outcome = canonicalize(
                    std::slice::from_ref(&(*idx, d.clone())),
                    real_key,
                    stats,
                    &wands_queries_text,
                    HAS_REAL_VARIANT_GROUPING,
                    false,
                );
                if let CanonicalOutcome::Promoted(pd) = &outcome {
                    if is_structural(pd.semantic_role) {
                        single_run_any_promoted.push((real_key.clone(), pd.semantic_role));
                    }
                }
            }
        }
    }
    let single_run_worst_case_unsafe =
        unsafe_accepted_count(&single_run_any_promoted, &oracle_by_key);
    println!(
        "worst-case unsafe_accepted across every individual single-run canonicalization: {single_run_worst_case_unsafe}"
    );

    // ==================== Unstable -> stable conversion / stable-but-wrong (Treatment C only, per protocol section 10) ====================
    println!("\n=== Unstable -> stable conversion and stable-but-wrong rate (Treatment C, CANONICAL_CONFIGS) ===");
    let mut unstable_raw_keys: BTreeSet<String> = BTreeSet::new();
    let mut now_stable = 0usize;
    let mut stable_but_wrong = 0usize;
    let mut stable_total = 0usize;
    for config in CANONICAL_CONFIGS {
        let Some(runs) = per_config_runs.get(*config) else {
            continue;
        };
        let by_key = group_by_real_key(config, runs, &anon_mapping, &noisy_mapping);
        for (real_key, runs_for_key) in &by_key {
            if runs_for_key.len() < 2 {
                continue;
            }
            // SemanticRole/PhysicalPrimitive derive PartialEq but not Ord,
            // so uniqueness is checked directly rather than via BTreeSet.
            let first_role = runs_for_key[0].1.semantic_role;
            let first_prim = runs_for_key[0].1.candidate_physical_primitive;
            let was_unstable = runs_for_key.iter().any(|(_, d)| {
                d.semantic_role != first_role || d.candidate_physical_primitive != first_prim
            });
            if !was_unstable {
                continue;
            }
            unstable_raw_keys.insert(real_key.clone());

            let Some(stats) = all_unified.get(real_key) else {
                continue;
            };
            let outcomes = leave_one_out_outcomes(
                Treatment::C,
                runs_for_key,
                real_key,
                Some(stats),
                &wands_queries_text,
                HAS_REAL_VARIANT_GROUPING,
            );
            let r = pairwise_stability(&outcomes);
            let is_now_stable = r.total_pairs > 0 && r.full_agree == r.total_pairs;
            if is_now_stable {
                now_stable += 1;
                stable_total += 1;
                if let Some(CanonicalOutcome::Promoted(d)) = outcomes.first() {
                    if oracle_by_key.get(real_key) != Some(&d.semantic_role) {
                        stable_but_wrong += 1;
                        println!(
                            "  stable-but-wrong: {real_key} -- canonicalizer={:?} oracle={:?}",
                            d.semantic_role,
                            oracle_by_key.get(real_key)
                        );
                    }
                }
            }
        }
    }
    println!(
        "{}/{} raw-unstable real keys became canonically stable under Treatment C ({:.2}%); {}/{} of those are stable-but-wrong vs oracle ({:.2}%)",
        now_stable,
        unstable_raw_keys.len(),
        StabilityCounts::rate(now_stable, unstable_raw_keys.len()) * 100.0,
        stable_but_wrong,
        stable_total,
        StabilityCounts::rate(stable_but_wrong, stable_total) * 100.0,
    );

    // ==================== Relevance (criterion 5): naive end-to-end NDCG@10, oracle vs Treatment C ====================
    println!("\n=== Relevance (criterion 5): naive end-to-end NDCG@10/Recall@10, oracle vs Treatment C, byte-for-byte the same check E2b's own closure pass used ===");
    let wands_queries = load_wands_queries();
    let wands_labels = load_wands_labels();

    let mut treatment_c_wands_promoted: Vec<Descriptor> = Vec::new();
    if let Some(runs) = per_config_runs.get("wands_baseline") {
        let by_key = group_by_real_key("wands_baseline", runs, &anon_mapping, &noisy_mapping);
        for (real_key, runs_for_key) in &by_key {
            let Some(stats) = wands_unified.get(real_key) else {
                continue;
            };
            let outcome = canonicalize(
                runs_for_key,
                real_key,
                stats,
                &wands_queries_text,
                HAS_REAL_VARIANT_GROUPING,
                false,
            );
            if let Some(d) = outcome.promoted() {
                if is_structural(d.semantic_role) {
                    treatment_c_wands_promoted.push(as_synthetic_e2b_descriptor(d));
                }
            }
        }
    }

    let oracle_e2e =
        end_to_end_ndcg_recall(&oracle_wands_accepted(), &wands_queries, &wands_labels);
    let c_e2e = end_to_end_ndcg_recall(&treatment_c_wands_promoted, &wands_queries, &wands_labels);
    let e2e_check_reliable = oracle_e2e.n_scored >= 20 && oracle_e2e.ndcg >= 0.05;
    let relative_ndcg_gap = if oracle_e2e.ndcg > 0.0 {
        (oracle_e2e.ndcg - c_e2e.ndcg).abs() / oracle_e2e.ndcg
    } else {
        0.0
    };
    let c5_relevance_within_5pct = relative_ndcg_gap <= 0.05;
    println!(
        "oracle: NDCG@10={:.4} Recall@10={:.4} n_scored={} | Treatment C: NDCG@10={:.4} Recall@10={:.4} n_scored={} | relative_ndcg_gap={:.4} check_reliable={e2e_check_reliable} (E2b's own disclosed near-floor-NDCG caveat carried forward unchanged) => c5(relevance within 5%)={c5_relevance_within_5pct}",
        oracle_e2e.ndcg, oracle_e2e.recall, oracle_e2e.n_scored,
        c_e2e.ndcg, c_e2e.recall, c_e2e.n_scored,
        relative_ndcg_gap,
    );

    // ==================== GO gate (Treatment C; D reported alongside, per protocol section 11) ====================
    println!("\n=== GO gate (criteria 1-5, 7 -- criterion 6 measured separately by e2c_serving_overhead_eval) ===");

    #[allow(clippy::too_many_arguments)]
    fn go_gate_for(
        treatment_label: &str,
        unsafe_count: usize,
        primitive_stability: f64,
        full_stability: f64,
        recall: f64,
        c5_relevance_within_5pct: bool,
        relative_ndcg_gap: f64,
    ) -> (bool, bool, bool, bool, bool) {
        let c1_zero_unsafe = unsafe_count == 0;
        let c2_primitive_ge_99 = primitive_stability >= 0.99;
        let c3_full_ge_98 = full_stability >= 0.98;
        let c4_recall_within_5pp = recall >= 0.8684 - 0.05;
        println!(
            "  {treatment_label}: c1(zero_unsafe)={c1_zero_unsafe} c2(primitive>=99%)={c2_primitive_ge_99} ({:.2}%) c3(full>=98%)={c3_full_ge_98} ({:.2}%) c4(recall within 5pp of E2b's 86.84%)={c4_recall_within_5pp} ({:.2}%) c5(relevance within 5%)={c5_relevance_within_5pct} (gap={:.2}%)",
            primitive_stability * 100.0,
            full_stability * 100.0,
            recall * 100.0,
            relative_ndcg_gap * 100.0,
        );
        (
            c1_zero_unsafe,
            c2_primitive_ge_99,
            c3_full_ge_98,
            c4_recall_within_5pp,
            c5_relevance_within_5pct,
        )
    }

    // Criterion 5 is computed against Treatment C's own WANDS-scoped
    // promoted set (above). Treatment D's own WANDS-promoted set is a
    // subset of C's (D only ever demotes a role C would have promoted to
    // Ignore/abstain, never adds one) -- in THIS run D's promoted set for
    // WANDS is byte-identical to C's (both promoted 42/53 real keys
    // overall; verified directly, not merely assumed), so D's own
    // criterion 5 is reported as C's, not independently recomputed.
    let mut gate_by_treatment: BTreeMap<&str, (bool, bool, bool, bool, bool)> = BTreeMap::new();
    for &treatment in &[Treatment::C, Treatment::D] {
        let label = treatment_label(treatment);
        let r = &stability_by_treatment[label];
        let primitive_stability = StabilityCounts::rate(r.primitive_agree, r.total_pairs);
        let full_stability = StabilityCounts::rate(r.full_agree, r.total_pairs);
        let unsafe_count = unsafe_by_treatment[label];
        let recall = recall_by_treatment[label];
        let gate = go_gate_for(
            label,
            unsafe_count,
            primitive_stability,
            full_stability,
            recall,
            c5_relevance_within_5pct,
            relative_ndcg_gap,
        );
        gate_by_treatment.insert(label, gate);
    }
    println!(
        "\nNote: criterion 7 (order-independence) is verified structurally by \
         e2c_canonicalizer::tests::canonicalization_output_does_not_depend_on_run_order, not \
         re-derived numerically here."
    );

    // Single-run (stricter) GO-gate reading -- criteria 2/3 against the
    // single-run stability numbers above, criterion 1 against the
    // worst-case single-run safety check above. This is the reading a
    // single-proposal-per-field deployment (no multi-run ensemble) would
    // actually experience.
    let sr_primitive = StabilityCounts::rate(
        single_run_stability.primitive_agree,
        single_run_stability.total_pairs,
    );
    let sr_full = StabilityCounts::rate(
        single_run_stability.full_agree,
        single_run_stability.total_pairs,
    );
    let sr_c1_zero_unsafe = single_run_worst_case_unsafe == 0;
    let sr_c2_primitive_ge_99 = sr_primitive >= 0.99;
    let sr_c3_full_ge_98 = sr_full >= 0.98;
    println!(
        "\n=== Single-run (stricter) GO-gate reading, Treatment C ===\n  c1(zero_unsafe, worst-case)={sr_c1_zero_unsafe} ({single_run_worst_case_unsafe} found) c2(primitive>=99%)={sr_c2_primitive_ge_99} ({:.2}%) c3(full>=98%)={sr_c3_full_ge_98} ({:.2}%)\n  (c4/c5 not independently recomputed for the single-run design -- they are recall/relevance metrics defined against a promoted SET, which the single-run design does not produce one consistent version of; see docs/experiments/ISSUE45_PROTOCOL.md's own disclosed scope)",
        sr_primitive * 100.0,
        sr_full * 100.0,
    );

    // ==================== Write summary JSON ====================
    let summary = serde_json::json!({
        "experiment_id": "I45-E2c-canonicalization",
        "baseline_sha": BASELINE_SHA,
        "raw_stability_extended": {
            "role_pct": StabilityCounts::rate(raw_stability.role_agree, raw_stability.total_pairs) * 100.0,
            "type_pct": StabilityCounts::rate(raw_stability.type_agree, raw_stability.total_pairs) * 100.0,
            "scope_pct": StabilityCounts::rate(raw_stability.scope_agree, raw_stability.total_pairs) * 100.0,
            "primitive_pct": StabilityCounts::rate(raw_stability.primitive_agree, raw_stability.total_pairs) * 100.0,
            "full_pct": StabilityCounts::rate(raw_stability.full_agree, raw_stability.total_pairs) * 100.0,
            "total_pairs": raw_stability.total_pairs,
        },
        "c_single_run_stability_stricter_self_check": {
            "role_pct": StabilityCounts::rate(single_run_stability.role_agree, single_run_stability.total_pairs) * 100.0,
            "type_pct": StabilityCounts::rate(single_run_stability.type_agree, single_run_stability.total_pairs) * 100.0,
            "scope_pct": StabilityCounts::rate(single_run_stability.scope_agree, single_run_stability.total_pairs) * 100.0,
            "primitive_pct": StabilityCounts::rate(single_run_stability.primitive_agree, single_run_stability.total_pairs) * 100.0,
            "full_pct": StabilityCounts::rate(single_run_stability.full_agree, single_run_stability.total_pairs) * 100.0,
            "total_pairs": single_run_stability.total_pairs,
        },
        "treatments": treatments.iter().map(|&t| {
            let label = treatment_label(t);
            let r = &stability_by_treatment[label];
            let full = &full_by_treatment[label];
            serde_json::json!({
                "label": label,
                "stability": {
                    "role_pct": StabilityCounts::rate(r.role_agree, r.total_pairs) * 100.0,
                    "type_pct": StabilityCounts::rate(r.type_agree, r.total_pairs) * 100.0,
                    "scope_pct": StabilityCounts::rate(r.scope_agree, r.total_pairs) * 100.0,
                    "primitive_pct": StabilityCounts::rate(r.primitive_agree, r.total_pairs) * 100.0,
                    "full_pct": StabilityCounts::rate(r.full_agree, r.total_pairs) * 100.0,
                    "total_pairs": r.total_pairs,
                },
                "n_promoted": full.n_promoted,
                "n_total_keys": full.n_total_keys,
                "unsafe_accepted_count": unsafe_by_treatment[label],
                "retrieval_significant_recall_pct": recall_by_treatment[label] * 100.0,
                "abstention_rate_pct": abstention_by_treatment[label] * 100.0,
            })
        }).collect::<Vec<_>>(),
        "unstable_to_stable_conversion": {
            "raw_unstable_keys": unstable_raw_keys.len(),
            "now_stable_under_c": now_stable,
            "stable_but_wrong_under_c": stable_but_wrong,
        },
        "relevance_criterion_5": {
            "oracle_ndcg10": oracle_e2e.ndcg,
            "oracle_recall10": oracle_e2e.recall,
            "oracle_n_scored": oracle_e2e.n_scored,
            "treatment_c_ndcg10": c_e2e.ndcg,
            "treatment_c_recall10": c_e2e.recall,
            "treatment_c_n_scored": c_e2e.n_scored,
            "relative_ndcg_gap": relative_ndcg_gap,
            "check_reliable": e2e_check_reliable,
            "within_5pct": c5_relevance_within_5pct,
        },
        "go_gate": {
            "C": gate_by_treatment.get("C_canonicalizer"),
            "D": gate_by_treatment.get("D_conservative"),
        },
        "go_gate_single_run_stricter_reading": {
            "c1_zero_unsafe_worst_case": sr_c1_zero_unsafe,
            "single_run_worst_case_unsafe_count": single_run_worst_case_unsafe,
            "c2_primitive_ge_99pct": sr_c2_primitive_ge_99,
            "primitive_pct": sr_primitive * 100.0,
            "c3_full_ge_98pct": sr_c3_full_ge_98,
            "full_pct": sr_full * 100.0,
        },
    });
    println!("\n{}", serde_json::to_string_pretty(&summary).unwrap());
    if let Some(path) = env::args().nth(1) {
        fs::write(&path, serde_json::to_string_pretty(&summary).unwrap())
            .expect("write summary json");
        println!("summary written to {path}");
    }
}
