//! Phase 8 (Issue #21 Phase 8) P8-E03: three-way interaction -- does
//! running the native rebuild-churn load (H14/H17) and the shared-Solr-
//! contention load (H15/H18) SIMULTANEOUSLY make either quiet path's
//! degradation worse than that mechanism's own single-source gap
//! measured alone? See `docs/experiments/PHASE8_LOG.md`'s "P8-E03"
//! section for the falsifiable H19 hypothesis, measurement, and
//! pass/fail bar stated before this binary was written.
//!
//! **Prerequisite**: same as P7-E12/P8-E02 -- a Solr instance already
//! running and reachable at `--base-url` (default
//! `http://localhost:8983/solr`) with the `wands_bench` and
//! `wands_bench_20x` cores already built. This binary does NOT start
//! Solr itself.
//!
//! Four conditions per run, each measuring BOTH quiet paths (native
//! tenant "Rugs"; Solr core `wands_bench`) CONCURRENTLY so a true
//! "combined load" moment is actually captured, not two separate
//! sequential measurements:
//!
//!   1. BASELINE: both quiet paths alone, no churn, no Solr noise.
//!   2. NATIVE_CHURN: Furniture continuously rebuilt (H14/H17's exact
//!      mechanism); Solr side otherwise idle.
//!   3. SOLR_NOISY: 3 threads hammering `wands_bench_20x` (H15/H18's
//!      exact mechanism); native side otherwise idle.
//!   4. COMBINED: both of the above running at the same time.
//!
//! Usage: cargo run --release -p phase7-eval --bin
//!        p8_e03_combined_churn_solr_interaction [catalog.jsonl] [base_url]

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use commerce_core::domain::Catalog;
use commerce_core::index::CatalogIndex;
use phase7_eval::resident::facet_scan_once;
use phase7_eval::tenants::load_depth1_tenants;

const ISOLATION_RUN_DURATION: Duration = Duration::from_secs(5); // matches H14/H15/H17/H18
const ISOLATION_REPS: usize = 500; // matches H14/H15/H17/H18
const SOLR_NOISY_WORKERS: usize = 3; // matches H15/H18
const QUIET_SOLR_CORE: &str = "wands_bench";
const NOISY_SOLR_CORE: &str = "wands_bench_20x";
const PAGE_SIZE: &str = "24";
const RUNS: usize = 10; // H17's lesson applied proactively

fn percentile_ms(sorted_ns: &[u128], p: f64) -> f64 {
    if sorted_ns.is_empty() {
        return 0.0;
    }
    let idx = ((sorted_ns.len() as f64 - 1.0) * p).round() as usize;
    sorted_ns[idx] as f64 / 1_000_000.0
}

fn median_of(values: &[f64]) -> f64 {
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    if sorted.len().is_multiple_of(2) {
        (sorted[sorted.len() / 2 - 1] + sorted[sorted.len() / 2]) / 2.0
    } else {
        sorted[sorted.len() / 2]
    }
}

fn solr_query_once(base_url: &str, core: &str) {
    let url = format!("{base_url}/{core}/select");
    let resp = ureq::get(&url)
        .query("q", "*:*")
        .query("rows", PAGE_SIZE)
        .query("wt", "json")
        .call()
        .unwrap_or_else(|e| panic!("Solr request to {url} failed: {e}"));
    let json: serde_json::Value = resp
        .into_json()
        .unwrap_or_else(|e| panic!("Solr response from {url} was not valid JSON: {e}"));
    std::hint::black_box(json["response"]["numFound"].as_u64());
}

struct ConditionResult {
    rugs_p50: f64,
    rugs_p99: f64,
    solr_p50: f64,
    solr_p99: f64,
    rugs_n: usize,
    solr_n: usize,
    wall_secs: f64,
    churns_completed: u64,
}

/// Measures both quiet paths concurrently while `churn_active` and/or
/// `solr_noisy_active` background load is applied.
fn measure_condition(
    base_url: &str,
    rugs_index: &Arc<CatalogIndex>,
    rugs_catalog: &Arc<Catalog>,
    furniture_catalog: &Arc<Catalog>,
    churn_active: bool,
    solr_noisy_active: bool,
) -> ConditionResult {
    let stop = Arc::new(AtomicBool::new(false));
    let mut handles = Vec::new();
    let churn_count = Arc::new(AtomicU64::new(0));
    let condition_start = Instant::now();

    if churn_active {
        let furniture_catalog = Arc::clone(furniture_catalog);
        let stop = Arc::clone(&stop);
        let churn_count = Arc::clone(&churn_count);
        handles.push(std::thread::spawn(move || {
            let mut count = 0u64;
            while !stop.load(Ordering::Relaxed) {
                let rebuilt = CatalogIndex::build(&furniture_catalog);
                std::hint::black_box(Mutex::new(rebuilt));
                count += 1;
            }
            churn_count.store(count, Ordering::Relaxed);
        }));
    }

    if solr_noisy_active {
        for _ in 0..SOLR_NOISY_WORKERS {
            let base_url = base_url.to_string();
            let stop = Arc::clone(&stop);
            handles.push(std::thread::spawn(move || {
                while !stop.load(Ordering::Relaxed) {
                    solr_query_once(&base_url, NOISY_SOLR_CORE);
                }
            }));
        }
    }

    if churn_active || solr_noisy_active {
        std::thread::sleep(Duration::from_millis(200));
    }

    // Measure both quiet paths concurrently so the "combined load"
    // condition actually captures a moment when both mechanisms are
    // simultaneously active against both quiet paths, not two
    // sequential windows that could land in different contention
    // phases.
    let rugs_handle = {
        let rugs_index = Arc::clone(rugs_index);
        let rugs_catalog = Arc::clone(rugs_catalog);
        let unloaded = !churn_active && !solr_noisy_active;
        std::thread::spawn(move || {
            let mut ns = Vec::with_capacity(ISOLATION_REPS);
            if unloaded {
                for _ in 0..ISOLATION_REPS {
                    let start = Instant::now();
                    std::hint::black_box(facet_scan_once(&rugs_index, &rugs_catalog));
                    ns.push(start.elapsed().as_nanos());
                }
            } else {
                let deadline = Instant::now() + ISOLATION_RUN_DURATION;
                while ns.len() < ISOLATION_REPS && Instant::now() < deadline {
                    let start = Instant::now();
                    std::hint::black_box(facet_scan_once(&rugs_index, &rugs_catalog));
                    ns.push(start.elapsed().as_nanos());
                }
            }
            ns.sort_unstable();
            ns
        })
    };

    let solr_handle = {
        let base_url = base_url.to_string();
        let unloaded = !churn_active && !solr_noisy_active;
        std::thread::spawn(move || {
            let mut ns = Vec::with_capacity(ISOLATION_REPS);
            if unloaded {
                for _ in 0..ISOLATION_REPS {
                    let start = Instant::now();
                    solr_query_once(&base_url, QUIET_SOLR_CORE);
                    ns.push(start.elapsed().as_nanos());
                }
            } else {
                let deadline = Instant::now() + ISOLATION_RUN_DURATION;
                while ns.len() < ISOLATION_REPS && Instant::now() < deadline {
                    let start = Instant::now();
                    solr_query_once(&base_url, QUIET_SOLR_CORE);
                    ns.push(start.elapsed().as_nanos());
                }
            }
            ns.sort_unstable();
            ns
        })
    };

    let rugs_ns = rugs_handle.join().unwrap();
    let solr_ns = solr_handle.join().unwrap();
    let wall_secs = condition_start.elapsed().as_secs_f64();

    stop.store(true, Ordering::Relaxed);
    // join() already blocks until the churn thread's closure returns
    // (including its final churn_count.store), so no extra wait is
    // needed here.
    for h in handles {
        h.join().unwrap();
    }

    ConditionResult {
        rugs_p50: percentile_ms(&rugs_ns, 0.5),
        rugs_p99: percentile_ms(&rugs_ns, 0.99),
        solr_p50: percentile_ms(&solr_ns, 0.5),
        solr_p99: percentile_ms(&solr_ns, 0.99),
        rugs_n: rugs_ns.len(),
        solr_n: solr_ns.len(),
        wall_secs,
        churns_completed: churn_count.load(Ordering::Relaxed),
    }
}

fn main() {
    let catalog_path = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("dataset_cache/wands/catalog.jsonl"));
    let base_url = std::env::args()
        .nth(2)
        .unwrap_or_else(|| "http://localhost:8983/solr".to_string());

    println!(
        "=== P8-E03: three-way interaction -- native rebuild-churn + shared-Solr contention, simultaneous (H19, Issue #21 Phase 8) ==="
    );
    println!(
        "(native quiet tenant='Rugs'; native churn tenant='Furniture'; Solr quiet core='{QUIET_SOLR_CORE}'; Solr noisy core='{NOISY_SOLR_CORE}'; base_url={base_url})"
    );

    let ping_url = format!("{base_url}/{QUIET_SOLR_CORE}/select");
    ureq::get(&ping_url)
        .query("q", "*:*")
        .query("rows", "0")
        .query("wt", "json")
        .call()
        .unwrap_or_else(|e| {
            panic!(
                "Solr not reachable at {ping_url}: {e}. Start it first (see this binary's doc comment)."
            )
        });

    let warmup_deadline = Instant::now() + Duration::from_millis(500);
    while Instant::now() < warmup_deadline {
        solr_query_once(&base_url, QUIET_SOLR_CORE);
        solr_query_once(&base_url, NOISY_SOLR_CORE);
    }

    let mut tenants = load_depth1_tenants(&catalog_path, 55);
    let quiet_pos = tenants
        .iter()
        .position(|t| t.name == "Rugs")
        .expect("Rugs must be present");
    let quiet_tenant = tenants.remove(quiet_pos);
    let furniture_pos = tenants
        .iter()
        .position(|t| t.name == "Furniture")
        .expect("Furniture must be present");
    let furniture_tenant = tenants.swap_remove(furniture_pos);
    drop(tenants);

    println!(
        "native quiet tenant={:?} ({} products); native churn tenant={:?} ({} products)",
        quiet_tenant.name,
        quiet_tenant.catalog.products.len(),
        furniture_tenant.name,
        furniture_tenant.catalog.products.len()
    );

    let rugs_index = Arc::new(quiet_tenant.index);
    let rugs_catalog = Arc::new(quiet_tenant.catalog);
    let furniture_catalog = Arc::new(furniture_tenant.catalog);

    let mut csv = String::from("run,condition,rugs_p50_ms,rugs_p99_ms,solr_p50_ms,solr_p99_ms\n");
    let mut native_solo_ratios = Vec::new();
    let mut native_combined_ratios = Vec::new();
    let mut native_cross_amps = Vec::new();
    let mut solr_solo_ratios = Vec::new();
    let mut solr_combined_ratios = Vec::new();
    let mut solr_cross_amps = Vec::new();

    for run in 1..=RUNS {
        println!("\n--- run {run}/{RUNS} ---");

        let baseline = measure_condition(
            &base_url,
            &rugs_index,
            &rugs_catalog,
            &furniture_catalog,
            false,
            false,
        );
        println!(
            "  BASELINE:     rugs p50={:.4}ms p99={:.4}ms | solr p50={:.4}ms p99={:.4}ms",
            baseline.rugs_p50, baseline.rugs_p99, baseline.solr_p50, baseline.solr_p99
        );
        csv.push_str(&format!(
            "{run},baseline,{:.4},{:.4},{:.4},{:.4}\n",
            baseline.rugs_p50, baseline.rugs_p99, baseline.solr_p50, baseline.solr_p99
        ));

        let native_churn = measure_condition(
            &base_url,
            &rugs_index,
            &rugs_catalog,
            &furniture_catalog,
            true,
            false,
        );
        println!(
            "  NATIVE_CHURN: rugs p50={:.4}ms p99={:.4}ms | solr p50={:.4}ms p99={:.4}ms | wall={:.2}s churns={} (rugs_n={} solr_n={})",
            native_churn.rugs_p50,
            native_churn.rugs_p99,
            native_churn.solr_p50,
            native_churn.solr_p99,
            native_churn.wall_secs,
            native_churn.churns_completed,
            native_churn.rugs_n,
            native_churn.solr_n
        );
        csv.push_str(&format!(
            "{run},native_churn,{:.4},{:.4},{:.4},{:.4}\n",
            native_churn.rugs_p50,
            native_churn.rugs_p99,
            native_churn.solr_p50,
            native_churn.solr_p99
        ));

        let solr_noisy = measure_condition(
            &base_url,
            &rugs_index,
            &rugs_catalog,
            &furniture_catalog,
            false,
            true,
        );
        println!(
            "  SOLR_NOISY:   rugs p50={:.4}ms p99={:.4}ms | solr p50={:.4}ms p99={:.4}ms",
            solr_noisy.rugs_p50, solr_noisy.rugs_p99, solr_noisy.solr_p50, solr_noisy.solr_p99
        );
        csv.push_str(&format!(
            "{run},solr_noisy,{:.4},{:.4},{:.4},{:.4}\n",
            solr_noisy.rugs_p50, solr_noisy.rugs_p99, solr_noisy.solr_p50, solr_noisy.solr_p99
        ));

        let combined = measure_condition(
            &base_url,
            &rugs_index,
            &rugs_catalog,
            &furniture_catalog,
            true,
            true,
        );
        println!(
            "  COMBINED:     rugs p50={:.4}ms p99={:.4}ms | solr p50={:.4}ms p99={:.4}ms | wall={:.2}s churns={} (rugs_n={} solr_n={})",
            combined.rugs_p50,
            combined.rugs_p99,
            combined.solr_p50,
            combined.solr_p99,
            combined.wall_secs,
            combined.churns_completed,
            combined.rugs_n,
            combined.solr_n
        );
        csv.push_str(&format!(
            "{run},combined,{:.4},{:.4},{:.4},{:.4}\n",
            combined.rugs_p50, combined.rugs_p99, combined.solr_p50, combined.solr_p99
        ));

        let native_solo_ratio = native_churn.rugs_p99 / baseline.rugs_p99;
        let native_combined_ratio = combined.rugs_p99 / baseline.rugs_p99;
        let native_cross_amp = native_combined_ratio / native_solo_ratio;
        let solr_solo_ratio = solr_noisy.solr_p99 / baseline.solr_p99;
        let solr_combined_ratio = combined.solr_p99 / baseline.solr_p99;
        let solr_cross_amp = solr_combined_ratio / solr_solo_ratio;

        native_solo_ratios.push(native_solo_ratio);
        native_combined_ratios.push(native_combined_ratio);
        native_cross_amps.push(native_cross_amp);
        solr_solo_ratios.push(solr_solo_ratio);
        solr_combined_ratios.push(solr_combined_ratio);
        solr_cross_amps.push(solr_cross_amp);

        println!(
            "  native: solo_ratio={native_solo_ratio:.2}x combined_ratio={native_combined_ratio:.2}x cross_amp={native_cross_amp:.2}x | solr: solo_ratio={solr_solo_ratio:.2}x combined_ratio={solr_combined_ratio:.2}x cross_amp={solr_cross_amp:.2}x"
        );
    }

    println!("\nH19 verdict across {RUNS} runs:");
    let native_median_amp = median_of(&native_cross_amps);
    let solr_median_amp = median_of(&solr_cross_amps);
    println!(
        "  native side: median solo_ratio={:.2}x median combined_ratio={:.2}x median cross_amplification={native_median_amp:.2}x",
        median_of(&native_solo_ratios),
        median_of(&native_combined_ratios),
    );
    println!(
        "  solr side:   median solo_ratio={:.2}x median combined_ratio={:.2}x median cross_amplification={solr_median_amp:.2}x",
        median_of(&solr_solo_ratios),
        median_of(&solr_combined_ratios),
    );

    let native_confirmed = native_median_amp >= 1.25;
    let solr_confirmed = solr_median_amp >= 1.25;
    let verdict = if native_confirmed || solr_confirmed {
        "CONFIRMED -- at least one quiet path's own degradation gets materially worse when both mechanisms run simultaneously vs. that mechanism's own single-source gap alone; the two subsystems interact rather than acting as if on independent hardware"
    } else {
        "DISCONFIRMED -- neither quiet path's degradation gets materially worse under combined load; the native and Solr subsystems' contention appears confined to their own resources despite sharing the same physical hardware"
    };
    println!(
        "  H19 verdict: {verdict} (native side {}, solr side {})",
        if native_confirmed {
            "CONFIRMED"
        } else {
            "not confirmed"
        },
        if solr_confirmed {
            "CONFIRMED"
        } else {
            "not confirmed"
        }
    );

    let artifacts_dir =
        PathBuf::from("docs/research/artifacts/p8_e03_combined_churn_solr_interaction_run1");
    std::fs::create_dir_all(&artifacts_dir).ok();
    std::fs::write(artifacts_dir.join("results.csv"), &csv).ok();
    println!("\nartifacts written to {}", artifacts_dir.display());
}
