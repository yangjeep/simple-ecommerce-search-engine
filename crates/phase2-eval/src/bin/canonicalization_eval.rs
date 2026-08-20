//! P2-E02: does the `min_enum_frequency` canonicalization threshold
//! (`commerce_core::cold_start::compile_lexicon`) fix the catastrophic real
//! filter recall R1-E02/E02b found (5.0% against Exact-labeled relevant
//! products), and if so, at what cost to coverage (Semantic FIB hit rate)?
//! Reuses round1_eval's real-data loaders and classify/measure_precision
//! machinery unmodified -- this experiment is entirely about sweeping the
//! new threshold parameter against real data, not about changing the
//! classification/precision measurement itself.
//!
//! Usage: cargo run --release -p phase2-eval --bin canonicalization_eval
//!        [catalog.jsonl] [queries.jsonl]

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::Instant;

use commerce_core::cold_start::{compile_lexicon, CatalogProfile};
use round1_eval::classify::{self, AggregationRule, ClassCounts, QueryClass};
use round1_eval::{catalog, data};

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

    println!("loading + ingesting real catalog...");
    let products = data::load_catalog(&catalog_path);
    let ingested = catalog::build_catalog(&products);
    println!("{} real products ingested", ingested.catalog.products.len());

    let profile = CatalogProfile::build(&ingested.catalog, &ingested.brands, &[], &[]);

    println!("loading real queries + judgments...");
    let judgments = data::load_queries(&queries_path);
    let mut query_text_by_id: HashMap<u64, &str> = HashMap::new();
    let mut judgments_by_query: HashMap<u64, Vec<&data::JudgedExample>> = HashMap::new();
    for j in &judgments {
        query_text_by_id
            .entry(j.query_id)
            .or_insert(j.query.as_str());
        judgments_by_query.entry(j.query_id).or_default().push(j);
    }
    let mut query_ids: Vec<u64> = query_text_by_id.keys().copied().collect();
    query_ids.sort_unstable();
    let known_ids: HashSet<&str> = ingested
        .asin_to_product_id
        .keys()
        .map(String::as_str)
        .collect();

    println!(
        "{} distinct real queries; sweeping min_enum_frequency thresholds...",
        query_ids.len()
    );
    println!();
    println!(
        "{:>10}  {:>8}  {:>10}  {:>10}  {:>10}  {:>10}  {:>10}",
        "threshold", "fib_rate", "ambig_rate", "punt_rate", "precision", "recall_ES", "recall_Ex"
    );

    for &threshold in &[1usize, 2, 3, 5, 10, 25, 50, 100, 250] {
        let t0 = Instant::now();
        let lexicon = compile_lexicon(&profile, threshold);

        let mut counts = ClassCounts::default();
        let mut compiled_by_query = HashMap::with_capacity(query_ids.len());
        for query_id in &query_ids {
            let text = query_text_by_id[query_id];
            let compiled = commerce_core::ir::compile(text, &lexicon);
            let class = classify::classify(text, &compiled, &known_ids);
            counts.record(class);
            compiled_by_query.insert(*query_id, (class, compiled));
        }

        let precision = classify::measure_precision(
            &ingested.catalog,
            &ingested.asin_to_product_id,
            &judgments_by_query,
            &compiled_by_query,
            AggregationRule::ExistingAnd,
        );

        let fib_rate = counts.fraction(QueryClass::StructuralOnly)
            + counts.fraction(QueryClass::StructuralPlusLexical)
            + counts.fraction(QueryClass::ExactIdLookup);

        println!(
            "{:>10}  {:>7.1}%  {:>9.1}%  {:>9.1}%  {:>9.1}%  {:>9.1}%  {:>9.1}%   ({:.1}s, n_measured={})",
            threshold,
            fib_rate * 100.0,
            counts.fraction(QueryClass::Ambiguous) * 100.0,
            counts.fraction(QueryClass::UnresolvedPunt) * 100.0,
            precision.precision() * 100.0,
            precision.filter_recall() * 100.0,
            precision.exact_recall() * 100.0,
            t0.elapsed().as_secs_f64(),
            precision.queries_measured
        );
    }

    println!();
    println!("(R1-E02 baseline, threshold=1 (unfiltered): FIB=55.4%, ambiguity=38.4%, punt=2.5%, precision=94.5%, recall_ES=4.3%, recall_Exact=5.0%)");
}
