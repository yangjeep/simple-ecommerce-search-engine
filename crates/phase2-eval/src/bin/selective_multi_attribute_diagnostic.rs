//! P2-E13 follow-up diagnostic: the first fixed P1-D sweep found
//! `selective_multi_attribute_structural` at 100% zero-result for
//! commerce-native (22/22 real queries) while Tantivy scores NDCG@10=0.48
//! on the *same* queries -- a suspiciously total failure, not a narrow-
//! selectivity effect. Hypothesis: this harness passes empty
//! `product_types`/`categories` slices to `CatalogProfile::build` (the
//! real Amazon ESCI catalog has no such field --
//! `round1_eval::catalog`'s documented `UNKNOWN_PRODUCT_TYPE`/
//! `UNKNOWN_CATEGORY` sentinel), so the *only* structural entity type the
//! lexicon can ever emit is `Brand`/`BrandAny`. A query classified
//! `SelectiveMultiAttribute` (>=2 structural entity constraints) can
//! therefore only mean two *different* Brand constraints both landed in
//! the same query and were ANDed together -- which is impossible for any
//! real product (one product, one brand) and would explain a 100%,
//! not-just-low, zero-result rate. This diagnostic prints exactly what
//! each of those 22 real queries compiled to, to confirm or refute that
//! before treating it as an architecture finding rather than a query
//! artifact.
//!
//! Usage: cargo run --release -p phase2-eval --bin selective_multi_attribute_diagnostic
//!        [catalog.jsonl] [queries.jsonl]

use std::collections::BTreeMap;
use std::path::PathBuf;

use commerce_core::cold_start::{compile_lexicon, CatalogProfile};
use commerce_core::ir::compile;
use query_taxonomy::{classify9, QueryClass9};
use round1_eval::data::{self, EsciLabel};
use round1_eval::{catalog, query_taxonomy};

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

    // Same profile/lexicon construction as p1d_physical_advantage_eval.
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

    let mut shown = 0;
    for (&qid, (text, judged)) in &judged_by_query {
        if !judged.values().any(|l| l.is_relevant()) {
            continue;
        }
        let compiled = compile(text, &lexicon);
        let class = classify9(text, &compiled);
        if class != QueryClass9::SelectiveMultiAttribute {
            continue;
        }
        shown += 1;
        println!("--- qid={qid} text={text:?}");
        println!("    constraints: {:?}", compiled.constraints);
        println!(
            "    preferences: {:?}  residual_lexical: {:?}  ambiguous: {}",
            compiled.preferences,
            compiled.residual_lexical,
            compiled.ambiguous.len()
        );
    }
    println!("\n{shown} real queries classified SelectiveMultiAttribute");
}
