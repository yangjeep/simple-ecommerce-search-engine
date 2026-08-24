//! Phase 7 (Issue #21 Phase 7) P7-E11: high-churn tenant impact on
//! low-churn tenants -- Issue #21 explicitly names this as a required
//! Phase 7 experiment; no prior Phase 7 experiment has tested any
//! mutation/churn workload (every one of them used static, once-built
//! catalogs). See `docs/experiments/PHASE7_LOG.md`'s "P7-E11" section
//! for the falsifiable H14 hypothesis stated before this binary was
//! written.
//!
//! This project's architecture uses immutable tenant `CatalogIndex`
//! bundles (Issue #21's own Phase 9 "immutable tenant structural
//! bundle" concept) -- there is no in-place mutation API. So "churn"
//! here means repeated REBUILDS of a tenant's index (simulating real
//! commerce price/inventory update cycles that would, in this
//! architecture, produce a new immutable bundle to hot-swap in), not
//! an incremental update.
//!
//! Reuses H2's own exact quiet-tenant methodology (`p7_e00_tenant_
//! packing.rs`'s `measure_isolation`, same `ISOLATION_REPS`/
//! `ISOLATION_RUN_DURATION`, same quiet tenant "Rugs") so this result
//! is directly comparable to H2's own already-established "no material
//! cross-tenant degradation from noisy QUERY load" finding -- this
//! experiment asks the same question for CHURN load instead.
//!
//! Usage: cargo run --release -p phase7-eval --bin p7_e11_high_churn_impact
//!        [catalog.jsonl]

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use commerce_core::index::CatalogIndex;
use phase7_eval::resident::facet_scan_once;
use phase7_eval::tenants::load_depth1_tenants;

const ISOLATION_RUN_DURATION: Duration = Duration::from_secs(5); // matches H2/P7-E00
const ISOLATION_REPS: usize = 500; // matches H2/P7-E00

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

    println!("=== P7-E11: high-churn tenant impact on low-churn tenants (H14) ===");
    println!("(quiet/low-churn tenant = 'Rugs'; high-churn tenant = 'Furniture', repeatedly rebuilt and hot-swapped)");

    let mut tenants = load_depth1_tenants(&catalog_path, 55);
    let quiet_pos = tenants
        .iter()
        .position(|t| t.name == "Rugs")
        .expect("Rugs must be present");
    let churn_pos = tenants
        .iter()
        .position(|t| t.name == "Furniture")
        .expect("Furniture must be present");
    assert_ne!(quiet_pos, churn_pos, "quiet and churn tenants must differ");

    let quiet_tenant = tenants.remove(quiet_pos);
    // churn_pos shifts by one if it was after quiet_pos.
    let churn_pos = if churn_pos > quiet_pos {
        churn_pos - 1
    } else {
        churn_pos
    };
    let churn_tenant = tenants.remove(churn_pos);
    // Drop the other 53 built-but-unused tenants immediately, matching
    // H2's own memory-bloat-confound discipline.
    drop(tenants);

    println!(
        "quiet tenant={:?} ({} products); churn tenant={:?} ({} products)",
        quiet_tenant.name,
        quiet_tenant.catalog.products.len(),
        churn_tenant.name,
        churn_tenant.catalog.products.len()
    );

    let quiet_index = Arc::new(quiet_tenant.index);
    let quiet_catalog = Arc::new(quiet_tenant.catalog);
    let churn_catalog = Arc::new(churn_tenant.catalog);

    // ---- Baseline: quiet tenant queried alone, no churn activity ----
    let mut baseline_ns = Vec::with_capacity(ISOLATION_REPS);
    for _ in 0..ISOLATION_REPS {
        let start = Instant::now();
        std::hint::black_box(facet_scan_once(&quiet_index, &quiet_catalog));
        baseline_ns.push(start.elapsed().as_nanos());
    }
    baseline_ns.sort_unstable();

    // ---- Treatment: quiet tenant queried while a separate thread
    // continuously rebuilds the churn tenant's CatalogIndex from its
    // (unchanging) Catalog and hot-swaps it into a Mutex<Arc<...>> --
    // real allocation/deallocation churn, not just re-reading the same
    // built index, simulating a real "new immutable bundle" swap-in. ----
    let churn_slot: Arc<Mutex<Arc<CatalogIndex>>> =
        Arc::new(Mutex::new(Arc::new(CatalogIndex::build(&churn_catalog))));
    let stop = Arc::new(AtomicBool::new(false));
    let churn_count = Arc::new(AtomicU64::new(0));
    {
        let churn_catalog = Arc::clone(&churn_catalog);
        let churn_slot = Arc::clone(&churn_slot);
        let stop = Arc::clone(&stop);
        let churn_count = Arc::clone(&churn_count);
        std::thread::spawn(move || {
            let mut count = 0u64;
            while !stop.load(Ordering::Relaxed) {
                let rebuilt = Arc::new(CatalogIndex::build(&churn_catalog));
                *churn_slot.lock().unwrap() = rebuilt;
                count += 1;
            }
            churn_count.store(count, Ordering::Relaxed);
        });
    }
    std::thread::sleep(Duration::from_millis(200));

    let mut under_churn_ns = Vec::with_capacity(ISOLATION_REPS);
    let deadline = Instant::now() + ISOLATION_RUN_DURATION;
    while under_churn_ns.len() < ISOLATION_REPS && Instant::now() < deadline {
        let start = Instant::now();
        std::hint::black_box(facet_scan_once(&quiet_index, &quiet_catalog));
        under_churn_ns.push(start.elapsed().as_nanos());
    }
    stop.store(true, Ordering::Relaxed);
    // Give the churn thread a moment to observe `stop` and record its
    // final count (it checks `stop` once per rebuild iteration).
    std::thread::sleep(Duration::from_millis(200));
    under_churn_ns.sort_unstable();

    let baseline_p50 = percentile_ms(&baseline_ns, 0.5);
    let baseline_p99 = percentile_ms(&baseline_ns, 0.99);
    let churn_p50 = percentile_ms(&under_churn_ns, 0.5);
    let churn_p99 = percentile_ms(&under_churn_ns, 0.99);
    let p50_ratio = churn_p50 / baseline_p50;
    let p99_ratio = churn_p99 / baseline_p99;
    let churns_completed = churn_count.load(Ordering::Relaxed);
    let churns_per_sec = churns_completed as f64 / ISOLATION_RUN_DURATION.as_secs_f64();

    let slo_pass = p50_ratio <= 2.0 && p99_ratio <= 2.0;

    println!(
        "  quiet alone (baseline):        p50={baseline_p50:.4}ms p99={baseline_p99:.4}ms (n={})",
        baseline_ns.len()
    );
    println!(
        "  quiet + churn tenant rebuilding: p50={churn_p50:.4}ms p99={churn_p99:.4}ms (n={}) -- p50_ratio={p50_ratio:.2}x p99_ratio={p99_ratio:.2}x",
        under_churn_ns.len()
    );
    println!(
        "  churn tenant: {churns_completed} full index rebuilds in {:.1}s ({churns_per_sec:.2} rebuilds/sec)",
        ISOLATION_RUN_DURATION.as_secs_f64()
    );
    println!(
        "  H14 verdict: p50_ratio={p50_ratio:.2}x p99_ratio={p99_ratio:.2}x -- {}",
        if slo_pass {
            "CONFIRMED: high-churn tenant does not materially degrade low-churn tenant's own latency"
        } else {
            "FALSIFIED: high-churn tenant materially degrades low-churn tenant's own latency"
        }
    );

    let csv = format!(
        "condition,p50_ms,p99_ms,n,ratio_vs_baseline_p50,ratio_vs_baseline_p99,churns_completed,churns_per_sec\nbaseline,{baseline_p50:.4},{baseline_p99:.4},{},1.0,1.0,,\nunder_churn,{churn_p50:.4},{churn_p99:.4},{},{p50_ratio:.4},{p99_ratio:.4},{churns_completed},{churns_per_sec:.2}\n",
        baseline_ns.len(),
        under_churn_ns.len(),
    );

    let artifacts_dir = PathBuf::from("docs/research/artifacts/p7_e11_high_churn_impact_run1");
    std::fs::create_dir_all(&artifacts_dir).ok();
    std::fs::write(artifacts_dir.join("results.csv"), &csv).ok();
    println!("\nartifacts written to {}", artifacts_dir.display());
}
