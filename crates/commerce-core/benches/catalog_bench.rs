//! Gate 0 benchmark harness. The fixed seed makes the synthetic catalog
//! reproducible across runs; this is a correctness-agnostic performance
//! probe (see docs/EXPERIMENT_LOOP.md "Benchmark rules"), not a relevance
//! claim. See `index_bench.rs` for the Gate 3 index-vs-linear-scan
//! comparison at scale.

#[path = "common/mod.rs"]
mod common;

use commerce_core::domain::{Constraint, NumericOp};
use criterion::{black_box, criterion_group, criterion_main, Criterion};

const PRODUCT_COUNT: u64 = 5_000;

fn bench_search(c: &mut Criterion) {
    let catalog = common::synthetic_catalog(PRODUCT_COUNT);
    let constraints = vec![
        Constraint::Enum {
            attribute: "color".to_string(),
            value: "Black".to_string(),
        },
        Constraint::Numeric {
            attribute: "size".to_string(),
            op: NumericOp::Gte,
            value: 9.0,
        },
    ];
    c.bench_function("catalog_search_5k_products_2_variants", |b| {
        b.iter(|| black_box(catalog.search(black_box(&constraints))))
    });
}

criterion_group!(benches, bench_search);
criterion_main!(benches);
