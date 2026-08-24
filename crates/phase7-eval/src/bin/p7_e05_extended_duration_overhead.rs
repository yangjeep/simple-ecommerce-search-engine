//! Phase 7 (Issue #21 Phase 7) P7-E05: extended-duration resident
//! overhead. See `docs/experiments/PHASE7_LOG.md`'s "P7-E05" section for
//! the falsifiable H8 hypothesis stated before this binary was written.
//!
//! P7-E04/H7 found a real, materially-higher peak RSS for a process
//! actively serving real queries over a 20-second window than an
//! immediate post-load snapshot -- but Furniture's (the largest real
//! tenant) RSS curve was STILL RISING, decelerating but not fully
//! plateaued, at the end of that 20-second window. This binary asks the
//! obvious next question: does that curve plateau given a much longer
//! window, or does it keep climbing (which would suggest a genuine
//! unbounded leak rather than a bounded allocator/arena warm-up)?
//!
//! Reuses the exact same `phase7_eval::resident` sampling primitives
//! P7-E04 uses (idle-resident / active-resident, periodic RSS sampling),
//! just with a much longer duration and interval, focused on Furniture
//! (the only P7-E04 condition that hadn't plateaued) plus idle-resident
//! as a cheap comparison point (idle already plateaued within P7-E04's
//! first 5-second sample, so this is mostly a sanity re-check, not a new
//! question).
//!
//! Usage:
//!   orchestrator (default): cargo run --release -p phase7-eval
//!     --bin p7_e05_extended_duration_overhead -- [catalog.jsonl]
//!   child (invoked by the orchestrator itself, not normally run by hand):
//!     p7_e05_extended_duration_overhead child idle|active [catalog.jsonl] [tenant_name]

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

const WORKER_THREADS: usize = 4; // matches this container's real CPU count, same as P7-E04
const RUN_DURATION: Duration = Duration::from_secs(180); // 9x P7-E04's 20-second window
const SAMPLE_INTERVAL: Duration = Duration::from_secs(15);

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

/// H8 pass/fail, defined before this binary was written (see
/// PHASE7_LOG.md): compare growth in the FIRST HALF of the window
/// against growth in the SECOND HALF. If the second half's growth is
/// materially smaller than the first half's (say, less than half), the
/// curve is decelerating toward a plateau -- CONFIRMED (bounded
/// warm-up, not a leak). If the second half grows by a similar or
/// larger amount than the first half, the curve is not decelerating --
/// FALSIFIED (a genuine, more concerning open-ended growth pattern that
/// would need further investigation before any long-running deployment
/// claim could be trusted).
fn plateau_verdict(with_tenant: u64, samples: &[(u64, u64)]) -> (i64, i64, bool) {
    if samples.is_empty() {
        return (0, 0, true);
    }
    let midpoint = samples.len() / 2;
    let mid_kb = samples[midpoint.saturating_sub(1).min(samples.len() - 1)].1;
    let final_kb = samples.last().unwrap().1;
    let first_half_growth = mid_kb as i64 - with_tenant as i64;
    let second_half_growth = final_kb as i64 - mid_kb as i64;
    let decelerating = second_half_growth <= first_half_growth / 2 || second_half_growth <= 0;
    (first_half_growth, second_half_growth, decelerating)
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

    println!("=== P7-E05: extended-duration resident overhead (H8 plateau check) ===");
    println!(
        "RUN_DURATION={}s, WORKER_THREADS={WORKER_THREADS}, sampling every {}s",
        RUN_DURATION.as_secs(),
        SAMPLE_INTERVAL.as_secs()
    );

    println!("\nspawning 1 idle-resident bare process (no tenant data, {WORKER_THREADS} parked worker threads, {}s window)...", RUN_DURATION.as_secs());
    let idle = spawn_child(&exe, "idle", &catalog_path, None);
    let idle_peak = peak_kb(&idle.samples);
    println!(
        "  idle: t0={} KB  samples: {}  peak={idle_peak} KB",
        idle.baseline_rss,
        fmt_samples(&idle.samples)
    );
    let (idle_first_half, idle_second_half, idle_decelerating) =
        plateau_verdict(idle.baseline_rss, &idle.samples);
    println!(
        "  idle plateau check: first-half growth={idle_first_half} KB, second-half growth={idle_second_half} KB, decelerating={idle_decelerating}"
    );

    println!(
        "\nspawning 1 active-resident Furniture process ({}s of sustained real query serving)...",
        RUN_DURATION.as_secs()
    );
    let all = partition_depth1(&PathBuf::from(&catalog_path), 55, Order::LargestFirst);
    let furniture = &all[0].0;
    let single_tenant_path = write_single_tenant_jsonl(
        &PathBuf::from(&catalog_path),
        furniture,
        "p7_e05_single_tenant_tmp",
    );
    let r = spawn_child(
        &exe,
        "active",
        single_tenant_path.to_str().expect("valid utf8 path"),
        Some(furniture),
    );
    let with_tenant = r.with_tenant_rss.unwrap_or(r.baseline_rss);
    let peak = peak_kb(&r.samples);
    let growth = peak as i64 - with_tenant as i64;
    println!(
        "  tenant={furniture:?} products={:<7} t0_rss_kb={:<8} with_tenant_rss_kb={with_tenant:<8} peak_serving_rss_kb={peak:<8} peak_growth_kb={growth:<8} total_queries_served={}  samples: {}",
        r.products, r.baseline_rss, r.total_queries, fmt_samples(&r.samples)
    );
    let (first_half, second_half, decelerating) = plateau_verdict(with_tenant, &r.samples);
    println!(
        "  Furniture plateau check: first-half growth={first_half} KB, second-half growth={second_half} KB, decelerating={decelerating}"
    );

    println!(
        "\nH8 verdict: {}",
        if decelerating {
            "CONFIRMED -- Furniture's RSS growth decelerates materially in the window's second half; consistent with a bounded allocator/arena warm-up, not an open-ended leak"
        } else {
            "FALSIFIED -- Furniture's RSS growth does NOT decelerate in the window's second half; this is a more concerning, unresolved pattern that would need further investigation (e.g. a profiler) before trusting a long-running deployment's memory footprint to stabilize"
        }
    );
    println!(
        "  (idle-resident included as a cheap comparison point only -- it already plateaued within P7-E04's first 5-second sample, so it is not the focus of this experiment)"
    );

    let mut csv = String::from(
        "condition,tenant_name,tenant_products,t0_rss_kb,with_tenant_rss_kb,peak_serving_rss_kb,peak_growth_kb,first_half_growth_kb,second_half_growth_kb,decelerating,total_queries_served\n",
    );
    csv.push_str(&format!(
        "idle_resident,,,{},,{idle_peak},{},{idle_first_half},{idle_second_half},{idle_decelerating},\n",
        idle.baseline_rss,
        idle_peak as i64 - idle.baseline_rss as i64
    ));
    csv.push_str(&format!(
        "active_resident,{furniture},{},{},{with_tenant},{peak},{growth},{first_half},{second_half},{decelerating},{}\n",
        r.products, r.baseline_rss, r.total_queries
    ));

    let artifacts_dir = PathBuf::from("docs/research/artifacts/p7_e05_extended_duration_run1");
    std::fs::create_dir_all(&artifacts_dir).ok();
    std::fs::write(artifacts_dir.join("results.csv"), &csv).ok();
    println!("\nartifacts written to {}", artifacts_dir.display());
}
