//! Issue #8: real-data benchmark for the commerce-state fast path
//! (`commerce_core::state`) against Issue #7's revised performance thesis
//! bar (>=5x P50/P95 improvement, not a 20-30% gain) and against this
//! project's own current baseline for *any* state change: a full
//! `CatalogIndex::build` rebuild (R1-E01 measured ~64s/1.2M products on a
//! different machine; this binary re-measures it fresh in this run's own
//! environment so the comparison is apples-to-apples, not just similarly
//! named).
//!
//! **Evidence class**: the catalog SCALE and STRUCTURE are real (the
//! 1,215,854-product ESCI export used throughout Round 1/Phase 2). The
//! update WORKLOAD is synthetic: the real ESCI catalog carries no real
//! inventory/availability signal at all --
//! `round1_eval::catalog::build_catalog` hardcodes `Inventory::in_stock(0)`
//! for every product, because the source data has no such field. This is
//! recorded explicitly, matching Gate 7/R1-E05's evidence-class
//! discipline, not hidden behind a "real data" label the workload doesn't
//! earn.
//!
//! **A real-catalog shape limitation, also recorded rather than hidden**:
//! `round1_eval::catalog::build_catalog` assigns exactly one `Variant` per
//! real `Product` (the source ESCI export has no multi-variant/color-size
//! grouping). Issue #8's headline correctness case -- "OOS on one variant
//! must not hide an in-stock sibling variant of the same product" -- is
//! therefore *not* exercisable against this real catalog at all; it is
//! only provable via the synthetic `product_x` fixture already covered by
//! seven passing unit tests in `commerce_core::state::tests`. What *is*
//! testable at real 1.2M-product scale (§8 below) is the more general
//! no-cross-contamination invariant: marking one product's only variant
//! OOS must never change any other product's visibility.
//!
//! Usage: cargo run --release -p realtime-eval --bin variant_state_overlay_eval
//!        [catalog.jsonl]

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::thread;
use std::time::{Duration, Instant};

use rand::seq::SliceRandom;
use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;
use roaring::RoaringBitmap;

use commerce_core::domain::{Availability, BrandId, VariantId};
use commerce_core::index::CatalogIndex;
use commerce_core::ir::{CommerceQuery, ResolvedConstraint, StructuralConstraint};
use commerce_core::state::{
    execute_with_overlay, ApplyOutcome, Authority, CommerceStateOverlay, VariantStateDelta,
};
use round1_eval::{catalog, data};

const SEED: u64 = 7;

fn current_rss_kb() -> Option<u64> {
    let status = fs::read_to_string("/proc/self/status").ok()?;
    status
        .lines()
        .find_map(|line| line.strip_prefix("VmRSS:"))
        .and_then(|rest| rest.trim().trim_end_matches(" kB").trim().parse().ok())
}

/// Process CPU ticks (utime+stime) from `/proc/self/stat`, fields 14/15
/// (indices 11/12 after the `) ` that ends the possibly-space-containing
/// `comm` field). Converted to seconds assuming `USER_HZ=100`, the near-
/// universal value on Linux -- not queried via `sysconf` to avoid adding a
/// libc dependency for one diagnostic number; recorded as an assumption,
/// not silently treated as exact.
fn cpu_ticks() -> Option<u64> {
    let stat = fs::read_to_string("/proc/self/stat").ok()?;
    let after_comm = &stat[stat.rfind(')')? + 2..];
    let fields: Vec<&str> = after_comm.split_whitespace().collect();
    let utime: u64 = fields.get(11)?.parse().ok()?;
    let stime: u64 = fields.get(12)?.parse().ok()?;
    Some(utime + stime)
}

fn report_rss(label: &str, baseline_kb: u64) {
    match current_rss_kb() {
        Some(kb) => println!(
            "  {label}: {kb} KB ({:.2} MB) -- +{:.2} MB since process start",
            kb as f64 / 1024.0,
            kb.saturating_sub(baseline_kb) as f64 / 1024.0
        ),
        None => println!("  {label}: RSS unavailable"),
    }
}

fn percentile(sorted_nanos: &[u64], p: f64) -> u64 {
    if sorted_nanos.is_empty() {
        return 0;
    }
    let idx = (((sorted_nanos.len() - 1) as f64) * p).round() as usize;
    sorted_nanos[idx]
}

/// Nanosecond resolution throughout: this overlay's `apply()` cost is
/// consistently tens of nanoseconds, which microsecond-granularity timing
/// (this binary's first draft) rounds down to a uninformative "0us" for
/// every percentile -- caught by inspecting the first real run's own
/// output, not assumed in advance.
fn summarize_latencies(label: &str, mut nanos: Vec<u64>) -> (u64, u64, u64) {
    nanos.sort_unstable();
    let p50 = percentile(&nanos, 0.50);
    let p95 = percentile(&nanos, 0.95);
    let p99 = percentile(&nanos, 0.99);
    let mean: f64 = nanos.iter().sum::<u64>() as f64 / nanos.len().max(1) as f64;
    println!(
        "  {label}: n={} mean={:.1}ns p50={}ns p95={}ns p99={}ns",
        nanos.len(),
        mean,
        p50,
        p95,
        p99
    );
    (p50, p95, p99)
}

fn synthetic_delta(variant_id: VariantId, available: bool, observed_at: u64) -> VariantStateDelta {
    VariantStateDelta {
        product_id: commerce_core::domain::ProductId(variant_id.0),
        variant_id,
        availability: if available {
            Availability::InStock
        } else {
            Availability::OutOfStock
        },
        inventory_units: Some(if available { 5 } else { 0 }),
        observed_at,
        source: "synthetic-benchmark".to_string(),
        authority: Authority::Observed,
    }
}

fn main() {
    let catalog_path = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("dataset_cache/export/catalog.jsonl"));

    let baseline_kb = current_rss_kb().unwrap_or(0);
    println!("=== Issue #8: commerce-state fast path (variant availability overlay) real-data benchmark ===");
    println!(
        "process baseline RSS: {baseline_kb} KB ({:.2} MB)",
        baseline_kb as f64 / 1024.0
    );

    println!("\nloading real catalog...");
    let products = data::load_catalog(&catalog_path);
    println!("{} real products loaded", products.len());
    let ingested = catalog::build_catalog(&products);
    let n = ingested.catalog.products.len();

    // --- 1. Baseline: full CatalogIndex rebuild, measured fresh in this run ---
    println!("\n--- Baseline: full CatalogIndex::build rebuild (today's only mechanism for any state change) ---");
    let rebuild_start = Instant::now();
    let index = CatalogIndex::build(&ingested.catalog);
    let rebuild_secs = rebuild_start.elapsed().as_secs_f64();
    println!("  CatalogIndex::build: {rebuild_secs:.2}s for {n} real products (R1-E01, different machine: ~64s)");
    report_rss("after CatalogIndex::build", baseline_kb);

    // --- 2. Overlay build ---
    println!("\n--- CommerceStateOverlay::build (one-time, from the same index/catalog) ---");
    let overlay_build_start = Instant::now();
    let mut overlay = CommerceStateOverlay::build(&index, &ingested.catalog);
    println!(
        "  CommerceStateOverlay::build: {:.4}s",
        overlay_build_start.elapsed().as_secs_f64()
    );
    let baseline_available = overlay.available_ordinals().clone();
    report_rss("after CommerceStateOverlay::build", baseline_kb);
    let bitmap_bytes_before = overlay.available_ordinals().serialized_size();
    println!(
        "  overlay available_ordinals bitmap: {bitmap_bytes_before} bytes serialized ({} available/{} total)",
        baseline_available.len(),
        n
    );

    let mut rng = ChaCha8Rng::seed_from_u64(SEED);
    let mut all_variant_ids: Vec<VariantId> = ingested
        .catalog
        .products
        .iter()
        .flat_map(|p| p.variants.iter().map(|v| v.id))
        .collect();
    all_variant_ids.shuffle(&mut rng);

    // --- 3. Single-update latency (p50/p95/p99) ---
    println!("\n--- Single-update latency (20,000 sequential applies, single-threaded) ---");
    const SINGLE_N: usize = 20_000;
    let cpu_before = cpu_ticks();
    let mut nanos = Vec::with_capacity(SINGLE_N);
    let mut clock = 1u64;
    for (i, &vid) in all_variant_ids.iter().take(SINGLE_N).enumerate() {
        let delta = synthetic_delta(vid, i.is_multiple_of(2), clock);
        clock += 1;
        let t0 = Instant::now();
        let outcome = overlay.apply(&index, &delta);
        nanos.push(t0.elapsed().as_nanos() as u64);
        assert_eq!(
            outcome,
            ApplyOutcome::Applied,
            "synthetic delta must be a real state change"
        );
    }
    let cpu_after = cpu_ticks();
    let (single_p50, single_p95, _single_p99) = summarize_latencies("single-update apply()", nanos);
    if let (Some(before), Some(after)) = (cpu_before, cpu_after) {
        let cpu_secs = (after.saturating_sub(before)) as f64 / 100.0; // USER_HZ=100 assumed
        println!(
            "  CPU time (utime+stime, USER_HZ=100 assumed): {:.4}s total, {:.2}us/update -- wall-clock proxy, not a precise per-call CPU measurement",
            cpu_secs,
            (cpu_secs * 1_000_000.0) / SINGLE_N as f64
        );
    }

    // --- 4. Sustained throughput + stability across a larger run ---
    println!(
        "\n--- Sustained throughput (200,000 applies, bucketed into 10 deciles for drift) ---"
    );
    const SUSTAINED_N: usize = 200_000;
    let sustained_start = Instant::now();
    let mut decile_nanos: Vec<Vec<u64>> = vec![Vec::new(); 10];
    for i in 0..SUSTAINED_N {
        let vid = all_variant_ids[i % all_variant_ids.len()];
        let delta = synthetic_delta(vid, i.is_multiple_of(2), clock);
        clock += 1;
        let t0 = Instant::now();
        overlay.apply(&index, &delta);
        let ns = t0.elapsed().as_nanos() as u64;
        decile_nanos[(i * 10) / SUSTAINED_N].push(ns);
    }
    let sustained_secs = sustained_start.elapsed().as_secs_f64();
    let tps = SUSTAINED_N as f64 / sustained_secs;
    println!("  {SUSTAINED_N} applies in {sustained_secs:.3}s -> {tps:.0} updates/sec sustained");
    for (i, bucket) in decile_nanos.iter().enumerate() {
        let mean: f64 = bucket.iter().sum::<u64>() as f64 / bucket.len().max(1) as f64;
        println!("    decile {i}: n={} mean={:.1}ns", bucket.len(), mean);
    }

    // --- 5. Concurrent QPS/latency under mutation load vs. read-only baseline ---
    println!("\n--- Concurrent query throughput/latency, read-only baseline vs. under concurrent writes ---");
    println!("  (RwLock<CommerceStateOverlay> wraps only this benchmark's harness -- CommerceStateOverlay itself is unsynchronized, per its single-process-owner design)");
    println!("  (queries are real-brand-scoped structural constraints, not unconstrained full-catalog scans, so per-query latency reflects a representative candidate-set size rather than materializing ~1.2M hits per call)");
    // A brand-scoped constraint keeps each query's candidate set representative
    // of real selectivity, rather than an empty-constraint query that would
    // materialize a near-1.2M-hit Vec every call and swamp lock/overlay cost
    // with unrelated allocation cost.
    let mut brand_sample: Vec<BrandId> = ingested.brands.iter().map(|b| b.id).collect();
    brand_sample.shuffle(&mut rng);
    brand_sample.truncate(50);
    if brand_sample.is_empty() {
        brand_sample.push(BrandId(0));
    }
    let catalog_arc = Arc::new(ingested.catalog);
    let index_arc = Arc::new(index);
    let brand_sample_arc = Arc::new(brand_sample);

    for (label, run_writer) in [
        ("read-only baseline", false),
        ("under concurrent writes", true),
    ] {
        let shared = Arc::new(RwLock::new(overlay));
        let stop = Arc::new(AtomicBool::new(false));
        let write_count = Arc::new(AtomicU64::new(0));
        let duration = Duration::from_secs(2);

        let writer_handle = if run_writer {
            let shared = Arc::clone(&shared);
            let stop = Arc::clone(&stop);
            let write_count = Arc::clone(&write_count);
            let index_arc = Arc::clone(&index_arc);
            let ids: Vec<VariantId> = all_variant_ids.clone();
            let mut wclock = clock;
            Some(thread::spawn(move || {
                let mut i = 0usize;
                while !stop.load(Ordering::Relaxed) {
                    let vid = ids[i % ids.len()];
                    let delta = synthetic_delta(vid, i.is_multiple_of(2), wclock);
                    wclock += 1;
                    i += 1;
                    let mut guard = shared.write().expect("writer lock");
                    guard.apply(&index_arc, &delta);
                    drop(guard);
                    write_count.fetch_add(1, Ordering::Relaxed);
                }
            }))
        } else {
            None
        };

        let reader_handles: Vec<_> = (0..4)
            .map(|t| {
                let shared = Arc::clone(&shared);
                let stop = Arc::clone(&stop);
                let catalog_arc = Arc::clone(&catalog_arc);
                let index_arc = Arc::clone(&index_arc);
                let brand_sample_arc = Arc::clone(&brand_sample_arc);
                thread::spawn(move || {
                    let mut latencies = Vec::new();
                    let mut i = t * 97; // stagger starting offsets deterministically
                    while !stop.load(Ordering::Relaxed) {
                        let brand_id = brand_sample_arc[i % brand_sample_arc.len()];
                        let q = CommerceQuery {
                            constraints: vec![ResolvedConstraint::Structural(
                                StructuralConstraint::Brand(brand_id),
                            )],
                            preferences: vec![],
                            ambiguous: vec![],
                            residual_lexical: vec![],
                        };
                        let t0 = Instant::now();
                        let guard = shared.read().expect("reader lock");
                        let _hits = execute_with_overlay(&index_arc, &guard, &q, &catalog_arc);
                        drop(guard);
                        latencies.push(t0.elapsed().as_nanos() as u64);
                        i += 1;
                    }
                    latencies
                })
            })
            .collect();

        thread::sleep(duration);
        stop.store(true, Ordering::Relaxed);

        let mut all_latencies = Vec::new();
        for h in reader_handles {
            all_latencies.extend(h.join().expect("reader thread"));
        }
        let writes = if let Some(h) = writer_handle {
            h.join().expect("writer thread");
            write_count.load(Ordering::Relaxed)
        } else {
            0
        };

        let qps = all_latencies.len() as f64 / duration.as_secs_f64();
        println!(
            "  [{label}] {} queries in {:.1}s -> {qps:.0} QPS",
            all_latencies.len(),
            duration.as_secs_f64()
        );
        summarize_latencies(&format!("  [{label}] query latency"), all_latencies);
        if run_writer {
            println!("  [{label}] writer applied {writes} updates in {:.1}s -> {:.0} updates/sec concurrent with reads", duration.as_secs_f64(), writes as f64 / duration.as_secs_f64());
        }

        overlay = Arc::try_unwrap(shared)
            .unwrap_or_else(|_| panic!("all threads joined, overlay must be uniquely owned"))
            .into_inner()
            .expect("no poisoned lock");
    }

    let index = Arc::try_unwrap(index_arc).unwrap_or_else(|_| panic!("index still shared"));
    let ingested_catalog =
        Arc::try_unwrap(catalog_arc).unwrap_or_else(|_| panic!("catalog still shared"));

    // --- 6. Memory overhead after all mutation load ---
    println!("\n--- Memory overhead after ~220,000 total applies ---");
    report_rss(
        "RSS after all single/sustained/concurrent applies",
        baseline_kb,
    );
    let bitmap_bytes_after = overlay.available_ordinals().serialized_size();
    println!(
        "  overlay available_ordinals bitmap: {bitmap_bytes_after} bytes serialized ({} available/{} total) -- was {bitmap_bytes_before} bytes before any updates",
        overlay.available_ordinals().len(),
        n
    );

    // --- 7. Write amplification (coarse proxy, disclosed as such) ---
    println!(
        "\n--- Write amplification (coarse proxy: serialized bitmap-size delta / update count) ---"
    );
    let total_applies = SINGLE_N + SUSTAINED_N;
    let bitmap_delta = bitmap_bytes_after.abs_diff(bitmap_bytes_before);
    println!(
        "  {bitmap_delta} bytes serialized-size delta over {total_applies} applies -> {:.4} bytes/update (proxy on the compressed-bitmap representation, not an instrumented measurement of memory pages actually dirtied)",
        bitmap_delta as f64 / total_applies as f64
    );
    println!(
        "  contrast: today's only alternative -- a full CatalogIndex::build rebuild -- necessarily reallocates/rewrites all ~{} bytes ({:.1} MB) of every structural index, for a single state change",
        index.approximate_size_bytes(),
        index.approximate_size_bytes() as f64 / (1024.0 * 1024.0)
    );

    // --- 8. Product-vs-variant correctness at real 1.2M-product scale ---
    println!("\n--- Correctness at real scale: no-cross-contamination invariant (1% of real products marked OOS) ---");
    let mut fresh_overlay = CommerceStateOverlay::build(&index, &ingested_catalog);
    let full_set = index.indexed_candidates(&[]); // no constraints -> every ordinal
    let mut marked = RoaringBitmap::new();
    for ord in full_set.iter() {
        if ord % 100 == 0 {
            marked.insert(ord);
        }
    }
    for ord in marked.iter() {
        let vid = index.variant_id_at(ord).expect("ordinal from this index");
        let outcome = fresh_overlay.apply(&index, &synthetic_delta(vid, false, 1));
        assert!(matches!(outcome, ApplyOutcome::Applied));
    }
    let expected_available = &full_set - &marked;
    let actual_available = fresh_overlay.available_ordinals().clone();
    assert_eq!(
        actual_available, expected_available,
        "marking {} ordinals OOS must leave exactly the complement available -- no other product's visibility may change",
        marked.len()
    );
    println!(
        "  PASS: marked {} of {} real products OOS; available set is exactly the complement (bitmap equality, not sampled)",
        marked.len(),
        n
    );
    let hits = execute_with_overlay(
        &index,
        &fresh_overlay,
        &CommerceQuery {
            constraints: vec![],
            preferences: vec![],
            ambiguous: vec![],
            residual_lexical: vec![],
        },
        &ingested_catalog,
    );
    assert_eq!(hits.len(), expected_available.len() as usize, "end-to-end execute_with_overlay hit count must match the overlay's own available-ordinal count");
    println!(
        "  PASS: execute_with_overlay end-to-end hit count matches ({} hits)",
        hits.len()
    );
    println!(
        "  NOTE: the real ESCI catalog assigns exactly one Variant per Product (round1_eval::catalog::build_catalog), so the exact \"OOS variant must not hide an in-stock sibling variant of the same product\" scenario is not exercisable against real data at any scale -- that specific case is proven only by the synthetic product_x fixture (7 passing unit tests, commerce_core::state::tests)."
    );

    // --- 9. Recovery/replay behavior on "restart" ---
    println!("\n--- Recovery/replay: what happens to accepted deltas if the process restarts ---");
    let second_overlay = CommerceStateOverlay::build(&index, &ingested_catalog);
    assert_eq!(
        second_overlay.available_ordinals(),
        &baseline_available,
        "a freshly built overlay must re-derive only the catalog's own baked-in snapshot"
    );
    println!(
        "  CONFIRMED LIMITATION: rebuilding CommerceStateOverlay from the same index/catalog reproduces only the {}-available baked-in baseline, not the {} deltas applied to the prior instance in this run -- v1 has no durability/replay log. \
Every accepted VariantStateDelta is lost on process restart. \
Suggested follow-up: an operation log analogous to Havenask's UpdateFieldOperation/OperationLogReplayer (docs/research/havenask-realtime-update-archaeology.md), not designed or built here -- out of Issue #8's scope boundary as written.",
        baseline_available.len(),
        total_applies
    );

    // --- 10. The actual comparison against Issue #7's revised performance bar ---
    println!("\n=== Summary: overlay apply() vs. full CatalogIndex::build rebuild, Issue #7's >=5x P50/P95 bar ===");
    let rebuild_ns = rebuild_secs * 1_000_000_000.0;
    println!("  full rebuild (this run, this machine): {rebuild_secs:.2}s ({rebuild_ns:.0}ns)");
    println!("  overlay apply() p50: {single_p50}ns, p95: {single_p95}ns");
    println!(
        "  multiplier: rebuild/p50 = {:.0}x, rebuild/p95 = {:.0}x (bar: >=5x)",
        rebuild_ns / single_p50.max(1) as f64,
        rebuild_ns / single_p95.max(1) as f64
    );
}
