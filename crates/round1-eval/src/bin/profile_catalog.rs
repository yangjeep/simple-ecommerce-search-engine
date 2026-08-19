//! R1-E01: ingest the real catalog, profile it, and measure `CatalogIndex`
//! build time / RSS / index size at real scale (extends Gate 7's scale
//! ladder with real rather than synthetic data).
//!
//! Usage: cargo run --release -p round1-eval --bin profile_catalog
//!        [path/to/catalog.jsonl] [max_products]

use std::fs;
use std::path::PathBuf;
use std::time::Instant;

use commerce_core::index::CatalogIndex;
use round1_eval::{catalog, data, profile};

fn current_rss_kb() -> Option<u64> {
    let status = fs::read_to_string("/proc/self/status").ok()?;
    status
        .lines()
        .find_map(|line| line.strip_prefix("VmRSS:"))
        .and_then(|rest| rest.trim().trim_end_matches(" kB").trim().parse().ok())
}

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("dataset_cache/export/catalog.jsonl"));
    let max_products: Option<usize> = args.next().and_then(|s| s.parse().ok());

    println!("loading {path:?}...");
    let load_start = Instant::now();
    let mut products = data::load_catalog(&path);
    if let Some(max) = max_products {
        products.truncate(max);
    }
    println!(
        "loaded {} products in {:.2}s",
        products.len(),
        load_start.elapsed().as_secs_f64()
    );

    let raw_text_bytes: usize = products
        .iter()
        .map(|p| {
            p.title.len()
                + p.description.as_ref().map_or(0, String::len)
                + p.bullets.as_ref().map_or(0, String::len)
                + p.brand.as_ref().map_or(0, String::len)
                + p.color.as_ref().map_or(0, String::len)
        })
        .sum();
    println!(
        "raw source text bytes (title+description+bullets+brand+color, UTF-8, no overhead): {} ({:.2} MB, {:.1} bytes/product)",
        raw_text_bytes,
        raw_text_bytes as f64 / (1024.0 * 1024.0),
        raw_text_bytes as f64 / products.len() as f64
    );

    let report = profile::profile(&products);
    report.print();

    let rss_before_catalog = current_rss_kb();
    let build_catalog_start = Instant::now();
    let ingested = catalog::build_catalog(&products);
    println!(
        "Catalog struct build: {:.2}s for {} products, {} distinct brands",
        build_catalog_start.elapsed().as_secs_f64(),
        ingested.catalog.products.len(),
        ingested.brands.len()
    );
    let rss_after_catalog = current_rss_kb();

    let index_build_start = Instant::now();
    let index = CatalogIndex::build(&ingested.catalog);
    let index_build_secs = index_build_start.elapsed().as_secs_f64();
    let rss_after_index = current_rss_kb();

    println!();
    println!("=== CatalogIndex at real scale (evidence class: real catalog, synthetic-free) ===");
    println!("products: {}", ingested.catalog.products.len());
    println!("index build time: {index_build_secs:.3}s");
    println!(
        "index size (approx): {} bytes ({:.2} MB)",
        index.approximate_size_bytes(),
        index.approximate_size_bytes() as f64 / (1024.0 * 1024.0)
    );
    if let (Some(before), Some(after_catalog), Some(after_index)) =
        (rss_before_catalog, rss_after_catalog, rss_after_index)
    {
        println!(
            "RSS: before_catalog={before} KB, after_catalog_struct={after_catalog} KB (+{} KB), after_index={after_index} KB (+{} KB total index+catalog)",
            after_catalog.saturating_sub(before),
            after_index.saturating_sub(before)
        );
        let approx = index.approximate_size_bytes() as u64 / 1024;
        let rss_delta = after_index.saturating_sub(before);
        println!(
            "approximate_size_bytes as fraction of RSS delta: {:.1}%",
            approx as f64 / rss_delta.max(1) as f64 * 100.0
        );
    } else {
        println!("RSS: unavailable");
    }
    println!(
        "bytes/product (index only, approx): {:.1}",
        index.approximate_size_bytes() as f64 / ingested.catalog.products.len() as f64
    );
}
