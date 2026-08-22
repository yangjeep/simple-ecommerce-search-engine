//! Issue #38, Experiment E1 (the architectural falsification gate).
//!
//! Four paths over the identical real WANDS catalog, physical primitive
//! (`RoaringBitmap` intersection), and query workload (a real distribution
//! of `product_type` values spanning the catalog's actual selectivity
//! range):
//!
//! - **A**: `commerce_core::index::CatalogIndex`'s existing hard-coded
//!   `product_type_bitmaps: HashMap<ProductTypeId, RoaringBitmap>` --
//!   `product_type` is a fixed Rust struct field on `Product`, resolved via
//!   a typed integer key.
//! - **B**: `issue38_eval::CompiledEnumIndex` -- the identical
//!   `product_type` data, discovered and compiled the same catalog-
//!   agnostic way `cold_start::profile`/`compile_lexicon` already discover
//!   real attribute vocabulary, into the SAME physical primitive
//!   (`RoaringBitmap`) `CatalogIndex`'s own generic `enum_bitmaps` path
//!   already uses for arbitrary attributes -- reached via a
//!   `(String, String)`-keyed lookup instead of a typed one.
//! - **B2**: `issue38_eval::CompiledOrdinalIndex` -- added after B's own
//!   first measured result found a real, avoidable per-query allocation in
//!   B (see below); a redesigned compiled-schema path, one dedicated
//!   `HashMap<String, RoaringBitmap>` per compiled field, mirroring
//!   `CatalogIndex`'s own Issue #21 Phase 6D dictionary precedent, that
//!   needs no per-query allocation at all.
//! - **C**: `issue38_eval::GenericStore` -- the deliberate strawman, a
//!   fully runtime-generic per-document field map with no precomputed
//!   index at all, dispatching on value type per document per query.
//! - **D**: the same fresh, same-run Apache Solr `wands_bench` core
//!   already established in Phase 9, queried via
//!   `fq=product_class:"..."` (WANDS's raw column name).
//!
//! **E1 falsification gate** (stated before running, per Issue #38): path
//! B must preserve most of path A's physical advantage. `<=5%` serving
//! overhead vs. A is the initial target, reported as an actual measured
//! result, not tuned to hit it. If B is materially slower than A, this
//! binary stops short of drawing an E2-E5 recommendation and instead
//! reports the metrics needed to localize the source of overhead.
//!
//! **Metrics measured**: P50/P95/P99 latency (`bench_harness::Distribution`,
//! round-robin-interleaved reps per Issue #6's own anti-drift discipline),
//! mean single-threaded QPS (total queries / total wall time within the
//! timed loop), allocation count + bytes (a custom counting global
//! allocator), and index/structure size (`approximate_size_bytes`).
//! **cycles/instructions/branch-misses/cache-misses are NOT measured**:
//! this environment has no `perf` binary and
//! `/proc/sys/kernel/perf_event_paranoid` is unreadable (checked directly
//! before writing this binary) -- disclosed as a real environment
//! constraint per Issue #38's own "where practical" qualifier, not
//! fabricated. Resident memory (RSS) is measured in a *separate* process
//! per path (`--rss-only=a|b|b2|c`), to avoid one process's combined RSS
//! contaminating a single-path comparison -- run this binary with no
//! arguments for the main latency/allocation/index-size comparison, or
//! with `--rss-only=<a|b|b2|c>` for an isolated RSS reading.

use std::collections::HashMap;

use bench_harness::{round_robin_schedule, Distribution};
use commerce_core::domain::ProductTypeId;
use commerce_core::index::CatalogIndex;
use commerce_core::ir::{ResolvedConstraint, StructuralConstraint};
use issue38_eval::counting_alloc::{count_allocs, CountingAllocator};
use issue38_eval::{
    current_rss_kb, CompiledEnumIndex, CompiledOrdinalIndex, GenericStore, GenericValue,
};

#[global_allocator]
static GLOBAL: CountingAllocator = CountingAllocator;

const CATALOG_PATH: &str = "dataset_cache/wands/catalog.jsonl";
// Issue #6's own rigor bar: >=30 reps for anything decision-grade (this
// binary's gate verdict is exactly that). 10 reps was this binary's first
// draft and produced a sign flip on B's overhead between two otherwise
// identical runs (+19% one run, -15% the next, once B2 joined the
// round-robin) -- a real methodological problem caught before trusting
// either number, not picked-because-favorable: at ~1 microsecond per
// query, `Instant::now()`'s own call overhead and OS scheduling jitter are
// comparable in magnitude to the thing being measured. `IN_PROCESS_BATCH`
// times `IN_PROCESS_BATCH` consecutive same-query calls per timed sample
// and divides by the batch size, diluting timer/jitter overhead by two
// orders of magnitude -- applied only to A/B/B2 (each an O(1) hashmap
// lookup, genuinely sub-microsecond). Path C is a full O(catalog size)
// linear scan per call (~0.6ms measured, already ~1000x above timer
// resolution) -- batching it the same 200x turned a ~25-second experiment
// into one that did not finish in 5 minutes, caught directly rather than
// left to silently make every future run of this binary impractical.
// Solr (network-bound, ~1ms/call) is timed per-call, unbatched, for the
// same already-far-above-timer-resolution reason.
const REPS_PER_QUERY: usize = 30;
const IN_PROCESS_BATCH: usize = 200;
const LINEAR_SCAN_BATCH: usize = 1;
const WARMUP_PER_QUERY: usize = 2;
const NUM_WORKLOAD_QUERIES: usize = 40;

fn escape_solr_phrase(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

/// `phase6a_eval::catalog::build_catalog` assigns every product missing a
/// real WANDS `product_class` value the sentinel `ProductTypeId(0)`, but
/// (correctly) never pushes that sentinel into its own `product_types`
/// list -- it is not a real discovered entity. All three paths (A/B/C)
/// still need *some* string for it, since they iterate every product
/// unconditionally; the sentinel's name is deliberately excluded from the
/// query workload below (it is an ingestion artifact, not a real
/// merchant-relevant structural entity, so querying it would not be a
/// realistic filter).
const NO_PRODUCT_CLASS_SENTINEL_NAME: &str = "(no product_class)";

fn product_type_names(
    ingested: &phase6a_eval::catalog::IngestedCatalog,
) -> HashMap<ProductTypeId, String> {
    let mut names: HashMap<ProductTypeId, String> = ingested
        .product_types
        .iter()
        .map(|pt| (pt.id, pt.name.clone()))
        .collect();
    names.insert(ProductTypeId(0), NO_PRODUCT_CLASS_SENTINEL_NAME.to_string());
    names
}

/// A stratified sample of real `product_type` values spanning the
/// catalog's actual selectivity range (most common to least common),
/// rather than one arbitrarily chosen value -- a realistic structural
/// filter workload has queries of every selectivity, not just the
/// largest or smallest category.
fn stratified_workload(
    counts: &HashMap<ProductTypeId, usize>,
    names: &HashMap<ProductTypeId, String>,
    n: usize,
) -> Vec<(ProductTypeId, String, usize)> {
    let mut by_count: Vec<(ProductTypeId, usize)> =
        counts.iter().map(|(&id, &c)| (id, c)).collect();
    by_count.sort_by(|a, b| b.1.cmp(&a.1).then(a.0 .0.cmp(&b.0 .0)));
    let total = by_count.len();
    let n = n.min(total);
    (0..n)
        .map(|i| {
            let idx = if n <= 1 { 0 } else { i * (total - 1) / (n - 1) };
            let (id, count) = by_count[idx];
            (id, names[&id].clone(), count)
        })
        .collect()
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if let Some(only) = args
        .get(1)
        .and_then(|a| a.strip_prefix("--rss-only="))
        .map(str::to_string)
    {
        run_rss_only(&only);
        return;
    }

    println!("=== Issue #38 E1: hard-coded (A) vs compiled-schema (B) vs runtime-generic (C) vs Solr (D) ===");
    println!("environment check: perf/cycles/branch-misses/cache-misses NOT measured (no `perf` binary, perf_event_paranoid unreadable in this container) -- allocations + RSS + latency + index size measured instead");
    println!();

    let raw_products = phase6a_eval::data::load_catalog(std::path::Path::new(CATALOG_PATH));
    let ingested = phase6a_eval::catalog::build_catalog(&raw_products);
    let catalog = &ingested.catalog;
    println!(
        "catalog: {} products, {} product types",
        catalog.products.len(),
        ingested.product_types.len()
    );

    let product_type_name_by_id = product_type_names(&ingested);

    let mut variant_counts: HashMap<ProductTypeId, usize> = HashMap::new();
    for product in &catalog.products {
        // Exclude the ingestion sentinel (no real product_class) from the
        // query workload -- see `product_type_names`'s doc comment.
        if product.product_type == ProductTypeId(0) {
            continue;
        }
        *variant_counts.entry(product.product_type).or_insert(0) += product.variants.len();
    }

    let workload = stratified_workload(
        &variant_counts,
        &product_type_name_by_id,
        NUM_WORKLOAD_QUERIES,
    );
    println!(
        "workload: {} product_type queries, stratified by real selectivity (max count={}, min count={})",
        workload.len(),
        workload.first().map(|(_, _, c)| *c).unwrap_or(0),
        workload.last().map(|(_, _, c)| *c).unwrap_or(0)
    );
    println!();

    println!("building path A (hard-coded CatalogIndex)...");
    let (a_alloc_count, a_alloc_bytes, index_a) = count_allocs(|| CatalogIndex::build(catalog));
    println!(
        "  A built: {a_alloc_count} allocations, {a_alloc_bytes} bytes, approx index size for product_type bitmaps not separable from A's other structures -- see approximate_size_bytes below for A's *total* index"
    );

    println!("building path B (CompiledEnumIndex, offline-compiled schema)...");
    let (b_alloc_count, b_alloc_bytes, index_b) = count_allocs(|| {
        CompiledEnumIndex::compile(catalog, "product_type", |p| {
            product_type_name_by_id[&p.product_type].clone()
        })
    });
    println!("  B built: {b_alloc_count} allocations, {b_alloc_bytes} bytes");

    println!("building path B2 (CompiledOrdinalIndex, redesigned compiled schema)...");
    let (b2_alloc_count, b2_alloc_bytes, index_b2) = count_allocs(|| {
        CompiledOrdinalIndex::compile(catalog, |p| {
            product_type_name_by_id[&p.product_type].clone()
        })
    });
    println!("  B2 built: {b2_alloc_count} allocations, {b2_alloc_bytes} bytes");

    println!("building path C (GenericStore, runtime-generic)...");
    let (c_alloc_count, c_alloc_bytes, store_c) = count_allocs(|| {
        GenericStore::build(catalog, |p| {
            vec![(
                "product_type".to_string(),
                GenericValue::Str(product_type_name_by_id[&p.product_type].clone()),
            )]
        })
    });
    println!("  C built: {c_alloc_count} allocations, {c_alloc_bytes} bytes");
    println!();

    println!("=== index/structure size ===");
    println!(
        "A (CatalogIndex, ALL structures, not just product_type): {} bytes",
        index_a.approximate_size_bytes()
    );
    println!(
        "B (CompiledEnumIndex, product_type only): {} bytes",
        index_b.approximate_size_bytes()
    );
    println!(
        "B2 (CompiledOrdinalIndex, product_type only): {} bytes",
        index_b2.approximate_size_bytes()
    );
    println!(
        "C (GenericStore, product_type only): {} bytes",
        store_c.approximate_size_bytes()
    );
    println!();

    let solr_base_url = args
        .get(1)
        .cloned()
        .unwrap_or_else(|| "http://localhost:8983/solr/wands_bench".to_string());

    // Warmup pass, this project's own established discipline (P2-E16,
    // P9-E02, P9-E04): an unwarmed first measurement of a cross-process
    // call (Solr here) is unreliable.
    println!("warmup pass (all 5 paths, discarded)...");
    for (id, name, _) in &workload {
        let _ = index_a.indexed_candidates(&[ResolvedConstraint::Structural(
            StructuralConstraint::ProductType(*id),
        )]);
        let _ = index_b.query_eq("product_type", name);
        let _ = index_b2.query_eq(name);
        let _ = store_c.query_eq("product_type", &GenericValue::Str(name.clone()));
        let fq = format!("product_class:\"{}\"", escape_solr_phrase(name));
        let _ = round1_eval::solr::solr_search(&solr_base_url, "*:*", &[fq], 50_000);
    }

    // Round-robin interleave A/B/B2/C/D reps per query, Issue #6's own
    // anti-drift discipline (bench_harness::round_robin_schedule) -- run
    // method 0..5's reps in a freshly shuffled order per round rather than
    // back to back, so no single method is favored by monotonic drift in
    // shared process/OS state.
    const METHOD_COUNT: usize = 5;
    let mut latencies_ms: [Vec<f64>; METHOD_COUNT] = Default::default();
    let mut hit_count_mismatches = 0usize;

    for (id, name, _selectivity) in &workload {
        let schedule = round_robin_schedule(METHOD_COUNT, REPS_PER_QUERY, 1234567);
        // A tiny per-query warmup for the methods before the timed
        // round-robin reps begin, matching `measured_repeat`'s own
        // warmup-then-measure shape but across several interleaved methods
        // instead of one.
        for _ in 0..WARMUP_PER_QUERY {
            let _ = index_a.indexed_candidates(&[ResolvedConstraint::Structural(
                StructuralConstraint::ProductType(*id),
            )]);
            let _ = index_b.query_eq("product_type", name);
            let _ = index_b2.query_eq(name);
            let _ = store_c.query_eq("product_type", &GenericValue::Str(name.clone()));
        }

        let mut a_hits = None;
        let mut b_hits = None;
        let mut b2_hits = None;
        let mut c_hits = None;

        for &method in &schedule {
            match method {
                0 => {
                    let start = std::time::Instant::now();
                    let mut last_len = 0;
                    for _ in 0..IN_PROCESS_BATCH {
                        let bm = index_a.indexed_candidates(&[ResolvedConstraint::Structural(
                            StructuralConstraint::ProductType(*id),
                        )]);
                        last_len = bm.len();
                    }
                    let elapsed = start.elapsed().as_secs_f64() * 1000.0 / IN_PROCESS_BATCH as f64;
                    latencies_ms[0].push(elapsed);
                    a_hits = Some(last_len);
                }
                1 => {
                    let start = std::time::Instant::now();
                    let mut last_len = 0;
                    for _ in 0..IN_PROCESS_BATCH {
                        let bm = index_b.query_eq("product_type", name);
                        last_len = bm.len();
                    }
                    let elapsed = start.elapsed().as_secs_f64() * 1000.0 / IN_PROCESS_BATCH as f64;
                    latencies_ms[1].push(elapsed);
                    b_hits = Some(last_len);
                }
                2 => {
                    let start = std::time::Instant::now();
                    let mut last_len = 0;
                    for _ in 0..IN_PROCESS_BATCH {
                        let bm = index_b2.query_eq(name);
                        last_len = bm.len();
                    }
                    let elapsed = start.elapsed().as_secs_f64() * 1000.0 / IN_PROCESS_BATCH as f64;
                    latencies_ms[2].push(elapsed);
                    b2_hits = Some(last_len);
                }
                3 => {
                    let start = std::time::Instant::now();
                    let mut last_len = 0;
                    for _ in 0..LINEAR_SCAN_BATCH {
                        let hits =
                            store_c.query_eq("product_type", &GenericValue::Str(name.clone()));
                        last_len = hits.len() as u64;
                    }
                    let elapsed = start.elapsed().as_secs_f64() * 1000.0 / LINEAR_SCAN_BATCH as f64;
                    latencies_ms[3].push(elapsed);
                    c_hits = Some(last_len);
                }
                _ => {
                    let fq = format!("product_class:\"{}\"", escape_solr_phrase(name));
                    let start = std::time::Instant::now();
                    let result =
                        round1_eval::solr::solr_search(&solr_base_url, "*:*", &[fq], 50_000);
                    let elapsed = start.elapsed().as_secs_f64() * 1000.0;
                    latencies_ms[4].push(elapsed);
                    let _ = result;
                }
            }
        }

        if let (Some(a), Some(b), Some(b2), Some(c)) = (a_hits, b_hits, b2_hits, c_hits) {
            if a != b || a != b2 || a != c {
                hit_count_mismatches += 1;
                println!("  WARNING: hit-count mismatch for {name:?}: A={a} B={b} B2={b2} C={c}");
            }
        }
    }

    println!();
    println!(
        "hit-count cross-check across all {} workload queries: {} mismatches (must be 0 for this comparison to be trustworthy)",
        workload.len(),
        hit_count_mismatches
    );
    println!();

    let labels = [
        "A (hard-coded)",
        "B (compiled schema)",
        "B2 (compiled ordinal schema)",
        "C (runtime-generic)",
        "D (Solr)",
    ];
    let mut dists = Vec::new();
    for (i, label) in labels.iter().enumerate() {
        let dist = Distribution::compute(&latencies_ms[i]);
        dist.print(label, "ms");
        dists.push(dist);
    }
    println!();

    for (i, label) in labels.iter().enumerate() {
        let total_ms: f64 = latencies_ms[i].iter().sum();
        let qps = latencies_ms[i].len() as f64 / (total_ms / 1000.0);
        println!("{label}: single-threaded QPS (within the timed loop) = {qps:.1}");
    }
    println!();

    println!("=== allocations (per index build, not per query -- see below for per-query) ===");
    println!("A: {a_alloc_count} allocs, {a_alloc_bytes} bytes");
    println!("B: {b_alloc_count} allocs, {b_alloc_bytes} bytes");
    println!("B2: {b2_alloc_count} allocs, {b2_alloc_bytes} bytes");
    println!("C: {c_alloc_count} allocs, {c_alloc_bytes} bytes");
    println!();

    // Per-query allocation counts, one representative query (the median
    // by selectivity), isolated from build-time allocation.
    if let Some((id, name, _)) = workload.get(workload.len() / 2) {
        let (a_q_allocs, a_q_bytes, _) = count_allocs(|| {
            index_a.indexed_candidates(&[ResolvedConstraint::Structural(
                StructuralConstraint::ProductType(*id),
            )])
        });
        let (b_q_allocs, b_q_bytes, _) = count_allocs(|| index_b.query_eq("product_type", name));
        let (b2_q_allocs, b2_q_bytes, _) = count_allocs(|| index_b2.query_eq(name));
        let (c_q_allocs, c_q_bytes, _) =
            count_allocs(|| store_c.query_eq("product_type", &GenericValue::Str(name.clone())));
        println!(
            "=== per-query allocations (one representative median-selectivity query: {name:?}) ==="
        );
        println!("A: {a_q_allocs} allocs, {a_q_bytes} bytes");
        println!("B: {b_q_allocs} allocs, {b_q_bytes} bytes");
        println!("B2: {b2_q_allocs} allocs, {b2_q_bytes} bytes");
        println!("C: {c_q_allocs} allocs, {c_q_bytes} bytes");
    }
    println!();

    println!("=== E1 falsification gate: compiled-schema vs A serving overhead ===");
    let a_p50 = dists[0].p50;
    let b_p50 = dists[1].p50;
    let b2_p50 = dists[2].p50;
    let b_overhead_pct = if a_p50 > 0.0 {
        (b_p50 - a_p50) / a_p50 * 100.0
    } else {
        0.0
    };
    let b2_overhead_pct = if a_p50 > 0.0 {
        (b2_p50 - a_p50) / a_p50 * 100.0
    } else {
        0.0
    };
    println!(
        "A p50={a_p50:.4}ms  B p50={b_p50:.4}ms  B overhead={b_overhead_pct:+.2}%  (target: <=5%)"
    );
    println!(
        "A p50={a_p50:.4}ms  B2 p50={b2_p50:.4}ms  B2 overhead={b2_overhead_pct:+.2}%  (target: <=5%)"
    );
    if b_overhead_pct <= 5.0 {
        println!("=== E1 GATE (B, naive tuple-keyed compiled schema): PASSES ===");
    } else {
        println!("=== E1 GATE (B, naive tuple-keyed compiled schema): DOES NOT PASS -- localizing overhead below ===");
    }
    if b2_overhead_pct <= 5.0 {
        println!(
            "=== E1 GATE (B2, ordinal/dictionary-redesigned compiled schema): PASSES -- the naive B design's overhead was implementation-specific, not fundamental ==="
        );
    } else {
        println!("=== E1 GATE (B2, ordinal/dictionary-redesigned compiled schema): DOES NOT PASS EITHER -- some overhead survives even the redesign; localize further before continuing to E2-E5 ===");
    }

    println!();
    println!("=== context: C (runtime-generic) and D (Solr) vs A ===");
    let c_p50 = dists[3].p50;
    let d_p50 = dists[4].p50;
    println!(
        "C/A ratio: {:.2}x   D/A ratio: {:.2}x",
        c_p50 / a_p50,
        d_p50 / a_p50
    );
}

fn run_rss_only(which: &str) {
    let baseline_kb = current_rss_kb();
    let raw_products = phase6a_eval::data::load_catalog(std::path::Path::new(CATALOG_PATH));
    let ingested = phase6a_eval::catalog::build_catalog(&raw_products);
    let catalog = &ingested.catalog;
    let after_load_kb = current_rss_kb();

    let product_type_name_by_id = product_type_names(&ingested);

    let built_kb = match which {
        "a" => {
            let idx = CatalogIndex::build(catalog);
            let kb = current_rss_kb();
            std::hint::black_box(&idx);
            kb
        }
        "b" => {
            let idx = CompiledEnumIndex::compile(catalog, "product_type", |p| {
                product_type_name_by_id[&p.product_type].clone()
            });
            let kb = current_rss_kb();
            std::hint::black_box(&idx);
            kb
        }
        "b2" => {
            let idx = CompiledOrdinalIndex::compile(catalog, |p| {
                product_type_name_by_id[&p.product_type].clone()
            });
            let kb = current_rss_kb();
            std::hint::black_box(&idx);
            kb
        }
        "c" => {
            let store = GenericStore::build(catalog, |p| {
                vec![(
                    "product_type".to_string(),
                    GenericValue::Str(product_type_name_by_id[&p.product_type].clone()),
                )]
            });
            let kb = current_rss_kb();
            std::hint::black_box(&store);
            kb
        }
        other => {
            eprintln!("unknown --rss-only value: {other:?} (expected a, b, b2, or c)");
            std::process::exit(1);
        }
    };

    println!("=== Issue #38 E1: isolated RSS for path {which} ===");
    println!("RSS before catalog load: {baseline_kb} KB");
    println!(
        "RSS after catalog load:  {after_load_kb} KB (catalog fixed cost, shared by every path)"
    );
    println!("RSS after building {which}: {built_kb} KB");
    println!(
        "path {which}'s own structure cost (delta from post-load): {} KB",
        built_kb.saturating_sub(after_load_kb)
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_solr_phrase_handles_quotes_and_backslashes() {
        assert_eq!(
            escape_solr_phrase("Coffee & Cocktail Tables"),
            "Coffee & Cocktail Tables"
        );
        assert_eq!(
            escape_solr_phrase(r#"a "quoted" thing"#),
            r#"a \"quoted\" thing"#
        );
        assert_eq!(escape_solr_phrase(r"back\slash"), r"back\\slash");
    }

    #[test]
    fn stratified_workload_spans_from_most_to_least_common() {
        let mut counts = HashMap::new();
        let mut names = HashMap::new();
        for i in 0..10u32 {
            counts.insert(ProductTypeId(i), (10 - i) as usize);
            names.insert(ProductTypeId(i), format!("type{i}"));
        }
        let workload = stratified_workload(&counts, &names, 4);
        assert_eq!(workload.len(), 4);
        assert_eq!(workload.first().unwrap().0, ProductTypeId(0));
        assert_eq!(workload.last().unwrap().0, ProductTypeId(9));
        // Monotonically non-increasing selectivity across the sample.
        for pair in workload.windows(2) {
            assert!(pair[0].2 >= pair[1].2);
        }
    }

    #[test]
    fn stratified_workload_caps_at_available_distinct_count() {
        let mut counts = HashMap::new();
        let mut names = HashMap::new();
        counts.insert(ProductTypeId(1), 5);
        names.insert(ProductTypeId(1), "only".to_string());
        let workload = stratified_workload(&counts, &names, 40);
        assert_eq!(workload.len(), 1);
    }
}
