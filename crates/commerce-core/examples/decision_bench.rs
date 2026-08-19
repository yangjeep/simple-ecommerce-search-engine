//! Gate 7 benchmark/decision report: a CLI report tool per
//! `docs/EXPERIMENT_LOOP.md` ("no UI unless required to inspect an
//! experiment and a CLI/report would not suffice"). Prints P50/P95/P99
//! query latency (indexed vs. linear scan), facet latency, index build
//! time, approximate index size, RSS delta, and QPS/core (derived from
//! single-threaded median latency), at three scale-ladder tiers.
//!
//! Not run in CI: multi-second wall clock, meant for a human/experimenter
//! to read. Run with: `cargo run --release --example decision_bench`.

#[path = "../benches/common/mod.rs"]
mod common;

use std::fs;
use std::time::Instant;

use commerce_core::domain::{Constraint, NumericOp};
use commerce_core::index::CatalogIndex;
use commerce_core::ir::{CommerceQuery, ResolvedConstraint};

fn percentile_us(sorted_ns: &[u128], p: f64) -> f64 {
    if sorted_ns.is_empty() {
        return 0.0;
    }
    let idx = ((sorted_ns.len() as f64 - 1.0) * p).round() as usize;
    sorted_ns[idx] as f64 / 1000.0
}

fn current_rss_kb() -> Option<u64> {
    let status = fs::read_to_string("/proc/self/status").ok()?;
    status
        .lines()
        .find_map(|line| line.strip_prefix("VmRSS:"))
        .and_then(|rest| rest.trim().trim_end_matches(" kB").trim().parse().ok())
}

fn structural_query() -> CommerceQuery {
    CommerceQuery {
        constraints: vec![
            ResolvedConstraint::Attribute(Constraint::Enum {
                attribute: "color".to_string(),
                value: "Black".to_string(),
            }),
            ResolvedConstraint::Attribute(Constraint::Numeric {
                attribute: "size".to_string(),
                op: NumericOp::Gte,
                value: 9.0,
            }),
        ],
        ..CommerceQuery::default()
    }
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

fn print_percentiles(label: &str, samples: &[u128], n: usize) {
    println!(
        "  {label:<20} p50={:>9.1}us  p95={:>9.1}us  p99={:>9.1}us  (n={n})",
        percentile_us(samples, 0.5),
        percentile_us(samples, 0.95),
        percentile_us(samples, 0.99),
    );
}

fn report_tier(product_count: u64) {
    let variant_count = product_count * 2;
    println!("=== {product_count} products / {variant_count} variants ===");

    let gen_start = Instant::now();
    let catalog = common::synthetic_catalog(product_count);
    let catalog_gen_ms = gen_start.elapsed().as_secs_f64() * 1000.0;

    let rss_before = current_rss_kb();
    let build_samples = time_iters(
        || {
            std::hint::black_box(CatalogIndex::build(std::hint::black_box(&catalog)));
        },
        10,
    );
    let index = CatalogIndex::build(&catalog);
    let rss_after = current_rss_kb();

    let query = structural_query();
    let indexed_query_iters = if product_count >= 100_000 { 500 } else { 2000 };
    let linear_query_iters = if product_count >= 100_000 { 20 } else { 100 };

    let indexed_samples = time_iters(
        || {
            std::hint::black_box(
                index.execute(std::hint::black_box(&query), std::hint::black_box(&catalog)),
            );
        },
        indexed_query_iters,
    );
    let linear_samples = time_iters(
        || {
            std::hint::black_box(query.execute(std::hint::black_box(&catalog)));
        },
        linear_query_iters,
    );

    let candidates = index.indexed_candidates(&query.constraints);
    let facet_samples = time_iters(
        || {
            std::hint::black_box(index.facet_counts(
                std::hint::black_box("color"),
                std::hint::black_box(&candidates),
            ));
        },
        indexed_query_iters,
    );

    println!("  catalog generation:  {catalog_gen_ms:.2} ms");
    print_percentiles("index build", &build_samples, build_samples.len());
    print_percentiles("indexed query", &indexed_samples, indexed_samples.len());
    print_percentiles("linear-scan query", &linear_samples, linear_samples.len());
    print_percentiles("facet_counts", &facet_samples, facet_samples.len());

    let indexed_p50_us = percentile_us(&indexed_samples, 0.5);
    let linear_p50_us = percentile_us(&linear_samples, 0.5);
    println!(
        "  QPS/core (indexed, 1/p50):   {:.0}",
        1_000_000.0 / indexed_p50_us
    );
    println!(
        "  QPS/core (linear, 1/p50):    {:.0}",
        1_000_000.0 / linear_p50_us
    );
    println!(
        "  speedup (linear p50 / indexed p50):  {:.1}x",
        linear_p50_us / indexed_p50_us
    );
    println!(
        "  index size (approx):  {} bytes ({:.2} MB)",
        index.approximate_size_bytes(),
        index.approximate_size_bytes() as f64 / (1024.0 * 1024.0)
    );
    match (rss_before, rss_after) {
        (Some(before), Some(after)) => println!(
            "  RSS around index build:  {before} KB -> {after} KB (+{} KB)",
            after.saturating_sub(before)
        ),
        _ => println!("  RSS: unavailable (/proc/self/status not readable on this platform)"),
    }
    println!(
        "  correctness check: {} hits for the sample query",
        index.execute(&query, &catalog).len()
    );
    println!();
}

fn main() {
    for &tier in &[1_000u64, 10_000, 100_000] {
        report_tier(tier);
    }
}
