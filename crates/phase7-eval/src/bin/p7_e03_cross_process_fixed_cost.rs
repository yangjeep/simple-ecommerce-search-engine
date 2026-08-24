//! Phase 7 (Issue #21 Phase 7) P7-E03: cross-process fixed cost. See
//! `docs/experiments/PHASE7_LOG.md`'s "P7-E03" section for the
//! falsifiable H6 hypothesis stated before this binary was written.
//!
//! Every prior Phase 7 measurement (P7-E00/E01/E02) is IN-PROCESS. This
//! binary spawns actual separate OS processes (via `std::process::Command`,
//! running this same compiled binary in `child` mode) to measure the real
//! per-process baseline overhead a one-process-per-tenant deployment
//! model would pay once PER TENANT -- the first Phase 7 measurement of
//! the cost pooling (H1/H5's in-process design) actually avoids.
//!
//! Usage:
//!   orchestrator (default): cargo run --release -p phase7-eval
//!     --bin p7_e03_cross_process_fixed_cost -- [catalog.jsonl]
//!   child (invoked by the orchestrator itself, not normally run by hand):
//!     p7_e03_cross_process_fixed_cost child [catalog.jsonl] [tenant_name]

use std::path::PathBuf;
use std::process::Command;

use phase7_eval::tenants::{
    load_single_tenant, partition_depth1, write_single_tenant_jsonl, Order,
};

const BASELINE_CHILD_COUNT: usize = 20;

fn current_rss_kb() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    status
        .lines()
        .find_map(|line| line.strip_prefix("VmRSS:"))
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(|kb| kb.parse().ok())
}

/// Child-mode entry point: reports this PROCESS's own RSS as early as
/// possible (before touching any tenant data), then -- only if a tenant
/// name was given -- loads that one real tenant's catalog and reports
/// RSS again, so the orchestrator can separate pure process/runtime
/// baseline from one tenant's actual data cost.
fn run_child(catalog_path: &std::path::Path, tenant_name: Option<&str>) {
    let baseline_rss = current_rss_kb().unwrap_or(0);
    println!("baseline_rss_kb={baseline_rss}");

    if let Some(name) = tenant_name {
        let catalog = load_single_tenant(catalog_path, name);
        let products = catalog.products.len();
        let index = commerce_core::index::CatalogIndex::build(&catalog);
        std::hint::black_box(&index);
        let with_tenant_rss = current_rss_kb().unwrap_or(0);
        println!("tenant_name={name}");
        println!("tenant_products={products}");
        println!("with_tenant_rss_kb={with_tenant_rss}");
    }
}

fn spawn_child(
    exe: &str,
    catalog_path: &str,
    tenant_name: Option<&str>,
) -> (u64, Option<u64>, usize) {
    let mut cmd = Command::new(exe);
    cmd.arg("child").arg(catalog_path);
    if let Some(name) = tenant_name {
        cmd.arg(name);
    }
    let output = cmd.output().expect("failed to spawn child process");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut baseline = 0u64;
    let mut with_tenant: Option<u64> = None;
    let mut products = 0usize;
    for line in stdout.lines() {
        if let Some(v) = line.strip_prefix("baseline_rss_kb=") {
            baseline = v.parse().unwrap_or(0);
        } else if let Some(v) = line.strip_prefix("with_tenant_rss_kb=") {
            with_tenant = v.parse().ok();
        } else if let Some(v) = line.strip_prefix("tenant_products=") {
            products = v.parse().unwrap_or(0);
        }
    }
    (baseline, with_tenant, products)
}

fn main() {
    let mut args = std::env::args().skip(1);
    let first = args.next();

    if first.as_deref() == Some("child") {
        let catalog_path = args
            .next()
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("dataset_cache/wands/catalog.jsonl"));
        let tenant_name = args.next();
        run_child(&catalog_path, tenant_name.as_deref());
        return;
    }

    let catalog_path = first.unwrap_or_else(|| "dataset_cache/wands/catalog.jsonl".to_string());
    let exe = std::env::current_exe()
        .expect("current_exe")
        .to_string_lossy()
        .to_string();

    println!("=== P7-E03: cross-process fixed cost ===");
    println!("spawning {BASELINE_CHILD_COUNT} bare child processes (no tenant data) to measure pure process/runtime baseline...");

    let mut baselines = Vec::with_capacity(BASELINE_CHILD_COUNT);
    for _ in 0..BASELINE_CHILD_COUNT {
        let (baseline, _, _) = spawn_child(&exe, &catalog_path, None);
        baselines.push(baseline);
    }
    baselines.sort_unstable();
    let mean_baseline: f64 = baselines.iter().sum::<u64>() as f64 / baselines.len() as f64;
    let min_baseline = baselines.first().copied().unwrap_or(0);
    let max_baseline = baselines.last().copied().unwrap_or(0);
    println!(
        "  bare process RSS: mean={mean_baseline:.1} KB min={min_baseline} KB max={max_baseline} KB (n={})",
        baselines.len()
    );

    println!("\nspawning children with one real tenant's data each (smallest/medium/largest)...");
    let all = partition_depth1(&PathBuf::from(&catalog_path), 55, Order::LargestFirst);
    let largest = &all[0].0;
    let mid = &all[all.len() / 2].0;
    let smallest = &all[all.len() - 1].0;
    let mut csv = String::from(
        "condition,tenant_name,tenant_products,baseline_rss_kb,with_tenant_rss_kb,tenant_marginal_rss_kb\n",
    );
    csv.push_str(&format!("bare_process_mean,,,{mean_baseline:.1},,\n"));

    for name in [largest.as_str(), mid.as_str(), smallest.as_str()] {
        let single_tenant_path = write_single_tenant_jsonl(
            &PathBuf::from(&catalog_path),
            name,
            "p7_e03_single_tenant_tmp",
        );
        let (baseline, with_tenant, products) = spawn_child(
            &exe,
            single_tenant_path.to_str().expect("valid utf8 path"),
            Some(name),
        );
        let with_tenant = with_tenant.unwrap_or(baseline);
        let tenant_marginal = with_tenant.saturating_sub(baseline);
        println!(
            "  tenant={name:?} products={products:<7} baseline_rss_kb={baseline:<8} with_tenant_rss_kb={with_tenant:<8} tenant_marginal_rss_kb={tenant_marginal}"
        );
        csv.push_str(&format!(
            "one_tenant_process,{name},{products},{baseline},{with_tenant},{tenant_marginal}\n"
        ));
    }

    // The economic comparison: H1/H5 already established in-process
    // pooled marginal cost at ~1.26 KB/product with ~0 KB fixed cost per
    // near-empty tenant. Compare that against this process's own bare
    // baseline to quantify what a one-process-per-tenant model would pay
    // per tenant that pooling avoids.
    const IN_PROCESS_KB_PER_PRODUCT: f64 = 1.263; // from P7-E02/H5
    println!(
        "\nH6 comparison: bare per-process baseline ({mean_baseline:.1} KB) vs in-process pooled marginal cost (~{IN_PROCESS_KB_PER_PRODUCT} KB/product, ~0 KB fixed per near-empty tenant, from H1/H5)"
    );
    let ratio_vs_1000_products = mean_baseline / (IN_PROCESS_KB_PER_PRODUCT * 1000.0);
    println!(
        "  the bare per-process baseline alone equals the in-process marginal cost of ~{:.0} products worth of pooled tenant data",
        mean_baseline / IN_PROCESS_KB_PER_PRODUCT
    );
    println!(
        "  (equivalently: ~{ratio_vs_1000_products:.2}x the pooled marginal cost of a 1,000-product tenant)"
    );
    println!(
        "  H6 verdict: {}",
        if mean_baseline > IN_PROCESS_KB_PER_PRODUCT * 1000.0 {
            "CONFIRMED -- per-process baseline is larger than a 1,000-product tenant's entire in-process marginal cost; process-per-tenant isolation has a real, quantifiable fixed cost pooling avoids"
        } else {
            "FALSIFIED -- per-process baseline is smaller than a 1,000-product tenant's in-process marginal cost; process isolation would carry no material fixed-cost penalty here"
        }
    );

    let artifacts_dir = PathBuf::from("docs/research/artifacts/p7_e03_cross_process_run1");
    std::fs::create_dir_all(&artifacts_dir).ok();
    std::fs::write(artifacts_dir.join("results.csv"), &csv).ok();
    println!("\nartifacts written to {}", artifacts_dir.display());
}
