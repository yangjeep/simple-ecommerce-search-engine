//! Phase 7 (Issue #21 Phase 7) P7-E08: extending H4's QPS-vs-breadth
//! finding to controlled-stress-replicated tenant counts. See
//! `docs/experiments/PHASE7_LOG.md`'s "P7-E08" section for the
//! falsifiable H11 hypothesis stated before this binary was written.
//!
//! P7-E01/H4 confirmed that a fixed tenant's own throughput/latency does
//! not degrade as the BREADTH of other, distinct, concurrently-touched
//! tenants grows -- but only up to WANDS' real ceiling of 54 OTHER
//! tenants (55 total). P7-E02/H5 separately confirmed memory scales
//! linearly (not tenant-count-dependent) all the way to 6,500
//! controlled-stress-replicated tenants, a scale the real partition
//! alone could never reach. This binary asks the natural combination:
//! does H4's throughput/latency finding ALSO continue to hold at the
//! much larger tenant counts H5 reached for memory, using the exact
//! same controlled-stress replication methodology (real 55-tenant
//! population repeated end to end, `-copyN` suffixed to stay distinct)?
//!
//! Usage: cargo run --release -p phase7-eval --bin p7_e08_extended_breadth_qps
//!        [catalog.jsonl]

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use commerce_core::domain::Catalog;
use commerce_core::index::CatalogIndex;
use phase7_eval::resident::{current_rss_kb, facet_scan_once};
use phase7_eval::tenants::{partition_depth1, replicate_tenants, Order};

const TENANT_COUNTS: &[usize] = &[55, 200, 500, 1000, 2000];
const WORKERS: usize = 4; // matches this container's real CPU count
const RUN_DURATION: Duration = Duration::from_secs(4); // matches P7-E01
const RSS_SAFETY_CAP_KB: u64 = 6 * 1024 * 1024; // 6 GB, same cap P7-E02 used

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

    println!("=== P7-E08: extending H4's QPS-vs-breadth finding to controlled-stress-replicated tenant counts ===");
    println!(
        "(quiet tenant = 'Rugs-copy0', held fixed; n = total replicated tenant count including it; safety cap {} MB)",
        RSS_SAFETY_CAP_KB / 1024
    );

    let base = partition_depth1(&PathBuf::from(&catalog_path), 55, Order::LargestFirst);

    let mut csv = String::from(
        "n_tenants,quiet_total_requests,quiet_throughput_rps,quiet_p50_ms,quiet_p99_ms,noisy_total_requests,rss_kb\n",
    );
    let mut baseline_rps: Option<f64> = None;

    for &n in TENANT_COUNTS {
        let replicated = replicate_tenants(&base, n);
        let rugs_pos = replicated
            .iter()
            .position(|(name, _)| name == "Rugs-copy0")
            .expect("Rugs-copy0 must be present for every tested n (>=55, so copy0 covers all 55 real tenants)");

        let indexes: Vec<Arc<CatalogIndex>> = replicated
            .iter()
            .map(|(_, c)| Arc::new(CatalogIndex::build(c)))
            .collect();
        let catalogs: Vec<Arc<Catalog>> =
            replicated.into_iter().map(|(_, c)| Arc::new(c)).collect();

        let rss = current_rss_kb();
        if rss > RSS_SAFETY_CAP_KB {
            println!(
                "  n={n}: SAFETY CAP REACHED ({} MB > {} MB) -- stopping here, not extrapolating further",
                rss / 1024,
                RSS_SAFETY_CAP_KB / 1024
            );
            break;
        }

        let quiet_index = Arc::clone(&indexes[rugs_pos]);
        let quiet_catalog = Arc::clone(&catalogs[rugs_pos]);
        let other_positions: Vec<usize> = (0..indexes.len()).filter(|&i| i != rugs_pos).collect();
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
            "  n={n:<5} quiet_throughput_rps={quiet_throughput:>9.1} quiet_p50_ms={quiet_p50:>8.4} quiet_p99_ms={quiet_p99:>8.4} rps_ratio_vs_n55={rps_ratio:>6.2} noisy_total_requests={noisy_total_requests:<12} rss_kb={rss}"
        );
        csv.push_str(&format!(
            "{n},{quiet_total},{quiet_throughput:.2},{quiet_p50:.4},{quiet_p99:.4},{noisy_total_requests},{rss}\n"
        ));
    }

    let artifacts_dir = PathBuf::from("docs/research/artifacts/p7_e08_extended_breadth_run1");
    std::fs::create_dir_all(&artifacts_dir).ok();
    std::fs::write(artifacts_dir.join("results.csv"), &csv).ok();
    println!("\nartifacts written to {}", artifacts_dir.display());
}
