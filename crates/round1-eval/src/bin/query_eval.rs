//! R1-E02: the central Round 1 experiment. Real catalog -> cold-start
//! lexicon (Gate 6 machinery, unmodified) -> compile every real held-out
//! test query -> classify -> measure Semantic FIB hit rate and semantic
//! precision against real ESCI human judgments.
//!
//! Usage: cargo run --release -p round1-eval --bin query_eval
//!        [catalog.jsonl] [queries.jsonl] [max_products]

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::Instant;

use commerce_core::cold_start::{compile_lexicon, CatalogProfile};
use round1_eval::classify::{self, ClassCounts};
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
    let max_products: Option<usize> = args.next().and_then(|s| s.parse().ok());

    println!("loading catalog from {catalog_path:?}...");
    let t0 = Instant::now();
    let mut products = data::load_catalog(&catalog_path);
    if let Some(max) = max_products {
        products.truncate(max);
    }
    println!(
        "loaded {} products in {:.2}s",
        products.len(),
        t0.elapsed().as_secs_f64()
    );

    let t1 = Instant::now();
    let ingested = catalog::build_catalog(&products);
    println!(
        "built Catalog + {} distinct brands in {:.2}s",
        ingested.brands.len(),
        t1.elapsed().as_secs_f64()
    );

    println!("deriving cold-start lexicon from the real catalog (commerce_core::cold_start, unmodified)...");
    let t2 = Instant::now();
    let profile = CatalogProfile::build(&ingested.catalog, &ingested.brands, &[], &[]);
    // min_enum_frequency=1: unfiltered, preserving this entry's exact
    // recorded R1-E02 behavior. The canonicalization fix this finding
    // motivated is swept separately in phase2-eval (P2-E02,
    // docs/experiments/PHASE2_LOG.md) rather than changed here, so R1-E02's
    // evidence stays reproducible as originally recorded.
    let lexicon = compile_lexicon(&profile, 1);
    println!("lexicon derived in {:.2}s", t2.elapsed().as_secs_f64());

    println!("loading queries from {queries_path:?}...");
    let t3 = Instant::now();
    let judgments = data::load_queries(&queries_path);
    println!(
        "loaded {} judgment rows covering {} distinct queries in {:.2}s",
        judgments.len(),
        judgments
            .iter()
            .map(|j| j.query_id)
            .collect::<HashSet<_>>()
            .len(),
        t3.elapsed().as_secs_f64()
    );

    let mut query_text_by_id: HashMap<u64, &str> = HashMap::new();
    let mut judgments_by_query: HashMap<u64, Vec<&data::JudgedExample>> = HashMap::new();
    for j in &judgments {
        query_text_by_id
            .entry(j.query_id)
            .or_insert(j.query.as_str());
        judgments_by_query.entry(j.query_id).or_default().push(j);
    }

    let known_ids: HashSet<&str> = ingested
        .asin_to_product_id
        .keys()
        .map(String::as_str)
        .collect();

    let t4 = Instant::now();
    let mut counts = ClassCounts::default();
    let mut compiled_by_query = HashMap::with_capacity(query_text_by_id.len());
    let mut query_ids: Vec<u64> = query_text_by_id.keys().copied().collect();
    query_ids.sort_unstable();
    for query_id in &query_ids {
        let text = query_text_by_id[query_id];
        let compiled = commerce_core::ir::compile(text, &lexicon);
        let class = classify::classify(text, &compiled, &known_ids);
        counts.record(class);
        compiled_by_query.insert(*query_id, (class, compiled));
    }
    println!(
        "classified {} distinct queries in {:.2}s",
        query_ids.len(),
        t4.elapsed().as_secs_f64()
    );
    println!();
    counts.print();

    let t5 = Instant::now();
    let precision_and = classify::measure_precision(
        &ingested.catalog,
        &ingested.asin_to_product_id,
        &judgments_by_query,
        &compiled_by_query,
        classify::AggregationRule::ExistingAnd,
    );
    let precision_or = classify::measure_precision(
        &ingested.catalog,
        &ingested.asin_to_product_id,
        &judgments_by_query,
        &compiled_by_query,
        classify::AggregationRule::OrWithinAttribute,
    );
    println!(
        "precision measurement took {:.2}s",
        t5.elapsed().as_secs_f64()
    );
    precision_and.print("existing compiler rule (AND across every extracted value)");
    precision_or.print("proposed fix: OR within an attribute, AND across attributes");

    // A handful of concrete examples per class, for qualitative
    // inspection (not itself a metric, but essential for sanity-checking
    // that the classifier's automated buckets mean what they claim to).
    println!("=== Sample queries per class ===");
    for class in [
        classify::QueryClass::ExactIdLookup,
        classify::QueryClass::Ambiguous,
        classify::QueryClass::StructuralOnly,
        classify::QueryClass::StructuralPlusLexical,
        classify::QueryClass::SemanticOccasion,
        classify::QueryClass::LexicalDominant,
        classify::QueryClass::UnresolvedPunt,
    ] {
        let samples: Vec<&str> = query_ids
            .iter()
            .filter(|qid| compiled_by_query[qid].0 == class)
            .take(8)
            .map(|qid| query_text_by_id[qid])
            .collect();
        println!("{}: {:?}", class.label(), samples);
    }

    // Diagnostic: for a few structural_only queries, print the *actual*
    // extracted constraints, to check whether low filter recall (see
    // above) is explained by spurious brand/color matches rather than a
    // correct-but-too-strict filter.
    println!();
    println!("=== structural_only: extracted constraints for the first 10 (diagnostic) ===");
    for qid in query_ids
        .iter()
        .filter(|qid| compiled_by_query[qid].0 == classify::QueryClass::StructuralOnly)
        .take(10)
    {
        let (_, compiled) = &compiled_by_query[qid];
        println!("{:?} -> {:?}", query_text_by_id[qid], compiled.constraints);
    }
}
