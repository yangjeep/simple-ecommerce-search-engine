//! R1-E05: named adversarial physical workloads against the real
//! 1.2M-product index (Area 6) — cases chosen to be bad for a
//! bitmap/narrow-then-verify design specifically, not generic slow
//! queries: a `Text` constraint with no structural predicate to narrow
//! the candidate set first (forces a full-catalog scan), and a
//! high-cardinality facet computation (169k+ distinct real color
//! values). Reported independently, not folded into an aggregate.
//!
//! Usage: cargo run --release -p round1-eval --bin adversarial_physical
//!        [catalog.jsonl]

use std::path::PathBuf;
use std::time::Instant;

use commerce_core::domain::Constraint;
use commerce_core::index::CatalogIndex;
use commerce_core::ir::{CommerceQuery, ResolvedConstraint};
use round1_eval::{catalog, data};

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

fn main() {
    let path = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("dataset_cache/export/catalog.jsonl"));

    println!("loading + indexing {path:?}...");
    let products = data::load_catalog(&path);
    let ingested = catalog::build_catalog(&products);
    let index = CatalogIndex::build(&ingested.catalog);
    println!(
        "ready: {} products indexed",
        ingested.catalog.products.len()
    );
    println!();

    // Adversarial case 1: a Text constraint with NO structural predicate
    // to narrow first. indexed_candidates() has nothing to intersect, so
    // it returns every ordinal in the catalog; execute() then verifies
    // the Text constraint against all of them, one by one.
    println!("=== Adversarial 1: unnarrowed Text scan (no structural predicate at all) ===");
    let text_only_query = CommerceQuery {
        constraints: vec![ResolvedConstraint::Attribute(Constraint::Text {
            attribute: "description".to_string(),
            contains: "waterproof".to_string(),
        })],
        ..CommerceQuery::default()
    };
    let hits = index.execute(&text_only_query, &ingested.catalog).len();
    let samples = time_iters(
        || {
            std::hint::black_box(index.execute(
                std::hint::black_box(&text_only_query),
                std::hint::black_box(&ingested.catalog),
            ));
        },
        30,
    );
    println!(
        "  p50={:.2}ms  p95={:.2}ms  p99={:.2}ms  (n=30, hits={hits}, candidates_scanned={})",
        percentile_ms(&samples, 0.5),
        percentile_ms(&samples, 0.95),
        percentile_ms(&samples, 0.99),
        ingested.catalog.products.len()
    );
    println!();

    // Adversarial case 2: high-cardinality facet — every distinct real
    // color value (169k+ in the full catalog) intersected against the
    // full candidate set.
    println!("=== Adversarial 2: high-cardinality facet (color, full catalog as candidates) ===");
    let all_candidates = index.indexed_candidates(&[]);
    let facet_start = Instant::now();
    let facets = index.facet_counts("color", &all_candidates);
    let facet_elapsed = facet_start.elapsed();
    println!(
        "  single facet_counts(\"color\", all {} ordinals) call: {:.2}ms  ({} distinct non-zero values returned)",
        all_candidates.len(),
        facet_elapsed.as_secs_f64() * 1000.0,
        facets.len()
    );
    let facet_samples = time_iters(
        || {
            std::hint::black_box(index.facet_counts(
                std::hint::black_box("color"),
                std::hint::black_box(&all_candidates),
            ));
        },
        10,
    );
    println!(
        "  repeated (n=10): p50={:.2}ms  p95={:.2}ms  p99={:.2}ms",
        percentile_ms(&facet_samples, 0.5),
        percentile_ms(&facet_samples, 0.95),
        percentile_ms(&facet_samples, 0.99)
    );
    println!();

    // Adversarial case 3: the single most common brand alone (Nike,
    // ~6,165 products / 0.5% of catalog) — a moderately-selective single
    // structural predicate, as a baseline the other cases can be
    // compared against.
    println!("=== Baseline for comparison: single moderately-selective structural filter (brand=Nike alone) ===");
    if let Some(nike) = ingested
        .brands
        .iter()
        .find(|b| b.name == "nike")
        .map(|b| b.id)
    {
        let brand_only_query = CommerceQuery {
            constraints: vec![ResolvedConstraint::Structural(
                commerce_core::ir::StructuralConstraint::Brand(nike),
            )],
            ..CommerceQuery::default()
        };
        let hits = index.execute(&brand_only_query, &ingested.catalog).len();
        let samples = time_iters(
            || {
                std::hint::black_box(index.execute(
                    std::hint::black_box(&brand_only_query),
                    std::hint::black_box(&ingested.catalog),
                ));
            },
            2000,
        );
        println!(
            "  p50={:.4}ms  p95={:.4}ms  p99={:.4}ms  (n=2000, hits={hits})",
            percentile_ms(&samples, 0.5),
            percentile_ms(&samples, 0.95),
            percentile_ms(&samples, 0.99)
        );
    }
}
