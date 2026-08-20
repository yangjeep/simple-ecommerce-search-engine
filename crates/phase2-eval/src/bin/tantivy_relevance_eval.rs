//! P2-E01: validates ADR 0008's central bet. Does an embedded Tantivy
//! index, given the exact same real catalog and real ESCI judgments used
//! throughout Round 1 (docs/experiments/ROUND1_LOG.md), recover the
//! relevance quality Solr demonstrated in R1-E04 (NDCG@10=0.3052,
//! Recall@10=0.1811, zero-result rate=0.2%) -- in-process, without Solr's
//! JVM/HTTP overhead, and without `commerce_core`'s architecturally-absent
//! ranking (R1-E04/E07)?
//!
//! Metric definitions match scripts/round1/solr_bench.py exactly (graded
//! relevance E=3/S=2/C=1/I=0, NDCG@10 with log2(i+2) discount and an ideal
//! ranking from ALL judged grades for that query, Recall@10 against
//! Exact+Substitute, MRR, zero-result rate) so results are directly
//! comparable, not just similarly named. Evaluated against the FULL real
//! 22,458-query set (every query with at least one Exact/Substitute
//! judgment), not a 1,000-query sample like R1-E04's Python/HTTP
//! benchmark -- Solr's Python random.Random(seed=7).sample cannot be
//! reproduced bit-for-bit in Rust, and the full set is strictly stronger
//! evidence when it's computationally tractable, which an in-process
//! query is.
//!
//! Usage: cargo run --release -p phase2-eval --bin tantivy_relevance_eval
//!        [catalog.jsonl] [queries.jsonl] [index_dir]

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Instant;

use round1_eval::data::{self, EsciLabel, RealProduct};
use tantivy::collector::TopDocs;
use tantivy::query::QueryParser;
use tantivy::schema::{Schema, Value, STORED, STRING, TEXT};
use tantivy::{doc, Index, IndexWriter, ReloadPolicy, TantivyDocument};

fn relevance_gain(label: EsciLabel) -> f64 {
    match label {
        EsciLabel::Exact => 3.0,
        EsciLabel::Substitute => 2.0,
        EsciLabel::Complement => 1.0,
        EsciLabel::Irrelevant => 0.0,
    }
}

fn percentile_ms(sorted_ns: &[u128], p: f64) -> f64 {
    if sorted_ns.is_empty() {
        return 0.0;
    }
    let idx = ((sorted_ns.len() as f64 - 1.0) * p).round() as usize;
    sorted_ns[idx] as f64 / 1_000_000.0
}

fn build_index(products: &[RealProduct], index_dir: &PathBuf) -> tantivy::Result<Index> {
    if index_dir.exists() {
        std::fs::remove_dir_all(index_dir).expect("clear stale index dir");
    }
    std::fs::create_dir_all(index_dir).expect("create index dir");

    let mut schema_builder = Schema::builder();
    let id_field = schema_builder.add_text_field("id", STRING | STORED);
    let text_field = schema_builder.add_text_field("all_text", TEXT);
    let schema = schema_builder.build();

    let index = Index::create_in_dir(index_dir, schema)?;
    // 512MB indexing heap: generous for 1.2M real products' worth of
    // title+description+bullets text, well within this environment's RAM.
    let mut writer: IndexWriter = index.writer(512_000_000)?;

    for product in products {
        let all_text = format!(
            "{} {} {}",
            product.title,
            product.description.as_deref().unwrap_or(""),
            product.bullets.as_deref().unwrap_or("")
        );
        writer.add_document(doc!(
            id_field => product.id.clone(),
            text_field => all_text,
        ))?;
    }
    writer.commit()?;
    Ok(index)
}

fn main() -> tantivy::Result<()> {
    let mut args = std::env::args().skip(1);
    let catalog_path = args
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("dataset_cache/export/catalog.jsonl"));
    let queries_path = args
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("dataset_cache/export/queries.jsonl"));
    let index_dir = args
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("dataset_cache/tantivy_index"));

    println!("loading real catalog...");
    let products = data::load_catalog(&catalog_path);
    println!("{} real products loaded", products.len());

    println!("building Tantivy index (schema: id STRING|STORED, all_text TEXT, default tokenizer, default BM25 scoring)...");
    let build_start = Instant::now();
    let index = build_index(&products, &index_dir)?;
    let build_elapsed = build_start.elapsed();
    let index_bytes: u64 = walk_dir_size(&index_dir);
    println!(
        "index built in {:.1}s, on-disk size: {} bytes ({:.1} MB)",
        build_elapsed.as_secs_f64(),
        index_bytes,
        index_bytes as f64 / 1_000_000.0
    );

    let schema = index.schema();
    let id_field = schema.get_field("id").unwrap();
    let text_field = schema.get_field("all_text").unwrap();
    let reader = index
        .reader_builder()
        .reload_policy(ReloadPolicy::OnCommitWithDelay)
        .try_into()?;
    let searcher = reader.searcher();
    let query_parser = QueryParser::for_index(&index, vec![text_field]);

    println!("loading real queries + judgments...");
    let judgments = data::load_queries(&queries_path);
    let mut judged_by_query: HashMap<u64, (String, HashMap<String, EsciLabel>)> = HashMap::new();
    for j in &judgments {
        judged_by_query
            .entry(j.query_id)
            .or_insert_with(|| (j.query.clone(), HashMap::new()))
            .1
            .insert(j.product_id.clone(), j.label);
    }

    let mut ndcgs = Vec::new();
    let mut recalls = Vec::new();
    let mut mrrs = Vec::new();
    let mut zero_result = 0usize;
    let mut evaluated = 0usize;
    let mut latency_samples = Vec::new();

    for (query_text, judged) in judged_by_query.values() {
        let relevant_ids: std::collections::HashSet<&String> = judged
            .iter()
            .filter(|(_, label)| label.is_relevant())
            .map(|(pid, _)| pid)
            .collect();
        if relevant_ids.is_empty() {
            continue;
        }

        let (query, _parse_errors) = query_parser.parse_query_lenient(query_text);

        let start = Instant::now();
        let top_docs = searcher
            .search(&query, &TopDocs::with_limit(10))
            .unwrap_or_default();
        latency_samples.push(start.elapsed().as_nanos());

        evaluated += 1;
        if top_docs.is_empty() {
            zero_result += 1;
        }

        let ranked_ids: Vec<String> = top_docs
            .iter()
            .map(|(_score, addr)| {
                let retrieved: TantivyDocument = searcher.doc(*addr).expect("stored doc");
                retrieved
                    .get_first(id_field)
                    .and_then(|v| v.as_str())
                    .expect("id field is stored")
                    .to_string()
            })
            .collect();

        let dcg: f64 = ranked_ids
            .iter()
            .enumerate()
            .map(|(i, pid)| {
                let gain = judged
                    .get(pid)
                    .map(|&label| relevance_gain(label))
                    .unwrap_or(0.0);
                gain / (i as f64 + 2.0).log2()
            })
            .sum();
        let mut ideal_gains: Vec<f64> = judged.values().map(|&l| relevance_gain(l)).collect();
        ideal_gains.sort_by(|a, b| b.total_cmp(a));
        let idcg: f64 = ideal_gains
            .iter()
            .take(10)
            .enumerate()
            .map(|(i, &g)| g / (i as f64 + 2.0).log2())
            .sum();
        ndcgs.push(if idcg > 0.0 { dcg / idcg } else { 0.0 });

        let hit = ranked_ids
            .iter()
            .filter(|pid| relevant_ids.contains(pid))
            .count();
        recalls.push(hit as f64 / relevant_ids.len() as f64);

        let rr = ranked_ids
            .iter()
            .position(|pid| relevant_ids.contains(pid))
            .map(|pos| 1.0 / (pos as f64 + 1.0))
            .unwrap_or(0.0);
        mrrs.push(rr);
    }

    latency_samples.sort_unstable();
    let n = ndcgs.len() as f64;
    println!();
    println!("=== Relevance: NDCG@10 / Recall@10 / MRR (real ESCI judgments, FULL query set) ===");
    println!(
        "  queries with >=1 Exact/Substitute judgment (evaluated): {}",
        evaluated
    );
    println!(
        "  zero-result rate: {}/{} ({:.1}%)",
        zero_result,
        evaluated,
        zero_result as f64 / evaluated as f64 * 100.0
    );
    println!("  NDCG@10:   {:.4}", ndcgs.iter().sum::<f64>() / n);
    println!("  Recall@10: {:.4}", recalls.iter().sum::<f64>() / n);
    println!("  MRR:       {:.4}", mrrs.iter().sum::<f64>() / n);
    println!();
    println!(
        "  query latency (in-process, n={}): p50={:.4}ms  p95={:.4}ms  p99={:.4}ms",
        latency_samples.len(),
        percentile_ms(&latency_samples, 0.5),
        percentile_ms(&latency_samples, 0.95),
        percentile_ms(&latency_samples, 0.99)
    );
    println!();
    println!("(R1-E04 Solr baseline, 1,000-query sample, HTTP round-trip: zero-result rate=0.2%, NDCG@10=0.3052, Recall@10=0.1811, MRR=0.4910, p50=1486us)");

    Ok(())
}

fn walk_dir_size(dir: &PathBuf) -> u64 {
    let mut total = 0u64;
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            if let Ok(meta) = entry.metadata() {
                if meta.is_file() {
                    total += meta.len();
                } else if meta.is_dir() {
                    total += walk_dir_size(&entry.path());
                }
            }
        }
    }
    total
}
