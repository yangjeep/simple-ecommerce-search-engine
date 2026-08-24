//! Phase 8 (Issue #21 Phase 8) P8-E02: does a correlated burst make
//! H15's already-confirmed shared-Solr-contention isolation gap worse?
//! See `docs/experiments/PHASE8_LOG.md`'s "P8-E02" section for the
//! falsifiable H18 hypothesis, measurement, and pass/fail bar stated
//! before this binary was written.
//!
//! **Prerequisite**: same as P7-E12 -- a Solr instance already running
//! and reachable at `--base-url` (default `http://localhost:8983/solr`)
//! with the `wands_bench`, `wands_bench_2x`, `wands_bench_5x`,
//! `wands_bench_10x`, and `wands_bench_20x` cores already built (all
//! five already exist from Phase 6A/6B's own scale-ladder work). This
//! binary does NOT start Solr itself.
//!
//! Three conditions, measured in the same process/run:
//!
//!   1. TRUE_BASELINE: quiet tenant (`wands_bench` core) queried alone
//!      (reproduces H15/P7-E12's own baseline).
//!   2. IDLE_NOISY: quiet tenant queried while 3 worker threads hammer
//!      `wands_bench_20x` (reproduces H15/P7-E12 exactly) -- no other
//!      core under load.
//!   3. BURST_NOISY: identical quiet-query and noisy-core threads,
//!      PLUS 3 additional burst worker threads, one each hammering
//!      `wands_bench_2x`, `wands_bench_5x`, and `wands_bench_10x` --
//!      simulating several more tenants' traffic joining the same
//!      shared Solr instance during a correlated sale event, not just
//!      the one noisy tenant H15 already measured.
//!
//! Applies H17/P8-E01's own lesson proactively (rather than
//! re-discovering it): starts directly with `RUNS=10` and a
//! median-based verdict, plus the same >=2.0x material-regression
//! hit-rate secondary statistic H17 introduced.
//!
//! Usage: cargo run --release -p phase7-eval --bin
//!        p8_e02_burst_amplified_solr_contention [base_url]
//!        (default base_url: http://localhost:8983/solr)

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

const ISOLATION_RUN_DURATION: Duration = Duration::from_secs(5); // matches H15/P7-E12
const ISOLATION_REPS: usize = 500; // matches H15/P7-E12
const NOISY_WORKERS: usize = 3; // matches H15/P7-E12
const QUIET_CORE: &str = "wands_bench";
const NOISY_CORE: &str = "wands_bench_20x";
const BURST_CORES: &[&str] = &["wands_bench_2x", "wands_bench_5x", "wands_bench_10x"];
const PAGE_SIZE: &str = "24"; // matches H15/P7-E12's PAGE_SIZE convention
const RUNS: usize = 10; // H17's lesson applied proactively, not re-discovered

fn percentile_ms(sorted_ns: &[u128], p: f64) -> f64 {
    if sorted_ns.is_empty() {
        return 0.0;
    }
    let idx = ((sorted_ns.len() as f64 - 1.0) * p).round() as usize;
    sorted_ns[idx] as f64 / 1_000_000.0
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

/// Measures the quiet core's own latency for `ISOLATION_REPS` (or
/// until `ISOLATION_RUN_DURATION` elapses) while zero or more noisy
/// cores are hammered concurrently by dedicated worker threads.
fn measure_condition(base_url: &str, noisy_cores: &[&str], workers_per_core: usize) -> Vec<u128> {
    let stop = Arc::new(AtomicBool::new(false));
    let mut handles = Vec::new();
    for &core in noisy_cores {
        for _ in 0..workers_per_core {
            let base_url = base_url.to_string();
            let stop = Arc::clone(&stop);
            let core = core.to_string();
            handles.push(std::thread::spawn(move || {
                while !stop.load(Ordering::Relaxed) {
                    solr_query_once(&base_url, &core);
                }
            }));
        }
    }

    if !handles.is_empty() {
        std::thread::sleep(Duration::from_millis(200));
    }

    let mut latencies_ns = Vec::with_capacity(ISOLATION_REPS);
    if handles.is_empty() {
        for _ in 0..ISOLATION_REPS {
            let start = Instant::now();
            solr_query_once(base_url, QUIET_CORE);
            latencies_ns.push(start.elapsed().as_nanos());
        }
    } else {
        let deadline = Instant::now() + ISOLATION_RUN_DURATION;
        while latencies_ns.len() < ISOLATION_REPS && Instant::now() < deadline {
            let start = Instant::now();
            solr_query_once(base_url, QUIET_CORE);
            latencies_ns.push(start.elapsed().as_nanos());
        }
    }

    stop.store(true, Ordering::Relaxed);
    for h in handles {
        h.join().unwrap();
    }
    latencies_ns.sort_unstable();
    latencies_ns
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

fn main() {
    let base_url = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "http://localhost:8983/solr".to_string());

    println!("=== P8-E02: burst-amplified shared-Solr contention (H18, Issue #21 Phase 8) ===");
    println!(
        "(quiet core='{QUIET_CORE}'; noisy core='{NOISY_CORE}'; burst cores={BURST_CORES:?}; base_url={base_url})"
    );

    let ping_url = format!("{base_url}/{QUIET_CORE}/select");
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

    // Warm up every core before trusting any measurement -- H15/P7-E12's
    // own first draft self-caught a JVM/query-cache cold-start artifact
    // from skipping this step.
    let warmup_deadline = Instant::now() + Duration::from_millis(500);
    let mut all_cores: Vec<&str> = vec![QUIET_CORE, NOISY_CORE];
    all_cores.extend_from_slice(BURST_CORES);
    while Instant::now() < warmup_deadline {
        for &core in &all_cores {
            solr_query_once(&base_url, core);
        }
    }

    let mut csv = String::from("run,condition,p50_ms,p99_ms,n\n");
    let mut idle_ratios = Vec::new();
    let mut burst_ratios = Vec::new();
    let mut amplifications = Vec::new();

    for run in 1..=RUNS {
        println!("\n--- run {run}/{RUNS} ---");

        let baseline_ns = measure_condition(&base_url, &[], 0);
        let baseline_p50 = percentile_ms(&baseline_ns, 0.5);
        let baseline_p99 = percentile_ms(&baseline_ns, 0.99);
        println!(
            "  TRUE_BASELINE: p50={baseline_p50:.4}ms p99={baseline_p99:.4}ms (n={})",
            baseline_ns.len()
        );
        csv.push_str(&format!(
            "{run},true_baseline,{baseline_p50:.4},{baseline_p99:.4},{}\n",
            baseline_ns.len()
        ));

        let idle_ns = measure_condition(&base_url, &[NOISY_CORE], NOISY_WORKERS);
        let idle_p50 = percentile_ms(&idle_ns, 0.5);
        let idle_p99 = percentile_ms(&idle_ns, 0.99);
        println!(
            "  IDLE_NOISY:    p50={idle_p50:.4}ms p99={idle_p99:.4}ms (n={})",
            idle_ns.len()
        );
        csv.push_str(&format!(
            "{run},idle_noisy,{idle_p50:.4},{idle_p99:.4},{}\n",
            idle_ns.len()
        ));

        let mut burst_noisy_cores: Vec<&str> = vec![NOISY_CORE];
        burst_noisy_cores.extend_from_slice(BURST_CORES);
        // NOISY_CORE keeps its original 3 workers; each burst core gets
        // 1 additional worker (a lighter per-tenant load, since a real
        // burst adds MORE distinct tenants rather than one tenant
        // getting 3x noisier).
        let stop = Arc::new(AtomicBool::new(false));
        let mut handles = Vec::new();
        for _ in 0..NOISY_WORKERS {
            let base_url = base_url.clone();
            let stop = Arc::clone(&stop);
            handles.push(std::thread::spawn(move || {
                while !stop.load(Ordering::Relaxed) {
                    solr_query_once(&base_url, NOISY_CORE);
                }
            }));
        }
        for &core in BURST_CORES {
            let base_url = base_url.clone();
            let stop = Arc::clone(&stop);
            let core = core.to_string();
            handles.push(std::thread::spawn(move || {
                while !stop.load(Ordering::Relaxed) {
                    solr_query_once(&base_url, &core);
                }
            }));
        }
        std::thread::sleep(Duration::from_millis(200));
        let mut burst_ns = Vec::with_capacity(ISOLATION_REPS);
        let deadline = Instant::now() + ISOLATION_RUN_DURATION;
        while burst_ns.len() < ISOLATION_REPS && Instant::now() < deadline {
            let start = Instant::now();
            solr_query_once(&base_url, QUIET_CORE);
            burst_ns.push(start.elapsed().as_nanos());
        }
        stop.store(true, Ordering::Relaxed);
        for h in handles {
            h.join().unwrap();
        }
        burst_ns.sort_unstable();

        let burst_p50 = percentile_ms(&burst_ns, 0.5);
        let burst_p99 = percentile_ms(&burst_ns, 0.99);
        println!(
            "  BURST_NOISY:   p50={burst_p50:.4}ms p99={burst_p99:.4}ms (n={})",
            burst_ns.len()
        );
        csv.push_str(&format!(
            "{run},burst_noisy,{burst_p50:.4},{burst_p99:.4},{}\n",
            burst_ns.len()
        ));

        let idle_ratio = idle_p99 / baseline_p99;
        let burst_ratio = burst_p99 / baseline_p99;
        let amplification = burst_ratio / idle_ratio;
        idle_ratios.push(idle_ratio);
        burst_ratios.push(burst_ratio);
        amplifications.push(amplification);
        println!(
            "  idle_ratio={idle_ratio:.2}x burst_ratio={burst_ratio:.2}x amplification={amplification:.2}x"
        );
    }

    println!("\nH18 verdict across {RUNS} runs:");
    for i in 0..RUNS {
        println!(
            "  run {}: idle_ratio={:.2}x burst_ratio={:.2}x amplification={:.2}x",
            i + 1,
            idle_ratios[i],
            burst_ratios[i],
            amplifications[i]
        );
    }
    println!(
        "  median idle_ratio={:.2}x median burst_ratio={:.2}x",
        median_of(&idle_ratios),
        median_of(&burst_ratios)
    );

    let min_amp = amplifications.iter().cloned().fold(f64::MAX, f64::min);
    let max_amp = amplifications.iter().cloned().fold(0.0, f64::max);
    let median_amp = median_of(&amplifications);
    println!(
        "  amplification across {RUNS} runs: median={median_amp:.2}x range=[{min_amp:.2}x, {max_amp:.2}x]"
    );
    let verdict = if median_amp >= 1.25 {
        "CONFIRMED -- a correlated burst (additional tenants' traffic joining the shared Solr instance) materially amplifies H15's already-confirmed shared-Solr-contention isolation gap; the risk is worse than H15 alone showed"
    } else if median_amp <= 0.8 {
        "SURPRISING ATTENUATION -- burst load appears to REDUCE the median contention-driven degradation; reported, not assumed, pending a mechanism explanation"
    } else {
        "DISCONFIRMED (burst-invariant on the median) -- median amplification falls within 0.8x-1.25x; H15's shared-Solr-contention gap is real but its typical magnitude does not depend on additional tenants joining the shared backend"
    };
    println!("  H18 verdict: {verdict}");

    const MATERIAL_BAR: f64 = 2.0;
    let idle_hit_rate = idle_ratios.iter().filter(|&&r| r >= MATERIAL_BAR).count();
    let burst_hit_rate = burst_ratios.iter().filter(|&&r| r >= MATERIAL_BAR).count();
    println!(
        "\n  Secondary statistic -- how often does a >= {MATERIAL_BAR}x material-regression event happen at all:"
    );
    println!("    IDLE_NOISY:  {idle_hit_rate}/{RUNS} runs showed >= {MATERIAL_BAR}x degradation");
    println!("    BURST_NOISY: {burst_hit_rate}/{RUNS} runs showed >= {MATERIAL_BAR}x degradation");

    let artifacts_dir = std::path::PathBuf::from(
        "docs/research/artifacts/p8_e02_burst_amplified_solr_contention_run1",
    );
    std::fs::create_dir_all(&artifacts_dir).ok();
    std::fs::write(artifacts_dir.join("results.csv"), &csv).ok();
    println!("\nartifacts written to {}", artifacts_dir.display());
}
