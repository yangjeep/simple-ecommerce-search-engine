//! Phase 7 (Issue #21 Phase 7) P7-E12: lexical-backend contention --
//! Issue #21's last remaining required-experiments item. Every prior
//! Phase 7 experiment (H1-H14) tested only the native
//! `commerce_core::index::CatalogIndex` path; none of them ever touched
//! Solr. See `docs/experiments/PHASE7_LOG.md`'s "P7-E12" section for the
//! falsifiable H15 hypothesis stated before this binary was written.
//!
//! **Prerequisite**: a Solr instance must already be running and
//! reachable at `--base-url` (default `http://localhost:8983/solr`),
//! with the `wands_bench` and `wands_bench_20x` cores already built
//! (both already exist from Phase 6A/6B's own scale-ladder work -- see
//! `docs/research/artifacts/p6b_e00_scale_ladder_run1/` for how they
//! were built). Start Solr with (from the Solr install directory):
//! `bin/solr start -p 8983 --force` (`--force` is needed only because
//! this container runs as root). This binary does NOT start Solr
//! itself -- unlike every other Phase 7 experiment, which is fully
//! self-contained, this one has a real external-process prerequisite,
//! disclosed here rather than silently assumed.
//!
//! Reuses H2/P7-E00's exact quiet/noisy-tenant methodology (same
//! `ISOLATION_REPS`/`ISOLATION_RUN_DURATION`) so this result is directly
//! comparable to H2's own already-established finding that the NATIVE
//! in-process path shows no material cross-tenant degradation from
//! noisy query load -- this experiment asks the same question for the
//! LEXICAL BACKEND path instead: when multiple tenants' lexical-fallback
//! traffic shares one Solr instance (a realistic shared-backend
//! deployment), does one tenant's heavy Solr load degrade another's own
//! Solr query latency?
//!
//! Usage: cargo run --release -p phase7-eval --bin
//!        p7_e12_lexical_backend_contention [base_url]
//!        (default base_url: http://localhost:8983/solr)

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

const ISOLATION_RUN_DURATION: Duration = Duration::from_secs(5); // matches H2/P7-E00
const ISOLATION_REPS: usize = 500; // matches H2/P7-E00
const NOISY_WORKERS: usize = 3; // matches H2/P7-E00
const QUIET_CORE: &str = "wands_bench"; // 42,994 docs, the original real corpus
const NOISY_CORE: &str = "wands_bench_20x"; // 859,880 docs, Phase 6B's largest scale-ladder rung
const PAGE_SIZE: &str = "24"; // matches Phase 6A's own PAGE_SIZE convention

fn percentile_ms(sorted_ns: &[u128], p: f64) -> f64 {
    if sorted_ns.is_empty() {
        return 0.0;
    }
    let idx = ((sorted_ns.len() as f64 - 1.0) * p).round() as usize;
    sorted_ns[idx] as f64 / 1_000_000.0
}

/// A single real page-browse query against one core: `q=*:*&rows=24`,
/// matching Phase 6A's own PAGE_SIZE=24 convention for a representative
/// "category render" style request -- not an artificially cheap
/// `rows=0` count-only query, nor an artificially expensive one.
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

fn main() {
    let base_url = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "http://localhost:8983/solr".to_string());

    println!(
        "=== P7-E12: lexical-backend contention across tenants sharing one Solr instance (H15) ==="
    );
    println!(
        "(quiet tenant core='{QUIET_CORE}'; noisy tenant core='{NOISY_CORE}'; base_url={base_url})"
    );

    // Fail fast with a clear message if Solr isn't reachable, rather
    // than a confusing panic deep inside the measurement loop.
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

    // Warm up both cores before trusting any measurement -- the first
    // real query against a given core after Solr startup pays real JIT/
    // query-cache/connection-pool warm-up cost (self-caught in this
    // binary's first draft: run 1's baseline p99 was 8.98ms vs. 3.5-3.9ms
    // in runs 2-3, an outlier traced to this effect, matching this
    // project's established discipline of not trusting the very first
    // in-process/in-session measurement -- see P7-E09's cold-start p99
    // lesson).
    let warmup_deadline = Instant::now() + Duration::from_millis(500);
    while Instant::now() < warmup_deadline {
        solr_query_once(&base_url, QUIET_CORE);
        solr_query_once(&base_url, NOISY_CORE);
    }

    // ---- Baseline: quiet tenant queried alone ----
    let mut baseline_ns = Vec::with_capacity(ISOLATION_REPS);
    for _ in 0..ISOLATION_REPS {
        let start = Instant::now();
        solr_query_once(&base_url, QUIET_CORE);
        baseline_ns.push(start.elapsed().as_nanos());
    }
    baseline_ns.sort_unstable();

    // ---- Cross-tenant condition: noisy threads hammer the NOISY core
    // (a different tenant) while the quiet tenant is measured ----
    let stop = Arc::new(AtomicBool::new(false));
    let mut noisy_handles = Vec::new();
    for _ in 0..NOISY_WORKERS {
        let base_url = base_url.clone();
        let stop = Arc::clone(&stop);
        noisy_handles.push(std::thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                solr_query_once(&base_url, NOISY_CORE);
            }
        }));
    }
    std::thread::sleep(Duration::from_millis(200));

    let mut cross_tenant_ns = Vec::with_capacity(ISOLATION_REPS);
    let deadline = Instant::now() + ISOLATION_RUN_DURATION;
    while cross_tenant_ns.len() < ISOLATION_REPS && Instant::now() < deadline {
        let start = Instant::now();
        solr_query_once(&base_url, QUIET_CORE);
        cross_tenant_ns.push(start.elapsed().as_nanos());
    }
    stop.store(true, Ordering::Relaxed);
    for h in noisy_handles {
        h.join().unwrap();
    }
    cross_tenant_ns.sort_unstable();

    let baseline_p50 = percentile_ms(&baseline_ns, 0.5);
    let baseline_p99 = percentile_ms(&baseline_ns, 0.99);
    let cross_p50 = percentile_ms(&cross_tenant_ns, 0.5);
    let cross_p99 = percentile_ms(&cross_tenant_ns, 0.99);
    let p50_ratio = cross_p50 / baseline_p50;
    let p99_ratio = cross_p99 / baseline_p99;
    let slo_pass = p50_ratio <= 2.0 && p99_ratio <= 2.0;

    println!(
        "  quiet alone (baseline):                 p50={baseline_p50:.4}ms p99={baseline_p99:.4}ms (n={})",
        baseline_ns.len()
    );
    println!(
        "  quiet + {NOISY_WORKERS} threads hammering NOISY core (cross-tenant): p50={cross_p50:.4}ms p99={cross_p99:.4}ms (n={}) -- p50_ratio={p50_ratio:.2}x p99_ratio={p99_ratio:.2}x",
        cross_tenant_ns.len()
    );
    println!(
        "  H15 verdict: p50_ratio={p50_ratio:.2}x p99_ratio={p99_ratio:.2}x -- {}",
        if slo_pass {
            "CONFIRMED: shared-Solr cross-tenant load does not materially degrade the quiet tenant's own latency"
        } else {
            "FALSIFIED: shared-Solr cross-tenant load materially degrades the quiet tenant's own latency"
        }
    );

    let csv = format!(
        "condition,p50_ms,p99_ms,n,ratio_vs_baseline_p50,ratio_vs_baseline_p99\nbaseline,{baseline_p50:.4},{baseline_p99:.4},{},1.0,1.0\ncross_tenant,{cross_p50:.4},{cross_p99:.4},{},{p50_ratio:.4},{p99_ratio:.4}\n",
        baseline_ns.len(),
        cross_tenant_ns.len(),
    );

    let artifacts_dir =
        std::path::PathBuf::from("docs/research/artifacts/p7_e12_lexical_backend_contention_run1");
    std::fs::create_dir_all(&artifacts_dir).ok();
    std::fs::write(artifacts_dir.join("results.csv"), &csv).ok();
    println!("\nartifacts written to {}", artifacts_dir.display());
}
