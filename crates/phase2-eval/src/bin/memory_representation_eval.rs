//! Issue #6 priority 6: does delegating lexical storage to Tantivy's
//! segment/mmap model close R1-E04's measured RSS gap -- `commerce_core`'s
//! own in-memory bitmap index growing live RSS by 3.76GB for the real
//! 1.2M-product catalog, vs. Solr's 175MB growth for a comparable catalog
//! (`docs/experiments/ROUND1_LOG.md` R1-E04)?
//!
//! `commerce_core`'s structural index is not being replaced by this
//! project's own architecture (ADR 0008: delegate lexical/ranking, keep
//! structural) -- so the real, honest question is not "does the
//! integrated system's RSS match Solr's," it is "does *adding* a
//! delegated Tantivy index on top of the structural index that already
//! exists cost roughly what Tantivy's own mmap-based design promises (a
//! small, mostly-page-cache-backed increment), or does it stack another
//! multi-GB, fully-resident cost on top of the one R1-E04 already found?"
//! Same RSS measurement method as R1-E01 (`round1_eval`'s
//! `profile_catalog.rs`: `/proc/self/status`'s `VmRSS:` line) so numbers
//! are directly comparable, not just similarly named.
//!
//! Usage: cargo run --release -p phase2-eval --bin memory_representation_eval
//!        [catalog.jsonl] [queries.jsonl] [index_dir]

use std::fs;
use std::path::PathBuf;
use std::time::Instant;

use commerce_core::index::CatalogIndex;
use round1_eval::{catalog, data};

use tantivy::collector::TopDocs;
use tantivy::query::QueryParser;
use tantivy::schema::{Schema, STORED, STRING, TEXT};
use tantivy::{doc, Index, IndexWriter, ReloadPolicy};

fn current_rss_kb() -> Option<u64> {
    let status = fs::read_to_string("/proc/self/status").ok()?;
    status
        .lines()
        .find_map(|line| line.strip_prefix("VmRSS:"))
        .and_then(|rest| rest.trim().trim_end_matches(" kB").trim().parse().ok())
}

fn report_rss(label: &str, baseline_kb: u64) -> Option<u64> {
    let rss = current_rss_kb();
    match rss {
        Some(kb) => println!(
            "  {label}: {kb} KB ({:.2} MB) -- +{} KB (+{:.2} MB) since process start",
            kb as f64 / 1024.0,
            kb.saturating_sub(baseline_kb),
            kb.saturating_sub(baseline_kb) as f64 / 1024.0
        ),
        None => println!("  {label}: RSS unavailable"),
    }
    rss
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
        .unwrap_or_else(|| PathBuf::from("dataset_cache/tantivy_memory_eval_index"));

    let baseline_kb = current_rss_kb().unwrap_or(0);
    println!("=== Memory representation: commerce_core structural index + delegated Tantivy index, in the same process ===");
    println!(
        "process baseline RSS (before loading anything): {baseline_kb} KB ({:.2} MB)",
        baseline_kb as f64 / 1024.0
    );

    println!();
    println!("loading real catalog...");
    let products = data::load_catalog(&catalog_path);
    println!("{} real products loaded", products.len());
    report_rss(
        "after loading raw catalog.jsonl into RealProduct structs",
        baseline_kb,
    );

    println!();
    println!("ingesting into commerce_core::domain::Catalog...");
    let ingested = catalog::build_catalog(&products);
    let rss_after_catalog_struct =
        report_rss("after Catalog struct built", baseline_kb).unwrap_or(0);

    println!();
    println!("building commerce_core::index::CatalogIndex (bitmap/range structural index)...");
    let index_build_start = Instant::now();
    let index = CatalogIndex::build(&ingested.catalog);
    println!(
        "  CatalogIndex::build: {:.2}s, approximate_size_bytes={} ({:.2} MB)",
        index_build_start.elapsed().as_secs_f64(),
        index.approximate_size_bytes(),
        index.approximate_size_bytes() as f64 / (1024.0 * 1024.0)
    );
    let rss_after_structural_index = report_rss(
        "after CatalogIndex::build (structural index resident)",
        baseline_kb,
    )
    .unwrap_or(0);

    println!();
    println!("building Tantivy index (same schema/config as P2-E01/P2-E05) on disk...");
    let build_start = Instant::now();
    let tantivy_index = build_tantivy_index(&products, &index_dir)?;
    let index_bytes: u64 = walk_dir_size(&index_dir);
    println!(
        "  Tantivy index built in {:.1}s, on-disk size: {} bytes ({:.1} MB)",
        build_start.elapsed().as_secs_f64(),
        index_bytes,
        index_bytes as f64 / 1_000_000.0
    );
    let rss_after_tantivy_build = report_rss(
        "after Tantivy index built (writer/commit dropped)",
        baseline_kb,
    )
    .unwrap_or(0);

    println!();
    println!("opening a Tantivy reader/searcher (this is where mmap pages get mapped in)...");
    let schema = tantivy_index.schema();
    let text_field = schema.get_field("all_text").unwrap();
    let reader = tantivy_index
        .reader_builder()
        .reload_policy(ReloadPolicy::OnCommitWithDelay)
        .try_into()?;
    let searcher = reader.searcher();
    let query_parser = QueryParser::for_index(&tantivy_index, vec![text_field]);
    let rss_after_reader_open = report_rss(
        "after Tantivy reader/searcher opened (before any query)",
        baseline_kb,
    )
    .unwrap_or(0);

    println!();
    println!("running the real 22,458-query workload once (touches mmap pages under real access patterns)...");
    let judgments = data::load_queries(&queries_path);
    let mut query_texts: Vec<&str> = judgments
        .iter()
        .map(|j| j.query.as_str())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();
    query_texts.sort_unstable();
    let query_count = query_texts.len();
    for query_text in &query_texts {
        let (query, _errors) = query_parser.parse_query_lenient(query_text);
        let _ = searcher.search(&query, &TopDocs::with_limit(10));
    }
    let rss_after_full_query_sweep = report_rss(
        &format!("after running all {query_count} distinct real queries once"),
        baseline_kb,
    )
    .unwrap_or(0);

    println!();
    println!("=== Summary (all deltas relative to process-start baseline={baseline_kb} KB) ===");
    println!(
        "  commerce_core structural index alone:     +{} KB (+{:.2} MB)   (R1-E01's real-scale measurement: +3.76 GB)",
        rss_after_structural_index.saturating_sub(baseline_kb),
        rss_after_structural_index.saturating_sub(baseline_kb) as f64 / 1024.0
    );
    println!(
        "  + Tantivy index build (writer dropped):   +{} KB (+{:.2} MB)   (incremental: +{:.2} MB over structural-only)",
        rss_after_tantivy_build.saturating_sub(baseline_kb),
        rss_after_tantivy_build.saturating_sub(baseline_kb) as f64 / 1024.0,
        rss_after_tantivy_build.saturating_sub(rss_after_structural_index) as f64 / 1024.0
    );
    println!(
        "  + Tantivy reader/searcher opened:         +{} KB (+{:.2} MB)   (incremental: +{:.2} MB)",
        rss_after_reader_open.saturating_sub(baseline_kb),
        rss_after_reader_open.saturating_sub(baseline_kb) as f64 / 1024.0,
        rss_after_reader_open.saturating_sub(rss_after_tantivy_build) as f64 / 1024.0
    );
    println!(
        "  + after full real {query_count}-query sweep:      +{} KB (+{:.2} MB)   (incremental: +{:.2} MB -- mmap pages touched under real access)",
        rss_after_full_query_sweep.saturating_sub(baseline_kb),
        rss_after_full_query_sweep.saturating_sub(baseline_kb) as f64 / 1024.0,
        rss_after_full_query_sweep.saturating_sub(rss_after_reader_open) as f64 / 1024.0
    );
    println!();
    println!(
        "  (for reference, rss_after_catalog_struct (Catalog only, before CatalogIndex) was +{:.2} MB)",
        rss_after_catalog_struct.saturating_sub(baseline_kb) as f64 / 1024.0
    );
    println!();
    println!("(R1-E04 real baseline: commerce-core index (approx, not RSS) 7.3x smaller than Solr's on-disk index, yet commerce-core's RSS grew +3.76GB vs. Solr's +175MB for the same real catalog)");

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
