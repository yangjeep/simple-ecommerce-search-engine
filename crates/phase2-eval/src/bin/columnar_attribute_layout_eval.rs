//! Issue #7 residual hypothesis #2: does a genuinely columnar
//! (struct-of-arrays) representation of the same real catalog data reduce
//! RSS versus `commerce_core`'s current struct-of-Vec-of-structs `Catalog`
//! by >=5x, without regressing per-field random-access read latency by
//! more than 2x?
//!
//! **Evidence class**: real (full real 1,215,854-product
//! `dataset_cache/export/catalog.jsonl`).
//!
//! P2-E06 (`docs/experiments/PHASE2_LOG.md`) measured
//! `commerce_core::domain::Catalog::build` costing +2,926.17 MB RSS to
//! hold the real catalog -- the dominant cost in the whole pipeline, well
//! above `CatalogIndex::build`'s own +827.60 MB -- and named this
//! project's own open question: does a denser domain-model representation
//! (string interning / dense IDs / columnar attributes) measurably reduce
//! that cost. This binary builds a NEW, standalone columnar structure
//! (struct-of-arrays for the single-value numeric/enum-shaped fields --
//! brand id, color id, price, inventory, product-type/category sentinels
//! -- plus an offset-table+blob for the genuinely variable-length text
//! fields: title, description, bullets) from the SAME real records, and
//! measures RSS for holding it, plus per-field random-access read
//! latency, against `commerce_core::domain::Catalog` built from the same
//! records in the same process.
//!
//! `commerce_core`'s actual `Product`/`Variant` types are NOT modified by
//! this file -- this is a standalone "what would this cost look like
//! represented differently" comparison structure, not a production
//! rewrite. It is populated via `round1_eval::data::load_catalog` and
//! mirrors exactly what `round1_eval::catalog::build_catalog` populates
//! into `commerce_core::domain::Catalog` (title, description, bullets,
//! brand id (normalized-string-interned, matching `build_catalog`'s own
//! brand normalization), color id/string (un-normalized, also matching
//! `build_catalog`), plus the same `ProductTypeId(0)`/`CategoryId(0)`/
//! `Price::usd(0)`/`Inventory::in_stock(0)` sentinels `build_catalog`
//! assigns because the real ESCI export has no price/category/type
//! field) -- so the RSS comparison is against comparable real
//! information, not a stripped-down straw structure.
//!
//! One acknowledged, deliberate simplification (documented, not hidden):
//! the offset-table+blob text columns collapse "field absent" and "field
//! present but empty string" into the same zero-length range. This does
//! not change how many real text bytes are stored (both cases store zero
//! bytes either way) and does not affect the RSS or latency comparison;
//! it would only matter for a presence query ("does this product have a
//! description at all"), which this experiment does not need.
//!
//! Usage: cargo run --release -p phase2-eval --bin columnar_attribute_layout_eval
//!        [catalog.jsonl]

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::time::Instant;

use round1_eval::{catalog, data};

fn current_rss_kb() -> Option<u64> {
    let status = fs::read_to_string("/proc/self/status").ok()?;
    status
        .lines()
        .find_map(|line| line.strip_prefix("VmRSS:"))
        .and_then(|rest| rest.trim().trim_end_matches(" kB").trim().parse().ok())
}

fn report_rss(label: &str, baseline_kb: u64) -> u64 {
    let rss = current_rss_kb().unwrap_or(0);
    println!(
        "  {label}: {rss} KB ({:.2} MB) -- +{} KB (+{:.2} MB) since process start",
        rss as f64 / 1024.0,
        rss.saturating_sub(baseline_kb),
        rss.saturating_sub(baseline_kb) as f64 / 1024.0
    );
    rss
}

/// Standalone columnar (struct-of-arrays) representation of the same real
/// catalog data `commerce_core::domain::Catalog` holds. Deliberately kept
/// outside `commerce-core` -- see module doc comment.
struct ColumnarCatalog {
    len: usize,

    // Fixed-shape single-value columns: one flat, densely packed array
    // per field, no per-product heap allocation and no map/enum-tag
    // overhead per value.
    product_type: Vec<u32>, // always 0 (UNKNOWN_PRODUCT_TYPE), matches build_catalog
    category: Vec<u32>,     // always 0 (UNKNOWN_CATEGORY), matches build_catalog
    brand: Vec<u32>,        // 0 = no brand, else 1-based interned id (matches BrandId)
    color_id: Vec<u32>,     // u32::MAX = no color, else index into `color_pool`
    price_cents: Vec<i64>,  // always 0 (Price::usd(0)), matches build_catalog
    available_units: Vec<u32>, // always 0 (Inventory::in_stock(0)), matches build_catalog

    // Variable-length text columns: one offset table (len+1 u32s) plus a
    // single contiguous blob per field, instead of one heap-allocated
    // String per product.
    title_offsets: Vec<u32>,
    title_blob: String,
    desc_offsets: Vec<u32>,
    desc_blob: String,
    bullets_offsets: Vec<u32>,
    bullets_blob: String,

    // Interning pools -- small relative to the catalog (one entry per
    // distinct brand/color string, not per product).
    #[allow(dead_code)]
    brand_names: Vec<String>,
    color_pool: Vec<String>,
}

impl ColumnarCatalog {
    #[inline]
    fn title(&self, i: usize) -> &str {
        &self.title_blob[self.title_offsets[i] as usize..self.title_offsets[i + 1] as usize]
    }

    #[inline]
    fn brand_of(&self, i: usize) -> u32 {
        self.brand[i]
    }

    #[inline]
    fn price_cents_of(&self, i: usize) -> i64 {
        self.price_cents[i]
    }

    #[inline]
    fn color_of(&self, i: usize) -> Option<&str> {
        let id = self.color_id[i];
        if id == u32::MAX {
            None
        } else {
            Some(self.color_pool[id as usize].as_str())
        }
    }

    #[inline]
    fn description(&self, i: usize) -> &str {
        &self.desc_blob[self.desc_offsets[i] as usize..self.desc_offsets[i + 1] as usize]
    }

    #[inline]
    fn bullets(&self, i: usize) -> &str {
        &self.bullets_blob[self.bullets_offsets[i] as usize..self.bullets_offsets[i + 1] as usize]
    }
}

fn build_columnar(products: &[data::RealProduct]) -> ColumnarCatalog {
    let n = products.len();

    let mut product_type = Vec::with_capacity(n);
    let mut category = Vec::with_capacity(n);
    let mut brand = Vec::with_capacity(n);
    let mut color_id = Vec::with_capacity(n);
    let mut price_cents = Vec::with_capacity(n);
    let mut available_units = Vec::with_capacity(n);

    let mut title_offsets = Vec::with_capacity(n + 1);
    let mut title_blob = String::new();
    let mut desc_offsets = Vec::with_capacity(n + 1);
    let mut desc_blob = String::new();
    let mut bullets_offsets = Vec::with_capacity(n + 1);
    let mut bullets_blob = String::new();
    title_offsets.push(0u32);
    desc_offsets.push(0u32);
    bullets_offsets.push(0u32);

    let mut brand_ids: HashMap<String, u32> = HashMap::new();
    let mut brand_names: Vec<String> = Vec::new();
    let mut color_index: HashMap<String, u32> = HashMap::new();
    let mut color_pool: Vec<String> = Vec::new();

    for p in products {
        product_type.push(0u32);
        category.push(0u32);

        let bid = match &p.brand {
            Some(raw) => {
                let key = raw.trim().to_lowercase();
                match brand_ids.get(&key) {
                    Some(&id) => id,
                    None => {
                        brand_names.push(key.clone());
                        let id = brand_names.len() as u32; // 1-based, matches BrandId(0)=none
                        brand_ids.insert(key, id);
                        id
                    }
                }
            }
            None => 0,
        };
        brand.push(bid);

        price_cents.push(0i64);
        available_units.push(0u32);

        title_blob.push_str(&p.title);
        title_offsets.push(title_blob.len() as u32);

        if let Some(desc) = &p.description {
            desc_blob.push_str(desc);
        }
        desc_offsets.push(desc_blob.len() as u32);

        if let Some(bullets) = &p.bullets {
            bullets_blob.push_str(bullets);
        }
        bullets_offsets.push(bullets_blob.len() as u32);

        let cid = match &p.color {
            Some(c) => match color_index.get(c) {
                Some(&id) => id,
                None => {
                    let id = color_pool.len() as u32;
                    color_pool.push(c.clone());
                    color_index.insert(c.clone(), id);
                    id
                }
            },
            None => u32::MAX,
        };
        color_id.push(cid);
    }

    ColumnarCatalog {
        len: n,
        product_type,
        category,
        brand,
        color_id,
        price_cents,
        available_units,
        title_offsets,
        title_blob,
        desc_offsets,
        desc_blob,
        bullets_offsets,
        bullets_blob,
        brand_names,
        color_pool,
    }
}

/// Deterministic xorshift64 PRNG -- no external `rand` dependency
/// available to this file (see task constraints), and we want a fixed,
/// reproducible index sequence rather than OS randomness anyway.
struct Xorshift64(u64);

impl Xorshift64 {
    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    fn next_index(&mut self, bound: usize) -> usize {
        (self.next_u64() % bound as u64) as usize
    }
}

fn percentile_ms(sorted_ns: &[u128], p: f64) -> f64 {
    if sorted_ns.is_empty() {
        return 0.0;
    }
    let idx = ((sorted_ns.len() as f64 - 1.0) * p).round() as usize;
    sorted_ns[idx] as f64 / 1_000_000.0
}

fn main() {
    let catalog_path = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("dataset_cache/export/catalog.jsonl"));

    let baseline_kb = current_rss_kb().unwrap_or(0);
    println!("=== Columnar attribute layout vs. commerce_core::domain::Catalog: RSS + random-access read latency ===");
    println!(
        "process baseline RSS (before loading anything): {baseline_kb} KB ({:.2} MB)",
        baseline_kb as f64 / 1024.0
    );

    println!();
    println!("(a) loading real catalog.jsonl into round1_eval::data::RealProduct structs...");
    let products = data::load_catalog(&catalog_path);
    println!("{} real products loaded", products.len());
    let rss_after_raw = report_rss(
        "after loading raw catalog.jsonl into RealProduct structs",
        baseline_kb,
    );

    println!();
    println!(
        "(b) building commerce_core::domain::Catalog (same as memory_representation_eval.rs)..."
    );
    let build_start = Instant::now();
    let ingested = catalog::build_catalog(&products);
    println!(
        "  Catalog build: {:.2}s, {} products",
        build_start.elapsed().as_secs_f64(),
        ingested.catalog.products.len()
    );
    let rss_after_catalog = report_rss("after Catalog struct built", baseline_kb);

    println!();
    println!("(c) building the new standalone ColumnarCatalog from the SAME raw records...");
    let build_start = Instant::now();
    let columnar = build_columnar(&products);
    println!(
        "  ColumnarCatalog build: {:.2}s, {} products",
        build_start.elapsed().as_secs_f64(),
        columnar.len
    );
    let rss_after_columnar = report_rss("after ColumnarCatalog built", baseline_kb);

    let catalog_incremental_mb = rss_after_catalog.saturating_sub(rss_after_raw) as f64 / 1024.0;
    let columnar_incremental_mb =
        rss_after_columnar.saturating_sub(rss_after_catalog) as f64 / 1024.0;

    println!();
    println!("=== RSS summary ===");
    println!(
        "  raw RealProduct load (reference):        +{:.2} MB since process start",
        rss_after_raw.saturating_sub(baseline_kb) as f64 / 1024.0
    );
    println!(
        "  Catalog struct build, incremental:        +{catalog_incremental_mb:.2} MB   (P2-E06's same-shaped number, this run's own environment: +2,926.17 MB)"
    );
    println!("  ColumnarCatalog build, incremental:        +{columnar_incremental_mb:.2} MB");
    let ratio = if columnar_incremental_mb > 0.0 {
        catalog_incremental_mb / columnar_incremental_mb
    } else {
        f64::INFINITY
    };
    println!("  RSS reduction ratio (Catalog / Columnar):  {ratio:.2}x");

    // --- Random-access read latency ---
    // "Look up product N's brand and price 10,000 times at random N" for
    // both representations, in the same process, immediately after both
    // structures are built and resident (page faults from initial
    // allocation already paid for by the build step above).
    println!();
    println!("=== Random-access read latency: brand + price lookup, 10,000 lookups/batch, n=50 batches ===");
    const LOOKUPS_PER_BATCH: usize = 10_000;
    const BATCHES: usize = 50;
    let n = ingested.catalog.products.len();

    // One fixed, deterministic index sequence reused identically for both
    // representations so the comparison isn't sensitive to differing
    // access patterns between the two loops.
    let mut rng = Xorshift64(0x9E3779B97F4A7C15);
    let indices: Vec<usize> = (0..LOOKUPS_PER_BATCH).map(|_| rng.next_index(n)).collect();

    let mut catalog_samples_ns = Vec::with_capacity(BATCHES);
    for _ in 0..BATCHES {
        let start = Instant::now();
        let mut acc: i64 = 0;
        for &idx in &indices {
            let product = &ingested.catalog.products[idx];
            acc = acc.wrapping_add(product.brand.0 as i64);
            acc = acc.wrapping_add(product.variants[0].price.amount_cents);
        }
        std::hint::black_box(acc);
        catalog_samples_ns.push(start.elapsed().as_nanos());
    }
    catalog_samples_ns.sort_unstable();

    let mut columnar_samples_ns = Vec::with_capacity(BATCHES);
    for _ in 0..BATCHES {
        let start = Instant::now();
        let mut acc: i64 = 0;
        for &idx in &indices {
            acc = acc.wrapping_add(columnar.brand_of(idx) as i64);
            acc = acc.wrapping_add(columnar.price_cents_of(idx));
        }
        std::hint::black_box(acc);
        columnar_samples_ns.push(start.elapsed().as_nanos());
    }
    columnar_samples_ns.sort_unstable();

    let catalog_p50 = percentile_ms(&catalog_samples_ns, 0.5);
    let catalog_p95 = percentile_ms(&catalog_samples_ns, 0.95);
    let catalog_p99 = percentile_ms(&catalog_samples_ns, 0.99);
    let columnar_p50 = percentile_ms(&columnar_samples_ns, 0.5);
    let columnar_p95 = percentile_ms(&columnar_samples_ns, 0.95);
    let columnar_p99 = percentile_ms(&columnar_samples_ns, 0.99);

    println!(
        "  Catalog (struct-of-Vec-of-structs):   batch p50={catalog_p50:.4}ms p95={catalog_p95:.4}ms p99={catalog_p99:.4}ms  (per-lookup avg p50={:.2}ns)",
        catalog_p50 * 1_000_000.0 / LOOKUPS_PER_BATCH as f64
    );
    println!(
        "  ColumnarCatalog (struct-of-arrays):   batch p50={columnar_p50:.4}ms p95={columnar_p95:.4}ms p99={columnar_p99:.4}ms  (per-lookup avg p50={:.2}ns)",
        columnar_p50 * 1_000_000.0 / LOOKUPS_PER_BATCH as f64
    );
    let latency_ratio = columnar_p50 / catalog_p50;
    println!(
        "  latency ratio (Columnar p50 / Catalog p50): {latency_ratio:.3}x  ({} than direct struct access)",
        if latency_ratio <= 1.0 {
            "faster or equal"
        } else {
            "slower"
        }
    );

    // Secondary: variable-length field (title) random access, since the
    // offset-table+blob design is the more architecturally novel half of
    // the columnar layout (the fixed-field arrays are the easy case).
    println!();
    println!("=== Random-access read latency: title text lookup, 10,000 lookups/batch, n=50 batches (secondary) ===");
    let mut catalog_title_samples_ns = Vec::with_capacity(BATCHES);
    for _ in 0..BATCHES {
        let start = Instant::now();
        let mut acc: usize = 0;
        for &idx in &indices {
            acc = acc.wrapping_add(ingested.catalog.products[idx].title.len());
        }
        std::hint::black_box(acc);
        catalog_title_samples_ns.push(start.elapsed().as_nanos());
    }
    catalog_title_samples_ns.sort_unstable();

    let mut columnar_title_samples_ns = Vec::with_capacity(BATCHES);
    for _ in 0..BATCHES {
        let start = Instant::now();
        let mut acc: usize = 0;
        for &idx in &indices {
            acc = acc.wrapping_add(columnar.title(idx).len());
        }
        std::hint::black_box(acc);
        columnar_title_samples_ns.push(start.elapsed().as_nanos());
    }
    columnar_title_samples_ns.sort_unstable();

    let catalog_title_p50 = percentile_ms(&catalog_title_samples_ns, 0.5);
    let columnar_title_p50 = percentile_ms(&columnar_title_samples_ns, 0.5);
    println!(
        "  Catalog title access:      batch p50={catalog_title_p50:.4}ms  (per-lookup avg p50={:.2}ns)",
        catalog_title_p50 * 1_000_000.0 / LOOKUPS_PER_BATCH as f64
    );
    println!(
        "  ColumnarCatalog title access: batch p50={columnar_title_p50:.4}ms  (per-lookup avg p50={:.2}ns)",
        columnar_title_p50 * 1_000_000.0 / LOOKUPS_PER_BATCH as f64
    );
    println!(
        "  latency ratio (Columnar / Catalog): {:.3}x",
        columnar_title_p50 / catalog_title_p50
    );

    // Sanity check both representations agree on content for a handful of
    // real records, so a bug in the columnar build can't silently produce
    // a misleadingly small RSS number.
    println!();
    println!("=== Correctness spot-check (first 5 products, columnar vs. Catalog) ===");
    let mut mismatches = 0usize;
    for i in 0..n.min(5) {
        let cat_brand = ingested.catalog.products[i].brand.0;
        let col_brand = columnar.brand_of(i);
        let cat_price = ingested.catalog.products[i].variants[0].price.amount_cents;
        let col_price = columnar.price_cents_of(i);
        let cat_title = ingested.catalog.products[i].title.as_str();
        let col_title = columnar.title(i);
        let cat_desc = match ingested.catalog.products[i].attributes.get("description") {
            Some(commerce_core::domain::AttributeValue::Text(s)) => s.as_str(),
            _ => "",
        };
        let col_desc = columnar.description(i);
        let cat_bullets = match ingested.catalog.products[i].attributes.get("bullets") {
            Some(commerce_core::domain::AttributeValue::Text(s)) => s.as_str(),
            _ => "",
        };
        let col_bullets = columnar.bullets(i);
        let ok = cat_brand == col_brand
            && cat_price == col_price
            && cat_title == col_title
            && cat_desc == col_desc
            && cat_bullets == col_bullets;
        if !ok {
            mismatches += 1;
        }
        println!(
            "  [{i}] brand match={} price match={} title match={} desc match={} bullets match={} color={:?}",
            cat_brand == col_brand,
            cat_price == col_price,
            cat_title == col_title,
            cat_desc == col_desc,
            cat_bullets == col_bullets,
            columnar.color_of(i)
        );
    }
    // Full-catalog agreement check on brand + price + title length (cheap
    // enough to run over all 1.2M products, not just the spot sample).
    let mut full_mismatches = 0usize;
    for i in 0..n {
        let product = &ingested.catalog.products[i];
        let cat_brand = product.brand.0;
        let cat_price = product.variants[0].price.amount_cents;
        let cat_title_len = product.title.len();
        let cat_desc_len = match product.attributes.get("description") {
            Some(commerce_core::domain::AttributeValue::Text(s)) => s.len(),
            _ => 0,
        };
        let cat_bullets_len = match product.attributes.get("bullets") {
            Some(commerce_core::domain::AttributeValue::Text(s)) => s.len(),
            _ => 0,
        };
        if cat_brand != columnar.brand_of(i)
            || cat_price != columnar.price_cents_of(i)
            || cat_title_len != columnar.title(i).len()
            || cat_desc_len != columnar.description(i).len()
            || cat_bullets_len != columnar.bullets(i).len()
        {
            full_mismatches += 1;
        }
    }
    println!(
        "  full-catalog agreement check ({n} products): {full_mismatches} mismatches (brand id / price / title / description / bullets lengths)"
    );
    // product_type/category columns are always the same sentinel by
    // construction (both sides hard-code UNKNOWN_PRODUCT_TYPE/UNKNOWN_CATEGORY),
    // so they are not a meaningful correctness signal here; asserted once
    // instead of looped.
    debug_assert!(columnar.product_type.iter().all(|&v| v == 0));
    debug_assert!(columnar.category.iter().all(|&v| v == 0));
    debug_assert!(columnar.available_units.iter().all(|&v| v == 0));

    println!();
    println!("=== Hypothesis check ===");
    println!(
        "  target: >=5x RSS reduction AND <=2x random-access latency regression (brand+price)"
    );
    println!(
        "  measured: {ratio:.2}x RSS reduction, {latency_ratio:.3}x latency ratio (columnar/struct, brand+price)"
    );
    let rss_confirmed = ratio >= 5.0;
    let latency_confirmed = latency_ratio <= 2.0;
    let verdict = match (rss_confirmed, latency_confirmed) {
        (true, true) => "CONFIRMED",
        (false, true) => "FALSIFIED (RSS reduction below 5x threshold)",
        (true, false) => "PARTIALLY_CONFIRMED (RSS threshold met, latency regressed >2x)",
        (false, false) => "FALSIFIED (both thresholds missed)",
    };
    println!("  verdict: {verdict}");
    if mismatches > 0 || full_mismatches > 0 {
        println!(
            "  WARNING: correctness spot-check found {mismatches} spot mismatches, {full_mismatches} full-catalog mismatches -- treat the numbers above with caution"
        );
    }
}
