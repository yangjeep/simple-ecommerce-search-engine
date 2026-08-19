//! Issue #7 Experiment #1 (`docs/research/` synthesis, ranked highest
//! information value of 5 residual hypotheses): R1-E05
//! (`docs/experiments/ROUND1_LOG.md`) measured a raw, undelegated
//! `CatalogIndex::execute` call with no structural predicate at all --
//! "Text{contains:\"waterproof\"}" alone -- at a catastrophic 961.23ms
//! p50, ~36,700x worse than a moderately-selective single-brand baseline
//! (26.2us). That measurement predates Issue #6 priority 5's planner
//! (`commerce_core::plan`), which routes exactly this shape of query
//! (no structural constraint at all) to `ExecutionOutcome::Punt` --
//! delegating to Tantivy instead of ever calling the raw linear scan.
//!
//! The Issue #7 archaeology synthesis's Experiment #1 asked whether
//! *building* a bounded top-K early-termination mechanism would recover
//! Issue #7's revised >=5x P50/P95 bar. That would be reinventing
//! machinery Tantivy already has (the synthesis's own §4/Wheel-
//! Reinvention-Candidates list names exactly this: "Custom WAND/weak-AND
//! top-K pruning... Tantivy already implements this internally"). The
//! real, cheaper, honest question -- never directly measured until this
//! entry -- is simpler: **does the planner+Tantivy-delegate composition
//! Issue #6/P2-E05 already built and validated on aggregate relevance
//! also fix R1-E05's specific named adversarial latency case, without
//! writing any new mechanism at all?**
//!
//! Reuses `planner_integration_eval.rs`'s exact `TantivyDelegate`
//! wrapper/schema/build (kept self-contained per this crate's existing
//! one-binary-per-experiment convention) and R1-E05's own timing
//! methodology (`time_iters`, n=30) so the comparison is apples-to-apples,
//! not just similarly named.
//!
//! Usage: cargo run --release -p phase2-eval --bin punt_path_adversarial_eval
//!        [catalog.jsonl] [index_dir]

use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;
use std::time::Instant;

use commerce_core::domain::ProductId;
use commerce_core::index::CatalogIndex;
use commerce_core::ir::CommerceQuery;
use commerce_core::plan::{
    execute_planned, ExecutionOutcome, LexicalDelegate, LexicalHit, PlannerPolicy,
};
use round1_eval::{catalog, data};

use tantivy::collector::TopDocs;
use tantivy::query::QueryParser;
use tantivy::schema::{Schema, Value, STORED, STRING, TEXT};
use tantivy::{doc, Index, IndexWriter, ReloadPolicy, TantivyDocument};

fn percentile_ms(sorted_ns: &[u128], p: f64) -> f64 {
    if sorted_ns.is_empty() {
        return 0.0;
    }
    let idx = ((sorted_ns.len() as f64 - 1.0) * p).round() as usize;
    sorted_ns[idx] as f64 / 1_000_000.0
}

fn time_iters<F: FnMut()>(mut f: F, n: usize) -> Vec<u128> {
    let mut samples = Vec::with_capacity(n);
    for _ in 0..n {
        let start = Instant::now();
        f();
        samples.push(start.elapsed().as_nanos());
    }
    samples.sort_unstable();
    samples
}

fn build_tantivy_index(
    products: &[data::RealProduct],
    index_dir: &PathBuf,
) -> tantivy::Result<Index> {
    if index_dir.exists() {
        std::fs::remove_dir_all(index_dir).expect("clear stale index dir");
    }
    std::fs::create_dir_all(index_dir).expect("create index dir");
    let mut schema_builder = Schema::builder();
    let id_field = schema_builder.add_text_field("id", STRING | STORED);
    let text_field = schema_builder.add_text_field("all_text", TEXT);
    let schema = schema_builder.build();
    let index = Index::create_in_dir(index_dir, schema)?;
    let mut writer: IndexWriter = index.writer(512_000_000)?;
    for product in products {
        let all_text = format!(
            "{} {} {}",
            product.title,
            product.description.as_deref().unwrap_or(""),
            product.bullets.as_deref().unwrap_or("")
        );
        writer.add_document(doc!(id_field => product.id.clone(), text_field => all_text))?;
    }
    writer.commit()?;
    Ok(index)
}

struct TantivyDelegate<'a> {
    searcher: tantivy::Searcher,
    query_parser: QueryParser,
    id_field: tantivy::schema::Field,
    asin_to_product_id: &'a HashMap<String, ProductId>,
}

impl LexicalDelegate for TantivyDelegate<'_> {
    fn search(
        &self,
        terms: &[String],
        restrict_to: Option<&BTreeSet<ProductId>>,
        limit: usize,
    ) -> Vec<LexicalHit> {
        if terms.is_empty() {
            return Vec::new();
        }
        debug_assert!(
            restrict_to.is_none(),
            "this experiment only exercises the Punt path (see main()); restrict_to would need \
             planner_integration_eval.rs's TermSetQuery push-down if this delegate were reused for Hybrid"
        );
        let text = terms.join(" ");
        let (text_query, _errors) = self.query_parser.parse_query_lenient(&text);
        let top_docs = self
            .searcher
            .search(&text_query, &TopDocs::with_limit(limit))
            .unwrap_or_default();
        top_docs
            .into_iter()
            .filter_map(|(score, addr)| {
                let doc: TantivyDocument = self.searcher.doc(addr).ok()?;
                let asin = doc.get_first(self.id_field)?.as_str()?;
                let product = *self.asin_to_product_id.get(asin)?;
                Some(LexicalHit {
                    product,
                    score: score as f64,
                })
            })
            .collect()
    }
}

fn main() -> tantivy::Result<()> {
    let mut args = std::env::args().skip(1);
    let catalog_path = args
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("dataset_cache/export/catalog.jsonl"));
    let index_dir = args
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("dataset_cache/tantivy_punt_adversarial_index"));

    println!("loading + ingesting real catalog...");
    let products = data::load_catalog(&catalog_path);
    let ingested = catalog::build_catalog(&products);
    println!("{} real products ingested", ingested.catalog.products.len());

    println!("building commerce_core structural index...");
    let index = CatalogIndex::build(&ingested.catalog);

    println!("building Tantivy index (same schema/config as P2-E01/P2-E05)...");
    let t0 = Instant::now();
    let tantivy_index = build_tantivy_index(&products, &index_dir)?;
    println!("Tantivy index built in {:.1}s", t0.elapsed().as_secs_f64());
    let schema = tantivy_index.schema();
    let id_field = schema.get_field("id").unwrap();
    let text_field = schema.get_field("all_text").unwrap();
    let reader = tantivy_index
        .reader_builder()
        .reload_policy(ReloadPolicy::OnCommitWithDelay)
        .try_into()?;
    let delegate = TantivyDelegate {
        searcher: reader.searcher(),
        query_parser: QueryParser::for_index(&tantivy_index, vec![text_field]),
        id_field,
        asin_to_product_id: &ingested.asin_to_product_id,
    };

    // Exactly R1-E05 Case 1's query shape: no structural constraint at
    // all, one common free-text term ("waterproof", 14,839 real matches
    // per R1-E05). commerce_core::plan::plan() routes this straight to
    // Punt (query.constraints.is_empty()), never touching the raw linear
    // scan R1-E05 measured.
    let query = CommerceQuery {
        constraints: vec![],
        preferences: vec![],
        ambiguous: vec![],
        residual_lexical: vec!["waterproof".to_string()],
    };
    let policy = PlannerPolicy {
        selectivity_threshold: 0.05,
        delegate_oversample: 20,
    };
    const K: usize = 10;

    let planned =
        commerce_core::plan::plan(&query, &index, ingested.catalog.products.len(), &policy);
    assert_eq!(
        planned.outcome,
        ExecutionOutcome::Punt,
        "R1-E05 Case 1's exact query shape (no structural constraint) must route to Punt"
    );
    println!(
        "\nplanner routing confirmed: {:?} (as expected -- no structural constraint at all)",
        planned.outcome
    );

    let (_, hits) = execute_planned(
        &query,
        &ingested.catalog,
        &index,
        Some(&delegate),
        K,
        &policy,
    );
    println!("first call: {} hits returned (k={K})", hits.len());

    println!(
        "\n=== Reproducing R1-E05 Case 1 through the current planner+Tantivy-delegate path ==="
    );
    let samples = time_iters(
        || {
            std::hint::black_box(execute_planned(
                std::hint::black_box(&query),
                std::hint::black_box(&ingested.catalog),
                std::hint::black_box(&index),
                Some(&delegate),
                K,
                &policy,
            ));
        },
        30,
    );
    let p50 = percentile_ms(&samples, 0.5);
    let p95 = percentile_ms(&samples, 0.95);
    let p99 = percentile_ms(&samples, 0.99);
    println!("  p50={p50:.4}ms  p95={p95:.4}ms  p99={p99:.4}ms  (n=30, k={K})");

    println!("\n=== Comparison against R1-E05's real, recorded baselines ===");
    const R1_E05_CASE1_P50_MS: f64 = 961.23;
    const R1_E05_CASE3_P50_MS: f64 = 0.0262;
    println!("  R1-E05 Case 1 (raw unbounded linear scan, no delegate):        p50={R1_E05_CASE1_P50_MS:.2}ms");
    println!("  R1-E05 Case 3 (moderately-selective single-brand baseline):    p50={R1_E05_CASE3_P50_MS:.4}ms");
    println!("  this experiment (same query, current planner+delegate):       p50={p50:.4}ms");
    println!(
        "  multiplier vs. Case 1:  {:.1}x faster  (bar: >=5x)",
        R1_E05_CASE1_P50_MS / p50.max(0.0001)
    );
    println!(
        "  multiplier vs. Case 3 (how much further than the selective baseline this still is): {:.1}x",
        p50 / R1_E05_CASE3_P50_MS.max(0.0001)
    );
    println!();
    println!("(relevance cross-check: this exact query text has no real judged relevance ground truth in the ESCI query set, matching R1-E05's own case-1 choice -- see PHASE2_LOG.md P2-E05 for the already-measured aggregate NDCG@10/Recall@10/MRR across the FULL real query set including every Punt-routed query, which is the relevance evidence this latency win must not be read in isolation from)");

    Ok(())
}
