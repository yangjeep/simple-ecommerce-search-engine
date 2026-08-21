//! Phase 8 (Issue #21 Phase 8) P8-E00: partially-correlated burst on
//! the native path -- Issue #21's Phase 8 "Regime B" (a subset of
//! tenants enters a sale/campaign burst simultaneously). See
//! `docs/experiments/PHASE8_LOG.md`'s "P8-E00" section for the
//! falsifiable H16 hypothesis stated before this binary was written,
//! and `PHASE8_FEASIBILITY.md` for why this is the recommended first
//! Phase 8 experiment given this environment's real constraints.
//!
//! Lives inside `phase7-eval` (not a new `phase8-eval` crate),
//! mirroring this project's own precedent of folding Phase 6B into
//! `phase6a-eval` when a new phase directly extends a prior phase's
//! same dataset/harness rather than introducing a new one.
//!
//! Reuses H10/P7-E07's exact Zipfian-weighted, all-55-real-tenants,
//! shared-`WeightedIndex` design (weight(rank) = 1/rank, deterministic
//! per-thread `ChaCha8Rng` seed identical across runs) for direct
//! methodological continuity -- but instead of a single static weight
//! assignment for the whole run, this binary runs TWO SEQUENTIAL
//! phases per repeat: a STEADY phase (the plain Zipfian baseline) and
//! a BURST phase (a fixed SUBSET of the lowest-weight/longest-tail
//! tenants has its weight multiplied by `BURST_MULTIPLIER`, simulating
//! a sudden, correlated sale/promotion event affecting that group only
//! -- everyone else's weight, including the tracked bystander tenant,
//! is unchanged). This directly tests Phase 8's own stated thesis
//! ("cheap under heterogeneous steady state, elastic under correlated
//! burst") at the one point this environment can test it honestly: a
//! single-node, in-process burst, not a real multi-node scale-out.
//!
//! Usage: cargo run --release -p phase7-eval --bin p8_e00_partial_burst
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

const WORKERS: usize = 4; // matches this container's real CPU count, H4/H10's convention
const PHASE_DURATION: Duration = Duration::from_secs(60); // matches H10's per-phase sample-count needs, halved since only 1 tracked tenant (not a min-weight pair)
const BURST_GROUP_SIZE: usize = 10; // ~18% of 55 real tenants, a "subset", per Issue #21's Regime B
const BURST_MULTIPLIER: f64 = 10.0; // Issue #21's named middle value (5x/10x/20x)
const BYSTANDER_RANK: usize = 10; // 1-indexed Zipfian rank -- outside the burst group (ranks 46-55)

fn percentile_ms(sorted_ns: &[u128], p: f64) -> f64 {
    if sorted_ns.is_empty() {
        return 0.0;
    }
    let idx = ((sorted_ns.len() as f64 - 1.0) * p).round() as usize;
    sorted_ns[idx] as f64 / 1_000_000.0
}

struct PhaseResult {
    bystander_n: u64,
    bystander_p50: f64,
    bystander_p99: f64,
    burst_group_n: u64,
    burst_group_rps: f64,
    aggregate_total: u64,
    aggregate_rps: f64,
}

fn run_phase(
    indexes: &Arc<Vec<Arc<CatalogIndex>>>,
    catalogs: &Arc<Vec<Arc<Catalog>>>,
    weights: &[f64],
    bystander_idx: usize,
    burst_group: &[usize],
    seed_base: u64,
) -> PhaseResult {
    let dist = WeightedIndex::new(weights).expect("valid weights");
    let burst_group: Arc<Vec<usize>> = Arc::new(burst_group.to_vec());

    let stop = Arc::new(AtomicBool::new(false));
    let aggregate_total = Arc::new(AtomicU64::new(0));
    let burst_group_total = Arc::new(AtomicU64::new(0));
    let bystander_latencies = Arc::new(Mutex::new(Vec::new()));

    let mut handles = Vec::new();
    for worker_id in 0..WORKERS {
        let indexes = Arc::clone(indexes);
        let catalogs = Arc::clone(catalogs);
        let dist = dist.clone();
        let burst_group = Arc::clone(&burst_group);
        let stop = Arc::clone(&stop);
        let aggregate_total = Arc::clone(&aggregate_total);
        let burst_group_total = Arc::clone(&burst_group_total);
        let bystander_latencies = Arc::clone(&bystander_latencies);
        handles.push(std::thread::spawn(move || {
            // Same seed every run AND identical across the steady/burst
            // phase boundary within a run -- only the weight
            // distribution changes between phases, isolating that as
            // the only variable, matching H10's own discipline.
            let mut rng = ChaCha8Rng::seed_from_u64(seed_base + worker_id as u64);
            let mut count = 0u64;
            let mut burst_local = 0u64;
            let mut bystander_local = Vec::new();
            while !stop.load(Ordering::Relaxed) {
                let idx = dist.sample(&mut rng);
                if idx == bystander_idx {
                    let start = Instant::now();
                    std::hint::black_box(facet_scan_once(&indexes[idx], &catalogs[idx]));
                    bystander_local.push(start.elapsed().as_nanos());
                } else {
                    std::hint::black_box(facet_scan_once(&indexes[idx], &catalogs[idx]));
                    if burst_group.contains(&idx) {
                        burst_local += 1;
                    }
                }
                count += 1;
            }
            aggregate_total.fetch_add(count, Ordering::Relaxed);
            burst_group_total.fetch_add(burst_local, Ordering::Relaxed);
            bystander_latencies.lock().unwrap().extend(bystander_local);
        }));
    }

    std::thread::sleep(PHASE_DURATION);
    stop.store(true, Ordering::Relaxed);
    for h in handles {
        h.join().unwrap();
    }

    let mut bystander_lat = Arc::try_unwrap(bystander_latencies)
        .unwrap()
        .into_inner()
        .unwrap();
    bystander_lat.sort_unstable();
    let aggregate = aggregate_total.load(Ordering::Relaxed);
    let burst_group_n = burst_group_total.load(Ordering::Relaxed);

    PhaseResult {
        bystander_n: bystander_lat.len() as u64,
        bystander_p50: percentile_ms(&bystander_lat, 0.5),
        bystander_p99: percentile_ms(&bystander_lat, 0.99),
        burst_group_n,
        burst_group_rps: burst_group_n as f64 / PHASE_DURATION.as_secs_f64(),
        aggregate_total: aggregate,
        aggregate_rps: aggregate as f64 / PHASE_DURATION.as_secs_f64(),
    }
}

fn main() {
    let catalog_path = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("dataset_cache/wands/catalog.jsonl"));

    println!("=== P8-E00: partially-correlated burst on the native path (H16, Issue #21 Phase 8 Regime B) ===");

    let all_tenants = load_depth1_tenants(&catalog_path, 55);
    let n = all_tenants.len();

    // Zipfian weight by load-order rank (weight(rank) = 1/rank, same
    // well-established traffic-skew model H10 used). Load order from
    // `load_depth1_tenants` (largest-first, deterministic) fixes a
    // real, reproducible rank assignment.
    let base_weights: Vec<f64> = (1..=n).map(|rank| 1.0 / rank as f64).collect();

    // Burst group: the LAST `BURST_GROUP_SIZE` tenants by rank (lowest
    // weight, longest tail) -- typical small SMB tenants who could
    // plausibly all get swept into a correlated sale/campaign event.
    let burst_group: Vec<usize> = ((n - BURST_GROUP_SIZE)..n).collect();
    let burst_group_names: Vec<&str> = burst_group
        .iter()
        .map(|&i| all_tenants[i].name.as_str())
        .collect();

    // Bystander: a specific, fixed, NOT-in-burst-group tenant at a
    // mid-shallow rank (enough weight for a robust sample count in a
    // 60s window, matching H10's own sampling-adequacy discipline).
    let bystander_idx = BYSTANDER_RANK - 1;
    let bystander_name = all_tenants[bystander_idx].name.clone();

    println!(
        "  burst group ({} tenants, ranks {}-{}): {:?}",
        BURST_GROUP_SIZE,
        n - BURST_GROUP_SIZE + 1,
        n,
        burst_group_names
    );
    println!(
        "  bystander (rank {BYSTANDER_RANK}, NOT in burst group): {bystander_name:?}, weight={:.4}",
        base_weights[bystander_idx]
    );
    println!("  burst multiplier: {BURST_MULTIPLIER}x");

    let indexes: Arc<Vec<Arc<CatalogIndex>>> = Arc::new(
        all_tenants
            .iter()
            .map(|t| Arc::new(CatalogIndex::build(&t.catalog)))
            .collect(),
    );
    let catalogs: Arc<Vec<Arc<Catalog>>> = Arc::new(
        all_tenants
            .iter()
            .map(|t| Arc::new(t.catalog.clone()))
            .collect(),
    );

    let mut burst_weights = base_weights.clone();
    for &i in &burst_group {
        burst_weights[i] *= BURST_MULTIPLIER;
    }

    const RUNS: usize = 3;
    let mut csv = String::from(
        "run,phase,bystander_n,bystander_p50_ms,bystander_p99_ms,burst_group_n,burst_group_rps,aggregate_total,aggregate_rps\n",
    );
    let mut p99_ratios = Vec::new();

    for run in 1..=RUNS {
        println!("\n--- run {run}/{RUNS} ---");
        let seed_base = 2000 + (run as u64) * 100;

        let steady = run_phase(
            &indexes,
            &catalogs,
            &base_weights,
            bystander_idx,
            &burst_group,
            seed_base,
        );
        println!(
            "  STEADY: bystander n={:<6} p50={:>7.4}ms p99={:>7.4}ms | burst_group rps={:>7.1} | aggregate rps={:>7.1}",
            steady.bystander_n, steady.bystander_p50, steady.bystander_p99, steady.burst_group_rps, steady.aggregate_rps
        );
        csv.push_str(&format!(
            "{run},steady,{},{:.4},{:.4},{},{:.2},{},{:.2}\n",
            steady.bystander_n,
            steady.bystander_p50,
            steady.bystander_p99,
            steady.burst_group_n,
            steady.burst_group_rps,
            steady.aggregate_total,
            steady.aggregate_rps
        ));

        // Same seed_base -- the logical query sequence up to this point
        // is identical to the steady phase's; only the weight
        // distribution differs, isolating that as the only variable.
        let burst = run_phase(
            &indexes,
            &catalogs,
            &burst_weights,
            bystander_idx,
            &burst_group,
            seed_base,
        );
        println!(
            "  BURST:  bystander n={:<6} p50={:>7.4}ms p99={:>7.4}ms | burst_group rps={:>7.1} | aggregate rps={:>7.1}",
            burst.bystander_n, burst.bystander_p50, burst.bystander_p99, burst.burst_group_rps, burst.aggregate_rps
        );
        csv.push_str(&format!(
            "{run},burst,{},{:.4},{:.4},{},{:.2},{},{:.2}\n",
            burst.bystander_n,
            burst.bystander_p50,
            burst.bystander_p99,
            burst.burst_group_n,
            burst.burst_group_rps,
            burst.aggregate_total,
            burst.aggregate_rps
        ));

        let p99_ratio = burst.bystander_p99 / steady.bystander_p99;
        let p50_ratio = burst.bystander_p50 / steady.bystander_p50;
        let burst_group_rps_ratio = burst.burst_group_rps / steady.burst_group_rps;
        p99_ratios.push(p99_ratio);
        println!(
            "  bystander p50_ratio(burst/steady)={p50_ratio:.2}x p99_ratio(burst/steady)={p99_ratio:.2}x | burst_group's own rps grew {burst_group_rps_ratio:.2}x"
        );
    }

    println!("\nH16 verdict across {RUNS} runs (bystander p99 ratio, burst vs. steady):");
    for (i, r) in p99_ratios.iter().enumerate() {
        println!("  run {}: {r:.2}x", i + 1);
    }
    let max_ratio = p99_ratios.iter().cloned().fold(0.0, f64::max);
    let confirmed = max_ratio <= 2.0;
    println!(
        "  H16 verdict: {}",
        if confirmed {
            "CONFIRMED -- the bystander tenant's own p99 stays within the 2x material-regression bar through a correlated 10x burst affecting 10 other tenants; the steady-state isolation properties (H2/H4/H10/H11) extend to this correlated-burst regime"
        } else {
            "FALSIFIED -- the bystander tenant's own p99 degrades materially (>2x) when a correlated burst hits a subset of other tenants, a real isolation gap under burst that steady-state testing alone could not surface"
        }
    );

    let artifacts_dir = PathBuf::from("docs/research/artifacts/p8_e00_partial_burst_run1");
    std::fs::create_dir_all(&artifacts_dir).ok();
    std::fs::write(artifacts_dir.join("results.csv"), &csv).ok();
    println!("\nartifacts written to {}", artifacts_dir.display());
}
