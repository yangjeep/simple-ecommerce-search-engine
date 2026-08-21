//! Issue #14 P3-E05: real-data relevance verdict for a *structurally-
//! anchored* variant of lexical narrowing. P3-E03 measured
//! `admission::admit_lexically_narrowed` on the full `UnresolvedResidual`
//! population and REJECTed it: every swept cap failed every one of Issue
//! #14's four relevance budgets, because independent per-token presence
//! verification is a weak precision signal with no ranking function
//! behind it. P3-E04's diagnostic (no Solr call needed -- reused P3-E03's
//! own per-query data) found that restricting to queries that *also*
//! carry an existing structural constraint (Brand/ProductType/etc.)
//! alongside the residual text shows a consistently smaller NDCG delta
//! and a 2-3x lower false-positive rate than pure-lexical-only queries,
//! widening as the cap loosens -- exactly the kind of independent
//! precision anchor P3-E02 found tolerates having no ranking signal
//! reasonably well.
//!
//! This binary supplies the real, non-approximated verdict: unlike
//! P3-E04's diagnostic (which computed whole-workload impact from bucket
//! *means*, an approximation), this does a fresh whole-corpus Solr pass
//! and computes whole-workload degradation from each non-admitted query's
//! own real Solr score, exactly matching P3-E02/P3-E03's own methodology.
//!
//! No new commerce_core mechanism is needed for this experiment:
//! `admit_lexically_narrowed` already handles the "combined with an
//! existing structural constraint" case correctly (P3-E00's own tests
//! cover it). The restriction under test here -- never invoke lexical
//! narrowing when `query.constraints` is empty -- is a *policy* decision
//! at the call site, applied below by filtering the eligible population
//! before the cap sweep. If this measurement supports the policy, the
//! next step is to harden it into an explicit, tested commerce_core
//! contract rather than leaving it as caller discipline; if not, this
//! stays a documented negative result like P3-E03.
//!
//! Usage: cargo run --release -p phase3-eval --bin p3e05_structural_anchored_lexical_eval
//!        [catalog.jsonl] [queries.jsonl] [solr_base_url]

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
use std::time::Instant;

use bench_harness::{Distribution, RunManifest};
use commerce_core::admission::{
    admit, admit_lexically_narrowed, execute_lexically_narrowed, AdmissionDecision,
    AdmissionPolicy, RejectReason,
};
use commerce_core::cold_start::{compile_lexicon, CatalogProfile};
use commerce_core::domain::{BrandId, ProductId};
use commerce_core::index::CatalogIndex;
use commerce_core::ir::{compile, CommerceQuery};
use round1_eval::catalog;
use round1_eval::data::{self, EsciLabel};
use round1_eval::relevance::ndcg_recall_mrr;
use round1_eval::solr::{extract_brand_color, solr_query_for, solr_search};

const K: usize = 10;
const SEED: u64 = 7;
/// Same log-scale sweep P3-E02/P3-E03 used, for direct comparability.
const SWEEP: &[usize] = &[
    1, 2, 3, 5, 10, 20, 30, 50, 75, 100, 150, 250, 500, 1_000, 2_500, 5_000, 10_000, 50_000,
    200_000,
];
const UNLIMITED_CAP: usize = usize::MAX;

struct LexNarrowedQuery {
    qid: u64,
    combined_count: u64,
    native_ids: Vec<String>,
    native_ndcg: f64,
    native_recall: f64,
    native_mrr: f64,
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
    let solr_base_url = args
        .next()
        .unwrap_or_else(|| "http://localhost:8983/solr/commerce_bench".to_string());

    println!("loading + ingesting real catalog...");
    let products = data::load_catalog(&catalog_path);
    let ingested = catalog::build_catalog(&products);
    println!("{} real products ingested", ingested.catalog.products.len());

    println!("building commerce_core structural index...");
    let index = CatalogIndex::build(&ingested.catalog);

    println!("checking Solr ({solr_base_url})...");
    let Some(ping) = solr_search(&solr_base_url, "*:*", &[], 0) else {
        eprintln!("Solr NOT reachable at {solr_base_url} -- P3-E05 requires a live Solr instance. Aborting.");
        std::process::exit(1);
    };
    println!("  Solr reachable: numFound={}", ping.num_found);

    let brand_name_by_id: HashMap<BrandId, String> = ingested
        .brands
        .iter()
        .map(|b| (b.id, b.name.clone()))
        .collect();
    let product_id_to_asin: HashMap<ProductId, String> = ingested
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

    println!("\ncompiling every real query...");
    let compiled_cache: BTreeMap<u64, CommerceQuery> = judged_by_query
        .iter()
        .map(|(&qid, (text, _))| (qid, compile(text, &lexicon)))
        .collect();

    println!("querying Solr for every real query (whole-corpus pure-Solr baseline)...");
    let t0 = Instant::now();
    let mut solr_ndcg: BTreeMap<u64, (f64, f64, f64, usize)> = BTreeMap::new();
    for (&qid, (text, judged)) in &judged_by_query {
        let compiled = &compiled_cache[&qid];
        let (brand, color) = extract_brand_color(&compiled.constraints, &brand_name_by_id);
        let (q, fq) = solr_query_for(
            text,
            &compiled.residual_lexical,
            brand.as_deref(),
            color.as_deref(),
        );
        let ids = solr_search(&solr_base_url, &q, &fq, K)
            .map(|r| r.ids)
            .unwrap_or_default();
        let hit_count = ids.len();
        let (ndcg, recall, mrr) = ndcg_recall_mrr(&ids, judged, K);
        solr_ndcg.insert(qid, (ndcg, recall, mrr, hit_count));
    }
    println!(
        "  done in {:.1}s ({} queries)",
        t0.elapsed().as_secs_f64(),
        solr_ndcg.len()
    );
    let solr_only_ndcg_mean: f64 =
        solr_ndcg.values().map(|(n, _, _, _)| n).sum::<f64>() / total as f64;
    println!("\nwhole-workload pure-Solr-only baseline NDCG@10: {solr_only_ndcg_mean:.4}");

    println!(
        "\nfinding UnresolvedResidual-rejected queries WITH an existing structural constraint + lexical-narrowing eligibility..."
    );
    let unlimited_structural = AdmissionPolicy {
        max_candidates: UNLIMITED_CAP,
    };
    let mut rejected_residual_count = 0usize;
    let mut rejected_residual_with_constraint_count = 0usize;
    let mut blocked = 0usize;
    let mut eligible: Vec<LexNarrowedQuery> = Vec::new();
    let mut variant_correctness_violations = 0usize;
    for (&qid, compiled) in &compiled_cache {
        let AdmissionDecision::Reject(RejectReason::UnresolvedResidual) =
            admit(compiled, &index, &unlimited_structural)
        else {
            continue;
        };
        rejected_residual_count += 1;

        // The policy under test: never invoke lexical narrowing when
        // there is no existing structural constraint to anchor it --
        // P3-E04's own finding. Everything else about `admit_lexically_narrowed`
        // is unchanged from P3-E03.
        if compiled.constraints.is_empty() {
            continue;
        }
        rejected_residual_with_constraint_count += 1;

        let Some((narrow_by, combined_count)) =
            admit_lexically_narrowed(compiled, &index, UNLIMITED_CAP)
        else {
            blocked += 1;
            continue;
        };

        let hits = execute_lexically_narrowed(&index, compiled, &narrow_by, &ingested.catalog, K);
        for h in &hits {
            let product = ingested
                .catalog
                .products
                .iter()
                .find(|p| p.id == h.product)
                .expect(
                    "execute_lexically_narrowed only returns products that exist in this catalog",
                );
            let variant = product.variants.iter().find(|v| v.id == h.variant).expect(
                "execute_lexically_narrowed only returns variants that exist on their product",
            );
            if !compiled.matches_variant(product, variant) {
                variant_correctness_violations += 1;
            }
        }
        let native_ids: Vec<String> = hits
            .iter()
            .filter_map(|h| product_id_to_asin.get(&h.product).cloned())
            .collect();
        let (_, judged) = &judged_by_query[&qid];
        let (native_ndcg, native_recall, native_mrr) =
            ndcg_recall_mrr(native_ids.as_slice(), judged, K);
        eligible.push(LexNarrowedQuery {
            qid,
            combined_count,
            native_ids,
            native_ndcg,
            native_recall,
            native_mrr,
        });
    }
    println!(
        "  {rejected_residual_count}/{total} queries rejected for UnresolvedResidual ({:.2}% of corpus)",
        rejected_residual_count as f64 / total as f64 * 100.0
    );
    println!(
        "  {rejected_residual_with_constraint_count}/{rejected_residual_count} of those also carry an existing structural constraint (the policy under test)"
    );
    println!(
        "  {blocked}/{rejected_residual_with_constraint_count} blocked outright (out-of-vocabulary token or empty AND-combination, cap-independent)"
    );
    println!(
        "  {}/{rejected_residual_with_constraint_count} have a real combined candidate count (cap-independent, will be swept below)",
        eligible.len()
    );
    println!(
        "  variant-correctness violations: {variant_correctness_violations} (must be 0 -- commerce_core always exactly re-verifies hard constraints)"
    );
    assert_eq!(
        variant_correctness_violations, 0,
        "a lexically-narrowed hit failed its own compiled query's hard constraints -- this is a commerce_core correctness bug, not a Phase 3 harness issue"
    );

    // Native execution latency, measured once -- same rationale and
    // per-query individual-timing convention as P3-E02/P3-E03.
    {
        let mut small_sample: Vec<&LexNarrowedQuery> = eligible
            .iter()
            .filter(|eq| eq.combined_count <= 10)
            .collect();
        small_sample.sort_by_key(|eq| eq.qid);
        let sample: Vec<u64> = small_sample.iter().take(20).map(|eq| eq.qid).collect();
        if sample.is_empty() {
            println!(
                "\n(native latency measurement skipped: no eligible query has <=10 combined candidates)"
            );
        } else {
            let narrow_by_cache: HashMap<u64, roaring::RoaringBitmap> = sample
                .iter()
                .map(|&qid| {
                    let compiled = &compiled_cache[&qid];
                    let (narrow_by, _) =
                        admit_lexically_narrowed(compiled, &index, UNLIMITED_CAP).unwrap();
                    (qid, narrow_by)
                })
                .collect();
            for _ in 0..5 {
                for &qid in &sample {
                    let compiled = &compiled_cache[&qid];
                    let _ = execute_lexically_narrowed(
                        &index,
                        compiled,
                        &narrow_by_cache[&qid],
                        &ingested.catalog,
                        K,
                    );
                }
            }
            let mut per_query: Vec<f64> = Vec::with_capacity(sample.len() * 30);
            for _ in 0..30 {
                for &qid in &sample {
                    let compiled = &compiled_cache[&qid];
                    let start = Instant::now();
                    let _ = execute_lexically_narrowed(
                        &index,
                        compiled,
                        &narrow_by_cache[&qid],
                        &ingested.catalog,
                        K,
                    );
                    per_query.push(start.elapsed().as_secs_f64() * 1000.0);
                }
            }
            let d = Distribution::compute(&per_query);
            println!(
                "\nnative structurally-anchored lexically-narrowed execution latency (n={} small-combined-candidate-set queries, 30 reps each, {} individual samples):",
                sample.len(),
                per_query.len()
            );
            d.print("native_execute_lexically_narrowed_anchored", "ms");
            bench_harness::append_summary_row(
                &PathBuf::from("dataset_cache/p3e05_artifacts/summary_latency.csv"),
                "p3e05_structural_anchored_lexical_eval",
                "native_execute_lexically_narrowed_anchored",
                "latency_ms",
                &d,
            )
            .ok();
        }
    }

    let artifacts_dir = PathBuf::from("dataset_cache/p3e05_artifacts");
    let manifest = RunManifest::capture(
        "p3e05_structural_anchored_lexical_eval",
        "max_lexical_narrowed_candidates_sweep_structural_anchor_only",
        &catalog_path,
        &queries_path,
        serde_json::json!({
            "sweep": SWEEP,
            "rejected_residual_count": rejected_residual_count,
            "rejected_residual_with_constraint_count": rejected_residual_with_constraint_count,
            "blocked": blocked,
            "eligible_queries": eligible.len(),
            "total_queries": total,
            "solr_base_url": solr_base_url,
        }),
        SEED,
        0,
        1,
    );
    manifest.print();
    manifest
        .write_json(&artifacts_dir.join("manifest.json"))
        .ok();

    {
        use std::io::Write;
        std::fs::create_dir_all(&artifacts_dir).ok();
        let mut f = std::fs::File::create(artifacts_dir.join("eligible_queries_raw.csv")).unwrap();
        writeln!(
            f,
            "qid,combined_count,native_ndcg,native_recall,native_mrr,solr_ndcg,solr_recall,solr_mrr,native_hit_count,solr_hit_count"
        )
        .unwrap();
        for eq in &eligible {
            let (s_ndcg, s_recall, s_mrr, s_hits) = solr_ndcg[&eq.qid];
            writeln!(
                f,
                "{},{},{},{},{},{},{},{},{},{}",
                eq.qid,
                eq.combined_count,
                eq.native_ndcg,
                eq.native_recall,
                eq.native_mrr,
                s_ndcg,
                s_recall,
                s_mrr,
                eq.native_ids.len(),
                s_hits
            )
            .unwrap();
        }
    }

    println!("\n=== P3-E05 structurally-anchored lexical-narrowing coverage/relevance frontier (isolated marginal contribution) ===");
    println!(
        "{:>10} {:>10} {:>10} {:>9} {:>10} {:>10} {:>10} {:>12} {:>10} {:>10}",
        "cap",
        "admitted",
        "cov%_anch",
        "cov%_all",
        "native_ndcg",
        "solr_ndcg_sub",
        "ndcg_delta",
        "whole_wl_ndcg",
        "wl_degrad",
        "false_pos"
    );
    let mut frontier_csv = String::from(
        "max_lexical_narrowed_candidates,admitted,coverage_pct_of_anchored_pool,coverage_pct_of_whole_corpus,native_ndcg_mean,solr_ndcg_on_admitted_mean,ndcg_delta_on_admitted,whole_workload_ndcg,whole_workload_degradation,zero_result_native,zero_result_solr_on_admitted,false_positive_admissions\n",
    );
    for &cap in SWEEP {
        let admitted: Vec<&LexNarrowedQuery> = eligible
            .iter()
            .filter(|eq| eq.combined_count as usize <= cap)
            .collect();
        let admitted_count = admitted.len();
        let coverage_pct_of_anchored =
            admitted_count as f64 / rejected_residual_with_constraint_count as f64 * 100.0;
        let coverage_pct_of_whole = admitted_count as f64 / total as f64 * 100.0;

        let native_ndcg_sum: f64 = admitted.iter().map(|eq| eq.native_ndcg).sum();
        let native_ndcg_mean = if admitted_count > 0 {
            native_ndcg_sum / admitted_count as f64
        } else {
            0.0
        };
        let solr_on_admitted_sum: f64 = admitted.iter().map(|eq| solr_ndcg[&eq.qid].0).sum();
        let solr_on_admitted_mean = if admitted_count > 0 {
            solr_on_admitted_sum / admitted_count as f64
        } else {
            0.0
        };
        let ndcg_delta_on_admitted = native_ndcg_mean - solr_on_admitted_mean;

        // Isolated marginal contribution, same convention as P3-E03:
        // every query NOT admitted here (including ones the original
        // structural admit() would separately admit, and pure-lexical-only
        // queries this policy deliberately never touches) is scored as a
        // Solr fallback, using each query's own real Solr score.
        let admitted_qids: HashSet<u64> = admitted.iter().map(|eq| eq.qid).collect();
        let rest_solr_sum: f64 = solr_ndcg
            .iter()
            .filter(|(qid, _)| !admitted_qids.contains(qid))
            .map(|(_, (n, _, _, _))| n)
            .sum();
        let whole_workload_ndcg = (native_ndcg_sum + rest_solr_sum) / total as f64;
        let whole_workload_degradation = solr_only_ndcg_mean - whole_workload_ndcg;

        let zero_result_native = admitted
            .iter()
            .filter(|eq| eq.native_ids.is_empty())
            .count();
        let zero_result_solr_on_admitted = admitted
            .iter()
            .filter(|eq| solr_ndcg[&eq.qid].3 == 0)
            .count();
        let false_positive_admissions = admitted
            .iter()
            .filter(|eq| eq.native_ndcg == 0.0 && solr_ndcg[&eq.qid].0 > 0.0)
            .count();

        println!(
            "{:>10} {:>10} {:>9.2}% {:>8.2}% {:>10.4} {:>10.4} {:>+10.4} {:>12.4} {:>+10.4} {:>10}",
            cap,
            admitted_count,
            coverage_pct_of_anchored,
            coverage_pct_of_whole,
            native_ndcg_mean,
            solr_on_admitted_mean,
            ndcg_delta_on_admitted,
            whole_workload_ndcg,
            whole_workload_degradation,
            false_positive_admissions
        );
        frontier_csv.push_str(&format!(
            "{cap},{admitted_count},{coverage_pct_of_anchored},{coverage_pct_of_whole},{native_ndcg_mean},{solr_on_admitted_mean},{ndcg_delta_on_admitted},{whole_workload_ndcg},{whole_workload_degradation},{zero_result_native},{zero_result_solr_on_admitted},{false_positive_admissions}\n"
        ));
    }
    std::fs::write(artifacts_dir.join("frontier_sweep.csv"), &frontier_csv).ok();

    println!(
        "\n=== relevance-budget calibration (Issue #14 RQ2, structurally-anchored policy) ==="
    );
    for budget_pct in [0.0, 0.5, 1.0, 2.0] {
        let mut best: Option<(usize, usize, f64)> = None;
        for &cap in SWEEP {
            let admitted: Vec<&LexNarrowedQuery> = eligible
                .iter()
                .filter(|eq| eq.combined_count as usize <= cap)
                .collect();
            let admitted_count = admitted.len();
            let native_ndcg_sum: f64 = admitted.iter().map(|eq| eq.native_ndcg).sum();
            let admitted_qids: HashSet<u64> = admitted.iter().map(|eq| eq.qid).collect();
            let rest_solr_sum: f64 = solr_ndcg
                .iter()
                .filter(|(qid, _)| !admitted_qids.contains(qid))
                .map(|(_, (n, _, _, _))| n)
                .sum();
            let whole_workload_ndcg = (native_ndcg_sum + rest_solr_sum) / total as f64;
            let degradation_pct =
                (solr_only_ndcg_mean - whole_workload_ndcg) / solr_only_ndcg_mean * 100.0;
            if degradation_pct <= budget_pct {
                let coverage = admitted_count as f64 / total as f64 * 100.0;
                if best.is_none_or(|(_, c, _)| admitted_count > c) {
                    best = Some((cap, admitted_count, coverage));
                }
            }
        }
        match best {
            Some((cap, count, coverage)) => println!(
                "  budget<={budget_pct:.1}%: best cap={cap}, coverage={count}/{total} ({coverage:.2}% of whole corpus)"
            ),
            None => println!(
                "  budget<={budget_pct:.1}%: no swept cap value stays within this budget"
            ),
        }
    }

    println!("\nartifacts written to {}", artifacts_dir.display());
}
