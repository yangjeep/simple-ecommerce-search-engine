//! Issue #14 P3-E03: real-data relevance verdict for
//! `admission::admit_lexically_narrowed` -- the mechanism that safely
//! admits a query with non-empty `residual_lexical` when every residual
//! token is verifiable via Round 1's native `lexical_and_candidates`
//! token-postings index, narrowing the combined structural+lexical
//! candidate set. The pre-implementation diagnostic
//! (`p3e03_residual_lexical_diagnostic`) found this *could* newly admit
//! 54.02% of residual-rejected queries (41.54% of the whole corpus) under
//! a combined cap<=250 -- but candidate-set-size promise alone does not
//! establish relevance safety (no ranking signal exists on this path
//! either, the same open risk P2-E17/P3-E02 already flagged for the
//! original admission mechanism). This binary supplies the missing real
//! NDCG@10/Recall@10/MRR evidence against both the real ESCI judgments and
//! what Solr actually returns for the same queries.
//!
//! Population and method mirror P3-E02 exactly, applied to a disjoint
//! subset: every query `admit()` rejects for `UnresolvedResidual` (P3-E02:
//! 76.89% of all real traffic, the dominant rejection reason) is a
//! candidate here. `admit_lexically_narrowed` is called once per query
//! with an unlimited cap to get its real combined candidate count
//! cap-independently (mirroring P3-E02's own `full_policy` trick); the
//! `max_lexical_narrowed_candidates` sweep below is then a pure post-filter
//! on that count, since -- per `bench_harness`'s documented methodology --
//! `commerce_core`'s compile/plan/execute path has no model call and no
//! randomness, so one deterministic pass answers every sweep point.
//!
//! The whole-workload metric below deliberately isolates *this*
//! mechanism's own marginal contribution: every query NOT admitted via
//! lexical narrowing -- including ones the *original* structural `admit()`
//! would separately admit -- is scored as a Solr fallback. This is a fair
//! standalone read of P3-E03's own increment, not a compounded frontier
//! with whatever cap eventually gets chosen for the original mechanism
//! (the two populations are disjoint by construction: `admit_lexically_narrowed`
//! requires non-empty `residual_lexical`, `admit`'s non-reject path
//! requires it empty -- so the two contributions are strictly additive and
//! can be characterized independently without double-counting).
//!
//! Usage: cargo run --release -p phase3-eval --bin p3e03_lexical_narrowing_eval
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
/// Same log-scale sweep P3-E02 used for `max_candidates`, reused here for
/// `max_lexical_narrowed_candidates` so the two frontiers are directly
/// comparable point-for-point.
const SWEEP: &[usize] = &[
    1, 2, 3, 5, 10, 20, 30, 50, 75, 100, 150, 250, 500, 1_000, 2_500, 5_000, 10_000, 50_000,
    200_000,
];
const UNLIMITED_CAP: usize = usize::MAX;

struct LexNarrowedQuery {
    qid: u64,
    combined_count: u64,
    residual_token_count: usize,
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
        eprintln!("Solr NOT reachable at {solr_base_url} -- P3-E03 requires a live Solr instance. Aborting.");
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

    println!("\nfinding UnresolvedResidual-rejected queries + lexical-narrowing eligibility...");
    let unlimited_structural = AdmissionPolicy {
        max_candidates: UNLIMITED_CAP,
    };
    let mut rejected_residual_count = 0usize;
    let mut oov_blocked = 0usize;
    let mut eligible: Vec<LexNarrowedQuery> = Vec::new();
    let mut variant_correctness_violations = 0usize;
    for (&qid, compiled) in &compiled_cache {
        let AdmissionDecision::Reject(RejectReason::UnresolvedResidual) =
            admit(compiled, &index, &unlimited_structural)
        else {
            continue;
        };
        rejected_residual_count += 1;

        let residual_token_count = compiled
            .residual_lexical
            .iter()
            .flat_map(|phrase| phrase.split_whitespace())
            .count();

        let Some((narrow_by, combined_count)) =
            admit_lexically_narrowed(compiled, &index, UNLIMITED_CAP)
        else {
            oov_blocked += 1;
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
            residual_token_count,
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
        "  {oov_blocked}/{rejected_residual_count} blocked outright by an out-of-vocabulary residual token (cap-independent)"
    );
    println!(
        "  {}/{rejected_residual_count} have a real combined candidate count (cap-independent, will be swept below)",
        eligible.len()
    );
    println!(
        "  variant-correctness violations: {variant_correctness_violations} (must be 0 -- commerce_core always exactly re-verifies hard constraints)"
    );
    assert_eq!(
        variant_correctness_violations, 0,
        "a lexically-narrowed hit failed its own compiled query's hard constraints -- this is a commerce_core correctness bug, not a Phase 3 harness issue"
    );

    // Native execution latency, measured once (not per sweep point) --
    // same rationale and per-query individual-timing convention as
    // P3-E02's own native-latency measurement.
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
                "\nnative lexically-narrowed execution latency (n={} small-combined-candidate-set queries, 30 reps each, {} individual samples):",
                sample.len(),
                per_query.len()
            );
            d.print("native_execute_lexically_narrowed", "ms");
            bench_harness::append_summary_row(
                &PathBuf::from("dataset_cache/p3e03_artifacts/summary_latency.csv"),
                "p3e03_lexical_narrowing_eval",
                "native_execute_lexically_narrowed",
                "latency_ms",
                &d,
            )
            .ok();
        }
    }

    let artifacts_dir = PathBuf::from("dataset_cache/p3e03_artifacts");
    let manifest = RunManifest::capture(
        "p3e03_lexical_narrowing_eval",
        "max_lexical_narrowed_candidates_sweep",
        &catalog_path,
        &queries_path,
        serde_json::json!({
            "sweep": SWEEP,
            "rejected_residual_count": rejected_residual_count,
            "oov_blocked": oov_blocked,
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
            "qid,combined_count,residual_token_count,native_ndcg,native_recall,native_mrr,solr_ndcg,solr_recall,solr_mrr,native_hit_count,solr_hit_count"
        )
        .unwrap();
        for eq in &eligible {
            let (s_ndcg, s_recall, s_mrr, s_hits) = solr_ndcg[&eq.qid];
            writeln!(
                f,
                "{},{},{},{},{},{},{},{},{},{},{}",
                eq.qid,
                eq.combined_count,
                eq.residual_token_count,
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

    println!("\n=== P3-E03 lexical-narrowing coverage/relevance frontier (isolated marginal contribution) ===");
    println!(
        "{:>10} {:>10} {:>10} {:>9} {:>10} {:>10} {:>10} {:>12} {:>10} {:>10}",
        "cap",
        "admitted",
        "cov%_resid",
        "cov%_all",
        "native_ndcg",
        "solr_ndcg_sub",
        "ndcg_delta",
        "whole_wl_ndcg",
        "wl_degrad",
        "false_pos"
    );
    let mut frontier_csv = String::from(
        "max_lexical_narrowed_candidates,admitted,coverage_pct_of_residual_rejected,coverage_pct_of_whole_corpus,native_ndcg_mean,solr_ndcg_on_admitted_mean,ndcg_delta_on_admitted,whole_workload_ndcg,whole_workload_degradation,zero_result_native,zero_result_solr_on_admitted,false_positive_admissions\n",
    );
    for &cap in SWEEP {
        let admitted: Vec<&LexNarrowedQuery> = eligible
            .iter()
            .filter(|eq| eq.combined_count as usize <= cap)
            .collect();
        let admitted_count = admitted.len();
        let coverage_pct_of_residual =
            admitted_count as f64 / rejected_residual_count as f64 * 100.0;
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

        // Isolated marginal contribution: every query NOT admitted here
        // (including ones the *original* structural admit() would
        // separately admit) is scored as a Solr fallback -- see the
        // module doc comment for why this is the fair standalone read.
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
            coverage_pct_of_residual,
            coverage_pct_of_whole,
            native_ndcg_mean,
            solr_on_admitted_mean,
            ndcg_delta_on_admitted,
            whole_workload_ndcg,
            whole_workload_degradation,
            false_positive_admissions
        );
        frontier_csv.push_str(&format!(
            "{cap},{admitted_count},{coverage_pct_of_residual},{coverage_pct_of_whole},{native_ndcg_mean},{solr_on_admitted_mean},{ndcg_delta_on_admitted},{whole_workload_ndcg},{whole_workload_degradation},{zero_result_native},{zero_result_solr_on_admitted},{false_positive_admissions}\n"
        ));
    }
    std::fs::write(artifacts_dir.join("frontier_sweep.csv"), &frontier_csv).ok();

    println!("\n=== relevance-budget calibration (Issue #14 RQ2, applied to this mechanism's isolated contribution) ===");
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
