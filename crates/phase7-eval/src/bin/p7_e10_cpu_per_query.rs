//! Phase 7 (Issue #21 Phase 7) P7-E10: CPU-seconds/query and CPU/tenant
//! -- Issue #21 explicitly names both as required Phase 7 measurements,
//! distinct from the wall-clock latency/throughput every prior Phase 7
//! experiment (P7-E01, P7-E06-E09) has measured instead. See
//! `docs/experiments/PHASE7_LOG.md`'s "P7-E10" section for the
//! falsifiable H13 hypothesis stated before this binary was written.
//!
//! Measures ONE tenant at a time, single-threaded, with no concurrent
//! noisy load -- deliberately different from every other Phase 7 QPS
//! experiment's quiet/noisy design, because CPU-time accounting here is
//! PROCESS-WIDE (`/proc/self/stat`'s utime+stime sum across all
//! threads), so a concurrent noisy thread would contaminate the signal
//! this experiment specifically isolates: one tenant's own CPU cost,
//! uncontended.
//!
//! Reuses H6/H7/H8/H9/H10's exact 3 real tenant sizes (largest/middle/
//! smallest already sampled throughout this phase) for direct
//! continuity: "Water Filter Pitchers" (1 product), "Faux Plants and
//! Trees" (5 products), "Furniture" (16,039 products).
//!
//! Usage: cargo run --release -p phase7-eval --bin p7_e10_cpu_per_query
//!        [catalog.jsonl]

use std::path::PathBuf;
use std::time::{Duration, Instant};

use commerce_core::index::CatalogIndex;
use phase7_eval::resident::{cpu_time_seconds, facet_scan_once};
use phase7_eval::tenants::load_single_tenant;

const TENANT_NAMES: &[&str] = &[
    "Water Filter Pitchers",
    "Faux Plants and Trees",
    "Furniture",
];
const WARMUP_DURATION: Duration = Duration::from_millis(500);
const RUN_DURATION: Duration = Duration::from_secs(4); // matches P7-E01/P7-E08's convention

fn main() {
    let catalog_path = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("dataset_cache/wands/catalog.jsonl"));

    println!("=== P7-E10: CPU-seconds/query and CPU/tenant (H13) ===");
    println!("(single-threaded, no concurrent noisy load -- isolates one tenant's own CPU cost)");

    let mut csv = String::from(
        "tenant,products,iterations,cpu_seconds_total,wall_seconds_total,cpu_us_per_query,wall_us_per_query,cpu_wall_ratio\n",
    );

    for &name in TENANT_NAMES {
        let catalog = load_single_tenant(&catalog_path, name);
        let index = CatalogIndex::build(&catalog);
        let products = catalog.products.len();

        // Warm up (allocator/cache/branch-predictor warm-up, matching
        // this project's standing discipline of not trusting the very
        // first in-process measurement -- P7-E09's own self-caught
        // cold-start-p99 lesson).
        let warmup_deadline = Instant::now() + WARMUP_DURATION;
        while Instant::now() < warmup_deadline {
            std::hint::black_box(facet_scan_once(&index, &catalog));
        }

        let cpu_start = cpu_time_seconds();
        let wall_start = Instant::now();
        let mut iterations = 0u64;
        while wall_start.elapsed() < RUN_DURATION {
            std::hint::black_box(facet_scan_once(&index, &catalog));
            iterations += 1;
        }
        let cpu_total = cpu_time_seconds() - cpu_start;
        let wall_total = wall_start.elapsed().as_secs_f64();

        let cpu_us_per_query = cpu_total * 1_000_000.0 / iterations as f64;
        let wall_us_per_query = wall_total * 1_000_000.0 / iterations as f64;
        let cpu_wall_ratio = cpu_total / wall_total;

        println!(
            "  tenant={name:<24} products={products:<7} iterations={iterations:<9} cpu_us_per_query={cpu_us_per_query:>9.2} wall_us_per_query={wall_us_per_query:>9.2} cpu/wall={cpu_wall_ratio:>5.3}"
        );
        csv.push_str(&format!(
            "{name},{products},{iterations},{cpu_total:.4},{wall_total:.4},{cpu_us_per_query:.3},{wall_us_per_query:.3},{cpu_wall_ratio:.4}\n"
        ));
    }

    let artifacts_dir = PathBuf::from("docs/research/artifacts/p7_e10_cpu_per_query_run1");
    std::fs::create_dir_all(&artifacts_dir).ok();
    std::fs::write(artifacts_dir.join("results.csv"), &csv).ok();
    println!("\nartifacts written to {}", artifacts_dir.display());
}
