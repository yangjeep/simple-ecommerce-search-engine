//! Phase 7 (Issue #21 Phase 7) P7-E04: long-running resident-process
//! overhead. See `docs/experiments/PHASE7_LOG.md`'s "P7-E04" section for
//! the falsifiable H7 hypothesis stated before this binary was written.
//!
//! P7-E03/H6 measured a real per-OS-process baseline (~2.1-2.2 MB) using
//! SHORT-LIVED child processes: spawn, (optionally) load one tenant,
//! print RSS, exit -- all in well under a second. Every unresolved risk
//! note since has named the same gap: a real deployed service does not
//! exit immediately -- it stays resident, keeps worker threads alive,
//! and serves a sustained query stream. This binary tests whether that
//! matters: does a process's RSS materially GROW beyond H6's immediate
//! snapshot once it actually behaves like a long-running service for a
//! sustained window, or does H6's floor already capture the steady
//! state?
//!
//! Two resident conditions, mirroring H6's bare/tenant split:
//!   - idle-resident: no tenant data, RUN_DURATION worker threads parked
//!     alive (a resident but idle connection-handler pool)
//!   - active-resident: one real tenant's data loaded, RUN_DURATION
//!     worker threads continuously executing real structural queries
//!     against it
//!
//! RSS is sampled periodically during the run (not just at the end) so a
//! genuine plateau can be distinguished from ongoing growth that this
//! bounded window might not have fully captured. (P7-E05 asks exactly
//! that question over a much longer window, reusing the same
//! `phase7_eval::resident` sampling primitives this binary uses.)
//!
//! Usage:
//!   orchestrator (default): cargo run --release -p phase7-eval
//!     --bin p7_e04_long_running_overhead -- [catalog.jsonl]
//!   child (invoked by the orchestrator itself, not normally run by hand):
//!     p7_e04_long_running_overhead child idle|active [catalog.jsonl] [tenant_name]

use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use commerce_core::index::CatalogIndex;
use phase7_eval::resident::{
    current_rss_kb, fmt_samples, parse_samples, peak_kb, run_active_resident, run_idle_resident,
};
use phase7_eval::tenants::{
    load_single_tenant, partition_depth1, write_single_tenant_jsonl, Order,
};

const BASELINE_CHILD_COUNT: usize = 3;
const WORKER_THREADS: usize = 4; // matches this container's real CPU count
const RUN_DURATION: Duration = Duration::from_secs(20);
const SAMPLE_INTERVAL: Duration = Duration::from_secs(5);

/// Child-mode entry point. Reports RSS at t0 (matching H6's methodology
/// exactly, for direct comparability), then either sits idle-resident or
/// actively serves queries for `RUN_DURATION`, sampling RSS
/// periodically, then reports RSS again AFTER the worker threads have
/// been joined (torn down). NOTE: this post-join reading is NOT the
/// primary "long-running resident cost" signal -- joining threads can
/// itself reclaim memory (thread stacks, per-thread allocator arenas)
/// that a real, continuously-running service would never tear down
/// mid-life. The orchestrator instead uses the PEAK of the periodic
/// samples taken while the process was actually behaving like a live
/// service (still running, not yet torn down) as the primary H7 metric;
/// the post-join reading here is kept only as a secondary, separately
/// labeled data point about teardown behavior.
fn run_child(mode: &str, catalog_path: &std::path::Path, tenant_name: Option<&str>) {
    let baseline_rss = current_rss_kb();
    println!("baseline_rss_kb={baseline_rss}");

    match (mode, tenant_name) {
        ("idle", _) => {
            run_idle_resident(WORKER_THREADS, RUN_DURATION, SAMPLE_INTERVAL);
        }
        ("active", Some(name)) => {
            let catalog = load_single_tenant(catalog_path, name);
            let products = catalog.products.len();
            let index = CatalogIndex::build(&catalog);
            let with_tenant_rss = current_rss_kb();
            println!("tenant_name={name}");
            println!("tenant_products={products}");
            println!("with_tenant_rss_kb={with_tenant_rss}");
            let total_queries = run_active_resident(
                Arc::new(index),
                Arc::new(catalog),
                WORKER_THREADS,
                RUN_DURATION,
                SAMPLE_INTERVAL,
            );
            println!("total_queries_served={total_queries}");
        }
        _ => panic!("active mode requires a tenant name"),
    }

    println!("steady_state_rss_kb={}", current_rss_kb());
}

struct ChildResult {
    baseline_rss: u64,
    with_tenant_rss: Option<u64>,
    steady_state_rss: u64,
    products: usize,
    total_queries: u64,
    samples: Vec<(u64, u64)>,
}

fn spawn_child(
    exe: &str,
    mode: &str,
    catalog_path: &str,
    tenant_name: Option<&str>,
) -> ChildResult {
    let mut cmd = Command::new(exe);
    cmd.arg("child").arg(mode).arg(catalog_path);
    if let Some(name) = tenant_name {
        cmd.arg(name);
    }
    let output = cmd.output().expect("failed to spawn child process");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut result = ChildResult {
        baseline_rss: 0,
        with_tenant_rss: None,
        steady_state_rss: 0,
        products: 0,
        total_queries: 0,
        samples: parse_samples(&stdout),
    };
    for line in stdout.lines() {
        if let Some(v) = line.strip_prefix("baseline_rss_kb=") {
            result.baseline_rss = v.parse().unwrap_or(0);
        } else if let Some(v) = line.strip_prefix("with_tenant_rss_kb=") {
            result.with_tenant_rss = v.parse().ok();
        } else if let Some(v) = line.strip_prefix("tenant_products=") {
            result.products = v.parse().unwrap_or(0);
        } else if let Some(v) = line.strip_prefix("steady_state_rss_kb=") {
            result.steady_state_rss = v.parse().unwrap_or(0);
        } else if let Some(v) = line.strip_prefix("total_queries_served=") {
            result.total_queries = v.parse().unwrap_or(0);
        }
    }
    result
}

fn main() {
    let mut args = std::env::args().skip(1);
    let first = args.next();

    if first.as_deref() == Some("child") {
        let mode = args.next().expect("child mode requires idle|active");
        let catalog_path = args
            .next()
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("dataset_cache/wands/catalog.jsonl"));
        let tenant_name = args.next();
        run_child(&mode, &catalog_path, tenant_name.as_deref());
        return;
    }

    let catalog_path = first.unwrap_or_else(|| "dataset_cache/wands/catalog.jsonl".to_string());
    let exe = std::env::current_exe()
        .expect("current_exe")
        .to_string_lossy()
        .to_string();

    println!("=== P7-E04: long-running resident-process overhead ===");
    println!(
        "RUN_DURATION={}s, WORKER_THREADS={WORKER_THREADS}, sampling every {}s",
        RUN_DURATION.as_secs(),
        SAMPLE_INTERVAL.as_secs()
    );

    println!(
        "\nspawning {BASELINE_CHILD_COUNT} idle-resident bare processes (no tenant data, {WORKER_THREADS} parked worker threads, {}s window)...",
        RUN_DURATION.as_secs()
    );

    let mut idle_t0 = Vec::with_capacity(BASELINE_CHILD_COUNT);
    let mut idle_peak = Vec::with_capacity(BASELINE_CHILD_COUNT);
    let mut idle_post_teardown = Vec::with_capacity(BASELINE_CHILD_COUNT);
    for i in 0..BASELINE_CHILD_COUNT {
        let r = spawn_child(&exe, "idle", &catalog_path, None);
        let peak = peak_kb(&r.samples);
        println!(
            "    idle child {i}: t0={} KB  samples: {}  peak={peak} KB  post_teardown={} KB",
            r.baseline_rss,
            fmt_samples(&r.samples),
            r.steady_state_rss
        );
        idle_t0.push(r.baseline_rss);
        idle_peak.push(peak);
        idle_post_teardown.push(r.steady_state_rss);
    }
    let mean = |v: &[u64]| v.iter().sum::<u64>() as f64 / v.len() as f64;
    let mean_idle_t0 = mean(&idle_t0);
    let mean_idle_peak = mean(&idle_peak);
    let mean_idle_post_teardown = mean(&idle_post_teardown);
    let idle_growth = mean_idle_peak - mean_idle_t0;
    println!(
        "  idle-resident: t0_mean={mean_idle_t0:.1} KB peak_mean={mean_idle_peak:.1} KB growth={idle_growth:.1} KB post_teardown_mean={mean_idle_post_teardown:.1} KB (n={})",
        idle_t0.len()
    );

    println!(
        "\nspawning active-resident single-tenant processes (largest/mid/smallest, {}s of sustained real query serving each)...",
        RUN_DURATION.as_secs()
    );
    let all = partition_depth1(&PathBuf::from(&catalog_path), 55, Order::LargestFirst);
    let largest = &all[0].0;
    let mid = &all[all.len() / 2].0;
    let smallest = &all[all.len() - 1].0;

    let mut csv = String::from(
        "condition,tenant_name,tenant_products,t0_rss_kb,with_tenant_rss_kb,peak_serving_rss_kb,peak_growth_kb,post_teardown_rss_kb,total_queries_served\n",
    );
    csv.push_str(&format!(
        "idle_resident_mean,,,{mean_idle_t0:.1},,{mean_idle_peak:.1},{idle_growth:.1},{mean_idle_post_teardown:.1},\n"
    ));

    let mut tenant_growths = Vec::new();
    for name in [largest.as_str(), mid.as_str(), smallest.as_str()] {
        let single_tenant_path = write_single_tenant_jsonl(
            &PathBuf::from(&catalog_path),
            name,
            "p7_e04_single_tenant_tmp",
        );
        let r = spawn_child(
            &exe,
            "active",
            single_tenant_path.to_str().expect("valid utf8 path"),
            Some(name),
        );
        let with_tenant = r.with_tenant_rss.unwrap_or(r.baseline_rss);
        let peak = peak_kb(&r.samples);
        // PRIMARY metric: peak RSS observed while the process is still
        // actually serving (not yet torn down) vs. the immediate
        // post-load snapshot. Signed, not saturating -- a real decrease
        // is information, not noise to clip away.
        let growth = peak as i64 - with_tenant as i64;
        tenant_growths.push(growth);
        println!(
            "  tenant={name:?} products={:<7} t0_rss_kb={:<8} with_tenant_rss_kb={with_tenant:<8} peak_serving_rss_kb={peak:<8} peak_growth_kb={growth:<8} post_teardown_rss_kb={:<8} total_queries_served={}  samples: {}",
            r.products, r.baseline_rss, r.steady_state_rss, r.total_queries, fmt_samples(&r.samples)
        );
        csv.push_str(&format!(
            "active_resident,{name},{},{},{with_tenant},{peak},{growth},{},{}\n",
            r.products, r.baseline_rss, r.steady_state_rss, r.total_queries
        ));
    }

    // H7 pass/fail, defined before this binary was written (see
    // PHASE7_LOG.md): a >=20% growth from t0/with-tenant snapshot to the
    // PEAK RSS observed while the process is still actually live and
    // serving (not a post-teardown reading, which thread-join can itself
    // reduce below the true in-service peak) is a material long-running-
    // process cost H6's instantaneous snapshot missed.
    let max_tenant_growth = tenant_growths.iter().copied().max().unwrap_or(0);
    let idle_growth_pct = if mean_idle_t0 > 0.0 {
        idle_growth / mean_idle_t0 * 100.0
    } else {
        0.0
    };
    println!(
        "\nH7 comparison: idle-resident peak growth={idle_growth:.1} KB ({idle_growth_pct:.1}% of t0), max active-resident tenant peak growth={max_tenant_growth} KB"
    );
    let material = idle_growth_pct.abs() >= 20.0 || max_tenant_growth >= 200;
    println!(
        "  H7 verdict: {}",
        if material {
            "CONFIRMED -- peak RSS while actually serving is materially higher than H6's immediate snapshot; a genuinely long-running service costs more than the short-lived-process floor"
        } else {
            "FALSIFIED -- peak RSS while serving stays close to the immediate snapshot; H6's short-lived-process floor is a reasonable proxy for this window's long-running cost"
        }
    );
    println!(
        "  (note: post-teardown RSS, printed above, is a distinct secondary signal about thread-join/shutdown behavior -- not used for this verdict, since a real long-running service does not tear down its worker pool mid-life)"
    );

    let artifacts_dir = PathBuf::from("docs/research/artifacts/p7_e04_long_running_run1");
    std::fs::create_dir_all(&artifacts_dir).ok();
    std::fs::write(artifacts_dir.join("results.csv"), &csv).ok();
    println!("\nartifacts written to {}", artifacts_dir.display());
}
