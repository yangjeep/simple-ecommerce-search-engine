//! Phase 7 (Issue #21 Phase 7) P7-E00: first multi-tenant packing-density
//! measurement. Builds up to 55 real, independently-sized WANDS
//! `category_depth_1` tenant catalogs in one process (see
//! `docs/experiments/PHASE7_LOG.md` for the tenant model and the
//! falsifiable H1/H2/H3 hypotheses stated before this binary was
//! written).
//!
//! Usage: cargo run --release -p phase7-eval --bin p7_e00_tenant_packing
//!        [catalog.jsonl]

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use commerce_core::index::CatalogIndex;
use phase7_eval::tenants::{load_depth1_tenants, partition_depth1, Tenant};

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

fn main() {
    let catalog_path = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("dataset_cache/wands/catalog.jsonl"));

    println!("=== P7-E00: tenant packing density ===");
    println!("loading real WANDS catalog and partitioning by category_depth_1...");

    // ---- H1: per-tenant RSS/build-time amortization ----
    // Partition first (no indexes built yet), then build each tenant's
    // CatalogIndex ONE AT A TIME, snapshotting RSS after each build, so
    // the marginal-RSS curve reflects genuine incremental construction
    // rather than measuring after everything was already materialized.
    let rss_baseline_kb = current_rss_kb().unwrap_or(0);
    let partitions = partition_depth1(&catalog_path, 55);
    let n_available = partitions.len();
    println!(
        "{n_available} real distinct category_depth_1 tenants available (real long-tail sizes, largest first)"
    );

    let mut csv_h1 = String::from(
        "tenant_count,tenant_name,tenant_products,cumulative_products,cumulative_rss_kb,rss_kb_per_tenant,build_ms_cumulative\n",
    );
    let mut cumulative_products = 0usize;
    let mut built_indexes: Vec<CatalogIndex> = Vec::with_capacity(n_available);
    let build_start = Instant::now();
    for (i, (name, catalog)) in partitions.iter().enumerate() {
        cumulative_products += catalog.products.len();
        built_indexes.push(CatalogIndex::build(catalog));
        let n = i + 1;
        if TENANT_CHECKPOINTS.contains(&n) || n == n_available {
            let rss = current_rss_kb().unwrap_or(0);
            let marginal_rss = rss.saturating_sub(rss_baseline_kb);
            let rss_per_tenant = marginal_rss as f64 / n as f64;
            let build_ms = build_start.elapsed().as_secs_f64() * 1000.0;
            println!(
                "  N={n:<3} cumulative_products={cumulative_products:<7} rss_kb={rss:<9} marginal_rss_kb={marginal_rss:<9} rss_kb/tenant={rss_per_tenant:>8.1} build_ms={build_ms:>8.1}"
            );
            csv_h1.push_str(&format!(
                "{n},{},{},{cumulative_products},{marginal_rss},{rss_per_tenant:.2},{build_ms:.2}\n",
                name.replace(',', ";"),
                catalog.products.len()
            ));
        }
    }
    // Keep the built indexes alive until after RSS has been reported for
    // the final checkpoint (dropping early would free memory and make
    // later snapshots meaningless); they are no longer needed for H2,
    // which builds its own two tenants fresh below.
    drop(built_indexes);

    let artifacts_dir = PathBuf::from("docs/research/artifacts/p7_e00_tenant_packing_run1");
    std::fs::create_dir_all(&artifacts_dir).ok();
    std::fs::write(artifacts_dir.join("h1_rss_amortization.csv"), &csv_h1).ok();

    // ---- H2: cross-tenant isolation (noisy-neighbor check) ----
    // Loaded tenant = the single largest real tenant (index 0, since
    // tenants_data is sorted largest-first); quiet tenant = "Rugs" if
    // present (continuity with Phase 6A/6B's own checkpoint), else the
    // smallest available. H2 only needs two tenants, so build them
    // fresh here via the eager loader rather than reusing H1's
    // already-dropped incremental build.
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

    fn facet_scan_once(
        index: &commerce_core::index::CatalogIndex,
        catalog: &commerce_core::domain::Catalog,
    ) -> usize {
        let all = index.indexed_candidates(&[]);
        index.facet_counts_by_scan(&all, catalog, "color").len()
    }

    // Baseline: quiet tenant alone, no concurrent load.
    let mut baseline_ns = Vec::with_capacity(ISOLATION_REPS);
    for _ in 0..ISOLATION_REPS {
        let start = Instant::now();
        std::hint::black_box(facet_scan_once(&quiet_index, &quiet_catalog));
        baseline_ns.push(start.elapsed().as_nanos());
    }
    baseline_ns.sort_unstable();

    // Loaded: quiet tenant measured while the loaded tenant is hammered
    // concurrently on all remaining real CPUs (this container has 4).
    let stop = Arc::new(AtomicBool::new(false));
    let noisy_workers = 3usize;
    let mut noisy_handles = Vec::new();
    for _ in 0..noisy_workers {
        let loaded_index = Arc::clone(&loaded_index);
        let loaded_catalog = Arc::clone(&loaded_catalog);
        let stop = Arc::clone(&stop);
        noisy_handles.push(std::thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                std::hint::black_box(facet_scan_once(&loaded_index, &loaded_catalog));
            }
        }));
    }
    // Let the noisy load ramp up before measuring.
    std::thread::sleep(Duration::from_millis(200));

    let mut loaded_ns = Vec::with_capacity(ISOLATION_REPS);
    let deadline = Instant::now() + ISOLATION_RUN_DURATION;
    while loaded_ns.len() < ISOLATION_REPS && Instant::now() < deadline {
        let start = Instant::now();
        std::hint::black_box(facet_scan_once(&quiet_index, &quiet_catalog));
        loaded_ns.push(start.elapsed().as_nanos());
    }
    stop.store(true, Ordering::Relaxed);
    for h in noisy_handles {
        h.join().unwrap();
    }
    loaded_ns.sort_unstable();

    let baseline_p50 = percentile_ms(&baseline_ns, 0.5);
    let baseline_p99 = percentile_ms(&baseline_ns, 0.99);
    let loaded_p50 = percentile_ms(&loaded_ns, 0.5);
    let loaded_p99 = percentile_ms(&loaded_ns, 0.99);
    let p50_ratio = if baseline_p50 > 0.0 {
        loaded_p50 / baseline_p50
    } else {
        f64::INFINITY
    };
    let p99_ratio = if baseline_p99 > 0.0 {
        loaded_p99 / baseline_p99
    } else {
        f64::INFINITY
    };

    println!(
        "  quiet tenant alone:              p50={baseline_p50:.4}ms p99={baseline_p99:.4}ms (n={})",
        baseline_ns.len()
    );
    println!(
        "  quiet tenant w/ {noisy_workers} noisy threads: p50={loaded_p50:.4}ms p99={loaded_p99:.4}ms (n={}) -- p50 ratio={p50_ratio:.2}x p99 ratio={p99_ratio:.2}x",
        loaded_ns.len()
    );
    println!(
        "  H2 verdict: {}",
        if p99_ratio < 2.0 {
            "PASS -- no material (< 2x) p99 regression under concurrent cross-tenant load"
        } else {
            "FAIL -- material (>= 2x) p99 regression, isolation not demonstrated"
        }
    );

    let csv_h2 = format!(
        "condition,p50_ms,p99_ms,n\nquiet_alone,{baseline_p50:.4},{baseline_p99:.4},{}\nquiet_with_noisy_neighbor,{loaded_p50:.4},{loaded_p99:.4},{}\n",
        baseline_ns.len(),
        loaded_ns.len()
    );
    std::fs::write(artifacts_dir.join("h2_isolation.csv"), csv_h2).ok();

    println!("\nartifacts written to {}", artifacts_dir.display());
}
