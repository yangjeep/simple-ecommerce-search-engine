//! Phase 7 (Issue #21 Phase 7) P7-E09: "tenants per fixed hardware
//! envelope at target SLO" -- one of Issue #21's explicitly-named
//! "Economic output" metrics, still an open gap after P7-E08. See
//! `docs/experiments/PHASE7_LOG.md`'s "P7-E09" section for the
//! falsifiable H12 hypothesis, this binary's first-draft self-caught
//! OOM bug, and the corrected design.
//!
//! P7-E02/H5 established that a 6 GB self-process-RSS safety cap
//! supports 6,500 controlled-stress-replicated tenants for MEMORY --
//! but that measurement keeps only each tenant's `CatalogIndex` resident
//! (the raw `Catalog` is dropped immediately after each tenant's index is
//! built, one at a time). P7-E08/H11's query-serving configuration needs
//! BOTH `Catalog` and `CatalogIndex` resident simultaneously per tenant
//! (querying requires the raw catalog too), so its real per-tenant
//! footprint is materially higher (~2.7-3.0 MB/tenant, P7-E08's own
//! measurement) than H5's index-only figure would suggest. This binary
//! builds INCREMENTALLY, one tenant at a time (immediately retiring any
//! transient per-tenant construction state, mirroring P7-E02's own
//! proven-safe pattern), checking this process's real RSS periodically
//! DURING construction -- not only once after a whole batch is built --
//! so a real safety trip happens before peak transient memory can exceed
//! either the chosen cap or this container's real hard limit.
//!
//! Usage: cargo run --release -p phase7-eval --bin p7_e09_slo_tenant_envelope
//!        [catalog.jsonl]

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use commerce_core::domain::Catalog;
use commerce_core::index::CatalogIndex;
use phase7_eval::resident::{current_rss_kb, facet_scan_once};
use phase7_eval::tenants::{partition_depth1, Order};

const WORKERS: usize = 4; // matches this container's real CPU count
const RUN_DURATION: Duration = Duration::from_secs(4); // matches P7-E01/P7-E08
const LATENCY_CHECKPOINTS: &[usize] = &[55, 2000]; // continuity with P7-E01 baseline and P7-E08/H11's already-tested ceiling
const CHECK_EVERY_N_TENANTS: usize = 250;
// This container's REAL hard memory limit, read directly from this
// process's own cgroup (/sys/fs/cgroup/memory/.../memory.limit_in_bytes
// = 14,327,726,080 bytes = 13.34 GiB) -- discovered the hard way when
// this binary's first draft was OOM-killed at that exact limit, well
// short of the naive host-level `free -h` total (~15 GB) this project
// had been assuming. The cap below leaves real margin under it.
const RSS_SAFETY_CAP_KB: u64 = 9 * 1024 * 1024; // 9 GB, ~4.3 GiB under the real 13.34 GiB hard limit
const HARD_TENANT_CEILING: usize = 8000; // backstop so a bug can't spin forever

fn percentile_ms(sorted_ns: &[u128], p: f64) -> f64 {
    if sorted_ns.is_empty() {
        return 0.0;
    }
    let idx = ((sorted_ns.len() as f64 - 1.0) * p).round() as usize;
    sorted_ns[idx] as f64 / 1_000_000.0
}

struct LatencyResult {
    n: usize,
    quiet_total: u64,
    quiet_throughput: f64,
    quiet_p50: f64,
    quiet_p99: f64,
    noisy_total_requests: u64,
    rss_kb: u64,
}

/// Run the quiet/noisy-tenant test (P7-E01/P7-E08's methodology) against
/// an already-built, fixed slice of tenants. Does not grow or shrink the
/// slice -- purely measures at whatever size is passed in.
fn run_quiet_noisy_test(
    indexes: &[Arc<CatalogIndex>],
    catalogs: &[Arc<Catalog>],
    quiet_pos: usize,
) -> LatencyResult {
    let n = indexes.len();
    let quiet_index = Arc::clone(&indexes[quiet_pos]);
    let quiet_catalog = Arc::clone(&catalogs[quiet_pos]);
    let other_positions: Vec<usize> = (0..n).filter(|&i| i != quiet_pos).collect();

    let stop = Arc::new(AtomicBool::new(false));
    let noisy_total = Arc::new(AtomicU64::new(0));
    let mut noisy_handles = Vec::new();
    let noisy_workers = WORKERS - 1;
    for worker_id in 0..noisy_workers {
        let indexes_arc: Vec<Arc<CatalogIndex>> = indexes.to_vec();
        let catalogs_arc: Vec<Arc<Catalog>> = catalogs.to_vec();
        let other_positions = other_positions.clone();
        let stop = Arc::clone(&stop);
        let noisy_total = Arc::clone(&noisy_total);
        noisy_handles.push(std::thread::spawn(move || {
            let mut i = worker_id;
            let mut count = 0u64;
            while !stop.load(Ordering::Relaxed) {
                let pos = other_positions[i % other_positions.len()];
                std::hint::black_box(facet_scan_once(&indexes_arc[pos], &catalogs_arc[pos]));
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
    let rss_kb = current_rss_kb();

    LatencyResult {
        n,
        quiet_total,
        quiet_throughput,
        quiet_p50,
        quiet_p99,
        noisy_total_requests,
        rss_kb,
    }
}

fn main() {
    let catalog_path = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("dataset_cache/wands/catalog.jsonl"));

    println!("=== P7-E09: tenants per fixed hardware envelope at target SLO (H12) ===");
    println!(
        "(quiet tenant = 'Rugs-copy0', held fixed; envelope = {} MB self-process RSS safety cap, chosen with real margin under this container's actual 13.34 GiB cgroup hard limit)",
        RSS_SAFETY_CAP_KB / 1024
    );

    let base = partition_depth1(&PathBuf::from(&catalog_path), 55, Order::LargestFirst);
    let rugs_pos_in_base = base
        .iter()
        .position(|(name, _)| name == "Rugs")
        .expect("Rugs must be present in the real base population");

    let mut indexes: Vec<Arc<CatalogIndex>> = Vec::new();
    let mut catalogs: Vec<Arc<Catalog>> = Vec::new();
    let mut results: Vec<LatencyResult> = Vec::new();
    let mut n = 0usize;
    let mut aborted = false;

    'build: while n < HARD_TENANT_CEILING {
        for (i, (_, catalog)) in base.iter().enumerate() {
            if n >= HARD_TENANT_CEILING {
                break 'build;
            }
            // Build ONE tenant's Catalog + CatalogIndex at a time and
            // immediately retire any transient construction state --
            // never materialize a separate all-tenants-at-once Vec of
            // raw Catalogs before indexes are built (the first draft's
            // bug: doing so via `replicate_tenants` + `.collect()` held
            // both the full raw-catalog Vec AND the full index Vec
            // simultaneously, and got OOM-killed mid-build for n=6500,
            // well before this binary's own RSS check ever ran).
            let tenant_catalog = Arc::new(Catalog {
                products: catalog.products.clone(),
            });
            let index = Arc::new(CatalogIndex::build(&tenant_catalog));
            catalogs.push(tenant_catalog);
            indexes.push(index);
            n += 1;
            let _ = i;

            if LATENCY_CHECKPOINTS.contains(&n) {
                // Rugs-copy0 (copy=0's Rugs) was pushed once, early, at
                // a fixed global index that never changes as later
                // tenants are appended -- NOT `copy * base.len() + ...`,
                // which would (incorrectly) point at whichever replica
                // happens to be under construction at this checkpoint.
                let result = run_quiet_noisy_test(&indexes, &catalogs, rugs_pos_in_base);
                println!(
                    "  [checkpoint] n={:<5} quiet_throughput_rps={:>9.1} quiet_p50_ms={:>8.4} quiet_p99_ms={:>8.4} noisy_total_requests={:<12} rss_kb={}",
                    result.n,
                    result.quiet_throughput,
                    result.quiet_p50,
                    result.quiet_p99,
                    result.noisy_total_requests,
                    result.rss_kb
                );
                results.push(result);
            }

            if n.is_multiple_of(CHECK_EVERY_N_TENANTS) {
                let rss = current_rss_kb();
                println!("  building... n={n:<6} rss_kb={rss}");
                if rss > RSS_SAFETY_CAP_KB {
                    println!(
                        "  SAFETY CAP REACHED at n={n} ({} MB > {} MB) -- stopping build here, not extrapolating further",
                        rss / 1024,
                        RSS_SAFETY_CAP_KB / 1024
                    );
                    aborted = true;
                    break 'build;
                }
            }
        }
    }

    // Final checkpoint at whatever count was actually, safely reached
    // (the real answer to "tenants per envelope at target SLO" for this
    // container), if it isn't already one of the fixed checkpoints above.
    if !LATENCY_CHECKPOINTS.contains(&n) && n > 0 {
        // Rugs-copy0's fixed global index, same as above.
        let result = run_quiet_noisy_test(&indexes, &catalogs, rugs_pos_in_base);
        println!(
            "  [final reached ceiling] n={:<5} quiet_throughput_rps={:>9.1} quiet_p50_ms={:>8.4} quiet_p99_ms={:>8.4} noisy_total_requests={:<12} rss_kb={}",
            result.n,
            result.quiet_throughput,
            result.quiet_p50,
            result.quiet_p99,
            result.noisy_total_requests,
            result.rss_kb
        );
        results.push(result);
    }

    println!(
        "\nfinal: {n} tenants built{}",
        if aborted {
            " (stopped early: RSS safety cap)"
        } else {
            " (stopped: hard tenant ceiling reached without hitting the RSS safety cap)"
        }
    );

    let baseline_rps = results.first().map(|r| r.quiet_throughput);
    let baseline_p99 = results.first().map(|r| r.quiet_p99);

    let mut csv = String::from(
        "n_tenants,quiet_total_requests,quiet_throughput_rps,quiet_p50_ms,quiet_p99_ms,rps_ratio_vs_n55,p99_ratio_vs_n55,slo_pass,noisy_total_requests,rss_kb\n",
    );
    for r in &results {
        let rps_ratio = r.quiet_throughput / baseline_rps.unwrap();
        let p99_ratio = r.quiet_p99 / baseline_p99.unwrap();
        let slo_pass = rps_ratio >= 0.80 && p99_ratio <= 2.0;
        println!(
            "  n={:<5} rps_ratio_vs_n55={rps_ratio:>6.2} p99_ratio_vs_n55={p99_ratio:>6.2} slo_pass={slo_pass}",
            r.n
        );
        csv.push_str(&format!(
            "{},{},{:.2},{:.4},{:.4},{:.3},{:.3},{},{},{}\n",
            r.n,
            r.quiet_total,
            r.quiet_throughput,
            r.quiet_p50,
            r.quiet_p99,
            rps_ratio,
            p99_ratio,
            slo_pass,
            r.noisy_total_requests,
            r.rss_kb
        ));
    }

    let artifacts_dir = PathBuf::from("docs/research/artifacts/p7_e09_slo_tenant_envelope_run1");
    std::fs::create_dir_all(&artifacts_dir).ok();
    std::fs::write(artifacts_dir.join("results.csv"), &csv).ok();
    println!("\nartifacts written to {}", artifacts_dir.display());
}
