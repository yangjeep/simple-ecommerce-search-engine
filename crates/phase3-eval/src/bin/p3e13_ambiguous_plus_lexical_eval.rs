//! Issue #14/#18 P3-E13: real relevance verdict for a COMBINED mechanism
//! -- frequency-based ambiguity resolution (P3-E11/P3-E12) plus lexical
//! narrowing on whatever residual text remains (P3-E03/P3-E05/P3-E09's
//! own machinery), rather than requiring residual to already be empty.
//!
//! P3-E12 found the frequency-resolution mechanism alone recovers almost
//! nothing (0.11% whole-corpus coverage) because 98.2% of its tractable
//! target population (4,279/4,356 single-span, all-Constraint ambiguous
//! queries) also carries unresolved residual text elsewhere in the
//! query, which `admit()`'s own completeness rule correctly rejects.
//! This binary directly targets that bottleneck: instead of discarding a
//! query with leftover residual, verify every residual token is known
//! in the catalog's own `lexical_postings` index (the identical
//! out-of-vocabulary check `admit_lexically_narrowed` already uses,
//! including its own already-fixed guaranteed-empty-combination bug --
//! see `docs/experiments/PHASE3_LOG.md` P3-E03), then AND-narrow the
//! frequency-resolved structural candidate set by those tokens.
//!
//! Requires no live Solr querying: reuses P3-E06's already-persisted
//! whole-corpus Solr baseline exactly as P3-E12 did.
//!
//! Two tunable dimensions, both real safety levers, mirroring every
//! prior Phase 3 sweep: the frequency-ratio threshold (unchanged from
//! P3-E12) and the resulting combined candidate-set cap (fixed at 250,
//! this project's own repeatedly-useful default, matching P3-E12's own
//! choice so the two experiments are directly comparable).
//!
//! Usage: cargo run --release -p phase3-eval --bin p3e13_ambiguous_plus_lexical_eval
//!        [catalog.jsonl] [queries.jsonl] [p3e06_whole_corpus_csv]

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;

use commerce_core::admission::{admit, AdmissionDecision, AdmissionPolicy, RejectReason};
use commerce_core::cold_start::{compile_lexicon, CatalogProfile};
use commerce_core::index::CatalogIndex;
use commerce_core::ir::compile;
use commerce_core::ir::lexicon::ResolvedTerm;
use round1_eval::catalog;
use round1_eval::data::{self, EsciLabel};
use round1_eval::relevance::ndcg_recall_mrr;

const K: usize = 10;
const RESOLVED_CANDIDATE_CAP: usize = 250;
const RATIO_SWEEP: &[f64] = &[1.0, 2.0, 3.0, 5.0, 10.0, 20.0, 50.0, 100.0];

struct AmbiguousQuery {
    qid: u64,
    ratio: f64,
    resolved_count: u64,
    native_ids: Vec<String>,
    native_ndcg: f64,
}

fn main() {
    let mut args = std::env::args().skip(1);
    let catalog_path = args
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("dataset_cache/export/catalog.jsonl"));
    let queries_path = args
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("dataset_cache/export/queries.jsonl"));
    let p3e06_csv = args.next().map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from("docs/research/artifacts/p3e06_run1/whole_corpus_solr_ndcg.csv")
    });

    println!("loading + ingesting real catalog...");
    let products = data::load_catalog(&catalog_path);
    let ingested = catalog::build_catalog(&products);
    println!("{} real products ingested", ingested.catalog.products.len());

    println!("building commerce_core structural index...");
    let index = CatalogIndex::build(&ingested.catalog);

    let product_id_to_asin: HashMap<_, _> = ingested
        .asin_to_product_id
        .iter()
        .map(|(asin, pid)| (*pid, asin.clone()))
        .collect();
    let profile = CatalogProfile::build(&ingested.catalog, &ingested.brands, &[], &[]);
    let lexicon = compile_lexicon(&profile, 25);

    println!("loading real queries + judgments...");
    let judgments = data::load_queries(&queries_path);
    let mut judged_by_query: BTreeMap<u64, (String, BTreeMap<String, EsciLabel>)> = BTreeMap::new();
    for j in &judgments {
        judged_by_query
            .entry(j.query_id)
            .or_insert_with(|| (j.query.clone(), BTreeMap::new()))
            .1
            .insert(j.product_id.clone(), j.label);
    }
    judged_by_query.retain(|_, (_, judged)| judged.values().any(|l| l.is_relevant()));
    let total = judged_by_query.len();
    println!("{total} distinct real judged queries with at least one relevant label loaded");

    println!("loading already-persisted whole-corpus Solr baseline from {p3e06_csv:?} (reused, no new Solr querying)...");
    let solr_ndcg: HashMap<u64, f64> = std::fs::read_to_string(&p3e06_csv)
        .unwrap_or_else(|e| panic!("failed to read {p3e06_csv:?}: {e}"))
        .lines()
        .skip(1)
        .filter(|l| !l.is_empty())
        .map(|l| {
            let mut cols = l.split(',');
            let qid: u64 = cols.next().unwrap().parse().unwrap();
            let ndcg: f64 = cols.next().unwrap().parse().unwrap();
            (qid, ndcg)
        })
        .collect();
    assert_eq!(
        solr_ndcg.len(),
        total,
        "the persisted whole-corpus Solr baseline must cover exactly this run's judged-query set"
    );
    let solr_only_ndcg_mean: f64 = solr_ndcg.values().sum::<f64>() / total as f64;
    println!("  whole-workload pure-Solr-only baseline NDCG@10: {solr_only_ndcg_mean:.4}");

    println!("\nresolving tractable ambiguous queries by catalog-frequency-dominant candidate, lexically narrowing any leftover residual...");
    let unlimited_policy = AdmissionPolicy {
        max_candidates: usize::MAX,
    };
    let mut ambiguous_count = 0usize;
    let mut tractable: Vec<AmbiguousQuery> = Vec::new();
    let mut variant_correctness_violations = 0usize;
    let mut excluded_multi_span = 0usize;
    let mut excluded_not_all_constraint = 0usize;
    let mut excluded_zero_top_freq = 0usize;
    let mut excluded_oov_residual_token = 0usize;
    let mut excluded_combined_zero = 0usize;
    let mut had_residual_and_survived = 0usize;

    for (&qid, (text, judged)) in &judged_by_query {
        let compiled = compile(text, &lexicon);
        let AdmissionDecision::Reject(reason) = admit(&compiled, &index, &unlimited_policy) else {
            continue;
        };
        if reason != RejectReason::Ambiguous {
            continue;
        }
        ambiguous_count += 1;
        if compiled.ambiguous.len() != 1 {
            excluded_multi_span += 1;
            continue;
        }
        let span = &compiled.ambiguous[0];
        let constraint_candidates: Vec<_> = span
            .candidates
            .iter()
            .filter_map(|c| match &c.resolved {
                ResolvedTerm::Constraint(rc) => Some(rc.clone()),
                ResolvedTerm::Preference(_) => None,
            })
            .collect();
        if constraint_candidates.len() != span.candidates.len() || constraint_candidates.len() < 2 {
            excluded_not_all_constraint += 1;
            continue;
        }

        let mut freqs: Vec<(usize, u64)> = constraint_candidates
            .iter()
            .enumerate()
            .map(|(i, c)| (i, index.indexed_candidates(std::slice::from_ref(c)).len()))
            .collect();
        freqs.sort_unstable_by_key(|&(_, f)| std::cmp::Reverse(f));
        let (winner_idx, top_freq) = freqs[0];
        let second_freq = freqs[1].1;
        let ratio = if second_freq == 0 {
            f64::INFINITY
        } else {
            top_freq as f64 / second_freq as f64
        };
        if top_freq == 0 {
            excluded_zero_top_freq += 1;
            continue;
        }

        let mut resolved_query = compiled.clone();
        resolved_query.ambiguous.clear();
        resolved_query
            .constraints
            .push(constraint_candidates[winner_idx].clone());

        let structural_bitmap = index.indexed_candidates(&resolved_query.constraints);

        // The P3-E13 addition: instead of requiring residual_lexical
        // empty, lexically narrow it -- same out-of-vocabulary check and
        // guaranteed-empty-combination rejection admission.rs's own
        // admit_lexically_narrowed already established and fixed.
        let combined_bitmap = if resolved_query.residual_lexical.is_empty() {
            structural_bitmap
        } else {
            let residual_tokens: Vec<String> = resolved_query
                .residual_lexical
                .iter()
                .flat_map(|phrase| phrase.split_whitespace().map(str::to_lowercase))
                .collect();
            if residual_tokens.iter().any(|t| {
                index
                    .lexical_and_candidates(std::slice::from_ref(t))
                    .is_empty()
            }) {
                excluded_oov_residual_token += 1;
                continue;
            }
            let lexical_bitmap = index.lexical_and_candidates(&residual_tokens);
            lexical_bitmap & structural_bitmap
        };
        let resolved_count = combined_bitmap.len();
        if resolved_count == 0 {
            excluded_combined_zero += 1;
            continue;
        }
        if !resolved_query.residual_lexical.is_empty() {
            had_residual_and_survived += 1;
        }

        let hits = index.execute_ranked_narrowed_by(
            &resolved_query,
            &combined_bitmap,
            &ingested.catalog,
            K,
        );
        for h in &hits {
            let product = ingested
                .catalog
                .products
                .iter()
                .find(|p| p.id == h.product)
                .expect(
                    "execute_ranked_narrowed_by only returns products that exist in this catalog",
                );
            let variant = product.variants.iter().find(|v| v.id == h.variant).expect(
                "execute_ranked_narrowed_by only returns variants that exist on their product",
            );
            if !resolved_query.matches_variant(product, variant) {
                variant_correctness_violations += 1;
            }
        }
        let native_ids: Vec<String> = hits
            .iter()
            .filter_map(|h| product_id_to_asin.get(&h.product).cloned())
            .collect();
        let (native_ndcg, _, _) = ndcg_recall_mrr(native_ids.as_slice(), judged, K);

        tractable.push(AmbiguousQuery {
            qid,
            ratio,
            resolved_count,
            native_ids,
            native_ndcg,
        });
    }
    println!(
        "  {}/{} ambiguous-rejected queries are tractable (single-span, all-Constraint, resolvable, combined candidate set nonzero)",
        tractable.len(),
        ambiguous_count
    );
    println!(
        "  exclusion breakdown: multi_span={excluded_multi_span} not_all_constraint={excluded_not_all_constraint} zero_top_freq={excluded_zero_top_freq} oov_residual_token={excluded_oov_residual_token} combined_zero={excluded_combined_zero}"
    );
    println!(
        "  of the tractable queries, {had_residual_and_survived} had non-empty residual text safely lexically narrowed (this is the population P3-E12 could not reach at all)"
    );
    println!(
        "  variant-correctness violations: {variant_correctness_violations} (must be 0 -- commerce_core always exactly re-verifies hard constraints)"
    );
    assert_eq!(
        variant_correctness_violations, 0,
        "a combined-mechanism hit failed its own resolved query's hard constraints -- this is a \
         commerce_core correctness bug, not a Phase 3 harness issue"
    );

    println!("\n=== P3-E13 ambiguity+lexical combined coverage/relevance frontier (isolated marginal contribution, cap<={RESOLVED_CANDIDATE_CAP}) ===");
    println!(
        "{:>12} {:>10} {:>9} {:>10} {:>10} {:>10} {:>12} {:>10} {:>10}",
        "ratio>=",
        "admitted",
        "cov%_amb",
        "cov%_all",
        "native_ndcg",
        "solr_ndcg_sub",
        "ndcg_delta",
        "whole_wl_ndcg",
        "wl_degrad"
    );
    let mut csv = String::from(
        "ratio_threshold,admitted,coverage_pct_of_ambiguous,coverage_pct_of_whole_corpus,native_ndcg_mean,solr_ndcg_on_admitted_mean,ndcg_delta_on_admitted,whole_workload_ndcg,whole_workload_degradation,false_positive_admissions\n",
    );
    for &ratio_threshold in RATIO_SWEEP {
        let admitted: Vec<&AmbiguousQuery> = tractable
            .iter()
            .filter(|q| {
                q.ratio >= ratio_threshold && q.resolved_count as usize <= RESOLVED_CANDIDATE_CAP
            })
            .collect();
        let admitted_count = admitted.len();
        let coverage_pct_of_ambiguous = admitted_count as f64 / ambiguous_count as f64 * 100.0;
        let coverage_pct_of_whole = admitted_count as f64 / total as f64 * 100.0;

        let native_ndcg_sum: f64 = admitted.iter().map(|q| q.native_ndcg).sum();
        let native_ndcg_mean = if admitted_count > 0 {
            native_ndcg_sum / admitted_count as f64
        } else {
            0.0
        };
        let solr_on_admitted_sum: f64 = admitted.iter().map(|q| solr_ndcg[&q.qid]).sum();
        let solr_on_admitted_mean = if admitted_count > 0 {
            solr_on_admitted_sum / admitted_count as f64
        } else {
            0.0
        };
        let ndcg_delta_on_admitted = native_ndcg_mean - solr_on_admitted_mean;

        let admitted_qids: HashSet<u64> = admitted.iter().map(|q| q.qid).collect();
        let rest_solr_sum: f64 = solr_ndcg
            .iter()
            .filter(|(qid, _)| !admitted_qids.contains(qid))
            .map(|(_, n)| n)
            .sum();
        let whole_workload_ndcg = (native_ndcg_sum + rest_solr_sum) / total as f64;
        let whole_workload_degradation = solr_only_ndcg_mean - whole_workload_ndcg;
        let false_positive_admissions = admitted
            .iter()
            .filter(|q| q.native_ndcg == 0.0 && solr_ndcg[&q.qid] > 0.0)
            .count();

        println!(
            "{:>12} {:>10} {:>8.2}% {:>8.2}% {:>10.4} {:>10.4} {:>+10.4} {:>12.4} {:>+10.4}",
            ratio_threshold,
            admitted_count,
            coverage_pct_of_ambiguous,
            coverage_pct_of_whole,
            native_ndcg_mean,
            solr_on_admitted_mean,
            ndcg_delta_on_admitted,
            whole_workload_ndcg,
            whole_workload_degradation,
        );
        csv.push_str(&format!(
            "{ratio_threshold},{admitted_count},{coverage_pct_of_ambiguous},{coverage_pct_of_whole},{native_ndcg_mean},{solr_on_admitted_mean},{ndcg_delta_on_admitted},{whole_workload_ndcg},{whole_workload_degradation},{false_positive_admissions}\n"
        ));
    }

    let artifacts_dir = PathBuf::from("dataset_cache/p3e13_artifacts");
    std::fs::create_dir_all(&artifacts_dir).ok();
    std::fs::write(artifacts_dir.join("frontier_sweep.csv"), &csv).ok();

    {
        use std::io::Write;
        let mut f = std::fs::File::create(artifacts_dir.join("tractable_queries_raw.csv")).unwrap();
        writeln!(
            f,
            "qid,ratio,resolved_count,native_ndcg,solr_ndcg,native_hit_count"
        )
        .unwrap();
        for q in &tractable {
            writeln!(
                f,
                "{},{},{},{},{},{}",
                q.qid,
                if q.ratio.is_finite() {
                    q.ratio.to_string()
                } else {
                    "inf".to_string()
                },
                q.resolved_count,
                q.native_ndcg,
                solr_ndcg[&q.qid],
                q.native_ids.len()
            )
            .unwrap();
        }
    }

    println!("\n=== relevance-budget calibration (Issue #14 RQ2, ambiguity+lexical combined mechanism) ===");
    for budget_pct in [0.0, 0.5, 1.0, 2.0] {
        let mut best: Option<(f64, usize, f64)> = None;
        for &ratio_threshold in RATIO_SWEEP {
            let admitted: Vec<&AmbiguousQuery> = tractable
                .iter()
                .filter(|q| {
                    q.ratio >= ratio_threshold
                        && q.resolved_count as usize <= RESOLVED_CANDIDATE_CAP
                })
                .collect();
            let admitted_count = admitted.len();
            let native_ndcg_sum: f64 = admitted.iter().map(|q| q.native_ndcg).sum();
            let admitted_qids: HashSet<u64> = admitted.iter().map(|q| q.qid).collect();
            let rest_solr_sum: f64 = solr_ndcg
                .iter()
                .filter(|(qid, _)| !admitted_qids.contains(qid))
                .map(|(_, n)| n)
                .sum();
            let whole_workload_ndcg = (native_ndcg_sum + rest_solr_sum) / total as f64;
            let degradation_pct =
                (solr_only_ndcg_mean - whole_workload_ndcg) / solr_only_ndcg_mean * 100.0;
            if degradation_pct <= budget_pct {
                let coverage = admitted_count as f64 / total as f64 * 100.0;
                if best.is_none_or(|(_, c, _)| admitted_count > c) {
                    best = Some((ratio_threshold, admitted_count, coverage));
                }
            }
        }
        match best {
            Some((ratio, count, coverage)) => println!(
                "  budget<={budget_pct:.1}%: best ratio_threshold={ratio}, coverage={count}/{total} ({coverage:.2}% of whole corpus)"
            ),
            None => println!(
                "  budget<={budget_pct:.1}%: no swept ratio threshold stays within this budget"
            ),
        }
    }
    println!("\nartifacts written to {}", artifacts_dir.display());
}
