//! Phase 7 (Issue #21 Phase 7) P7-E07: fairness + aggregate QPS under a
//! realistic Zipfian demand mix. See `docs/experiments/PHASE7_LOG.md`'s
//! "P7-E07" section for the falsifiable H10 hypothesis stated before
//! this binary was written.
//!
//! Issue #21's Phase 7 "Experiments" list explicitly names "aggregate
//! QPS," "fairness under skewed tenant load," and "hot tenant
//! saturation" as required measurements. None of H1-H9 tested a
//! realistic, single, shared query stream across all 55 real tenants at
//! once: P7-E01 (H4) held one tenant's own load fixed and varied only
//! the BREADTH of other, uniformly-touched tenants; P7-E06 (H9) used a
//! deliberately simple, artificial design (one dedicated hot thread
//! spinning as fast as possible, one dedicated cold thread on a fixed
//! 100ms interval, two background-noise threads). This binary asks
//! whether H9's finding (a real ~9-13x cold/hot latency-ratio effect,
//! plausibly CPU cache locality) is a genuine architectural property
//! that replicates under a DIFFERENT, more realistic query-arrival
//! pattern, or an artifact specific to H9's fixed-interval methodology.
//!
//! Design: reuses H9's exact same-size tenant-pair selection (isolating
//! query FREQUENCY from tenant SIZE) so results are directly comparable
//! to H9. Instead of dedicated threads, ALL 55 real tenants (including
//! the size-matched pair) are queried by 4 worker threads via a single
//! shared Zipfian weight distribution (weight(rank) = 1/rank, a
//! well-established real-world traffic-skew model), with the pair's own
//! weights overridden to the population's max/min (~55x apart) so the
//! pair gets a clean, known, realistic-in-shape traffic-share gap
//! embedded in genuine full-population contention -- a materially
//! different, more realistic arrival pattern than H9's isolated design.
//!
//! Each of the 3 repeated runs uses the SAME per-thread RNG seed, so the
//! logical query sequence is identical across runs (a deterministic
//! seed per this project's standing discipline) -- any run-to-run
//! difference in the measured latencies is attributable to genuine
//! runtime/scheduling noise, not sampling noise, isolating exactly the
//! variable this project's repeated-measurement discipline cares about.
//!
//! Usage: cargo run --release -p phase7-eval --bin p7_e07_realistic_demand_mix
//!        [catalog.jsonl]

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use commerce_core::domain::Catalog;
use commerce_core::index::CatalogIndex;
use phase7_eval::resident::facet_scan_once;
use phase7_eval::tenants::load_depth1_tenants;
use rand::distributions::{Distribution, WeightedIndex};
use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;

const WORKERS: usize = 4; // matches this container's real CPU count
                          // 120s, not 15s: a first-draft 15s run produced only 62-63 cold-tenant
                          // samples (the cold tenant's assigned weight is a real ~55x smaller
                          // share of the total). With n=62, p99 is essentially the max of a tiny
                          // sample -- self-caught before trusting it: the SAME deterministic
                          // query sequence (identical RNG seed every run) still produced p99
                          // ratios swinging 1.53x-5.50x across 3 runs, while the far more robust
                          // p50 ratio stayed stable at 2.04-2.08x throughout. 120s targets
                          // several hundred cold samples, enough for a meaningful p99.
const RUN_DURATION: Duration = Duration::from_secs(120);

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

    println!("=== P7-E07: fairness + aggregate QPS under a realistic Zipfian demand mix ===");

    let all_tenants = load_depth1_tenants(&catalog_path, 55);
    let n = all_tenants.len();

    // Reuse H9's exact same-size pair selection: adjacent tenants by
    // product count nearest the middle of the real 55-tenant size
    // distribution, so tenant SIZE is controlled and only assigned
    // TRAFFIC WEIGHT differs between the pair.
    let mut by_size: Vec<usize> = (0..n).collect();
    by_size.sort_by_key(|&i| all_tenants[i].catalog.products.len());
    let mid = by_size.len() / 2;
    let hot_idx = by_size[mid];
    let cold_idx = by_size[mid - 1];
    let hot_name = all_tenants[hot_idx].name.clone();
    let cold_name = all_tenants[cold_idx].name.clone();
    let hot_products = all_tenants[hot_idx].catalog.products.len();
    let cold_products = all_tenants[cold_idx].catalog.products.len();
    println!(
        "  size-matched pair (same as H9): hot={hot_name:?} products={hot_products}, cold={cold_name:?} products={cold_products}"
    );

    // Zipfian weight by real size-rank (weight(rank) = 1/rank, rank 1 =
    // largest/most-well-known tenant, a well-established real-world
    // traffic-skew model), then override the tracked pair's weights to
    // the population's max/min so they get a clean, known ~55x traffic-
    // share gap regardless of where they fall in the size ranking.
    let mut weights: Vec<f64> = (1..=n).map(|rank| 1.0 / rank as f64).collect();
    let max_weight = weights[0];
    let min_weight = weights[n - 1];
    weights[hot_idx] = max_weight;
    weights[cold_idx] = min_weight;
    let weight_ratio = max_weight / min_weight;
    println!(
        "  assigned traffic-weight ratio hot/cold = {weight_ratio:.1}x (max={max_weight:.4}, min={min_weight:.6})"
    );

    let indexes: Vec<Arc<CatalogIndex>> = all_tenants
        .iter()
        .map(|t| Arc::new(CatalogIndex::build(&t.catalog)))
        .collect();
    let catalogs: Vec<Arc<Catalog>> = all_tenants
        .iter()
        .map(|t| Arc::new(t.catalog.clone()))
        .collect();

    const RUNS: usize = 3;
    let mut all_ratios = Vec::new();
    let mut csv = String::from(
        "run,hot_tenant,hot_products,cold_tenant,cold_products,weight_ratio,hot_n,hot_p50_ms,hot_p99_ms,cold_n,cold_p50_ms,cold_p99_ms,p99_ratio_cold_over_hot,aggregate_total_requests,aggregate_rps\n",
    );

    for run in 1..=RUNS {
        println!("\n--- run {run}/{RUNS} ---");
        let weights = weights.clone();
        let dist = WeightedIndex::new(&weights).expect("valid weights");

        let stop = Arc::new(AtomicBool::new(false));
        let aggregate_total = Arc::new(AtomicU64::new(0));
        let hot_latencies = Arc::new(Mutex::new(Vec::new()));
        let cold_latencies = Arc::new(Mutex::new(Vec::new()));

        let mut handles = Vec::new();
        for worker_id in 0..WORKERS {
            let indexes = indexes.clone();
            let catalogs = catalogs.clone();
            let dist = dist.clone();
            let stop = Arc::clone(&stop);
            let aggregate_total = Arc::clone(&aggregate_total);
            let hot_latencies = Arc::clone(&hot_latencies);
            let cold_latencies = Arc::clone(&cold_latencies);
            handles.push(std::thread::spawn(move || {
                // Same seed every run (per-worker) -- the logical query
                // sequence is identical across the 3 repeats by design;
                // only real runtime/scheduling noise varies.
                let mut rng = ChaCha8Rng::seed_from_u64(1000 + worker_id as u64);
                let mut count = 0u64;
                let mut hot_local = Vec::new();
                let mut cold_local = Vec::new();
                while !stop.load(Ordering::Relaxed) {
                    let idx = dist.sample(&mut rng);
                    if idx == hot_idx {
                        let start = Instant::now();
                        std::hint::black_box(facet_scan_once(&indexes[idx], &catalogs[idx]));
                        hot_local.push(start.elapsed().as_nanos());
                    } else if idx == cold_idx {
                        let start = Instant::now();
                        std::hint::black_box(facet_scan_once(&indexes[idx], &catalogs[idx]));
                        cold_local.push(start.elapsed().as_nanos());
                    } else {
                        std::hint::black_box(facet_scan_once(&indexes[idx], &catalogs[idx]));
                    }
                    count += 1;
                }
                aggregate_total.fetch_add(count, Ordering::Relaxed);
                hot_latencies.lock().unwrap().extend(hot_local);
                cold_latencies.lock().unwrap().extend(cold_local);
            }));
        }

        std::thread::sleep(RUN_DURATION);
        stop.store(true, Ordering::Relaxed);
        for h in handles {
            h.join().unwrap();
        }

        let mut hot_lat = Arc::try_unwrap(hot_latencies)
            .unwrap()
            .into_inner()
            .unwrap();
        let mut cold_lat = Arc::try_unwrap(cold_latencies)
            .unwrap()
            .into_inner()
            .unwrap();
        hot_lat.sort_unstable();
        cold_lat.sort_unstable();
        let aggregate = aggregate_total.load(Ordering::Relaxed);
        let aggregate_rps = aggregate as f64 / RUN_DURATION.as_secs_f64();

        let hot_p50 = percentile_ms(&hot_lat, 0.5);
        let hot_p99 = percentile_ms(&hot_lat, 0.99);
        let cold_p50 = percentile_ms(&cold_lat, 0.5);
        let cold_p99 = percentile_ms(&cold_lat, 0.99);
        let ratio = if hot_p99 > 0.0 {
            cold_p99 / hot_p99
        } else {
            0.0
        };
        all_ratios.push(ratio);

        println!(
            "  aggregate: total={aggregate} rps={aggregate_rps:.1}   hot: n={:<8} p50={hot_p50:>8.4}ms p99={hot_p99:>8.4}ms   cold: n={:<6} p50={cold_p50:>8.4}ms p99={cold_p99:>8.4}ms   p99_ratio(cold/hot)={ratio:.2}x",
            hot_lat.len(),
            cold_lat.len()
        );

        csv.push_str(&format!(
            "{run},{hot_name},{hot_products},{cold_name},{cold_products},{weight_ratio:.1},{},{hot_p50:.4},{hot_p99:.4},{},{cold_p50:.4},{cold_p99:.4},{ratio:.4},{aggregate},{aggregate_rps:.1}\n",
            hot_lat.len(),
            cold_lat.len()
        ));
    }

    println!(
        "\nH10 comparison (this design's cold/hot p99 ratio vs. H9's established 9-13x range):"
    );
    for (i, r) in all_ratios.iter().enumerate() {
        println!("  run {}: {r:.2}x", i + 1);
    }
    let min_ratio = all_ratios.iter().cloned().fold(f64::INFINITY, f64::min);
    let max_ratio = all_ratios.iter().cloned().fold(0.0, f64::max);
    let replicates = min_ratio >= 2.0;
    println!(
        "  H10 verdict: {}",
        if replicates {
            "REPLICATES -- this design's cold/hot p99 ratio also clears the 2x material-regression threshold in every run, consistent with H9's finding being a real architectural property (CPU cache locality or similar), not an artifact specific to H9's fixed-interval methodology"
        } else {
            "DOES NOT REPLICATE -- this design's cold/hot p99 ratio drops below the 2x threshold in at least one run; H9's effect may be specific to its fixed-interval sampling methodology rather than a general architectural property under realistic, naturally-arriving traffic"
        }
    );
    println!("  (ratio range across 3 runs: {min_ratio:.2}x - {max_ratio:.2}x)");
    println!(
        "  (note: aggregate rps depends heavily on WHICH tenants are hot/cold and their per-query cost -- it is NOT directly comparable to H4/P7-E01's aggregate throughput number, the same workload-mix caveat P7-E01's own first draft had to learn)"
    );

    let artifacts_dir = PathBuf::from("docs/research/artifacts/p7_e07_realistic_demand_mix_run1");
    std::fs::create_dir_all(&artifacts_dir).ok();
    std::fs::write(artifacts_dir.join("results.csv"), &csv).ok();
    println!("\nartifacts written to {}", artifacts_dir.display());
}
