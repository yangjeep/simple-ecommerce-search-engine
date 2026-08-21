//! Issue #14/#18 P3-E15: corrected-baseline re-measurement of P3-E13's
//! combined ambiguity+lexical mechanism. P3-E14's Solr-baseline fairness
//! audit found P3-E13's target population -- by construction, every
//! query in it has a resolved ambiguous span -- has its Solr comparison
//! built via `round1_eval::solr::solr_query_for`, which drops an
//! ambiguous span's own words from Solr's `q=` entirely once
//! `residual_lexical` is non-empty (true for 98.23% of this population,
//! P3-E12's own measurement). This is very likely the real explanation
//! for P3-E13's surprising "native beats Solr" result: Solr was
//! effectively blinded to exactly the words this mechanism resolves and
//! exploits.
//!
//! This binary supplies the corrected comparison. Per P3-E14's own
//! adversarial review, a blanket "always use full query_text" fix was
//! REJECTED (it would double-count Brand/color signal already
//! represented in `fq`, introducing a new bias in the opposite
//! direction, concentrated in P3-E05/E06/E07/E10's own population). The
//! surgical fix used here instead: `q` = `residual_lexical.join(" ")`
//! plus each ambiguous span's own original text appended -- recovering
//! exactly the words this population's own dominant loss channel drops
//! (4,896/5,000 corpus-wide per P3-E14, vs. only 123/5,000 for the
//! smaller non-Brand/Color-constraint-classification gap, which is left
//! uncorrected here as a documented, bounded residual matching the scale
//! P3-E05's own 2.26% gap was already accepted at).
//!
//! Recomputes a FRESH, CORRECTED whole-corpus Solr baseline (not reused
//! from P3-E06's persisted, uncorrected CSV) so the whole-workload
//! degradation denominator is consistent with the corrected admitted-
//! subset comparison -- the same "isolated marginal contribution"
//! methodology P3-E03/P3-E05/P3-E09/P3-E13 all used, just with a fair
//! baseline this time. Reruns the identical admission logic P3-E13 used
//! (frequency-resolved ambiguity + lexical narrowing on any leftover
//! residual) so the only variable changed is the Solr comparison itself.
//!
//! Usage: cargo run --release -p phase3-eval --bin p3e15_ambiguous_plus_lexical_corrected_eval
//!        [catalog.jsonl] [queries.jsonl] [solr_base_url]

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
use std::time::Instant;

use commerce_core::admission::{admit, AdmissionDecision, AdmissionPolicy, RejectReason};
use commerce_core::cold_start::{compile_lexicon, CatalogProfile};
use commerce_core::domain::BrandId;
use commerce_core::index::CatalogIndex;
use commerce_core::ir::compile;
use commerce_core::ir::lexicon::ResolvedTerm;
use round1_eval::catalog;
use round1_eval::data::{self, EsciLabel};
use round1_eval::relevance::ndcg_recall_mrr;
use round1_eval::solr::{extract_brand_color, solr_search};

const K: usize = 10;
const RESOLVED_CANDIDATE_CAP: usize = 250;
const RATIO_SWEEP: &[f64] = &[1.0, 2.0, 3.0, 5.0, 10.0, 20.0, 50.0, 100.0];

/// The surgical corrected query construction P3-E14's audit recommended:
/// recover ambiguous-span words specifically (this population's own
/// dominant, measured loss channel), leaving Brand/color `fq`-only
/// exactly as `round1_eval::solr::solr_query_for` already does -- no
/// blanket full-text swap, no double-counting.
fn solr_query_for_corrected(
    residual_lexical: &[String],
    ambiguous_span_texts: &[String],
    brand: Option<&str>,
    color: Option<&str>,
) -> (String, Vec<String>) {
    let mut fq = Vec::new();
    if let Some(b) = brand {
        fq.push(format!(
            "brand:/{}/",
            round1_eval::solr::case_insensitive_field_regex(b)
        ));
    }
    if let Some(c) = color {
        fq.push(format!(
            "color:/{}/",
            round1_eval::solr::case_insensitive_field_regex(c)
        ));
    }
    let mut parts: Vec<&str> = residual_lexical.iter().map(String::as_str).collect();
    parts.extend(ambiguous_span_texts.iter().map(String::as_str));
    let text = parts.join(" ");
    let q = if text.trim().is_empty() {
        "*:*".to_string()
    } else {
        format!("{{!edismax qf=all_text}}{text}")
    };
    (q, fq)
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
        eprintln!(
            "Solr NOT reachable at {solr_base_url} -- P3-E15 requires a live Solr instance. Aborting."
        );
        std::process::exit(1);
    };
    println!("  Solr reachable: numFound={}", ping.num_found);

    let brand_name_by_id: HashMap<BrandId, String> = ingested
        .brands
        .iter()
        .map(|b| (b.id, b.name.clone()))
        .collect();
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

    println!("\ncompiling every real query...");
    let compiled_cache: BTreeMap<u64, commerce_core::ir::CommerceQuery> = judged_by_query
        .iter()
        .map(|(&qid, (text, _))| (qid, compile(text, &lexicon)))
        .collect();

    println!("querying Solr for every real query with the CORRECTED (ambiguous-span-recovering) baseline construction...");
    let t0 = Instant::now();
    let mut solr_ndcg: BTreeMap<u64, f64> = BTreeMap::new();
    for (&qid, (_, judged)) in &judged_by_query {
        let compiled = &compiled_cache[&qid];
        let (brand, color) = extract_brand_color(&compiled.constraints, &brand_name_by_id);
        let ambiguous_texts: Vec<String> =
            compiled.ambiguous.iter().map(|s| s.text.clone()).collect();
        let (q, fq) = solr_query_for_corrected(
            &compiled.residual_lexical,
            &ambiguous_texts,
            brand.as_deref(),
            color.as_deref(),
        );
        let ids = solr_search(&solr_base_url, &q, &fq, K)
            .map(|r| r.ids)
            .unwrap_or_default();
        let (ndcg, _, _) = ndcg_recall_mrr(&ids, judged, K);
        solr_ndcg.insert(qid, ndcg);
    }
    println!(
        "  done in {:.1}s ({} queries)",
        t0.elapsed().as_secs_f64(),
        solr_ndcg.len()
    );
    let solr_only_ndcg_mean: f64 = solr_ndcg.values().sum::<f64>() / total as f64;
    println!(
        "\nwhole-workload CORRECTED pure-Solr-only baseline NDCG@10: {solr_only_ndcg_mean:.4} (P3-E06's uncorrected baseline was 0.2335)"
    );

    println!("\nresolving tractable ambiguous queries by catalog-frequency-dominant candidate, lexically narrowing any leftover residual (identical logic to P3-E13)...");
    let unlimited_policy = AdmissionPolicy {
        max_candidates: usize::MAX,
    };
    let mut ambiguous_count = 0usize;
    struct AmbiguousQuery {
        qid: u64,
        ratio: f64,
        resolved_count: u64,
        native_ndcg: f64,
    }
    let mut tractable: Vec<AmbiguousQuery> = Vec::new();
    let mut variant_correctness_violations = 0usize;
    for (&qid, (_, judged)) in &judged_by_query {
        let compiled = &compiled_cache[&qid];
        let AdmissionDecision::Reject(reason) = admit(compiled, &index, &unlimited_policy) else {
            continue;
        };
        if reason != RejectReason::Ambiguous {
            continue;
        }
        ambiguous_count += 1;
        if compiled.ambiguous.len() != 1 {
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
            continue;
        }

        let mut resolved_query = compiled.clone();
        resolved_query.ambiguous.clear();
        resolved_query
            .constraints
            .push(constraint_candidates[winner_idx].clone());

        let structural_bitmap = index.indexed_candidates(&resolved_query.constraints);
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
                continue;
            }
            let lexical_bitmap = index.lexical_and_candidates(&residual_tokens);
            lexical_bitmap & structural_bitmap
        };
        let resolved_count = combined_bitmap.len();
        if resolved_count == 0 {
            continue;
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
            native_ndcg,
        });
    }
    println!(
        "  {}/{} ambiguous-rejected queries are tractable (identical to P3-E13's own count)",
        tractable.len(),
        ambiguous_count
    );
    println!("  variant-correctness violations: {variant_correctness_violations} (must be 0)");
    assert_eq!(variant_correctness_violations, 0);

    println!("\n=== P3-E15 CORRECTED-baseline ambiguity+lexical frontier (cap<={RESOLVED_CANDIDATE_CAP}) ===");
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
        "ratio_threshold,admitted,coverage_pct_of_ambiguous,coverage_pct_of_whole_corpus,native_ndcg_mean,solr_ndcg_on_admitted_mean,ndcg_delta_on_admitted,whole_workload_ndcg,whole_workload_degradation\n",
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
            "{ratio_threshold},{admitted_count},{coverage_pct_of_ambiguous},{coverage_pct_of_whole},{native_ndcg_mean},{solr_on_admitted_mean},{ndcg_delta_on_admitted},{whole_workload_ndcg},{whole_workload_degradation}\n"
        ));
    }

    let artifacts_dir = PathBuf::from("dataset_cache/p3e15_artifacts");
    std::fs::create_dir_all(&artifacts_dir).ok();
    std::fs::write(artifacts_dir.join("frontier_sweep.csv"), &csv).ok();

    println!("\n=== relevance-budget calibration (Issue #14 RQ2, CORRECTED baseline) ===");
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
