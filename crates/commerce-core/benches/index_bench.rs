//! Gate 3 benchmark: does replacing the O(products x variants) linear scan
//! with bitmap/range physical indexes actually reduce query latency at the
//! "small" scale-ladder tier (~10k products, docs/EXPERIMENT_LOOP.md)? Both
//! sides answer the exact same query against the exact same synthetic
//! catalog and are checked for identical results in
//! `tests/physical_index.rs`; this file only measures speed, never
//! correctness.

#[path = "common/mod.rs"]
mod common;

use commerce_core::domain::{Constraint, NumericOp};
use commerce_core::index::CatalogIndex;
use commerce_core::ir::{CommerceQuery, ResolvedConstraint};
use criterion::{black_box, criterion_group, criterion_main, Criterion};

const PRODUCT_COUNT: u64 = 10_000;

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

fn bench_index_build(c: &mut Criterion) {
    let catalog = common::synthetic_catalog(PRODUCT_COUNT);
    c.bench_function("index_build_10k_products_2_variants", |b| {
        b.iter(|| black_box(CatalogIndex::build(black_box(&catalog))))
    });
}

fn bench_query_linear_scan(c: &mut Criterion) {
    let catalog = common::synthetic_catalog(PRODUCT_COUNT);
    let query = structural_query();
    c.bench_function("query_linear_scan_10k_products_2_variants", |b| {
        b.iter(|| black_box(query.execute(black_box(&catalog))))
    });
}

fn bench_query_indexed(c: &mut Criterion) {
    let catalog = common::synthetic_catalog(PRODUCT_COUNT);
    let index = CatalogIndex::build(&catalog);
    let query = structural_query();
    c.bench_function("query_indexed_10k_products_2_variants", |b| {
        b.iter(|| black_box(index.execute(black_box(&query), black_box(&catalog))))
    });
}

criterion_group!(
    benches,
    bench_index_build,
    bench_query_linear_scan,
    bench_query_indexed
);
criterion_main!(benches);
