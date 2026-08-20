//! P2-E11 follow-up diagnostic: alias-normalization (tier 1) and fuzzy
//! matching (tier 2) both measured a negligible real-data effect. Before
//! concluding *why*, sample real cases where a judged-Exact product fails
//! the compiled brand filter and classify each by whether the product's
//! actual raw brand string is (a) alias-identical to the query's resolved
//! brand phrase (tier 1 should have caught this -- expected: none, since
//! tier 1 measured zero effect), (b) fuzzy-close (tier 2's territory), or
//! (c) neither (a genuinely different brand string -- not an
//! enforcement-mechanism problem at all).
//!
//! Usage: cargo run --release -p phase2-eval --bin brand_recall_gap_diagnostic
//!        [catalog.jsonl] [queries.jsonl]

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use commerce_core::cold_start::{alias, compile_lexicon, CatalogProfile};
use commerce_core::domain::BrandId;
use commerce_core::ir::{compile, ResolvedConstraint, StructuralConstraint};
use round1_eval::data::{self, EsciLabel};
use round1_eval::{catalog, classify};

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

    let brand_name_by_id: HashMap<BrandId, String> = ingested
        .brands
        .iter()
        .map(|b| (b.id, b.name.clone()))
        .collect();

    let profile = CatalogProfile::build(&ingested.catalog, &ingested.brands, &[], &[]);
    let lexicon = compile_lexicon(&profile, 25);

    println!("loading real queries + judgments...");
    let judgments = data::load_queries(&queries_path);
    let mut judged_by_query: HashMap<u64, (String, Vec<(String, EsciLabel)>)> = HashMap::new();
    for j in &judgments {
        judged_by_query
            .entry(j.query_id)
            .or_insert_with(|| (j.query.clone(), Vec::new()))
            .1
            .push((j.product_id.clone(), j.label));
    }
    let known_ids: HashSet<&str> = ingested
        .asin_to_product_id
        .keys()
        .map(String::as_str)
        .collect();

    let mut alias_identical = 0usize;
    let mut fuzzy_close = 0usize; // edit_distance(alias_key) in 1..=2
    let mut neither = 0usize;
    let mut samples: Vec<String> = Vec::new();

    // Distinct-query view: a single problematic brand term (e.g. "case")
    // can contribute many judged-product rows for one query, inflating
    // the row-level counts above without reflecting how many genuinely
    // *different* queries are affected. Track that separately.
    let mut queries_neither: HashSet<String> = HashSet::new();
    let mut queries_alias_or_fuzzy: HashSet<String> = HashSet::new();
    let mut neither_samples_by_query: HashMap<String, String> = HashMap::new();

    for (query_text, judged) in judged_by_query.values() {
        let compiled = compile(query_text, &lexicon);
        let class = classify::classify(query_text, &compiled, &known_ids);
        if !matches!(
            class,
            classify::QueryClass::StructuralOnly | classify::QueryClass::StructuralPlusLexical
        ) {
            continue;
        }
        let brand_constraint_id = compiled.constraints.iter().find_map(|c| match c {
            ResolvedConstraint::Structural(StructuralConstraint::Brand(id)) => Some(*id),
            _ => None,
        });
        let Some(expected_id) = brand_constraint_id else {
            continue;
        };
        let Some(expected_name) = brand_name_by_id.get(&expected_id) else {
            continue;
        };
        let expected_key = alias::alias_key(expected_name);

        for (asin, label) in judged {
            if *label != EsciLabel::Exact {
                continue;
            }
            let Some(&product_id) = ingested.asin_to_product_id.get(asin) else {
                continue;
            };
            let Some(product) = ingested.catalog.products.get(product_id.0 as usize) else {
                continue;
            };
            if product.brand == expected_id {
                continue; // satisfies the filter, not a gap case
            }
            let actual_name = brand_name_by_id
                .get(&product.brand)
                .map(String::as_str)
                .unwrap_or("<no brand>");
            let actual_key = alias::alias_key(actual_name);
            let d = alias::edit_distance(&expected_key, &actual_key);

            if expected_key == actual_key {
                alias_identical += 1;
                queries_alias_or_fuzzy.insert(query_text.clone());
            } else if d <= 2 {
                fuzzy_close += 1;
                queries_alias_or_fuzzy.insert(query_text.clone());
            } else {
                neither += 1;
                queries_neither.insert(query_text.clone());
                neither_samples_by_query.entry(query_text.clone()).or_insert_with(|| {
                    format!(
                        "query={query_text:?} expected_brand={expected_name:?} actual_brand={actual_name:?} edit_distance(alias_key)={d}"
                    )
                });
            }

            if samples.len() < 25 {
                samples.push(format!(
                    "query={query_text:?} expected_brand={expected_name:?} actual_brand={actual_name:?} edit_distance(alias_key)={d}"
                ));
            }
        }
    }

    println!();
    println!("=== P2-E11 follow-up: root cause of Exact-labeled brand-filter misses ===");
    println!("--- row-level (one row per judged Exact product that fails the filter) ---");
    println!("alias-identical (tier 1's territory, expect ~0): {alias_identical}");
    println!("fuzzy-close, edit_distance<=2 (tier 2's territory): {fuzzy_close}");
    println!("neither (genuinely different brand string): {neither}");
    println!("--- distinct-query-level (a single problematic brand term can inflate the row counts above) ---");
    println!(
        "distinct queries with an alias/fuzzy-explainable miss: {}",
        queries_alias_or_fuzzy.len()
    );
    println!(
        "distinct queries with a 'neither' miss: {}",
        queries_neither.len()
    );
    println!();
    println!("=== first 25 rows (unfiltered) ===");
    for s in &samples {
        println!("  {s}");
    }
    println!();
    println!("=== up to 25 DISTINCT queries in the 'neither' bucket (one row each) ===");
    for (i, s) in neither_samples_by_query.values().take(25).enumerate() {
        println!("  {}: {s}", i + 1);
    }
}
