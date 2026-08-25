//! Issue #55 (`docs/experiments/ISSUE55_RANK_SCALING_PROTOCOL.md`): does
//! `execute_ranked`'s cost scale as `O(n log n)` (a full sort over the
//! entire candidate set, per `docs/decisions/PHASE9_DECISION.md`'s
//! "what would be built next" item 2) or something cheaper, as a function
//! of candidate-set size `n`, with `k` fixed small (10, this project's
//! standard top-K)?
//!
//! **Corrected after an adversarial finding against this experiment's own
//! first draft**: an earlier version gave every candidate an identical
//! score (empty `residual_lexical`, `score_text_relevance`'s 0.0
//! short-circuit). That made the input already sorted with respect to
//! the ranking comparator (candidates come out of `index.execute()` in
//! ascending product-id order, and the comparator's only live
//! discriminator left was that same product id) -- Rust's `sort_by` is an
//! adaptive stable sort that runs close to `O(n)` on already-sorted
//! input, so the old "full sort" was *never actually exercising its own
//! O(n log n) worst case* in that benchmark, which hid any improvement a
//! partial-selection replacement could show. Fixed here by giving each
//! product a title built from a random subset of a small vocabulary and
//! querying with several of those tokens as `residual_lexical` -- the
//! real, shipping default ranking signal (`score_text_relevance`), which
//! genuinely varies per candidate and is uncorrelated with product-id
//! order, matching what real WANDS-style text-relevance scoring actually
//! produces.

#[path = "common/mod.rs"]
mod common;

use commerce_core::index::CatalogIndex;
use commerce_core::ir::CommerceQuery;
use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use rand::{seq::SliceRandom, SeedableRng};
use rand_chacha::ChaCha8Rng;

const K: usize = 10;
const SEED: u64 = 55;
const VOCAB: [&str; 8] = [
    "waterproof",
    "lightweight",
    "running",
    "trail",
    "leather",
    "insulated",
    "breathable",
    "cushioned",
];
/// How many of `VOCAB`'s tokens a given product's title draws, chosen
/// per-product by the seeded RNG -- deliberately uncorrelated with
/// product id, so the resulting text-relevance score sequence (in
/// candidate/product-id order, the order `index.execute()` yields) is not
/// already sorted by score.
fn realistic_catalog(n: u64) -> commerce_core::domain::Catalog {
    use commerce_core::domain::{
        attributes, AttributeValue, BrandId, CategoryId, Inventory, Price, Product, ProductId,
        ProductTypeId, Variant, VariantId,
    };
    let mut rng = ChaCha8Rng::seed_from_u64(SEED);
    let products = (0..n)
        .map(|i| {
            let mut vocab = VOCAB;
            vocab.shuffle(&mut rng);
            let word_count = 1 + (i as usize % VOCAB.len());
            let title = vocab[..word_count].join(" ");
            Product {
                id: ProductId(i),
                product_type: ProductTypeId(1),
                brand: BrandId(1),
                category: CategoryId(1),
                title,
                attributes: attributes([("in_stock_flag", AttributeValue::Boolean(true))]),
                variants: vec![Variant {
                    id: VariantId(i),
                    attributes: attributes([]),
                    price: Price::usd(1_999),
                    inventory: Inventory::in_stock(1),
                }],
            }
        })
        .collect();
    commerce_core::domain::Catalog { products }
}

/// A pure-FastPath query (empty `residual_lexical` would route the same
/// way, but a non-empty one is what actually exercises
/// `score_text_relevance`'s real scoring path -- the point of this fix).
/// No structural constraint at all means `index.execute` with an empty
/// `constraints` list returns every candidate via `indexed_candidates`'s
/// own "no constraints -> everything" contract -- verified below by the
/// exact candidate count, not assumed.
fn text_query() -> CommerceQuery {
    CommerceQuery {
        constraints: vec![],
        preferences: vec![],
        ambiguous: vec![],
        residual_lexical: vec![
            "waterproof".to_string(),
            "running".to_string(),
            "cushioned".to_string(),
        ],
    }
}

fn bench_execute_ranked_by_candidate_set_size(c: &mut Criterion) {
    let mut group = c.benchmark_group("execute_ranked_scaling");
    for &n in &[100u64, 1_000, 10_000, 100_000] {
        let catalog = realistic_catalog(n);
        let index = CatalogIndex::build(&catalog);
        let query = text_query();
        let candidates = index.execute(&query, &catalog);
        assert_eq!(
            candidates.len(),
            n as usize,
            "an empty constraint list must match the whole catalog"
        );
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, _| {
            b.iter(|| black_box(index.execute_ranked(black_box(&query), &catalog, K)))
        });
    }
    group.finish();
}

criterion_group!(benches, bench_execute_ranked_by_candidate_set_size);
criterion_main!(benches);
