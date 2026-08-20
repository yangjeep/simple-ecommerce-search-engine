//! Issue #14/#18 P3-E12: real relevance verdict for frequency-based
//! ambiguity resolution. P3-E11's diagnostic found that within the
//! tractable subclass (exactly one ambiguous span, every candidate a
//! real hard Constraint -- 4,356/5,005 ambiguous-rejected queries,
//! 87.03%), the catalog-frequency-dominant candidate (via
//! `CatalogIndex::indexed_candidates` on a single-element constraint
//! slice -- no delegate call, no learned/model signal) is overwhelming:
//! 89.05% have a top candidate at least 10x more catalog-frequent than
//! the runner-up. Candidate-set-size promise alone is not sufficient
//! evidence (the same lesson P3-E03 already taught this project): this
//! binary supplies the missing real NDCG@10/Recall@10/MRR evidence.
//!
//! Requires no live Solr querying: P3-E06's `whole_corpus_solr_ndcg.csv`
//! already carries every one of the 22,458 real queries' own Solr score,
//! computed once and persisted for exactly this kind of reuse -- this
//! includes every ambiguous query, since that whole-corpus pass ran
//! before any admission filtering. Only the *native* side needs fresh
//! computation: resolving each tractable ambiguous query by substituting
//! its frequency-dominant candidate as a real hard constraint, then
//! executing it exactly as `execute_admitted` would (this is not a new
//! execution path -- a resolved query looks exactly like any other
//! admitted query once the ambiguous span is gone), and scoring against
//! the real ESCI judgments.
//!
//! Two tunable dimensions, both real safety levers: the frequency-ratio
//! threshold (how dominant the winning candidate must be over the
//! runner-up to trust it) and the resulting candidate-set cap (the same
//! "small candidate set is safe without a ranking signal" principle
//! every other Phase 3 admission mechanism relies on). Fixes the cap at
//! 250 (this project's own repeatedly-useful default) and sweeps the
//! ratio threshold, mirroring every prior Phase 3 sweep's own discipline
//! of holding one dimension fixed at a reasonable value while
//! characterizing the other.
//!
//! A real anomaly, investigated rather than accepted: P3-E11's own
//! diagnostic reported 2,113 queries passing a nonzero/<=250 combined-
//! candidate check, but never verified `residual_lexical` was empty
//! after resolution -- only candidate-set size. This eval's own
//! exclusion breakdown (printed below) found 4,279/4,356 (98.2%) of the
//! single-span tractable subclass *still* carries unresolved residual
//! text elsewhere in the query even after resolving its one ambiguous
//! span, which `admit()`'s own "complete" requirement (empty residual)
//! correctly rejects. This is not a bug in either binary -- it is a
//! real, load-bearing fact about this corpus's queries: an ambiguous
//! phrase is usually just *one part* of a longer, multi-word real
//! shopper query, not the whole thing. Ambiguity resolution alone
//! therefore recovers almost nothing; a mechanism that also lexically
//! narrows the co-occurring residual (composing this technique with
//! P3-E03/P3-E05/P3-E09's own lexical-narrowing machinery) is the
//! natural next experiment, not a larger ratio/cap sweep on this one.
//!
//! Usage: cargo run --release -p phase3-eval --bin p3e12_ambiguous_frequency_eval
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
/// Ratio thresholds to sweep: `unlimited` (1.0, always pick top
/// regardless of margin) down to progressively stricter dominance
/// requirements.
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

    println!("\nresolving tractable ambiguous queries by catalog-frequency-dominant candidate...");
    let unlimited_policy = AdmissionPolicy {
        max_candidates: usize::MAX,
    };
    let mut ambiguous_count = 0usize;
    let mut tractable: Vec<AmbiguousQuery> = Vec::new();
    let mut variant_correctness_violations = 0usize;
    // Exclusion breakdown: P3-E11's diagnostic never checked whether a
    // query's residual_lexical was empty (it only checked combined
    // candidate-set size), so its 2,113-query estimate overstated real
    // tractability. This breaks down exactly why the real tractable
    // count is far smaller -- an anomaly investigated, not smoothed over
    // (see the module doc comment and PHASE3_LOG.md's P3-E12 entry).
    let mut excluded_multi_span = 0usize;
    let mut excluded_not_all_constraint = 0usize;
    let mut excluded_zero_top_freq = 0usize;
    let mut excluded_residual_still_nonempty = 0usize;
    let mut excluded_resolved_zero = 0usize;
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

        // Reject outright a query whose winner itself has zero real
        // catalog matches -- resolving to a constraint nothing satisfies
        // is not "safely narrows to zero," the same distinction
        // admission.rs's own doc comments have drawn for every other
        // mechanism in this phase.
        if top_freq == 0 {
            excluded_zero_top_freq += 1;
            continue;
        }

        let mut resolved_query = compiled.clone();
        resolved_query.ambiguous.clear();
        resolved_query
            .constraints
            .push(constraint_candidates[winner_idx].clone());
        if !resolved_query.residual_lexical.is_empty() {
            // Not "complete" per admit()'s own definition even after
            // resolving the ambiguity -- must still fall back. This is
            // the dominant exclusion reason by far: see PHASE3_LOG.md.
            excluded_residual_still_nonempty += 1;
            continue;
        }
        let resolved_count = index.indexed_candidates(&resolved_query.constraints).len();
        if resolved_count == 0 {
            excluded_resolved_zero += 1;
            continue;
        }

        let hits = index.execute_ranked(&resolved_query, &ingested.catalog, K);
        for h in &hits {
            let product = ingested
                .catalog
                .products
                .iter()
                .find(|p| p.id == h.product)
                .expect("execute_ranked only returns products that exist in this catalog");
            let variant = product
                .variants
                .iter()
                .find(|v| v.id == h.variant)
                .expect("execute_ranked only returns variants that exist on their product");
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
        "  {}/{} ambiguous-rejected queries are tractable (single-span, all-Constraint, resolvable, residual-lexical-empty after resolution)",
        tractable.len(),
        ambiguous_count
    );
    println!(
        "  exclusion breakdown: multi_span={excluded_multi_span} not_all_constraint={excluded_not_all_constraint} zero_top_freq={excluded_zero_top_freq} residual_still_nonempty_after_resolution={excluded_residual_still_nonempty} resolved_candidate_set_zero={excluded_resolved_zero}"
    );
    println!(
        "  variant-correctness violations: {variant_correctness_violations} (must be 0 -- commerce_core always exactly re-verifies hard constraints)"
    );
    assert_eq!(
        variant_correctness_violations, 0,
        "a frequency-resolved hit failed its own resolved query's hard constraints -- this is a \
         commerce_core correctness bug, not a Phase 3 harness issue"
    );

    println!("\n=== P3-E12 frequency-resolved-ambiguity coverage/relevance frontier (isolated marginal contribution, cap<={RESOLVED_CANDIDATE_CAP}) ===");
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

    let artifacts_dir = PathBuf::from("dataset_cache/p3e12_artifacts");
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

    println!("\n=== relevance-budget calibration (Issue #14 RQ2, frequency-resolved-ambiguity mechanism) ===");
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
