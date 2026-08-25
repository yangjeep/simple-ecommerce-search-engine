//! Issue #51, diagnostic (NOT part of the preregistered R1/#51 GO gate --
//! see `docs/experiments/ISSUE51_PROTOCOL.md`/`ISSUE51_LOG.md`): does
//! Treatment D's overhead actually grow with catalog size the way an
//! "O(catalog-size) scan" diagnosis predicts, and does Treatment E's stay
//! flat? R1's own frozen fixture has only 5 products, so
//! `constraint_kind_registered_on_product_type`'s scan is already almost
//! free there regardless of mechanism -- this diagnostic scales the SAME
//! fixture up with harmless decoy products of the same corroborating
//! product types (Jeans/Wiper Blades/Brake Pads) to see whether the
//! asymptotic claim actually holds, independent of whether either
//! treatment clears R1's own 5% bar at N=5.
//!
//! Decoys never collide with `AMBIGUOUS_SIZE_VALUE` ("22") or any other
//! row's real value, so they cannot become spurious hits/false positives
//! if this diagnostic's rows were ever run through the full correctness
//! gate (they are not here -- this measures latency only).
//!
//! Reproduction: `cargo build --release -p issue42-eval &&
//! ./target/release/i51_e00_catalog_scale_diagnostic`

use commerce_core::cold_start::{compile_lexicon, CatalogProfile};
use commerce_core::domain::{
    attributes, AttributeValue, Price, Product, ProductId, Variant, VariantId,
};
use commerce_core::index::CatalogIndex;
use issue42_eval::r1_experimental::{build_attribute_kind_registry, resolve_d, resolve_e};
use issue42_eval::r1_workload::build_typed_ambiguity_catalog;

const LATENCY_BATCH: usize = 500;
const LATENCY_TRIALS: usize = 5;

fn median(mut values: Vec<f64>) -> f64 {
    values.sort_by(|a, b| a.total_cmp(b));
    values[values.len() / 2]
}

/// Adds `decoys_per_type` harmless decoy products for each of the three
/// corroborating product types R1's rows 2/3/6 use (Jeans=1, Wiper
/// Blades=2, Brake Pads=3), each with a distinct size value far outside
/// any row's real value so they can never become a spurious match.
fn scaled_catalog(
    base: &commerce_core::domain::Catalog,
    decoys_per_type: usize,
) -> commerce_core::domain::Catalog {
    let mut products = base.products.clone();
    let mut next_id = 1000u64;
    for product_type_id in [1u32, 2, 3] {
        for i in 0..decoys_per_type {
            let decoy = Product {
                id: ProductId(next_id),
                product_type: commerce_core::domain::ProductTypeId(product_type_id),
                brand: base.products[0].brand,
                category: base.products[0].category,
                title: format!("Decoy product {next_id}"),
                attributes: attributes([]),
                variants: vec![Variant {
                    id: VariantId(next_id * 10),
                    attributes: attributes([("size", AttributeValue::Enum(format!("decoy-{i}")))]),
                    price: Price::usd(1000),
                    inventory: commerce_core::domain::Inventory::in_stock(1),
                }],
            };
            products.push(decoy);
            next_id += 1;
        }
    }
    commerce_core::domain::Catalog { products }
}

fn main() {
    println!("=== Issue #51 diagnostic: does D's overhead scale with catalog size while E's stays flat? ===");
    println!("(NOT part of the preregistered R1/#51 GO gate -- see ISSUE51_PROTOCOL.md/LOG.md)\n");

    let fixture = build_typed_ambiguity_catalog();
    const MIN_ENUM_FREQUENCY: usize = 1;

    // Only the 3 corroborated rows exercise resolve_d/resolve_e's
    // catalog-scanning path at all (rows without a corroborating
    // ProductType constraint fall back to Treatment C immediately, per
    // both resolve_d and resolve_e's own first check).
    let rows = [
        "size 22 jeans",
        "size 22 wiper blades",
        "2015 honda civic brake pads",
    ];

    for decoys_per_type in [0usize, 5, 50, 500, 5000] {
        let catalog = scaled_catalog(&fixture.catalog, decoys_per_type);
        let _index = CatalogIndex::build(&catalog); // built for parity with the real harness; unused by D/E directly
        let profile = CatalogProfile::build(
            &catalog,
            &fixture.brands,
            &fixture.product_types,
            &fixture.categories,
        );
        let lexicon = compile_lexicon(&profile, MIN_ENUM_FREQUENCY);
        let registry = build_attribute_kind_registry(&catalog);
        let catalog_size = catalog.products.len();

        let d_trials: Vec<f64> = (0..LATENCY_TRIALS)
            .map(|_| {
                let t0 = std::time::Instant::now();
                for _ in 0..LATENCY_BATCH {
                    for text in rows {
                        let r = std::hint::black_box(resolve_d(
                            std::hint::black_box(text),
                            &lexicon,
                            &catalog,
                        ));
                        std::hint::black_box(r.queries.len());
                    }
                }
                t0.elapsed().as_secs_f64() * 1000.0 / (LATENCY_BATCH * rows.len()) as f64
            })
            .collect();
        let e_trials: Vec<f64> = (0..LATENCY_TRIALS)
            .map(|_| {
                let t0 = std::time::Instant::now();
                for _ in 0..LATENCY_BATCH {
                    for text in rows {
                        let r = std::hint::black_box(resolve_e(
                            std::hint::black_box(text),
                            &lexicon,
                            &registry,
                        ));
                        std::hint::black_box(r.queries.len());
                    }
                }
                t0.elapsed().as_secs_f64() * 1000.0 / (LATENCY_BATCH * rows.len()) as f64
            })
            .collect();

        let d_med = median(d_trials.clone());
        let e_med = median(e_trials.clone());
        println!(
            "catalog_size={catalog_size:>6}: D median={d_med:.6}ms (trials={d_trials:?}), \
             E median={e_med:.6}ms (trials={e_trials:?}), D/E ratio={:.2}x",
            d_med / e_med
        );
    }
}
