//! Issue #42 E2b: actual offline LLM-assisted feature discovery and
//! physical selection (`docs/experiments/ISSUE42_PROTOCOL.md`'s E2b
//! amendment 1). Reproduction: `cargo build --release -p issue42-eval &&
//! ./target/release/e2b_feature_discovery_eval [output_summary_json_path]`.

use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;

use issue42_eval::e2b_ingest::{build_catalog, naive_constraints_for_query};
use issue42_eval::e2b_oracle::{automotive_oracle, wands_oracle};
use issue42_eval::e2b_pipeline::{
    build_baselines_2_and_3, is_structural, load_all_runs, macro_f1, Baselines2And3, CONFIGS,
};
use issue42_eval::e2b_schema::{Descriptor, SemanticRole};
use issue42_eval::e2b_statistics_baseline;
use issue42_eval::e2b_validator::wands_query_texts;
use issue42_eval::e2b_workload::{
    automotive_unified_stats, load_wands_feed, load_wands_labels, load_wands_queries,
    UnifiedFieldStats,
};
use phase9_eval::wands_relevance::ndcg_recall_mrr;

const K: usize = 10;
const BASELINE_SHA: &str = "fe2e52e0fe872a0f4ab86c63ccc839e61de8f3e6";

/// `feature_NNN`/noisy-name -> real key. Originally written only to a
/// session-local `/tmp/e2b_key_mappings.json` by the bounded-input dump
/// step and never committed -- a real reproducibility gap (Issue #42 rule
/// 8) fixed by reconstructing and committing both mappings directly (see
/// `e2b_key_mapping`'s own doc comment for the reconstruction method and
/// its cross-check against real per-key statistics).
fn load_key_mapping() -> (BTreeMap<String, String>, BTreeMap<String, String>) {
    (
        issue42_eval::e2b_key_mapping::anonymized_mapping(),
        issue42_eval::e2b_key_mapping::noisy_mapping(),
    )
}

/// "Unsafe accepted structural classification" (Issue #42's own GO-gate
/// wording): a field the validator let through as an accepted
/// Enum/Numeric/Boolean facet despite its real, oracle-confirmed nature
/// being Identifier or Relationship -- exactly the cross-item-identity
/// conflation R3's identifier-serving-primitive work exists to prevent.
/// Checked against the final, deduped-by-real-key accepted set, never
/// per raw proposal -- a validator *rejection* is the validator working
/// correctly, not an unsafe acceptance (an earlier version of this
/// binary conflated the two by counting `!validation.accepted` instead;
/// see `ISSUE42_LOG.md`'s E2b section for the correction).
fn unsafe_accepted_count(
    validated_by_key: &BTreeMap<String, Descriptor>,
    oracle_by_key: &BTreeMap<String, SemanticRole>,
) -> usize {
    validated_by_key
        .iter()
        .filter(|(real_key, d)| {
            !d.abstain
                && is_structural(d.semantic_role)
                && matches!(
                    oracle_by_key.get(real_key.as_str()),
                    Some(SemanticRole::Identifier) | Some(SemanticRole::Relationship)
                )
        })
        .count()
}

struct BaselineResult {
    label: String,
    descriptors_by_real_key: BTreeMap<String, Descriptor>,
}

fn print_and_score(
    result: &BaselineResult,
    oracle_by_key: &BTreeMap<String, SemanticRole>,
    oracle_accepted: &BTreeSet<String>,
) -> serde_json::Value {
    let predicted_roles: BTreeMap<String, SemanticRole> = result
        .descriptors_by_real_key
        .iter()
        .map(|(k, d)| (k.clone(), d.semantic_role))
        .collect();
    let f1 = macro_f1(&predicted_roles, oracle_by_key);

    let predicted_accepted: BTreeSet<String> = result
        .descriptors_by_real_key
        .iter()
        .filter(|(_, d)| !d.abstain && is_structural(d.semantic_role))
        .map(|(k, _)| k.clone())
        .collect();
    let tp = predicted_accepted.intersection(oracle_accepted).count();
    let precision = if predicted_accepted.is_empty() {
        0.0
    } else {
        tp as f64 / predicted_accepted.len() as f64
    };
    let recall = if oracle_accepted.is_empty() {
        0.0
    } else {
        tp as f64 / oracle_accepted.len() as f64
    };

    let abstention_rate = result
        .descriptors_by_real_key
        .values()
        .filter(|d| d.abstain)
        .count() as f64
        / result.descriptors_by_real_key.len().max(1) as f64;

    println!(
        "{}: feature-role macro F1={f1:.4}, accepted-structural precision={precision:.4} \
         recall={recall:.4} (n_accepted={}), abstention_rate={abstention_rate:.4}",
        result.label,
        predicted_accepted.len()
    );

    serde_json::json!({
        "label": result.label,
        "macro_f1": f1,
        "accepted_structural_precision": precision,
        "accepted_structural_recall": recall,
        "n_accepted_structural": predicted_accepted.len(),
        "abstention_rate": abstention_rate,
    })
}

/// `n_scored` (queries with >=1 naive constraint and a judgment row) and
/// `mean_match_set_size` (raw `Catalog::search` hits before the `take(K)`
/// truncation) are reported alongside NDCG/Recall so a near-floor score
/// can be diagnosed: sparse constraint coverage (few queries scored) and
/// unranked truncation (large match sets, arbitrary first-K order) are
/// both real, disclosed limitations of this naive substring-matched
/// check (see `e2b_ingest`'s own doc comment) -- not silently folded
/// into a single opaque number.
struct EndToEndResult {
    ndcg: f64,
    recall: f64,
    n_scored: usize,
    mean_match_set_size: f64,
}

fn end_to_end_ndcg_recall(
    accepted: &[Descriptor],
    queries: &[issue42_eval::e2b_workload::WandsQuery],
    labels: &BTreeMap<String, BTreeMap<String, phase9_eval::wands_relevance::WandsLabel>>,
) -> EndToEndResult {
    let ingested = build_catalog(accepted);
    let mut ndcg_sum = 0.0;
    let mut recall_sum = 0.0;
    let mut match_set_size_sum = 0usize;
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
        match_set_size_sum += matches.len();
        let hits: Vec<String> = matches
            .iter()
            .filter_map(|(pid, _)| {
                ingested
                    .wands_id_to_product_id
                    .iter()
                    .find(|(_, v)| **v == *pid)
                    .map(|(k, _)| k.clone())
            })
            .take(K)
            .collect();
        let (ndcg, recall, _mrr) = ndcg_recall_mrr(&hits, judged, K);
        ndcg_sum += ndcg;
        recall_sum += recall;
        n += 1;
    }
    if n == 0 {
        EndToEndResult {
            ndcg: 0.0,
            recall: 0.0,
            n_scored: 0,
            mean_match_set_size: 0.0,
        }
    } else {
        EndToEndResult {
            ndcg: ndcg_sum / n as f64,
            recall: recall_sum / n as f64,
            n_scored: n,
            mean_match_set_size: match_set_size_sum as f64 / n as f64,
        }
    }
}

fn main() {
    println!("=== Issue #42 E2b: actual offline LLM-assisted feature discovery ===");
    println!("baseline_sha: {BASELINE_SHA}");

    let (anon_mapping, noisy_mapping) = load_key_mapping();

    // --- Oracle (baseline 4) ---
    let mut oracle_all: Vec<Descriptor> = Vec::new();
    oracle_all.extend(wands_oracle());
    oracle_all.extend(automotive_oracle());
    let oracle_by_key: BTreeMap<String, SemanticRole> = oracle_all
        .iter()
        .map(|d| (d.key.clone(), d.semantic_role))
        .collect();
    let oracle_accepted: BTreeSet<String> = oracle_all
        .iter()
        .filter(|d| !d.abstain && is_structural(d.semantic_role))
        .map(|d| d.key.clone())
        .collect();
    println!(
        "\noracle: {} total fields, {} accepted as structural (Enum/Numeric/Boolean)",
        oracle_all.len(),
        oracle_accepted.len()
    );

    // --- Unified stats (bounded inputs every other baseline sees) ---
    let wands_feed = load_wands_feed();
    let wands_unified: BTreeMap<String, UnifiedFieldStats> = wands_feed
        .stats
        .iter()
        .map(|(k, s)| (k.clone(), UnifiedFieldStats::from(s)))
        .collect();
    let automotive_unified = automotive_unified_stats(1500);
    let mut all_unified: BTreeMap<String, UnifiedFieldStats> = wands_unified.clone();
    all_unified.extend(automotive_unified.clone());

    // --- Baseline 1: statistics-only (name-invariant by construction) ---
    let stats_only_descriptors: BTreeMap<String, Descriptor> = all_unified
        .values()
        .map(|s| (s.key.clone(), e2b_statistics_baseline::classify(s)))
        .collect();
    let stats_only_result = BaselineResult {
        label: "Baseline 1: statistics-only".to_string(),
        descriptors_by_real_key: stats_only_descriptors,
    };

    println!("\n--- Baseline results (feature-role macro F1, accepted-structural precision/recall vs oracle) ---");
    let mut summary_baselines = Vec::new();
    summary_baselines.push(print_and_score(
        &stats_only_result,
        &oracle_by_key,
        &oracle_accepted,
    ));

    // --- Baselines 2/3: LLM proposal (no validator) / LLM + validator ---
    let wands_queries_text = wands_query_texts();
    let per_config_runs = load_all_runs(CONFIGS);

    if per_config_runs.is_empty() {
        println!(
            "\nNo LLM proposal artifacts found under dataset_cache/export/ yet -- run the 8 \
             offline LLM passes and save their output there first (see \
             docs/experiments/ISSUE42_PROTOCOL.md's E2b amendment 1). Reporting \
             statistics-only/oracle results only."
        );
    } else {
        let Baselines2And3 {
            no_validator_by_key,
            validated_by_key,
            stability_agreements,
            stability_total,
            per_config_f1,
            per_config_stability,
        } = build_baselines_2_and_3(
            &per_config_runs,
            &anon_mapping,
            &noisy_mapping,
            &all_unified,
            &wands_queries_text,
            &oracle_by_key,
        );

        let no_validator_result = BaselineResult {
            label: "Baseline 2: LLM proposal, no validator".to_string(),
            descriptors_by_real_key: no_validator_by_key,
        };
        let validated_result = BaselineResult {
            label: "Baseline 3: LLM + deterministic validator".to_string(),
            descriptors_by_real_key: validated_by_key.clone(),
        };
        summary_baselines.push(print_and_score(
            &no_validator_result,
            &oracle_by_key,
            &oracle_accepted,
        ));
        summary_baselines.push(print_and_score(
            &validated_result,
            &oracle_by_key,
            &oracle_accepted,
        ));

        let unsafe_accepted_count = unsafe_accepted_count(&validated_by_key, &oracle_by_key);

        let stability_rate = if stability_total == 0 {
            0.0
        } else {
            stability_agreements as f64 / stability_total as f64
        };
        println!(
            "\nrepeated-run agreement (role + primitive, every pairwise comparison of runs \
             within a configuration, across all 4 configurations): {stability_agreements}/{stability_total} \
             ({:.2}%)",
            100.0 * stability_rate
        );
        println!("repeated-run agreement by configuration:");
        for (config, (agreements, total)) in &per_config_stability {
            let rate = if *total == 0 {
                0.0
            } else {
                *agreements as f64 / *total as f64
            };
            println!("  {config}: {agreements}/{total} ({:.2}%)", 100.0 * rate);
        }
        println!(
            "unsafe-accepted count (accepted structural facet whose oracle role is actually \
             Identifier/Relationship): {unsafe_accepted_count}"
        );
        println!("\nfeature-role macro F1 by perturbation configuration (degradation check):");
        for (config, f1) in &per_config_f1 {
            println!("  {config}: {f1:.4}");
        }

        // --- End-to-end Recall@10/NDCG@10 on WANDS's real 480 queries/233,448 judgments ---
        let queries = load_wands_queries();
        let labels = load_wands_labels();
        println!("\n--- End-to-end Recall@{K}/NDCG@{K} on WANDS's real queries (naive keyword-matched structural constraints; see e2b_ingest's own doc comment for this check's disclosed scope) ---");

        let oracle_wands_accepted: Vec<Descriptor> = wands_oracle()
            .into_iter()
            .filter(|d| !d.abstain && is_structural(d.semantic_role))
            .collect();
        let oracle_e2e = end_to_end_ndcg_recall(&oracle_wands_accepted, &queries, &labels);
        println!(
            "oracle: NDCG@{K}={:.4} Recall@{K}={:.4} (n_queries_scored={}, mean_match_set_size={:.1})",
            oracle_e2e.ndcg, oracle_e2e.recall, oracle_e2e.n_scored, oracle_e2e.mean_match_set_size
        );

        let stats_only_wands_accepted: Vec<Descriptor> = wands_unified
            .values()
            .map(e2b_statistics_baseline::classify)
            .filter(|d| !d.abstain && is_structural(d.semantic_role))
            .collect();
        let stats_e2e = end_to_end_ndcg_recall(&stats_only_wands_accepted, &queries, &labels);
        println!(
            "statistics-only: NDCG@{K}={:.4} Recall@{K}={:.4} (n_queries_scored={}, mean_match_set_size={:.1})",
            stats_e2e.ndcg, stats_e2e.recall, stats_e2e.n_scored, stats_e2e.mean_match_set_size
        );

        let validated_wands_accepted: Vec<Descriptor> = validated_by_key
            .values()
            .filter(|d| {
                wands_unified.contains_key(&d.key.clone())
                    || d.real_key
                        .as_deref()
                        .is_some_and(|k| wands_unified.contains_key(k))
            })
            .filter(|d| !d.abstain && is_structural(d.semantic_role))
            .cloned()
            .collect();
        let validated_e2e = end_to_end_ndcg_recall(&validated_wands_accepted, &queries, &labels);
        println!(
            "LLM + validator: NDCG@{K}={:.4} Recall@{K}={:.4} (n_queries_scored={}, mean_match_set_size={:.1})",
            validated_e2e.ndcg,
            validated_e2e.recall,
            validated_e2e.n_scored,
            validated_e2e.mean_match_set_size
        );

        let relative_ndcg_gap = if oracle_e2e.ndcg > 0.0 {
            (oracle_e2e.ndcg - validated_e2e.ndcg).abs() / oracle_e2e.ndcg
        } else {
            1.0
        };
        // This naive substring-matched check scores extremely few
        // queries (most of WANDS's 480 real queries are noun phrases
        // with no literal enum-value substring) and applies no ranking
        // to `Catalog::search`'s raw hits (just a `take(K)` in catalog
        // iteration order) -- so absolute NDCG is expected to sit near
        // the floor for every baseline, oracle included. That underlying
        // limitation is disclosed in `e2b_ingest.rs`'s own doc comment
        // from the start; the specific n_scored/ndcg thresholds below
        // are a diagnostic ANNOTATION added here, after computing the
        // actual results, to characterize how much weight the
        // preregistered 5%-relative-gap computation deserves -- it does
        // not alter that computation or its own pass/fail boolean
        // (`relevance_within_5pct` below is untouched), only reports
        // alongside it whether the underlying numbers are large enough
        // to trust. Disclosed as a post-hoc diagnostic, not backdated as
        // if it were part of the original preregistered design.
        let e2e_check_reliable = oracle_e2e.n_scored >= 20 && oracle_e2e.ndcg >= 0.05;

        // --- GO gate (Issue #42's own E2b section, verbatim thresholds) ---
        println!("\n--- GO gate evaluation (per Issue #42's own preregistered E2b thresholds) ---");
        let zero_unsafe = unsafe_accepted_count == 0;
        let recall_significant_ge_80pct = {
            let sig_oracle: BTreeSet<&String> = oracle_all
                .iter()
                .filter(|d| {
                    d.retrieval_significance
                        == issue42_eval::e2b_schema::Significance::RetrievalSignificant
                })
                .map(|d| &d.key)
                .collect();
            // A confirmed defect (found by a fresh adversarial review):
            // this used to check `validated_by_key.contains_key(k)`
            // alone -- true for nearly every real key regardless of
            // validator quality, since `validated_by_key` inserts an
            // entry even for a rejected/abstained proposal. "Recovered"
            // must mean actually accepted as structural, matching
            // `print_and_score`'s own identical filter 30 lines above.
            let recovered = sig_oracle
                .iter()
                .filter(|k| {
                    validated_by_key
                        .get(k.as_str())
                        .is_some_and(|d| !d.abstain && is_structural(d.semantic_role))
                })
                .count();
            if sig_oracle.is_empty() {
                0.0
            } else {
                recovered as f64 / sig_oracle.len() as f64
            }
        };
        let relevance_within_5pct = relative_ndcg_gap <= 0.05;
        let stability_ge_90pct = stability_rate >= 0.90;
        // Corrected by a fresh adversarial review (Issue #42's own E2b
        // serving-contract-closure pass): this field previously asserted
        // `true` on the unaudited claim "WANDS is a real, unseen,
        // external structured feed" -- true as far as it goes, but
        // Issue #42's own dataset requirement is "real structured
        // catalog/feed with category-specific attributes AND
        // Product/Variant or relationship complexity," and a direct
        // audit (docs/experiments/ISSUE42_LOG.md's "I42-E2b
        // serving-contract closure" section, item 4) found WANDS as
        // actually used in this pipeline has no real Variant concept
        // (e2b_ingest::build_catalog maps every WANDS row to exactly one
        // Product with exactly one Variant) and never materializes or
        // exercises either of its two oracle-labeled Relationship
        // fields anywhere in this pipeline (accepted_typed_keys filters
        // to Enum/Boolean/Numeric only). This criterion is therefore NOT
        // ESTABLISHED, not PASS -- `false` here reflects "not confirmed
        // to pass," the correct, conservative reading, not a claim that
        // the criterion affirmatively fails.
        let real_feed_evidence = false;
        const REAL_FEED_EVIDENCE_STATUS: &str = "not_established";

        println!(
            "zero unsafe accepted structural classifications: {zero_unsafe} ({unsafe_accepted_count} found)"
        );
        println!(
            ">=80% recall on retrieval-significant reference features: {recall_significant_ge_80pct:.4} \
             (gate: {})",
            recall_significant_ge_80pct >= 0.80
        );
        println!(
            "end-to-end relevance within 5% of oracle: relative NDCG gap={relative_ndcg_gap:.4} \
             (gate: {relevance_within_5pct}, CHECK RELIABLE: {e2e_check_reliable} -- oracle scored \
             only {} queries at NDCG@{K}={:.4}; a relative gap against a near-floor oracle score is \
             not a trustworthy signal in either direction, see this binary's own doc comment)",
            oracle_e2e.n_scored, oracle_e2e.ndcg
        );
        println!(
            ">=90% repeated-run agreement on accepted physical primitive: {stability_rate:.4} \
             (gate: {stability_ge_90pct})"
        );
        println!(
            "real structured unseen feed evidence (WANDS): {real_feed_evidence} \
             (status: {REAL_FEED_EVIDENCE_STATUS} -- WANDS has no real Variant concept and never \
             exercises its 2 oracle-labeled Relationship fields anywhere in this pipeline; see \
             docs/experiments/ISSUE42_LOG.md's WANDS-qualification audit)"
        );
        println!(
            "NOTE: this binary computes 5 of Issue #42's own 6 preregistered E2b GO-gate \
             criteria. Criterion 5 (<=5% serving overhead vs the hand-authored compiled oracle) \
             is measured by a separate binary, e2b_serving_overhead_eval, and is NOT included in \
             the `go` boolean below -- see docs/experiments/ISSUE42_LOG.md's corrected \
             six-criterion table for the combined verdict."
        );

        let go = zero_unsafe
            && recall_significant_ge_80pct >= 0.80
            && relevance_within_5pct
            && stability_ge_90pct
            && real_feed_evidence;
        println!(
            "=> E2b GO gate (5 of 6 criteria computed by this binary; criterion 5 excluded, see \
             note above): {}",
            if go { "PASS" } else { "FAIL" }
        );

        let summary = serde_json::json!({
            "experiment_id": "I42-E2b",
            "baseline_sha": BASELINE_SHA,
            "baselines": summary_baselines,
            "repeated_run_agreement": stability_rate,
            "repeated_run_agreement_numerator": stability_agreements,
            "repeated_run_agreement_denominator": stability_total,
            "repeated_run_agreement_by_configuration": per_config_stability.iter().map(|(k, (a, t))| {
                (k.clone(), serde_json::json!({"agreements": a, "total": t, "rate": if *t == 0 { 0.0 } else { *a as f64 / *t as f64 }}))
            }).collect::<BTreeMap<_, _>>(),
            "unsafe_accepted_count": unsafe_accepted_count,
            "macro_f1_by_configuration": per_config_f1,
            "end_to_end": {
                "oracle": {"ndcg": oracle_e2e.ndcg, "recall": oracle_e2e.recall, "n_queries_scored": oracle_e2e.n_scored, "mean_match_set_size": oracle_e2e.mean_match_set_size},
                "statistics_only": {"ndcg": stats_e2e.ndcg, "recall": stats_e2e.recall, "n_queries_scored": stats_e2e.n_scored, "mean_match_set_size": stats_e2e.mean_match_set_size},
                "llm_plus_validator": {"ndcg": validated_e2e.ndcg, "recall": validated_e2e.recall, "n_queries_scored": validated_e2e.n_scored, "mean_match_set_size": validated_e2e.mean_match_set_size},
                "relative_ndcg_gap_llm_plus_validator_vs_oracle": relative_ndcg_gap,
                "check_reliable": e2e_check_reliable,
            },
            "go_gate": {
                "zero_unsafe_accepted": zero_unsafe,
                "recall_significant_ge_80pct": recall_significant_ge_80pct,
                "relevance_within_5pct": relevance_within_5pct,
                "relevance_check_reliable": e2e_check_reliable,
                "stability_ge_90pct": stability_ge_90pct,
                "real_feed_evidence": real_feed_evidence,
                "real_feed_evidence_status": REAL_FEED_EVIDENCE_STATUS,
                "go": go,
                "go_note": "This binary computes 5 of Issue #42's own 6 preregistered E2b GO-gate \
                    criteria (criterion 5, <=5% serving overhead, is measured separately by \
                    e2b_serving_overhead_eval and is NOT a term in `go` above). `go` here is \
                    therefore a necessary-but-not-sufficient partial check, never a complete \
                    six-criterion verdict on its own -- see \
                    docs/experiments/ISSUE42_LOG.md's corrected six-criterion table for the \
                    combined verdict across both binaries.",
            },
        });
        println!("\n{}", serde_json::to_string_pretty(&summary).unwrap());

        if let Some(path) = env::args().nth(1) {
            fs::write(&path, serde_json::to_string_pretty(&summary).unwrap())
                .expect("write summary json");
            println!("summary written to {path}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use issue42_eval::e2b_schema::{
        LlmPassOutput, Operator, PhysicalPrimitive, Scope, Significance, ValueType,
    };

    fn descriptor(role: SemanticRole, abstain: bool) -> Descriptor {
        Descriptor {
            key: "k".to_string(),
            real_key: None,
            semantic_role: role,
            value_type: ValueType::String,
            scope: Scope::Product,
            supported_operators: vec![Operator::Eq],
            aliases: vec![],
            relationship_semantics: None,
            retrieval_significance: Significance::RetrievalSignificant,
            candidate_physical_primitive: PhysicalPrimitive::BitmapEnum,
            confidence: 0.9,
            evidence: "test".to_string(),
            abstain,
        }
    }

    /// Regression test for a real bug: an earlier version of this
    /// binary counted `unsafe_accepted_count` by incrementing on every
    /// validator *rejection* (`!validation.accepted`) -- i.e. it
    /// measured the validator successfully catching a bad proposal, the
    /// opposite of "unsafe accepted structural classification". A
    /// rejected/abstained field must never count, and an accepted field
    /// whose oracle role genuinely IS structural (Enum/Numeric/Boolean)
    /// must never count either -- only an *accepted* structural facet
    /// whose oracle-confirmed true nature is Identifier or Relationship
    /// is the unsafe case.
    #[test]
    fn unsafe_accepted_count_flags_only_accepted_structural_identity_conflation() {
        let mut validated = BTreeMap::new();
        validated.insert(
            "part_number".to_string(),
            descriptor(SemanticRole::Enum, false),
        );
        validated.insert("color".to_string(), descriptor(SemanticRole::Enum, false));
        validated.insert(
            "compatible_chair".to_string(),
            descriptor(SemanticRole::Relationship, false),
        );
        validated.insert(
            "rejected_field".to_string(),
            descriptor(SemanticRole::Ignore, true),
        );

        let mut oracle = BTreeMap::new();
        oracle.insert("part_number".to_string(), SemanticRole::Identifier);
        oracle.insert("color".to_string(), SemanticRole::Enum);
        oracle.insert("compatible_chair".to_string(), SemanticRole::Relationship);
        oracle.insert("rejected_field".to_string(), SemanticRole::Identifier);

        // Only "part_number": accepted as a structural Enum despite the
        // oracle confirming it is really an Identifier. "color" is a
        // genuine, correctly-typed Enum (not unsafe). "compatible_chair"
        // is accepted with the correct Relationship role, not treated as
        // structural at all, so it does not count. "rejected_field" was
        // never accepted (abstained), so a validator rejection is never
        // counted as an unsafe acceptance.
        assert_eq!(unsafe_accepted_count(&validated, &oracle), 1);
    }

    #[test]
    fn unsafe_accepted_count_is_zero_when_nothing_conflates_identity() {
        let mut validated = BTreeMap::new();
        validated.insert("color".to_string(), descriptor(SemanticRole::Enum, false));
        let mut oracle = BTreeMap::new();
        oracle.insert("color".to_string(), SemanticRole::Enum);
        assert_eq!(unsafe_accepted_count(&validated, &oracle), 0);
    }

    fn pass(config: &str, run_index: u32, descriptors: Vec<Descriptor>) -> LlmPassOutput {
        LlmPassOutput {
            configuration: config.to_string(),
            run_index,
            descriptors,
        }
    }

    fn unified(key: &str) -> UnifiedFieldStats {
        UnifiedFieldStats {
            key: key.to_string(),
            occurrences: 1000,
            distinct_values: 50,
            uniqueness_ratio: 0.05,
            numeric_parseable_fraction: 0.0,
            mean_value_length: 8.0,
            variant_scoped: Some(true),
            sample_values: vec!["blue".to_string(), "red".to_string()],
        }
    }

    /// Regression test for a real bug (found by a fresh adversarial
    /// review): `per_config_runs` is a `BTreeMap`, iterated in
    /// alphabetical config-name order, not the intended
    /// "canonical/unperturbed configs only" order. Since
    /// `no_validator_by_key`/`validated_by_key` used `.entry().or_insert()`
    /// across EVERY config (not just the canonical ones), and
    /// "wands_anonymized" sorts before "wands_baseline" alphabetically,
    /// every WANDS real key's headline classification silently came from
    /// the ANONYMIZED config -- the hardest perturbation -- rather than
    /// the intended "real key names visible" baseline. This test
    /// reproduces exactly that scenario: an anonymized-config proposal
    /// with a wrong role for "color" must NOT win over a
    /// canonical-config ("wands_baseline") proposal with the correct
    /// role, regardless of which one a `BTreeMap` would visit first.
    #[test]
    fn headline_baselines_prefer_the_canonical_config_over_alphabetical_order() {
        let wrong_from_anonymized = Descriptor {
            key: "color".to_string(),
            semantic_role: SemanticRole::Numeric,
            ..descriptor(SemanticRole::Numeric, false)
        };
        let correct_from_baseline = Descriptor {
            key: "color".to_string(),
            semantic_role: SemanticRole::Enum,
            ..descriptor(SemanticRole::Enum, false)
        };

        let mut per_config_runs = BTreeMap::new();
        // "wands_anonymized" sorts alphabetically before "wands_baseline"
        // -- exactly the ordering that triggered the original defect.
        per_config_runs.insert(
            "wands_anonymized".to_string(),
            vec![pass(
                "wands_anonymized",
                1,
                vec![wrong_from_anonymized.clone()],
            )],
        );
        per_config_runs.insert(
            "wands_baseline".to_string(),
            vec![pass("wands_baseline", 1, vec![correct_from_baseline])],
        );

        let anon_mapping = BTreeMap::new();
        let noisy_mapping = BTreeMap::new();
        let mut all_unified = BTreeMap::new();
        all_unified.insert("color".to_string(), unified("color"));
        let mut oracle_by_key = BTreeMap::new();
        oracle_by_key.insert("color".to_string(), SemanticRole::Enum);

        let result = build_baselines_2_and_3(
            &per_config_runs,
            &anon_mapping,
            &noisy_mapping,
            &all_unified,
            &[],
            &oracle_by_key,
        );

        assert_eq!(
            result.no_validator_by_key["color"].semantic_role,
            SemanticRole::Enum,
            "Baseline 2 must reflect wands_baseline's real proposal, not wands_anonymized's, \
             regardless of BTreeMap iteration order"
        );
        assert_eq!(
            result.validated_by_key["color"].semantic_role,
            SemanticRole::Enum
        );
        assert!(!result.no_validator_by_key["color"].abstain);
    }

    /// Regression test for the missing preregistered "type consistency"
    /// check (found by a fresh adversarial review): two runs of the
    /// SAME canonical config disagreeing between a categorical and a
    /// Numeric role for the same key (automotive's own real
    /// `thread_size` enum/numeric disagreement is the concrete case)
    /// must force that key to abstain in Baseline 3 (the validator's
    /// job), while Baseline 2 (explicitly "no validator") still reflects
    /// whichever run happened to resolve first -- the whole point of
    /// the no-validator/validator comparison.
    #[test]
    fn same_config_run1_vs_run2_type_conflict_forces_baseline_3_to_abstain() {
        let run1_enum = Descriptor {
            key: "thread_size".to_string(),
            semantic_role: SemanticRole::Enum,
            ..descriptor(SemanticRole::Enum, false)
        };
        let run2_numeric = Descriptor {
            key: "thread_size".to_string(),
            semantic_role: SemanticRole::Numeric,
            ..descriptor(SemanticRole::Numeric, false)
        };

        let mut per_config_runs = BTreeMap::new();
        per_config_runs.insert(
            "automotive".to_string(),
            vec![
                pass("automotive", 1, vec![run1_enum]),
                pass("automotive", 2, vec![run2_numeric]),
            ],
        );

        let anon_mapping = BTreeMap::new();
        let noisy_mapping = BTreeMap::new();
        let mut all_unified = BTreeMap::new();
        all_unified.insert("thread_size".to_string(), unified("thread_size"));
        let mut oracle_by_key = BTreeMap::new();
        oracle_by_key.insert("thread_size".to_string(), SemanticRole::Enum);

        let result = build_baselines_2_and_3(
            &per_config_runs,
            &anon_mapping,
            &noisy_mapping,
            &all_unified,
            &[],
            &oracle_by_key,
        );

        assert!(
            result.validated_by_key["thread_size"].abstain,
            "an unresolved Enum-vs-Numeric conflict between two runs of the same config must \
             force Baseline 3 to abstain, not silently accept whichever run resolved first"
        );
        assert!(
            !result.no_validator_by_key["thread_size"].abstain,
            "Baseline 2 has no validator by definition -- it must NOT apply this check"
        );
    }
}
