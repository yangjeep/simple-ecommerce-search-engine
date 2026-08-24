//! Phase 7 (Issue #21 Phase 7) P7-E02: packing ceiling via
//! controlled-stress tenant-count replication. See
//! `docs/experiments/PHASE7_LOG.md`'s "P7-E02" section for the
//! falsifiable H5 hypothesis stated before this binary was written.
//!
//! The real WANDS `category_depth_1` partition tops out at 55 tenants
//! (H3, left untested by P7-E00/P7-E01). This replicates the real
//! 55-tenant population K times -- each copy's tenants renamed with a
//! `-copyN` suffix to stay distinct -- to reach tenant counts in the
//! hundreds to thousands. Explicitly NOT a claim about organic tenant
//! growth (real independent SaaS tenants would not be K copies of the
//! same 55 real categories); this isolates tenant COUNT as a variable,
//! mirroring Phase 6B's disclosed controlled-stress-replication
//! methodology, applied here to tenant count rather than catalog size.
//!
//! Usage: cargo run --release -p phase7-eval --bin p7_e02_packing_ceiling
//!        [catalog.jsonl]

use std::path::PathBuf;
use std::time::Instant;

use commerce_core::domain::Catalog;
use commerce_core::index::CatalogIndex;
use phase7_eval::tenants::{partition_depth1, Order};

/// Safety cap on this process's OWN RSS (not system-wide) -- abort the
/// ladder before risking a real OOM in this container (~15 GB total,
/// ~10 GB available at the start of this experiment, Solr's JVM already
/// resident at ~4.8 GB).
const RSS_SAFETY_CAP_KB: u64 = 6 * 1024 * 1024; // 6 GB
const CHECK_EVERY_N_TENANTS: usize = 100;

fn current_rss_kb() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    status
        .lines()
        .find_map(|line| line.strip_prefix("VmRSS:"))
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(|kb| kb.parse().ok())
}

fn main() {
    let catalog_path = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("dataset_cache/wands/catalog.jsonl"));

    println!("=== P7-E02: packing ceiling via controlled-stress tenant-count replication ===");
    println!(
        "safety cap: this process's own RSS kept below {} MB",
        RSS_SAFETY_CAP_KB / 1024
    );

    let base = partition_depth1(&catalog_path, 55, Order::LargestFirst);
    let base_products: usize = base.iter().map(|(_, c)| c.products.len()).sum();
    println!(
        "real base population: {} tenants, {} products (1 replica)",
        base.len(),
        base_products
    );

    let rss_baseline_kb = current_rss_kb().unwrap_or(0);
    let mut csv = String::from(
        "tenant_count,replica_index,cumulative_products,rss_kb,linear_prediction_rss_kb,ratio_to_linear,build_ms_cumulative\n",
    );

    let mut cumulative_tenants = 0usize;
    let mut cumulative_products = 0usize;
    let mut built_indexes: Vec<CatalogIndex> = Vec::new();
    let build_start = Instant::now();
    let mut rss_at_replica0: Option<u64> = None;
    let mut aborted = false;

    'replicas: for replica in 0.. {
        for (name, catalog) in &base {
            let tenant_catalog = Catalog {
                products: catalog.products.clone(),
            };
            let index = CatalogIndex::build(&tenant_catalog);
            built_indexes.push(index);
            cumulative_tenants += 1;
            cumulative_products += tenant_catalog.products.len();
            let _ = name; // name only matters for real deployments' addressing, not this measurement

            if cumulative_tenants.is_multiple_of(CHECK_EVERY_N_TENANTS) {
                let rss = current_rss_kb().unwrap_or(0);
                let marginal_rss = rss.saturating_sub(rss_baseline_kb);
                let linear_prediction = if let Some(r0) = rss_at_replica0 {
                    r0 * (cumulative_products as f64 / base_products as f64).ceil() as u64
                } else {
                    marginal_rss
                };
                let ratio = if linear_prediction > 0 {
                    marginal_rss as f64 / linear_prediction as f64
                } else {
                    1.0
                };
                let build_ms = build_start.elapsed().as_secs_f64() * 1000.0;
                println!(
                    "  tenants={cumulative_tenants:<6} products={cumulative_products:<8} rss_kb={marginal_rss:<9} build_ms={build_ms:>9.1}"
                );
                csv.push_str(&format!(
                    "{cumulative_tenants},{replica},{cumulative_products},{marginal_rss},{linear_prediction},{ratio:.3},{build_ms:.2}\n"
                ));

                if rss > RSS_SAFETY_CAP_KB {
                    println!(
                        "  SAFETY CAP REACHED at {cumulative_tenants} tenants ({} MB > {} MB cap) -- stopping, not extrapolating further",
                        rss / 1024,
                        RSS_SAFETY_CAP_KB / 1024
                    );
                    aborted = true;
                    break 'replicas;
                }
            }
        }
        if replica == 0 {
            rss_at_replica0 = Some(
                current_rss_kb()
                    .unwrap_or(0)
                    .saturating_sub(rss_baseline_kb),
            );
        }
        // Hard stop regardless of safety cap once we reach a very large
        // replication factor, so a bug can't spin this forever.
        if replica >= 200 {
            println!("  reached replica cap (200x = {} tenants) without hitting the RSS safety cap -- stopping here", base.len() * 201);
            break;
        }
    }

    println!(
        "\nfinal: {cumulative_tenants} tenants, {cumulative_products} products built{}",
        if aborted {
            " (stopped early: RSS safety cap)"
        } else {
            " (stopped: replica cap, RSS safety cap never reached)"
        }
    );
    drop(built_indexes);

    let artifacts_dir = PathBuf::from("docs/research/artifacts/p7_e02_packing_ceiling_run1");
    std::fs::create_dir_all(&artifacts_dir).ok();
    std::fs::write(artifacts_dir.join("results.csv"), &csv).ok();
    println!("artifacts written to {}", artifacts_dir.display());
}
