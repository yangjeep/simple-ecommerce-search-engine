//! Issue #14 P3-E04 diagnostic (no Solr call needed -- reuses P3-E03's
//! already-computed per-query relevance data): P3-E03 found naive
//! independent-token-presence lexical narrowing REJECTED overall, at
//! every swept cap, because it lacks a real precision/ranking signal.
//! But `admit_lexically_narrowed` treats a query with *both* an existing
//! structural constraint (Brand/ProductType/etc.) and residual text
//! identically to one with *no* structural constraint at all -- the
//! former's structural half is exactly the strong precision anchor
//! P3-E02 found tolerates having no ranking signal reasonably well. This
//! checks, before building anything, whether that distinction alone
//! explains a meaningful share of P3-E03's aggregate failure -- i.e.
//! whether restricting lexical narrowing to "residual text ON TOP OF an
//! existing structural constraint" (never as the sole signal) would have
//! been safer than what was measured.
//!
//! Recompiles every P3-E03-eligible query (cheap, deterministic, no Solr)
//! solely to check `!compiled.constraints.is_empty()`, then joins against
//! `docs/research/artifacts/p3e03_run1/eligible_queries_raw.csv`'s
//! already-measured native/Solr NDCG per query, bucketing by that one
//! property at the same representative cap points P3-E03's own frontier
//! table used (1, 20, 250, unlimited).
//!
//! Usage: cargo run --release -p phase3-eval --bin p3e04_structural_plus_lexical_diagnostic
//!        [catalog.jsonl] [queries.jsonl] [p3e03_eligible_queries_raw.csv]

use std::collections::BTreeMap;
use std::path::PathBuf;

use commerce_core::cold_start::{compile_lexicon, CatalogProfile};
use commerce_core::ir::compile;
use round1_eval::catalog;
use round1_eval::data;

struct Row {
    combined_count: u64,
    native_ndcg: f64,
    solr_ndcg: f64,
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
    let p3e03_csv_path = args.next().map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from("docs/research/artifacts/p3e03_run1/eligible_queries_raw.csv")
    });

    println!("loading + ingesting real catalog...");
    let products = data::load_catalog(&catalog_path);
    let ingested = catalog::build_catalog(&products);
    println!("{} real products ingested", ingested.catalog.products.len());

    let profile = CatalogProfile::build(&ingested.catalog, &ingested.brands, &[], &[]);
    let lexicon = compile_lexicon(&profile, 25);

    println!("loading real queries...");
    let judgments = data::load_queries(&queries_path);
    let mut query_text_by_qid: BTreeMap<u64, String> = BTreeMap::new();
    for j in &judgments {
        query_text_by_qid
            .entry(j.query_id)
            .or_insert_with(|| j.query.clone());
    }

    println!("loading P3-E03's already-measured per-query results from {p3e03_csv_path:?}...");
    let csv_text = std::fs::read_to_string(&p3e03_csv_path)
        .unwrap_or_else(|e| panic!("failed to read {p3e03_csv_path:?}: {e}"));
    let mut rows: BTreeMap<u64, Row> = BTreeMap::new();
    for line in csv_text.lines().skip(1) {
        let cols: Vec<&str> = line.split(',').collect();
        if cols.len() < 7 {
            continue;
        }
        let qid: u64 = cols[0].parse().expect("qid");
        let combined_count: u64 = cols[1].parse().expect("combined_count");
        let native_ndcg: f64 = cols[3].parse().expect("native_ndcg");
        let solr_ndcg: f64 = cols[6].parse().expect("solr_ndcg");
        rows.insert(
            qid,
            Row {
                combined_count,
                native_ndcg,
                solr_ndcg,
            },
        );
    }
    println!("  {} P3-E03-eligible queries loaded", rows.len());

    println!("\nrecompiling each to check for an existing structural constraint...");
    let mut has_constraint: BTreeMap<u64, bool> = BTreeMap::new();
    for &qid in rows.keys() {
        let text = query_text_by_qid
            .get(&qid)
            .unwrap_or_else(|| panic!("qid {qid} missing from queries.jsonl"));
        let compiled = compile(text, &lexicon);
        has_constraint.insert(qid, !compiled.constraints.is_empty());
    }
    let with_constraint = has_constraint.values().filter(|&&v| v).count();
    let without_constraint = has_constraint.len() - with_constraint;
    let total = has_constraint.len();
    println!(
        "  {with_constraint}/{total} have an existing structural constraint alongside residual text; {without_constraint} are pure-lexical-only"
    );

    println!("\n=== P3-E04 diagnostic: structural+lexical vs. pure-lexical-only, at representative caps ===");
    println!(
        "{:>10} {:>18} {:>10} {:>12} {:>12} {:>10} {:>10}",
        "cap", "bucket", "admitted", "native_ndcg", "solr_ndcg", "delta", "false_pos"
    );
    for &cap in &[1u64, 20, 250, u64::MAX] {
        for with_c in [true, false] {
            let admitted: Vec<&Row> = rows
                .iter()
                .filter(|(qid, r)| r.combined_count <= cap && has_constraint[qid] == with_c)
                .map(|(_, r)| r)
                .collect();
            let n = admitted.len();
            let native_mean = if n > 0 {
                admitted.iter().map(|r| r.native_ndcg).sum::<f64>() / n as f64
            } else {
                0.0
            };
            let solr_mean = if n > 0 {
                admitted.iter().map(|r| r.solr_ndcg).sum::<f64>() / n as f64
            } else {
                0.0
            };
            let false_pos = admitted
                .iter()
                .filter(|r| r.native_ndcg == 0.0 && r.solr_ndcg > 0.0)
                .count();
            let bucket = if with_c {
                "has_constraint"
            } else {
                "pure_lexical"
            };
            let cap_label = if cap == u64::MAX {
                "unlimited".to_string()
            } else {
                cap.to_string()
            };
            println!(
                "{:>10} {:>18} {:>10} {:>12.4} {:>12.4} {:>+10.4} {:>9}/{}",
                cap_label,
                bucket,
                n,
                native_mean,
                solr_mean,
                native_mean - solr_mean,
                false_pos,
                n
            );
        }
    }
}
