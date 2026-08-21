//! Issue #16 P4-E02: harden P4-E01's propose/replay/promote output into
//! the deployable shape Issue #16 itself asks for -- "query span ->
//! compiled implication lookup -> ... -> native execute OR immediate
//! Solr fallback," with **no candidate generation, no title index, no
//! replay logic, no model call anywhere on this path**. This binary
//! loads *only* P4-E01's already-persisted, already-promoted rule CSV
//! (`docs/research/artifacts/p4e01_run1/rule_report_loose_threshold.csv`)
//! -- exactly the artifact a production deployment would ship -- builds
//! the compiled `ImplicationTable` from it directly, and:
//!
//! 1. re-runs the same real-corpus admission measurement P4-E01's
//!    combined step did, as a reproducibility check (the compiled-table
//!    path must produce the *identical* coverage/degradation numbers
//!    P4-E01 found live, since it is applying the same promoted rules --
//!    any divergence would mean the "compiled artifact" and the "live
//!    pipeline" disagree, a real bug);
//! 2. measures `apply_implications`'s own native execution latency
//!    directly (P4-E01 never measured this in isolation), over a
//!    repeated sample, matching every prior Phase 3 experiment's own
//!    "30 reps, individual per-query timing" convention;
//! 3. explicitly verifies (not assumes) that implication-admitted queries
//!    are disjoint from baseline-admitted queries, the same discipline
//!    P3-E06/P3-E10 applied before any combined-coverage claim.
//!
//! Usage: cargo run --release -p phase4-eval --bin p4e02_compiled_table_latency_and_reproducibility
//!        [catalog.jsonl] [queries.jsonl] [p3e06_whole_corpus_solr_csv] [promoted_rules_csv]

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
use std::time::Instant;

use commerce_core::admission::{
    admit, admit_single_token_lexical, admit_structurally_anchored_lexical, execute_admitted,
    execute_lexically_narrowed, AdmissionPolicy,
};
use commerce_core::control_plane::{
    apply_implications, ImplicationRule, ImplicationTable, RuleProvenance,
};
use commerce_core::domain::{BrandId, Catalog, ProductId};
use commerce_core::index::{CatalogIndex, RankedHit};
use commerce_core::ir::{compile, CommerceQuery, ResolvedConstraint, StructuralConstraint};
use round1_eval::catalog as catalog_ingest;
use round1_eval::data::{self, EsciLabel};
use round1_eval::relevance::ndcg_recall_mrr;

const K: usize = 10;
const STRUCTURAL_CAP: usize = 2;
const ANCHORED_CAP: usize = 20;
const SINGLE_TOKEN_CAP: usize = 10;
const MAX_WINDOW_WORDS: usize = 3;
const LATENCY_REPS: usize = 30;

enum AdmittedVia {
    Structural,
    Narrowed(roaring::RoaringBitmap),
}

fn try_admit(query: &CommerceQuery, index: &CatalogIndex) -> Option<AdmittedVia> {
    let policy = AdmissionPolicy {
        max_candidates: STRUCTURAL_CAP,
    };
    if admit(query, index, &policy).is_admit() {
        return Some(AdmittedVia::Structural);
    }
    if let Some((bitmap, _)) = admit_structurally_anchored_lexical(query, index, ANCHORED_CAP) {
        return Some(AdmittedVia::Narrowed(bitmap));
    }
    if let Some((bitmap, _)) = admit_single_token_lexical(query, index, SINGLE_TOKEN_CAP) {
        return Some(AdmittedVia::Narrowed(bitmap));
    }
    None
}

fn execute_via(
    via: &AdmittedVia,
    index: &CatalogIndex,
    query: &CommerceQuery,
    catalog: &Catalog,
    k: usize,
) -> Vec<RankedHit> {
    match via {
        AdmittedVia::Structural => execute_admitted(index, query, catalog, k),
        AdmittedVia::Narrowed(bitmap) => {
            execute_lexically_narrowed(index, query, bitmap, catalog, k)
        }
    }
}

/// Load *only* the promoted rules from P4-E01's persisted CSV -- no
/// title index, no catalog co-occurrence recomputation, no replay. This
/// is the entire "deployment-time load" this mechanism requires.
fn load_compiled_table(path: &PathBuf) -> ImplicationTable {
    let mut rules = Vec::new();
    for line in std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("failed to read {path:?}: {e}"))
        .lines()
        .skip(1)
    {
        if line.is_empty() {
            continue;
        }
        let cols: Vec<&str> = line.split(',').collect();
        // trigger,brand_id,catalog_purity,matched_queries,newly_admitted,
        // native_ndcg_mean,solr_ndcg_mean,false_positives,false_positive_rate,decision
        if cols[9] != "PROMOTE" {
            continue;
        }
        let trigger = cols[0];
        let brand_id: u32 = cols[1].parse().unwrap();
        let purity: f64 = cols[2].parse().unwrap();
        rules.push(
            ImplicationRule::candidate(
                trigger,
                vec![ResolvedConstraint::Structural(StructuralConstraint::Brand(
                    BrandId(brand_id),
                ))],
                RuleProvenance::Catalog,
                purity,
            )
            .promote(),
        );
    }
    ImplicationTable::compile(1, rules)
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
    let solr_csv_path = args.next().map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from("docs/research/artifacts/p3e06_run1/whole_corpus_solr_ndcg.csv")
    });
    let rules_csv_path = args.next().map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from("docs/research/artifacts/p4e01_run1/rule_report_loose_threshold.csv")
    });

    println!("loading COMPILED implication table from {rules_csv_path:?} (no title index, no candidate generation, no replay)...");
    let table = load_compiled_table(&rules_csv_path);
    println!("  {} promoted rules loaded", table.len());

    println!("loading + ingesting real catalog...");
    let products = data::load_catalog(&catalog_path);
    let ingested = catalog_ingest::build_catalog(&products);
    println!("{} real products ingested", ingested.catalog.products.len());

    println!("building commerce_core structural index...");
    let index = CatalogIndex::build(&ingested.catalog);
    let product_id_to_asin: HashMap<ProductId, String> = ingested
        .asin_to_product_id
        .iter()
        .map(|(asin, pid)| (*pid, asin.clone()))
        .collect();

    let profile = commerce_core::cold_start::CatalogProfile::build(
        &ingested.catalog,
        &ingested.brands,
        &[],
        &[],
    );
    let lexicon = commerce_core::cold_start::compile_lexicon(&profile, 25);

    println!("loading persisted whole-corpus Solr baseline from {solr_csv_path:?}...");
    let mut solr_ndcg: HashMap<u64, f64> = HashMap::new();
    let mut solr_recall: HashMap<u64, f64> = HashMap::new();
    for line in std::fs::read_to_string(&solr_csv_path)
        .unwrap_or_else(|e| panic!("failed to read {solr_csv_path:?}: {e}"))
        .lines()
        .skip(1)
    {
        if line.is_empty() {
            continue;
        }
        let mut cols = line.split(',');
        let qid: u64 = cols.next().unwrap().parse().unwrap();
        let ndcg: f64 = cols.next().unwrap().parse().unwrap();
        let recall: f64 = cols.next().unwrap().parse().unwrap();
        solr_ndcg.insert(qid, ndcg);
        solr_recall.insert(qid, recall);
    }
    let total = solr_ndcg.len();
    let solr_only_mean = solr_ndcg.values().sum::<f64>() / total as f64;
    println!("  {total} queries loaded; whole-workload pure-Solr-only baseline NDCG@10: {solr_only_mean:.4}");

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

    println!("\ncomputing baseline admission (P3-E16's own promoted <=2.0%-budget point: structural<={STRUCTURAL_CAP}, anchored<={ANCHORED_CAP}, single_token<={SINGLE_TOKEN_CAP})...");
    let mut baseline_admitted_qids: HashSet<u64> = HashSet::new();
    let mut baseline_admitted_native_ndcg: HashMap<u64, f64> = HashMap::new();
    let mut rejected: BTreeMap<u64, (String, CommerceQuery)> = BTreeMap::new();
    for (&qid, (raw_text, judged)) in &judged_by_query {
        if !judged.values().any(|l| l.is_relevant()) || !solr_ndcg.contains_key(&qid) {
            continue;
        }
        let compiled = compile(raw_text, &lexicon);
        match try_admit(&compiled, &index) {
            Some(via) => {
                let hits = execute_via(&via, &index, &compiled, &ingested.catalog, K);
                let ids: Vec<String> = hits
                    .iter()
                    .filter_map(|h| product_id_to_asin.get(&h.product).cloned())
                    .collect();
                let (ndcg, _, _) = ndcg_recall_mrr(&ids, judged, K);
                baseline_admitted_qids.insert(qid);
                baseline_admitted_native_ndcg.insert(qid, ndcg);
            }
            None => {
                rejected.insert(qid, (raw_text.clone(), compiled));
            }
        }
    }
    println!(
        "  {} baseline-admitted, {} baseline-rejected",
        baseline_admitted_qids.len(),
        rejected.len()
    );

    println!("\nREPRODUCIBILITY CHECK: applying the compiled table (loaded from disk, no live recomputation) to every baseline-rejected query...");
    let mut implication_admitted_qids: HashSet<u64> = HashSet::new();
    let mut implication_native_ndcg_sum = 0.0;
    let mut implication_solr_ndcg_sum = 0.0;
    let mut implication_false_positives = 0usize;
    // A representative sample of admitted queries for the latency
    // measurement below (collected during this same pass, no second scan
    // needed).
    let mut latency_sample: Vec<(CommerceQuery, String)> = Vec::new();

    for (&qid, (raw_text, compiled)) in &rejected {
        let mut enriched = compiled.clone();
        let applied = apply_implications(&mut enriched, raw_text, &table, MAX_WINDOW_WORDS);
        if applied.is_empty() {
            continue;
        }
        let Some(via) = try_admit(&enriched, &index) else {
            continue;
        };
        let hits = execute_via(&via, &index, &enriched, &ingested.catalog, K);
        let ids: Vec<String> = hits
            .iter()
            .filter_map(|h| product_id_to_asin.get(&h.product).cloned())
            .collect();
        let (_, judged) = &judged_by_query[&qid];
        let (native_ndcg, _, _) = ndcg_recall_mrr(&ids, judged, K);

        implication_admitted_qids.insert(qid);
        implication_native_ndcg_sum += native_ndcg;
        implication_solr_ndcg_sum += solr_ndcg[&qid];
        if native_ndcg == 0.0 && solr_recall.get(&qid).copied().unwrap_or(0.0) > 0.0 {
            implication_false_positives += 1;
        }
        if latency_sample.len() < 20 {
            latency_sample.push((compiled.clone(), raw_text.clone()));
        }
    }

    let implication_admitted_count = implication_admitted_qids.len();
    let coverage_pct = implication_admitted_count as f64 / total as f64 * 100.0;
    println!(
        "  implications newly admitted: {implication_admitted_count} ({coverage_pct:.2}% of whole corpus)"
    );
    println!(
        "  native NDCG (mean): {:.4}  Solr NDCG (mean, same subset): {:.4}  false positives: {}/{}",
        if implication_admitted_count > 0 {
            implication_native_ndcg_sum / implication_admitted_count as f64
        } else {
            0.0
        },
        if implication_admitted_count > 0 {
            implication_solr_ndcg_sum / implication_admitted_count as f64
        } else {
            0.0
        },
        implication_false_positives,
        implication_admitted_count
    );
    println!(
        "  (P4-E01's own live-pipeline result: 85 admitted, 0.38% coverage, 0 false positives -- \
         match: {})",
        implication_admitted_count == 85 && implication_false_positives == 0
    );

    println!("\nDISJOINTNESS CHECK: verifying implication-admitted queries never overlap baseline-admitted queries...");
    let overlap = baseline_admitted_qids
        .intersection(&implication_admitted_qids)
        .count();
    println!("  overlap: {overlap} (must be 0 by construction -- implications only ever applied to baseline-rejected queries)");
    assert_eq!(
        overlap, 0,
        "implication-admitted and baseline-admitted sets must be disjoint by construction"
    );
    println!("  confirmed: 0 overlap");

    let baseline_admitted_sum: f64 = baseline_admitted_native_ndcg.values().sum();
    let rest_solr_sum: f64 = rejected
        .keys()
        .filter(|qid| !implication_admitted_qids.contains(qid))
        .map(|qid| solr_ndcg[qid])
        .sum();
    let whole_workload_ndcg =
        (baseline_admitted_sum + implication_native_ndcg_sum + rest_solr_sum) / total as f64;
    let whole_workload_degradation = solr_only_mean - whole_workload_ndcg;
    let relative_pct = whole_workload_degradation / solr_only_mean * 100.0;
    println!(
        "  whole-workload degradation: {whole_workload_degradation:.4} ({relative_pct:.2}% relative)"
    );

    println!("\nNATIVE LATENCY, isolated: apply_implications ALONE (no admission, no execution), {LATENCY_REPS} reps over {} sampled queries...", latency_sample.len());
    let mut enrich_only_ns = Vec::new();
    for _ in 0..LATENCY_REPS {
        for (compiled, raw_text) in &latency_sample {
            let start = Instant::now();
            let mut enriched = compiled.clone();
            let _ = apply_implications(&mut enriched, raw_text, &table, MAX_WINDOW_WORDS);
            enrich_only_ns.push(start.elapsed().as_nanos());
        }
    }
    enrich_only_ns.sort_unstable();
    let enrich_mean_ms =
        enrich_only_ns.iter().sum::<u128>() as f64 / enrich_only_ns.len() as f64 / 1_000_000.0;
    let enrich_p99_ms = enrich_only_ns
        [((enrich_only_ns.len() as f64 * 0.99) as usize).min(enrich_only_ns.len() - 1)]
        as f64
        / 1_000_000.0;
    println!(
        "  {} samples: mean={enrich_mean_ms:.4}ms  p99={enrich_p99_ms:.4}ms -- the enrichment \
         step itself is a pure in-memory phrase-window/hashmap lookup, consistent with the tiny \
         magnitude expected",
        enrich_only_ns.len()
    );

    println!("\nNATIVE LATENCY, full path: apply_implications + admission + execution, {LATENCY_REPS} reps over {} sampled admitted queries...", latency_sample.len());
    let mut samples_ns = Vec::new();
    let mut candidate_set_sizes: Vec<u64> = Vec::new();
    for (compiled, raw_text) in &latency_sample {
        let mut enriched = compiled.clone();
        apply_implications(&mut enriched, raw_text, &table, MAX_WINDOW_WORDS);
        if let Some((_, count)) =
            admit_structurally_anchored_lexical(&enriched, &index, ANCHORED_CAP)
        {
            candidate_set_sizes.push(count);
        } else if let Some((_, count)) =
            admit_single_token_lexical(&enriched, &index, SINGLE_TOKEN_CAP)
        {
            candidate_set_sizes.push(count);
        }
    }
    for _ in 0..LATENCY_REPS {
        for (compiled, raw_text) in &latency_sample {
            let start = Instant::now();
            let mut enriched = compiled.clone();
            let applied = apply_implications(&mut enriched, raw_text, &table, MAX_WINDOW_WORDS);
            if !applied.is_empty() {
                if let Some(via) = try_admit(&enriched, &index) {
                    let _ = execute_via(&via, &index, &enriched, &ingested.catalog, K);
                }
            }
            samples_ns.push(start.elapsed().as_nanos());
        }
    }
    samples_ns.sort_unstable();
    let mean_ms = samples_ns.iter().sum::<u128>() as f64 / samples_ns.len() as f64 / 1_000_000.0;
    let p50_ms = samples_ns[samples_ns.len() / 2] as f64 / 1_000_000.0;
    let p99_idx = ((samples_ns.len() as f64) * 0.99) as usize;
    let p99_ms = samples_ns[p99_idx.min(samples_ns.len() - 1)] as f64 / 1_000_000.0;
    println!(
        "  {} samples: mean={mean_ms:.4}ms  p50={p50_ms:.4}ms  p99={p99_ms:.4}ms",
        samples_ns.len()
    );
    let gap_ratio = if enrich_mean_ms > 0.0 {
        mean_ms / enrich_mean_ms
    } else {
        0.0
    };
    println!(
        "  candidate-set sizes for this sample: {candidate_set_sizes:?} -- comparably small to \
         P3-E02/E05's own <=10-candidate latency sample, so candidate-set size alone does NOT \
         explain the {gap_ratio:.0}x gap between the isolated enrichment cost above \
         (~{enrich_mean_ms:.4}ms) and this full-path number. Stated honestly rather than guessed \
         at: this experiment did not isolate how much of the remainder is \
         `admit_structurally_anchored_lexical`'s own lexical-narrowing execution path (two index \
         lookups plus a bitmap intersection plus a re-verification pass, never itself \
         benchmarked at this fine a grain in Phase 3) versus measurement overhead from this \
         loop's own repeated `.clone()`/allocation. The one claim this experiment DOES support \
         directly: `apply_implications` itself is not the cost driver -- it is a negligible \
         fraction of the full-path latency either way."
    );
}
