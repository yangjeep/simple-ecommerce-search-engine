//! Phase 7 (Issue #21 Phase 7) P7-E01: does the BREADTH of concurrently-
//! touched OTHER tenants' data degrade one fixed tenant's own query
//! throughput, at fixed worker concurrency? See `docs/experiments/
//! PHASE7_LOG.md`'s "P7-E01" section for the falsifiable H4 hypothesis
//! and the self-caught confound in this binary's first draft (uniform-
//! random tenant selection over WANDS' long-tail population dramatically
//! changed AVERAGE per-query cost as N grew, since most load shifted to
//! tiny/cheap tenants -- that measured "workload mix drift", not a real
//! tenant-count effect).
//!
//! Corrected design: ONE dedicated worker repeatedly queries a FIXED
//! tenant ("Rugs", matching H2's own checkpoint for continuity); the
//! remaining `WORKERS - 1` threads continuously cycle through the OTHER
//! `n - 1` tenants (round-robin, not random, to guarantee genuine
//! coverage of all of them rather than a random subset). `n` is the
//! independent variable -- the quiet tenant's own query is held
//! completely fixed throughout, so any change in ITS throughput/latency
//! as `n` grows isolates the effect of touching more distinct tenants'
//! memory, not a workload-mix artifact.
//!
//! Usage: cargo run --release -p phase7-eval --bin p7_e01_qps_scaling
//!        [catalog.jsonl]

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use commerce_core::domain::Catalog;
use commerce_core::index::CatalogIndex;
use phase7_eval::tenants::load_depth1_tenants;

const TENANT_COUNTS: &[usize] = &[2, 5, 10, 25, 55];
const WORKERS: usize = 4; // matches this container's real CPU count
const RUN_DURATION: Duration = Duration::from_secs(4);

fn percentile_ms(sorted_ns: &[u128], p: f64) -> f64 {
    if sorted_ns.is_empty() {
        return 0.0;
    }
    let idx = ((sorted_ns.len() as f64 - 1.0) * p).round() as usize;
    sorted_ns[idx] as f64 / 1_000_000.0
}

fn facet_scan_once(index: &CatalogIndex, catalog: &Catalog) -> usize {
    let all = index.indexed_candidates(&[]);
    index.facet_counts_by_scan(&all, catalog, "color").len()
}

fn main() {
    let catalog_path = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("dataset_cache/wands/catalog.jsonl"));

    println!(
        "=== P7-E01: quiet tenant's own throughput as breadth of OTHER concurrently-touched tenants grows ==="
    );
    println!("(quiet tenant = 'Rugs', held fixed; n = total tenant count including Rugs)");

    let mut csv = String::from(
        "n_tenants,quiet_total_requests,quiet_throughput_rps,quiet_p50_ms,quiet_p99_ms,noisy_total_requests\n",
    );
    let mut baseline_rps: Option<f64> = None;

    // Load all 55 real tenants once; select subsets below so "Rugs" (not
    // among the largest-N by size) is guaranteed present at every tested
    // n, regardless of where it falls in the largest-first ordering.
    let all_tenants = load_depth1_tenants(&catalog_path, 55);
    let rugs_full_pos = all_tenants
        .iter()
        .position(|t| t.name == "Rugs")
        .expect("Rugs must be a real category in this WANDS catalog");

    for &n in TENANT_COUNTS {
        // Take Rugs plus the (n-1) other tenants nearest it in the
        // largest-first ordering (arbitrary but deterministic selection
        // -- this experiment only varies BREADTH of other tenants
        // touched, not their individual sizes).
        let mut indices: Vec<usize> = (0..all_tenants.len()).collect();
        indices.retain(|&i| i != rugs_full_pos);
        indices.truncate(n - 1);
        indices.push(rugs_full_pos);

        let quiet_pos = indices
            .iter()
            .position(|&i| i == rugs_full_pos)
            .expect("Rugs must be present at every tested n by construction");

        let mut indexes: Vec<Arc<CatalogIndex>> = Vec::with_capacity(indices.len());
        let mut catalogs: Vec<Arc<Catalog>> = Vec::with_capacity(indices.len());
        for &i in &indices {
            let catalog = all_tenants[i].catalog.clone();
            let index = CatalogIndex::build(&catalog);
            indexes.push(Arc::new(index));
            catalogs.push(Arc::new(catalog));
        }
        let quiet_index = Arc::clone(&indexes[quiet_pos]);
        let quiet_catalog = Arc::clone(&catalogs[quiet_pos]);
        let other_positions: Vec<usize> = (0..indexes.len()).filter(|&i| i != quiet_pos).collect();
        let indexes = Arc::new(indexes);
        let catalogs = Arc::new(catalogs);

        let stop = Arc::new(AtomicBool::new(false));
        let noisy_total = Arc::new(AtomicU64::new(0));
        let mut noisy_handles = Vec::new();
        let noisy_workers = WORKERS - 1;
        for worker_id in 0..noisy_workers {
            let indexes = Arc::clone(&indexes);
            let catalogs = Arc::clone(&catalogs);
            let other_positions = other_positions.clone();
            let stop = Arc::clone(&stop);
            let noisy_total = Arc::clone(&noisy_total);
            noisy_handles.push(std::thread::spawn(move || {
                if other_positions.is_empty() {
                    return;
                }
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

        let quiet_latencies = Arc::new(Mutex::new(Vec::new()));
        {
            let mut local = Vec::new();
            let deadline = Instant::now() + RUN_DURATION;
            while Instant::now() < deadline {
                let start = Instant::now();
                std::hint::black_box(facet_scan_once(&quiet_index, &quiet_catalog));
                local.push(start.elapsed().as_nanos());
            }
            quiet_latencies.lock().unwrap().extend(local);
        }
        stop.store(true, Ordering::Relaxed);
        for h in noisy_handles {
            h.join().unwrap();
        }

        let mut quiet_latencies = Arc::try_unwrap(quiet_latencies)
            .unwrap()
            .into_inner()
            .unwrap();
        quiet_latencies.sort_unstable();
        let quiet_total = quiet_latencies.len() as u64;
        let quiet_throughput = quiet_total as f64 / RUN_DURATION.as_secs_f64();
        let quiet_p50 = percentile_ms(&quiet_latencies, 0.5);
        let quiet_p99 = percentile_ms(&quiet_latencies, 0.99);
        let noisy_total_requests = noisy_total.load(Ordering::Relaxed);

        if baseline_rps.is_none() {
            baseline_rps = Some(quiet_throughput);
        }
        let rps_ratio = quiet_throughput / baseline_rps.unwrap();

        println!(
            "  n={n:<3} (breadth={:<3} other tenants) quiet_throughput_rps={quiet_throughput:>9.1} quiet_p50_ms={quiet_p50:>8.4} quiet_p99_ms={quiet_p99:>8.4} rps_ratio_vs_smallest_n={rps_ratio:>6.2} noisy_total_requests={noisy_total_requests}",
            n - 1
        );
        csv.push_str(&format!(
            "{n},{quiet_total},{quiet_throughput:.2},{quiet_p50:.4},{quiet_p99:.4},{noisy_total_requests}\n"
        ));
    }

    let artifacts_dir = PathBuf::from("docs/research/artifacts/p7_e01_qps_scaling_run1");
    std::fs::create_dir_all(&artifacts_dir).ok();
    std::fs::write(artifacts_dir.join("results.csv"), &csv).ok();
    println!("\nartifacts written to {}", artifacts_dir.display());
}
