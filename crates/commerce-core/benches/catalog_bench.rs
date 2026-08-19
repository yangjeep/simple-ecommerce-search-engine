//! Gate 0 benchmark harness. The fixed seed makes the synthetic catalog
//! reproducible across runs; this is a correctness-agnostic performance
//! probe (see docs/EXPERIMENT_LOOP.md "Benchmark rules"), not a relevance
//! claim, and is not yet the Gate 3/7 physical-index scale ladder.

use commerce_core::domain::{
    attributes, AttributeValue, BrandId, Catalog, CategoryId, Constraint, Inventory, NumericOp,
    Price, Product, ProductId, ProductTypeId, Variant, VariantId,
};
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;

const SEED: u64 = 42;
const PRODUCT_COUNT: u64 = 5_000;
const COLORS: [&str; 4] = ["Black", "Red", "Blue", "White"];

fn synthetic_catalog(product_count: u64) -> Catalog {
    let mut rng = ChaCha8Rng::seed_from_u64(SEED);
    let mut products = Vec::with_capacity(product_count as usize);
    for i in 0..product_count {
        let variants = (0..2)
            .map(|v| Variant {
                id: VariantId(i * 2 + v),
                attributes: attributes([
                    (
                        "color",
                        AttributeValue::Enum(COLORS[rng.gen_range(0..COLORS.len())].to_string()),
                    ),
                    (
                        "size",
                        AttributeValue::Numeric((6 + rng.gen_range(0..8)) as f64),
                    ),
                ]),
                price: Price::usd(4_999 + rng.gen_range(0..15_000)),
                inventory: Inventory::in_stock(rng.gen_range(0..50)),
            })
            .collect();
        products.push(Product {
            id: ProductId(i),
            product_type: ProductTypeId(1),
            brand: BrandId(1),
            category: CategoryId(1),
            title: format!("Synthetic Shoe {i}"),
            attributes: attributes([("waterproof", AttributeValue::Boolean(i % 3 == 0))]),
            variants,
        });
    }
    Catalog { products }
}

fn bench_search(c: &mut Criterion) {
    let catalog = synthetic_catalog(PRODUCT_COUNT);
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
