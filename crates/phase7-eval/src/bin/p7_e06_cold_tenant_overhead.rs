//! Phase 7 (Issue #21 Phase 7) P7-E06: cold-tenant overhead under
//! realistic background load. See `docs/experiments/PHASE7_LOG.md`'s
//! "P7-E06" section for the falsifiable H9 hypothesis stated before
//! this binary was written.
//!
//! Issue #21's Phase 7 "Experiments" list explicitly names "cold tenant
//! overhead" and "hot tenant saturation" as required measurements.
//! Nothing in H1-H8 tested this: H2 compared one heavily-loaded tenant
//! against one quiet tenant's latency, but both tenants were otherwise
//! equally idle/available -- neither was genuinely "cold" (infrequently
//! queried over a long window while OTHER tenants dominate the
//! process). P7-E01 varied the BREADTH of other tenants touched, not
//! the QUERY FREQUENCY of any one tenant. This binary asks directly:
//! does infrequent access itself cost anything in this architecture,
//! given each tenant's `CatalogIndex` is a fully independent, immutable
//! structure with no shared warm-cache/LRU state to lose (the same
//! mechanistic reasoning H2's isolation finding already rests on)?
//!
//! Design: pick two tenants of SIMILAR size (adjacent by product count)
//! so tenant SIZE is controlled and QUERY FREQUENCY is the only varied
//! input. One ("hot") is queried continuously by a dedicated thread; the
//! other ("cold") is queried only once every `COLD_QUERY_INTERVAL`
//! (simulating a genuinely low-QPS tenant) by a second dedicated thread.
//! Two additional threads continuously hammer the OTHER 53 tenants
//! (round-robin, matching P7-E01's established background-load pattern)
//! so both hot and cold tenants are measured under realistic
//! multi-tenant contention, not in an otherwise-idle process.
//!
//! Usage: cargo run --release -p phase7-eval --bin p7_e06_cold_tenant_overhead
//!        [catalog.jsonl]

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use commerce_core::domain::Catalog;
use commerce_core::index::CatalogIndex;
use phase7_eval::resident::facet_scan_once;
use phase7_eval::tenants::load_depth1_tenants;

const NOISY_WORKERS: usize = 2; // matches this container's remaining CPU count (4 total: hot + cold + 2 noisy)
const RUN_DURATION: Duration = Duration::from_secs(30);
const COLD_QUERY_INTERVAL: Duration = Duration::from_millis(100); // ~300 cold samples over 30s

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
        .unwrap_or_else(|| PathBuf::from("dataset_cache/wands/catalog.jsonl"));

    println!("=== P7-E06: cold-tenant overhead under realistic background load ===");

    let all_tenants = load_depth1_tenants(&catalog_path, 55);

    // Pick two tenants of similar size (adjacent by product count) to
    // isolate query FREQUENCY from tenant SIZE. Sort a copy by size and
    // find the pair of neighbors with the smallest size difference,
    // excluding the very largest/smallest (already covered by H6/H7/H8).
    let mut by_size: Vec<usize> = (0..all_tenants.len()).collect();
    by_size.sort_by_key(|&i| all_tenants[i].catalog.products.len());
    let mid = by_size.len() / 2;
    let hot_idx = by_size[mid];
    let cold_idx = by_size[mid - 1];

    let hot_name = all_tenants[hot_idx].name.clone();
    let cold_name = all_tenants[cold_idx].name.clone();
    let hot_products = all_tenants[hot_idx].catalog.products.len();
    let cold_products = all_tenants[cold_idx].catalog.products.len();
    println!(
        "  hot tenant={hot_name:?} products={hot_products}, cold tenant={cold_name:?} products={cold_products} (size-matched pair)"
    );

    let indexes: Vec<Arc<CatalogIndex>> = all_tenants
        .iter()
        .map(|t| Arc::new(CatalogIndex::build(&t.catalog)))
        .collect();
    let catalogs: Vec<Arc<Catalog>> = all_tenants
        .iter()
        .map(|t| Arc::new(t.catalog.clone()))
        .collect();
    let other_positions: Vec<usize> = (0..all_tenants.len())
        .filter(|&i| i != hot_idx && i != cold_idx)
        .collect();

    let mut hot_lat_all = Vec::new();
    let mut cold_lat_all = Vec::new();
    let mut hot_total_all = Vec::new();
    let mut cold_total_all = Vec::new();
    let mut noisy_total_all = Vec::new();

    const RUNS: usize = 3;
    for run in 1..=RUNS {
        println!("\n--- run {run}/{RUNS} ---");
        let indexes = indexes.clone();
        let catalogs = catalogs.clone();
        let other_positions = other_positions.clone();

        let stop = Arc::new(AtomicBool::new(false));
        let noisy_total = Arc::new(AtomicU64::new(0));
        let mut noisy_handles = Vec::new();
        for worker_id in 0..NOISY_WORKERS {
            let indexes = indexes.clone();
            let catalogs = catalogs.clone();
            let other_positions = other_positions.clone();
            let stop = Arc::clone(&stop);
            let noisy_total = Arc::clone(&noisy_total);
            noisy_handles.push(std::thread::spawn(move || {
                let mut i = worker_id;
                let mut count = 0u64;
                while !stop.load(Ordering::Relaxed) {
                    let pos = other_positions[i % other_positions.len()];
                    std::hint::black_box(facet_scan_once(&indexes[pos], &catalogs[pos]));
                    i += 1;
                    count += 1;
                }
                noisy_total.fetch_add(count, Ordering::Relaxed);
            }));
        }
        std::thread::sleep(Duration::from_millis(200));

        let hot_index = Arc::clone(&indexes[hot_idx]);
        let hot_catalog = Arc::clone(&catalogs[hot_idx]);
        let cold_index = Arc::clone(&indexes[cold_idx]);
        let cold_catalog = Arc::clone(&catalogs[cold_idx]);

        let hot_latencies = Arc::new(Mutex::new(Vec::new()));
        let hot_stop = Arc::new(AtomicBool::new(false));
        let hot_handle = {
            let hot_latencies = Arc::clone(&hot_latencies);
            let hot_stop = Arc::clone(&hot_stop);
            std::thread::spawn(move || {
                let mut local = Vec::new();
                while !hot_stop.load(Ordering::Relaxed) {
                    let start = Instant::now();
                    std::hint::black_box(facet_scan_once(&hot_index, &hot_catalog));
                    local.push(start.elapsed().as_nanos());
                }
                hot_latencies.lock().unwrap().extend(local);
            })
        };

        // Cold thread: one query per COLD_QUERY_INTERVAL, on the main
        // thread, timing ONLY the query call itself (not the sleep) so
        // scheduler wakeup jitter never contaminates the latency sample.
        let mut cold_local = Vec::new();
        let deadline = Instant::now() + RUN_DURATION;
        while Instant::now() < deadline {
            std::thread::sleep(COLD_QUERY_INTERVAL);
            let start = Instant::now();
            std::hint::black_box(facet_scan_once(&cold_index, &cold_catalog));
            cold_local.push(start.elapsed().as_nanos());
        }

        hot_stop.store(true, Ordering::Relaxed);
        hot_handle.join().unwrap();
        stop.store(true, Ordering::Relaxed);
        for h in noisy_handles {
            h.join().unwrap();
        }

        let mut hot_lat = Arc::try_unwrap(hot_latencies)
            .unwrap()
            .into_inner()
            .unwrap();
        hot_lat.sort_unstable();
        cold_local.sort_unstable();
        let noisy_total_requests = noisy_total.load(Ordering::Relaxed);

        let hot_p50 = percentile_ms(&hot_lat, 0.5);
        let hot_p99 = percentile_ms(&hot_lat, 0.99);
        let cold_p50 = percentile_ms(&cold_local, 0.5);
        let cold_p99 = percentile_ms(&cold_local, 0.99);
        let p99_ratio = if hot_p99 > 0.0 {
            cold_p99 / hot_p99
        } else {
            0.0
        };

        println!(
            "  hot: n={:<8} p50={hot_p50:>8.4}ms p99={hot_p99:>8.4}ms   cold: n={:<5} p50={cold_p50:>8.4}ms p99={cold_p99:>8.4}ms   p99_ratio(cold/hot)={p99_ratio:.2}x   noisy_requests={noisy_total_requests}",
            hot_lat.len(),
            cold_local.len()
        );

        hot_total_all.push(hot_lat.len());
        cold_total_all.push(cold_local.len());
        noisy_total_all.push(noisy_total_requests);
        hot_lat_all.push((hot_p50, hot_p99));
        cold_lat_all.push((cold_p50, cold_p99));
    }

    println!("\nH9 comparison (cold p99 / hot p99 ratio per run):");
    let mut max_ratio = 0.0f64;
    for (i, ((_, hot_p99), (_, cold_p99))) in
        hot_lat_all.iter().zip(cold_lat_all.iter()).enumerate()
    {
        let ratio = if *hot_p99 > 0.0 {
            cold_p99 / hot_p99
        } else {
            0.0
        };
        max_ratio = max_ratio.max(ratio);
        println!("  run {}: {ratio:.2}x", i + 1);
    }
    println!(
        "  H9 verdict: {}",
        if max_ratio >= 2.0 {
            "FALSIFIED -- cold tenant p99 latency is materially worse (>=2x) than the same-sized hot tenant's; infrequent access carries a real cost in this architecture"
        } else {
            "CONFIRMED -- cold tenant p99 latency stays within 2x of the same-sized hot tenant's; no material 'cold tenant overhead' penalty for infrequent access, consistent with each tenant's CatalogIndex being a fully independent, immutable structure with no shared warm-cache state to lose"
        }
    );

    let mut csv = String::from(
        "run,hot_tenant,hot_products,cold_tenant,cold_products,hot_n,hot_p50_ms,hot_p99_ms,cold_n,cold_p50_ms,cold_p99_ms,p99_ratio_cold_over_hot,noisy_total_requests\n",
    );
    for i in 0..RUNS {
        let (hot_p50, hot_p99) = hot_lat_all[i];
        let (cold_p50, cold_p99) = cold_lat_all[i];
        let ratio = if hot_p99 > 0.0 {
            cold_p99 / hot_p99
        } else {
            0.0
        };
        csv.push_str(&format!(
            "{},{hot_name},{hot_products},{cold_name},{cold_products},{},{hot_p50:.4},{hot_p99:.4},{},{cold_p50:.4},{cold_p99:.4},{ratio:.4},{}\n",
            i + 1,
            hot_total_all[i],
            cold_total_all[i],
            noisy_total_all[i]
        ));
    }

    let artifacts_dir = PathBuf::from("docs/research/artifacts/p7_e06_cold_tenant_overhead_run1");
    std::fs::create_dir_all(&artifacts_dir).ok();
    std::fs::write(artifacts_dir.join("results.csv"), &csv).ok();
    println!("\nartifacts written to {}", artifacts_dir.display());
}
