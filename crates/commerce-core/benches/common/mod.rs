//! Shared deterministic synthetic-catalog generator for benchmarks only.
//! Not a correctness fixture: see `docs/EXPERIMENT_LOOP.md` "Benchmark
//! rules" (synthetic expansion is fine for performance scaling, never for
//! relevance claims). `benches/common/` is not auto-discovered by Cargo as
//! its own bench target, the same idiom `tests/common/` uses for shared
//! test helpers.

use commerce_core::domain::{
    attributes, AttributeValue, BrandId, Catalog, CategoryId, Inventory, Price, Product, ProductId,
    ProductTypeId, Variant, VariantId,
};
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;

const SEED: u64 = 42;
const COLORS: [&str; 4] = ["Black", "Red", "Blue", "White"];

#[allow(dead_code)]
pub fn synthetic_catalog(product_count: u64) -> Catalog {
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
