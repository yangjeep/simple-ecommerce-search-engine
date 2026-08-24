//! Phase 7 (Issue #21 Phase 7) P7-E00: first multi-tenant packing-density
//! measurement. Builds up to 55 real, independently-sized WANDS
//! `category_depth_1` tenant catalogs in one process (see
//! `docs/experiments/PHASE7_LOG.md` for the tenant model and the
//! falsifiable H1/H2/H3 hypotheses stated before this binary was
//! written).
//!
//! Revised after adversarial review (see PHASE7_LOG.md): (1) the RSS
//! baseline is now captured AFTER partitioning/loading, isolating
//! per-tenant indexing cost from the one-time whole-catalog parse; (2)
//! `CatalogIndex::approximate_size_bytes()` -- a deterministic,
//! allocator-noise-free instrument already in commerce_core -- is now
//! recorded alongside RSS at every checkpoint; (3) an `order` CLI arg
//! (`forward`/`reversed`) supports an order-confound control, mirroring
//! Phase 6B's reversed-execution-order check; (4) H1's intermediate
//! state is explicitly dropped before H2 runs, removing a real (if
//! non-biasing) memory-bloat confound; (5) H2 now includes a same-tenant
//! control condition, so the isolation claim can be attributed to tenant
//! separation rather than generic CPU/memory contention.
//!
//! Usage: cargo run --release -p phase7-eval --bin p7_e00_tenant_packing
//!        [catalog.jsonl] [forward|reversed]

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use commerce_core::domain::Catalog;
use commerce_core::index::CatalogIndex;
use phase7_eval::tenants::{load_depth1_tenants, partition_depth1, Order, Tenant};

const TENANT_CHECKPOINTS: &[usize] = &[1, 5, 10, 25, 55];
const ISOLATION_RUN_DURATION: Duration = Duration::from_secs(5);
const ISOLATION_REPS: usize = 500;

fn current_rss_kb() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    status
        .lines()
        .find_map(|line| line.strip_prefix("VmRSS:"))
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(|kb| kb.parse().ok())
}

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

/// Run the quiet tenant's facet scan alone (no concurrent load), then
/// again while `noisy_workers` threads hammer `hammer_index`/
/// `hammer_catalog` (either the quiet tenant's OWN data -- the
/// same-tenant control -- or a different tenant's data -- the
/// cross-tenant condition). Returns (baseline_ns_sorted, loaded_ns_sorted).
fn measure_isolation(
    quiet_index: &Arc<CatalogIndex>,
    quiet_catalog: &Arc<Catalog>,
    hammer_index: &Arc<CatalogIndex>,
    hammer_catalog: &Arc<Catalog>,
    noisy_workers: usize,
) -> (Vec<u128>, Vec<u128>) {
    let mut baseline_ns = Vec::with_capacity(ISOLATION_REPS);
    for _ in 0..ISOLATION_REPS {
        let start = Instant::now();
        std::hint::black_box(facet_scan_once(quiet_index, quiet_catalog));
        baseline_ns.push(start.elapsed().as_nanos());
    }
    baseline_ns.sort_unstable();

    let stop = Arc::new(AtomicBool::new(false));
    let mut noisy_handles = Vec::new();
    for _ in 0..noisy_workers {
        let hammer_index = Arc::clone(hammer_index);
        let hammer_catalog = Arc::clone(hammer_catalog);
        let stop = Arc::clone(&stop);
        noisy_handles.push(std::thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                std::hint::black_box(facet_scan_once(&hammer_index, &hammer_catalog));
            }
        }));
    }
    std::thread::sleep(Duration::from_millis(200));

    let mut loaded_ns = Vec::with_capacity(ISOLATION_REPS);
    let deadline = Instant::now() + ISOLATION_RUN_DURATION;
    while loaded_ns.len() < ISOLATION_REPS && Instant::now() < deadline {
        let start = Instant::now();
        std::hint::black_box(facet_scan_once(quiet_index, quiet_catalog));
        loaded_ns.push(start.elapsed().as_nanos());
    }
    stop.store(true, Ordering::Relaxed);
    for h in noisy_handles {
        h.join().unwrap();
    }
    loaded_ns.sort_unstable();
    (baseline_ns, loaded_ns)
}

fn main() {
    let mut args = std::env::args().skip(1);
    let catalog_path = args
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("dataset_cache/wands/catalog.jsonl"));
    let order = match args.next().as_deref() {
        Some("reversed") => Order::SmallestFirst,
        _ => Order::LargestFirst,
    };
    let order_label = match order {
        Order::LargestFirst => "forward",
        Order::SmallestFirst => "reversed",
    };

    println!("=== P7-E00: tenant packing density (order={order_label}) ===");
    println!("loading real WANDS catalog and partitioning by category_depth_1...");

    // ---- H1: per-tenant RSS/build-time amortization ----
    // Baseline RSS is captured AFTER partitioning/loading (not before),
    // so the marginal RSS reported reflects only per-tenant INDEXING
    // cost, not the one-time whole-catalog parse (a real confound found
    // by adversarial review: the previous baseline point made N=1's
    // number include the full 42,994-product parse, which doesn't
    // replicate in a real multi-tenant setting where each tenant is
    // ingested separately).
    let partitions = partition_depth1(&catalog_path, 55, order);
    let rss_baseline_kb = current_rss_kb().unwrap_or(0);
    let n_available = partitions.len();
    println!(
        "{n_available} real distinct category_depth_1 tenants available (order={order_label})"
    );

    let mut csv_h1 = String::from(
        "order,tenant_count,tenant_name,tenant_products,cumulative_products,marginal_rss_kb,rss_kb_per_tenant,cumulative_index_bytes,index_bytes_per_tenant,build_ms_cumulative\n",
    );
    let mut cumulative_products = 0usize;
    let mut cumulative_index_bytes = 0usize;
    let mut built_indexes: Vec<CatalogIndex> = Vec::with_capacity(n_available);
    let build_start = Instant::now();
    for (i, (name, catalog)) in partitions.iter().enumerate() {
        cumulative_products += catalog.products.len();
        let index = CatalogIndex::build(catalog);
        cumulative_index_bytes += index.approximate_size_bytes();
        built_indexes.push(index);
        let n = i + 1;
        if TENANT_CHECKPOINTS.contains(&n) || n == n_available {
            let rss = current_rss_kb().unwrap_or(0);
            let marginal_rss = rss.saturating_sub(rss_baseline_kb);
            let rss_per_tenant = marginal_rss as f64 / n as f64;
            let index_bytes_per_tenant = cumulative_index_bytes as f64 / n as f64;
            let build_ms = build_start.elapsed().as_secs_f64() * 1000.0;
            println!(
                "  N={n:<3} cumulative_products={cumulative_products:<7} marginal_rss_kb={marginal_rss:<9} rss_kb/tenant={rss_per_tenant:>9.1} cumulative_index_bytes={cumulative_index_bytes:<10} index_bytes/tenant={index_bytes_per_tenant:>9.1} build_ms={build_ms:>8.1}"
            );
            csv_h1.push_str(&format!(
                "{order_label},{n},{},{},{cumulative_products},{marginal_rss},{rss_per_tenant:.2},{cumulative_index_bytes},{index_bytes_per_tenant:.2},{build_ms:.2}\n",
                name.replace(',', ";"),
                catalog.products.len()
            ));
        }
    }
    // Drop everything H1 built before H2 runs -- a real (if non-biasing)
    // memory-bloat confound adversarial review found: `partitions` and
    // `built_indexes` would otherwise stay resident (reachable) for the
    // rest of the process, so H2's absolute latency numbers would be
    // measured in a needlessly memory-bloated process.
    drop(built_indexes);
    drop(partitions);

    let artifacts_dir = PathBuf::from("docs/research/artifacts/p7_e00_tenant_packing_run1");
    std::fs::create_dir_all(&artifacts_dir).ok();
    let h1_path = artifacts_dir.join(format!("h1_rss_amortization_{order_label}.csv"));
    std::fs::write(&h1_path, &csv_h1).ok();

    // ---- H2: cross-tenant isolation (noisy-neighbor check) ----
    // Only run H2 for the forward (default) invocation -- it is
    // independent of the H1 order question and doesn't need to be
    // repeated per order.
    if order != Order::LargestFirst {
        println!("\n(order=reversed run: H1 only, skipping H2)");
        println!("\nartifacts written to {}", h1_path.display());
        return;
    }

    let mut tenants_data: Vec<Tenant> = load_depth1_tenants(&catalog_path, 55);
    let loaded_idx = 0usize;
    let quiet_idx = tenants_data
        .iter()
        .position(|t| t.name == "Rugs")
        .unwrap_or(tenants_data.len() - 1);
    assert_ne!(
        loaded_idx, quiet_idx,
        "loaded and quiet tenants must be distinct for a real isolation test"
    );

    let quiet_tenant: Tenant = tenants_data.remove(quiet_idx);
    let loaded_tenant: Tenant = tenants_data.remove(if loaded_idx < quiet_idx {
        loaded_idx
    } else {
        loaded_idx - 1
    });
    // Drop the other 53 built-but-unused tenants immediately -- the
    // other half of the same memory-bloat confound H1 had.
    drop(tenants_data);

    println!(
        "\nH2 isolation check: loaded tenant={:?} ({} products), quiet tenant={:?} ({} products)",
        loaded_tenant.name,
        loaded_tenant.catalog.products.len(),
        quiet_tenant.name,
        quiet_tenant.catalog.products.len()
    );

    let loaded_index = Arc::new(loaded_tenant.index);
    let loaded_catalog = Arc::new(loaded_tenant.catalog);
    let quiet_index = Arc::new(quiet_tenant.index);
    let quiet_catalog = Arc::new(quiet_tenant.catalog);
    let noisy_workers = 3usize;

    // Cross-tenant condition: noisy threads hammer a DIFFERENT tenant's
    // data while the quiet tenant is measured.
    let (baseline_ns, cross_tenant_ns) = measure_isolation(
        &quiet_index,
        &quiet_catalog,
        &loaded_index,
        &loaded_catalog,
        noisy_workers,
    );

    // Same-tenant control: noisy threads hammer the QUIET tenant's OWN
    // data instead. If this degrades the quiet tenant's own latency by
    // roughly the same amount as the cross-tenant condition, the result
    // is explained by generic CPU/memory contention, not a tenant-
    // separation property -- added after adversarial review found the
    // original design couldn't distinguish the two.
    let (baseline_ns_2, same_tenant_ns) = measure_isolation(
        &quiet_index,
        &quiet_catalog,
        &quiet_index,
        &quiet_catalog,
        noisy_workers,
    );

    let baseline_p50 = percentile_ms(&baseline_ns, 0.5);
    let baseline_p99 = percentile_ms(&baseline_ns, 0.99);
    let baseline2_p50 = percentile_ms(&baseline_ns_2, 0.5);
    let baseline2_p99 = percentile_ms(&baseline_ns_2, 0.99);
    let cross_p50 = percentile_ms(&cross_tenant_ns, 0.5);
    let cross_p99 = percentile_ms(&cross_tenant_ns, 0.99);
    let same_p50 = percentile_ms(&same_tenant_ns, 0.5);
    let same_p99 = percentile_ms(&same_tenant_ns, 0.99);

    let cross_p99_ratio = if baseline_p99 > 0.0 {
        cross_p99 / baseline_p99
    } else {
        f64::INFINITY
    };
    let same_p99_ratio = if baseline2_p99 > 0.0 {
        same_p99 / baseline2_p99
    } else {
        f64::INFINITY
    };

    println!(
        "  quiet alone (run 1):                  p50={baseline_p50:.4}ms p99={baseline_p99:.4}ms (n={})",
        baseline_ns.len()
    );
    println!(
        "  quiet + {noisy_workers} threads hammering LOADED tenant (cross-tenant): p50={cross_p50:.4}ms p99={cross_p99:.4}ms (n={}) -- p99 ratio={cross_p99_ratio:.2}x",
        cross_tenant_ns.len()
    );
    println!(
        "  quiet alone (run 2):                  p50={baseline2_p50:.4}ms p99={baseline2_p99:.4}ms (n={})",
        baseline_ns_2.len()
    );
    println!(
        "  quiet + {noisy_workers} threads hammering QUIET's OWN tenant (same-tenant control): p50={same_p50:.4}ms p99={same_p99:.4}ms (n={}) -- p99 ratio={same_p99_ratio:.2}x",
        same_tenant_ns.len()
    );
    println!(
        "  H2 verdict: cross-tenant p99 ratio={cross_p99_ratio:.2}x vs same-tenant-control p99 ratio={same_p99_ratio:.2}x -- {}",
        if cross_p99_ratio <= same_p99_ratio * 1.2 {
            "cross-tenant degradation is NOT materially worse than same-tenant contention -- consistent with generic CPU/memory contention, not a tenant-separation-specific effect"
        } else {
            "cross-tenant degradation IS materially worse than same-tenant contention -- some tenant-boundary-specific cost may exist beyond generic contention"
        }
    );

    let csv_h2 = format!(
        "condition,p50_ms,p99_ms,n\n\
         quiet_alone_run1,{baseline_p50:.4},{baseline_p99:.4},{}\n\
         quiet_with_cross_tenant_load,{cross_p50:.4},{cross_p99:.4},{}\n\
         quiet_alone_run2,{baseline2_p50:.4},{baseline2_p99:.4},{}\n\
         quiet_with_same_tenant_control_load,{same_p50:.4},{same_p99:.4},{}\n",
        baseline_ns.len(),
        cross_tenant_ns.len(),
        baseline_ns_2.len(),
        same_tenant_ns.len(),
    );
    std::fs::write(artifacts_dir.join("h2_isolation.csv"), csv_h2).ok();

    println!("\nartifacts written to {}", artifacts_dir.display());
}
