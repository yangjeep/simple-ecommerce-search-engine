//! Issue #55 diagnostic: `rank_bench.rs`'s end-to-end `execute_ranked`
//! benchmark showed no material improvement from replacing the full sort
//! with a partial selection -- surprising, given the O(n log n) vs. O(n)
//! asymptotic argument. This isolates the *selection step alone* (no
//! `CatalogIndex`, no `RoaringBitmap` iteration, no `HashMap` variant
//! lookup, no candidate materialization) on a precomputed
//! `Vec<RankedHit>`, to find out whether the selection step itself is
//! actually faster in isolation, or whether the theoretical argument
//! doesn't hold even there.
//!
//! `select_top_k`/`rank_order` are private to `commerce_core::index::rank`
//! -- this benchmark reimplements both arms directly (old full-sort,
//! current partial-select) rather than depending on crate-private items,
//! matching this project's own precedent for isolating a mechanism's cost
//! (`phase9_eval::bitmap_delegate`'s own doc comments describe the same
//! kind of isolation).

use commerce_core::domain::{ProductId, VariantId};
use commerce_core::index::RankedHit;
use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;

const K: usize = 10;

fn rank_order(a: &RankedHit, b: &RankedHit) -> std::cmp::Ordering {
    b.score
        .total_cmp(&a.score)
        .then(a.product.0.cmp(&b.product.0))
        .then(a.variant.0.cmp(&b.variant.0))
}

fn full_sort_then_truncate(mut scored: Vec<RankedHit>, k: usize) -> Vec<RankedHit> {
    scored.sort_by(rank_order);
    scored.truncate(k);
    scored
}

fn partial_select_then_sort(mut scored: Vec<RankedHit>, k: usize) -> Vec<RankedHit> {
    if scored.len() > k {
        scored.select_nth_unstable_by(k - 1, rank_order);
        scored.truncate(k);
    }
    scored.sort_by(rank_order);
    scored
}

fn random_scored(n: u64, seed: u64) -> Vec<RankedHit> {
    let mut rng = ChaCha8Rng::seed_from_u64(seed);
    (0..n)
        .map(|i| RankedHit {
            product: ProductId(i),
            variant: VariantId(i),
            score: rng.gen_range(0.0..1000.0),
        })
        .collect()
}

fn bench_selection_step_alone(c: &mut Criterion) {
    let mut group = c.benchmark_group("select_top_k_isolated");
    for &n in &[100u64, 1_000, 10_000, 100_000] {
        let data = random_scored(n, 55);
        group.bench_with_input(BenchmarkId::new("full_sort", n), &n, |b, _| {
            b.iter(|| black_box(full_sort_then_truncate(black_box(data.clone()), K)))
        });
        group.bench_with_input(BenchmarkId::new("partial_select", n), &n, |b, _| {
            b.iter(|| black_box(partial_select_then_sort(black_box(data.clone()), K)))
        });
    }
    group.finish();
}

criterion_group!(benches, bench_selection_step_alone);
criterion_main!(benches);
