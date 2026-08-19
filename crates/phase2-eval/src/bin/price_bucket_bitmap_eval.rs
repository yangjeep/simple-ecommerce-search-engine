//! Issue #7 residual hypothesis #3: is `CatalogIndex`'s numeric-range
//! mechanism -- binary-search a sorted `Vec<(i64, Ordinal)>`, then
//! `.iter().map(...).collect()` a FRESH `RoaringBitmap` on every call
//! (`structural_bitmap`'s `PriceUnderCents`/`PriceOverCents` arms, and the
//! free `numeric_range()` function, both in
//! `crates/commerce-core/src/index/mod.rs`) -- an "impedance mismatch"
//! against `brand_bitmaps`, which is already bitmap-shaped (a
//! `HashMap<BrandId, RoaringBitmap>` populated once at build time, so a
//! Brand constraint costs one hashmap lookup + one clone, no per-query
//! bitmap construction)?
//!
//! Hypothesis: pre-bucketing the numeric domain into a bounded number of
//! buckets, each holding a precomputed `RoaringBitmap` built once ahead of
//! query time (Havenask's / pre-BKD Lucene's approach), and answering a
//! range query by OR-ing the bucket bitmaps the range spans, then AND-ing
//! with the Brand bitmap using roaring's own operators, is >=5x faster than
//! today's per-query binary-search-then-collect mechanism for a compound
//! "Brand=X AND Price BETWEEN [a,b]" query.
//!
//! This is a first-time, standalone measurement of this specific compound-
//! query cost -- no prior experiment in this project isolates it.
//!
//! Evidence class: MIXED. The real ESCI catalog, as ingested by
//! `round1_eval::catalog::build_catalog`, assigns every real product
//! `Price::usd(0)` -- the source ESCI export has no price field at all
//! (see that module's doc comment / `docs/experiments/ROUND1_LOG.md`
//! R1-E01), so a real "Price BETWEEN" constraint over the unmodified
//! ingested catalog is trivial (every product has the same price). To make
//! the range constraint non-trivial, this experiment SYNTHESIZES a price in
//! cents for every real product, deterministically, from its `ProductId`
//! (a self-contained splitmix64 hash seeded per product id -- no `rand`
//! crate dependency, fully reproducible from source + a fixed seed) and
//! REPLACES `Price::usd(0)` with that synthetic price before building the
//! `CatalogIndex` under test. Nothing else about the real catalog (real
//! product identities/count, real titles, real brand assignments) is
//! touched or resynthesized. This keeps the actual thing under measurement
//! -- bitmap-construction mechanism vs. bucket-OR mechanism, over the real
//! 1.2M-row structure and real brand distribution -- as real as possible,
//! while being explicit that the *price values* themselves are fabricated
//! for this experiment only.
//!
//! Usage: cargo run --release -p phase2-eval --bin price_bucket_bitmap_eval
//!        [catalog.jsonl]

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Instant;

use commerce_core::domain::{BrandId, Catalog, Price};
use commerce_core::index::CatalogIndex;
use commerce_core::ir::{ResolvedConstraint, StructuralConstraint};
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

/// Deterministic, dependency-free pseudo-random hash (splitmix64) so the
/// synthetic price distribution is fully reproducible from source + a fixed
/// seed -- no `rand` crate is a direct dependency of this crate (see the
/// file-level evidence-class note above for why one is needed at all).
fn splitmix64(seed: u64) -> u64 {
    let mut z = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// A right-skewed synthetic price in cents, deterministic per product id.
/// Shape (exponential-ish with a long tail, roughly $3-$5000) is a rough
/// approximation of a real e-commerce catalog's price distribution -- not
/// fit to any real distribution, just realistic enough that a "Price
/// BETWEEN" range constraint is genuinely selective rather than trivial.
fn synthetic_price_cents(product_id: u64) -> i64 {
    let a = (splitmix64(product_id) >> 11) as f64 / (1u64 << 53) as f64; // [0,1)
    let b = (splitmix64(product_id ^ 0xA5A5_A5A5_A5A5_A5A5) >> 11) as f64 / (1u64 << 53) as f64;
    let skewed = -(a.max(1e-9)).ln() * 1400.0 + b * 400.0;
    (299.0 + skewed).clamp(299.0, 499_999.0) as i64
}

/// Quantile (equal-population, not equal-width) bucket edges over
/// `sorted_prices` (must already be sorted ascending): `num_buckets` cut
/// points chosen at real observed values, deduplicated so a run of
/// identical prices is never split across two buckets. Returns
/// `edges.len() - 1` buckets (may be fewer than `num_buckets` if heavy
/// duplication collapsed some cuts -- not expected at this scale, but
/// handled rather than assumed away). Bucket `i` covers
/// `[edges[i], edges[i + 1])` in cents.
fn quantile_edges(sorted_prices: &[i64], num_buckets: usize) -> Vec<i64> {
    let n = sorted_prices.len();
    let mut edges = Vec::with_capacity(num_buckets + 1);
    edges.push(sorted_prices[0]);
    for i in 1..num_buckets {
        let idx = ((n * i) / num_buckets).min(n - 1);
        edges.push(sorted_prices[idx]);
    }
    edges.push(sorted_prices[n - 1] + 1);
    edges.dedup();
    edges
}

/// Assign every `(price_cents, ordinal)` pair to exactly one bucket index
/// given `edges` (as produced by [`quantile_edges`]).
fn assign_buckets(edges: &[i64], ordinal_price: &[(i64, u32)]) -> Vec<Vec<u32>> {
    let num_buckets = edges.len() - 1;
    let mut bucket_ords: Vec<Vec<u32>> = vec![Vec::new(); num_buckets];
    for &(price, ord) in ordinal_price {
        let idx = edges.partition_point(|&e| e <= price);
        let bucket_idx = idx.saturating_sub(1).min(num_buckets - 1);
        bucket_ords[bucket_idx].push(ord);
    }
    bucket_ords
}

/// Materialize one precomputed `RoaringBitmap` per bucket (a one-time cost
/// paid before any query, analogous to index build time -- NOT included in
/// the per-query timings below). `seed_empty` is an empty bitmap obtained
/// from `CatalogIndex`'s own public API
/// (`lexical_and_candidates(&[])` -- an empty token list can never match
/// anything, so this is guaranteed empty; asserted at the call site)
/// purely so this file never needs to name `roaring::RoaringBitmap`
/// directly: `roaring` is not a direct dependency of `phase2-eval` (only
/// `commerce-core` depends on it, and does not re-export the type -- see
/// this experiment's task constraints), so every bitmap value in this file
/// is obtained from `CatalogIndex`'s public surface and manipulated only
/// via inherent methods (`.clone()`, `.extend()`, `.len()`) and the
/// `BitAnd`/`BitOr`/`PartialEq` operators, none of which require the
/// concrete type's name to be spelled out in this crate.
fn build_bucket_bitmaps<B: Clone + Extend<u32>>(
    seed_empty: &B,
    bucket_ords: Vec<Vec<u32>>,
) -> Vec<B> {
    bucket_ords
        .into_iter()
        .map(|ords| {
            let mut bm = seed_empty.clone();
            bm.extend(ords);
            bm
        })
        .collect()
}

fn main() {
    let catalog_path = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("dataset_cache/export/catalog.jsonl"));

    println!("=== Issue #7 residual hypothesis #3: bucketed-bitmap price range vs. binary-search-and-collect ===");
    println!("loading + indexing real catalog {catalog_path:?}...");
    let products = data::load_catalog(&catalog_path);
    let ingested = catalog::build_catalog(&products);
    println!("{} real products loaded", ingested.catalog.products.len());

    // --- price: real data has none (every product ingests as
    // Price::usd(0)); see the file-level doc comment. Synthesize
    // deterministically and replace, leaving everything else untouched.
    println!();
    println!("NOTE: real ESCI catalog has no price data (every product ingests as Price::usd(0)).");
    println!("Synthesizing a deterministic per-product price (splitmix64 seeded by ProductId, right-skewed ~$3-$5000) for THIS experiment only.");
    let mut synth_products = ingested.catalog.products.clone();
    for p in synth_products.iter_mut() {
        let cents = synthetic_price_cents(p.id.0);
        for v in p.variants.iter_mut() {
            v.price = Price::usd(cents);
        }
    }
    let synth_catalog = Catalog {
        products: synth_products,
    };

    let build_start = Instant::now();
    let index = CatalogIndex::build(&synth_catalog);
    println!(
        "CatalogIndex::build (with synthetic prices): {:.2}s, {} ordinals",
        build_start.elapsed().as_secs_f64(),
        synth_catalog.products.len()
    );

    // --- brand selection: nike, matching this project's other real-data
    // experiments (round1-eval's adversarial_physical.rs); fall back to the
    // largest brand if the export ever changes and "nike" isn't present.
    let brand_id = match ingested.brands.iter().find(|b| b.name == "nike") {
        Some(b) => b.id,
        None => {
            let mut counts: HashMap<BrandId, usize> = HashMap::new();
            for p in &ingested.catalog.products {
                *counts.entry(p.brand).or_insert(0) += 1;
            }
            counts
                .into_iter()
                .max_by_key(|&(_, n)| n)
                .map(|(id, _)| id)
                .expect("catalog has at least one product")
        }
    };
    let brand_name = ingested
        .brands
        .iter()
        .find(|b| b.id == brand_id)
        .map(|b| b.name.clone())
        .unwrap_or_else(|| "<unbranded>".to_string());
    let brand_only = vec![ResolvedConstraint::Structural(StructuralConstraint::Brand(
        brand_id,
    ))];
    let brand_count = index.indexed_candidates(&brand_only).len();
    println!(
        "brand under test: {brand_name:?} (BrandId={brand_id:?}), {brand_count} real products ({:.3}% of catalog)",
        brand_count as f64 * 100.0 / synth_catalog.products.len() as f64
    );

    // --- observed synthetic price range + ordinal/price pairs.
    let mut ordinal_price: Vec<(i64, u32)> = Vec::with_capacity(synth_catalog.products.len());
    for p in &synth_catalog.products {
        for v in &p.variants {
            let ord = index.ordinal_of(v.id).expect("built from this catalog");
            ordinal_price.push((v.price.amount_cents, ord));
        }
    }
    let mut sorted_prices: Vec<i64> = ordinal_price.iter().map(|&(p, _)| p).collect();
    sorted_prices.sort_unstable();
    let min_price = sorted_prices[0];
    let max_price = *sorted_prices.last().unwrap();
    let mean_price =
        sorted_prices.iter().map(|&p| p as f64).sum::<f64>() / sorted_prices.len() as f64 / 100.0;
    println!(
        "observed synthetic price range: ${:.2} - ${:.2}, mean ${:.2}, n={}",
        min_price as f64 / 100.0,
        max_price as f64 / 100.0,
        mean_price,
        sorted_prices.len()
    );

    // --- bucket structure: 32 quantile (equal-population) buckets over the
    // real, full 1.2M-row ordinal set, built once (the one-time cost the
    // Havenask/pre-BKD approach pays up front, in exchange for no per-query
    // bitmap construction). Quantile (not equal-width) buckets are used so
    // bucket population is balanced regardless of the synthetic
    // distribution's skew.
    let num_buckets = 32usize;
    let empty_seed = index.lexical_and_candidates(&[]); // guaranteed empty; see build_bucket_bitmaps doc
    assert_eq!(empty_seed.len(), 0, "seed bitmap must be empty");

    let bucket_build_start = Instant::now();
    let edges = quantile_edges(&sorted_prices, num_buckets);
    let actual_buckets = edges.len() - 1;
    let bucket_ords = assign_buckets(&edges, &ordinal_price);
    let bucket_bitmaps = build_bucket_bitmaps(&empty_seed, bucket_ords);
    println!(
        "built {actual_buckets} quantile bucket bitmaps in {:.2}ms (one-time cost, NOT included in the per-query timings below)",
        bucket_build_start.elapsed().as_secs_f64() * 1000.0
    );
    let bucket_total: u64 = bucket_bitmaps.iter().map(|b| b.len()).sum();
    assert_eq!(
        bucket_total as usize,
        ordinal_price.len(),
        "every ordinal must land in exactly one bucket"
    );

    // --- pick a "moderately wide" query range that exactly spans buckets
    // [start, end) -- so the two mechanisms answer the IDENTICAL query
    // (same candidate set), and the timing comparison is not confounded by
    // any bucket-boundary/approximation mismatch.
    let start = actual_buckets / 4;
    let end = (actual_buckets * 3) / 4;
    assert!(start < end, "range must span at least one bucket");
    let lo_cents = edges[start];
    let hi_cents = edges[end] - 1;
    println!(
        "query range under test: Price BETWEEN ${:.2} and ${:.2} (buckets [{start},{end}) of {actual_buckets}, {} buckets, ~{:.0}% of catalog by count)",
        lo_cents as f64 / 100.0,
        hi_cents as f64 / 100.0,
        end - start,
        (end - start) as f64 * 100.0 / actual_buckets as f64
    );

    // === (a) CURRENT mechanism: CatalogIndex::indexed_candidates, exactly
    // as it exists in commerce-core today (read-only use of the public API,
    // no modification). Brand -> hashmap lookup; each Price bound ->
    // binary-search `price_index`'s sorted Vec + collect a fresh
    // RoaringBitmap (structural_bitmap's PriceUnderCents/PriceOverCents
    // arms); then intersect all three via `&`.
    let compound_constraints = vec![
        ResolvedConstraint::Structural(StructuralConstraint::Brand(brand_id)),
        ResolvedConstraint::Structural(StructuralConstraint::PriceOverCents(lo_cents - 1)),
        ResolvedConstraint::Structural(StructuralConstraint::PriceUnderCents(hi_cents + 1)),
    ];
    let baseline_result = index.indexed_candidates(&compound_constraints);
    println!();
    println!("=== (a) CURRENT: CatalogIndex::indexed_candidates([Brand, PriceOverCents, PriceUnderCents]) ===");
    println!("  hits: {}", baseline_result.len());
    let n = 2000;
    let baseline_samples = time_iters(
        || {
            std::hint::black_box(
                index.indexed_candidates(std::hint::black_box(&compound_constraints)),
            );
        },
        n,
    );
    println!(
        "  p50={:.5}ms  p95={:.5}ms  p99={:.5}ms  (n={n})",
        percentile_ms(&baseline_samples, 0.5),
        percentile_ms(&baseline_samples, 0.95),
        percentile_ms(&baseline_samples, 0.99),
    );

    // === (b) NEW alternative: precomputed quantile-bucket bitmaps, composed
    // via roaring's own BitOr/BitAnd operators. The Brand bitmap is fetched
    // via the SAME `indexed_candidates` hashmap-lookup path used inside
    // (a), so the only mechanism under comparison is Price-bitmap
    // construction: binary-search-and-collect (a) vs.
    // OR-of-precomputed-buckets (b).
    let bucket_result = {
        let brand_bm = index.indexed_candidates(&brand_only);
        let mut range_bm = bucket_bitmaps[start].clone();
        for bm in &bucket_bitmaps[start + 1..end] {
            range_bm |= bm;
        }
        brand_bm & range_bm
    };
    println!();
    println!("=== (b) NEW: precomputed quantile-bucket bitmaps, Brand-bitmap AND (OR of spanned buckets) ===");
    println!("  hits: {}", bucket_result.len());
    let bucket_samples = time_iters(
        || {
            let brand_bm = index.indexed_candidates(std::hint::black_box(&brand_only));
            let mut range_bm = bucket_bitmaps[start].clone();
            for bm in &bucket_bitmaps[start + 1..end] {
                range_bm |= bm;
            }
            std::hint::black_box(brand_bm & range_bm);
        },
        n,
    );
    println!(
        "  p50={:.5}ms  p95={:.5}ms  p99={:.5}ms  (n={n})",
        percentile_ms(&bucket_samples, 0.5),
        percentile_ms(&bucket_samples, 0.95),
        percentile_ms(&bucket_samples, 0.99),
    );

    // --- correctness: the two mechanisms MUST answer the identical query
    // (same candidate ordinal set), since the range was chosen to align
    // exactly to bucket boundaries. Asserted, not just reported -- a speed
    // win from a wrong answer would not be a valid result.
    println!();
    let exact_match = baseline_result == bucket_result;
    println!("=== Correctness ===");
    println!(
        "  (a) hits={}  (b) hits={}  identical candidate sets: {exact_match}",
        baseline_result.len(),
        bucket_result.len()
    );
    assert!(
        exact_match,
        "bucketed alternative must answer the identical query as the baseline for this timing comparison to be valid"
    );

    // === Verdict ===
    let speedup =
        percentile_ms(&baseline_samples, 0.5) / percentile_ms(&bucket_samples, 0.5).max(1e-9);
    println!();
    println!("=== Verdict ===");
    println!(
        "  p50 speedup (bucketed vs current): {speedup:.2}x  (falsification threshold: >=5.0x)"
    );
    if speedup >= 5.0 {
        println!("  CONFIRMED: bucketed-bitmap composition is >=5x faster than the current binary-search-and-collect mechanism for this compound query.");
    } else if speedup >= 1.0 {
        println!("  FALSIFIED: bucketed-bitmap composition is faster but does NOT clear the 5x bar -- the impedance mismatch is real but smaller than hypothesized.");
    } else {
        println!("  FALSIFIED (reversed): bucketed-bitmap composition is SLOWER than the current mechanism for this compound query -- the compositional-uniformity tradeoff the Havenask archaeology predicted is real here.");
    }
}
