//! Issue #14 P3-E07: bootstrap confidence intervals for the two operating
//! points P3-E06 promoted (`docs/experiments/PHASE3_LOG.md`) --
//! (structural_cap=250, anchored_lexical_cap=1), clearing the <=1%
//! relevance budget at 1.80% coverage, and (structural_cap=50,
//! anchored_lexical_cap=20), clearing the <=2% budget at 5.31% coverage.
//!
//! Every prior Phase 2/3 relevance/coverage number has been a single
//! deterministic point estimate, per `bench_harness`'s own documented
//! methodology: `commerce_core`'s compile/plan/execute path has no model
//! call and no randomness, so a query's own native/Solr score is bit-for-
//! bit reproducible and needs no repeated *measurement*. But the
//! *aggregate* whole-workload NDCG is itself a statistic over a finite
//! (22,458-query) sample of a larger real-traffic population, and the
//! user's own instructions ask for bootstrap confidence intervals on
//! promoted headline results where practical -- this supplies that,
//! without any new Solr querying: every input here is already-persisted,
//! already-measured per-query data (P3-E02's/P3-E05's `eligible_queries_raw.csv`,
//! P3-E06's whole-corpus `whole_corpus_solr_ndcg.csv`), so this is pure
//! resampling arithmetic over existing evidence.
//!
//! Method: for each operating point, build one length-22,458 array where
//! each query holds its own two values -- its contribution under the
//! combined admission policy (native NDCG if admitted by either
//! mechanism at that point's caps, else its own Solr NDCG) and its own
//! pure-Solr NDCG. A *paired* percentile bootstrap resamples query
//! indices (not two independent samples) so each resample's whole-
//! workload-vs-Solr-only degradation is computed from the *same* drawn
//! queries for both quantities, correctly propagating the correlation
//! between them (bench_harness's own `bootstrap_ci_diff_of_means` is for
//! two *independent* sample sets, e.g. two separate latency arms -- not
//! this single-population, paired-by-construction case, so this binary
//! implements its own small percentile-bootstrap loop directly, using
//! the same seeded-`ChaCha8Rng` convention as every other Phase 3 sampler
//! for reproducibility).
//!
//! Usage: cargo run --release -p phase3-eval --bin p3e07_bootstrap_ci
//!        [p3e02_csv] [p3e05_csv] [p3e06_whole_corpus_csv]

use std::collections::HashMap;
use std::path::PathBuf;

use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;

const RESAMPLES: usize = 5000;
const ALPHA: f64 = 0.05;
const SEED: u64 = 7;

struct StructuralRow {
    candidates: u64,
    native_ndcg: f64,
}
struct AnchoredRow {
    combined_count: u64,
    native_ndcg: f64,
}

fn read_csv_col_f64(line: &str, idx: usize) -> f64 {
    line.split(',').nth(idx).unwrap().parse().unwrap()
}
fn read_csv_col_u64(line: &str, idx: usize) -> u64 {
    line.split(',').nth(idx).unwrap().parse().unwrap()
}

fn percentile_bootstrap_mean_ci(
    values: &[f64],
    resamples: usize,
    alpha: f64,
    seed: u64,
) -> (f64, f64, f64) {
    let n = values.len();
    let point = values.iter().sum::<f64>() / n as f64;
    let mut rng = ChaCha8Rng::seed_from_u64(seed);
    let mut means: Vec<f64> = (0..resamples)
        .map(|_| {
            let sum: f64 = (0..n).map(|_| values[rng.gen_range(0..n)]).sum();
            sum / n as f64
        })
        .collect();
    means.sort_by(f64::total_cmp);
    let lo_rank = ((alpha / 2.0) * (resamples - 1) as f64).round() as usize;
    let hi_rank = ((1.0 - alpha / 2.0) * (resamples - 1) as f64).round() as usize;
    (point, means[lo_rank], means[hi_rank.min(resamples - 1)])
}

/// Paired bootstrap for `mean(baseline) - mean(policy)` (the degradation),
/// resampling query INDICES once per iteration so both quantities are
/// computed from the identical drawn queries each time.
fn paired_bootstrap_degradation_ci(
    policy_values: &[f64],
    baseline_values: &[f64],
    resamples: usize,
    alpha: f64,
    seed: u64,
) -> (f64, f64, f64) {
    assert_eq!(policy_values.len(), baseline_values.len());
    let n = policy_values.len();
    let point = baseline_values.iter().sum::<f64>() / n as f64
        - policy_values.iter().sum::<f64>() / n as f64;
    let mut rng = ChaCha8Rng::seed_from_u64(seed);
    let mut diffs: Vec<f64> = (0..resamples)
        .map(|_| {
            let mut policy_sum = 0.0;
            let mut baseline_sum = 0.0;
            for _ in 0..n {
                let idx = rng.gen_range(0..n);
                policy_sum += policy_values[idx];
                baseline_sum += baseline_values[idx];
            }
            baseline_sum / n as f64 - policy_sum / n as f64
        })
        .collect();
    diffs.sort_by(f64::total_cmp);
    let lo_rank = ((alpha / 2.0) * (resamples - 1) as f64).round() as usize;
    let hi_rank = ((1.0 - alpha / 2.0) * (resamples - 1) as f64).round() as usize;
    (point, diffs[lo_rank], diffs[hi_rank.min(resamples - 1)])
}

fn main() {
    let mut args = std::env::args().skip(1);
    let p3e02_csv = args.next().map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from("docs/research/artifacts/p3e02_run1/eligible_queries_raw.csv")
    });
    let p3e05_csv = args.next().map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from("docs/research/artifacts/p3e05_run1/eligible_queries_raw.csv")
    });
    let p3e06_csv = args.next().map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from("docs/research/artifacts/p3e06_run1/whole_corpus_solr_ndcg.csv")
    });

    println!("loading whole-corpus Solr baseline from {p3e06_csv:?}...");
    let mut solr_ndcg: HashMap<u64, f64> = HashMap::new();
    for line in std::fs::read_to_string(&p3e06_csv)
        .unwrap_or_else(|e| panic!("failed to read {p3e06_csv:?}: {e}"))
        .lines()
        .skip(1)
    {
        if line.is_empty() {
            continue;
        }
        let qid = read_csv_col_u64(line, 0);
        solr_ndcg.insert(qid, read_csv_col_f64(line, 1));
    }
    let total = solr_ndcg.len();
    println!("  {total} queries loaded");

    println!("loading structurally-eligible queries from {p3e02_csv:?}...");
    let mut structural: HashMap<u64, StructuralRow> = HashMap::new();
    for line in std::fs::read_to_string(&p3e02_csv)
        .unwrap_or_else(|e| panic!("failed to read {p3e02_csv:?}: {e}"))
        .lines()
        .skip(1)
    {
        if line.is_empty() {
            continue;
        }
        let qid = read_csv_col_u64(line, 0);
        structural.insert(
            qid,
            StructuralRow {
                candidates: read_csv_col_u64(line, 1),
                native_ndcg: read_csv_col_f64(line, 2),
            },
        );
    }
    println!(
        "  {} structurally-eligible queries loaded",
        structural.len()
    );

    println!(
        "loading structurally-anchored lexically-narrowed eligible queries from {p3e05_csv:?}..."
    );
    let mut anchored: HashMap<u64, AnchoredRow> = HashMap::new();
    for line in std::fs::read_to_string(&p3e05_csv)
        .unwrap_or_else(|e| panic!("failed to read {p3e05_csv:?}: {e}"))
        .lines()
        .skip(1)
    {
        if line.is_empty() {
            continue;
        }
        let qid = read_csv_col_u64(line, 0);
        anchored.insert(
            qid,
            AnchoredRow {
                combined_count: read_csv_col_u64(line, 1),
                native_ndcg: read_csv_col_f64(line, 2),
            },
        );
    }
    println!(
        "  {} anchored-lexical eligible queries loaded",
        anchored.len()
    );

    let baseline_values: Vec<f64> = solr_ndcg.values().copied().collect();
    let baseline_mean = baseline_values.iter().sum::<f64>() / total as f64;
    println!("\nwhole-workload pure-Solr-only baseline NDCG@10: {baseline_mean:.4} (sanity check: matches P3-E02/E03/E05/E06's 0.2335)");

    let operating_points: &[(&str, usize, usize)] = &[
        ("budget<=1.0% promoted point", 250, 1),
        ("budget<=2.0% promoted point", 50, 20),
    ];

    for &(label, structural_cap, anchored_cap) in operating_points {
        println!("\n=== {label}: structural_cap={structural_cap}, anchored_lexical_cap={anchored_cap} ===");
        // Sorted, not raw HashMap iteration order: HashMap's default
        // hasher is randomized per-process, so an unsorted collect here
        // would feed a different array order into the seeded RNG on
        // every run, silently breaking the "deterministic given seed"
        // reproducibility this project's own bootstrap convention
        // requires (bench_harness::bootstrap_ci_diff_of_means's own doc
        // comment states this explicitly) -- caught by running this
        // binary twice and comparing output before trusting it.
        let mut qids: Vec<u64> = solr_ndcg.keys().copied().collect();
        qids.sort_unstable();
        let mut policy_values = Vec::with_capacity(total);
        let mut baseline_values_paired = Vec::with_capacity(total);
        let mut admitted = 0usize;
        for &qid in &qids {
            let solr_v = solr_ndcg[&qid];
            baseline_values_paired.push(solr_v);
            if let Some(row) = structural.get(&qid) {
                if row.candidates as usize <= structural_cap {
                    policy_values.push(row.native_ndcg);
                    admitted += 1;
                    continue;
                }
            }
            if let Some(row) = anchored.get(&qid) {
                if row.combined_count as usize <= anchored_cap {
                    policy_values.push(row.native_ndcg);
                    admitted += 1;
                    continue;
                }
            }
            policy_values.push(solr_v);
        }
        let coverage_pct = admitted as f64 / total as f64 * 100.0;

        let (point_wl, wl_lo, wl_hi) =
            percentile_bootstrap_mean_ci(&policy_values, RESAMPLES, ALPHA, SEED);
        let (point_degrad, degrad_lo, degrad_hi) = paired_bootstrap_degradation_ci(
            &policy_values,
            &baseline_values_paired,
            RESAMPLES,
            ALPHA,
            SEED,
        );

        println!(
            "  admitted: {admitted}/{total} ({coverage_pct:.2}% coverage) -- {RESAMPLES} paired bootstrap resamples, alpha={ALPHA}"
        );
        println!("  whole-workload NDCG: point={point_wl:.4}, 95% CI=[{wl_lo:.4}, {wl_hi:.4}]");
        println!(
            "  degradation vs. Solr-only (baseline - policy): point={point_degrad:.4} ({:.2}% relative), 95% CI=[{degrad_lo:.4}, {degrad_hi:.4}] ({:.2}%, {:.2}% relative)",
            point_degrad / baseline_mean * 100.0,
            degrad_lo / baseline_mean * 100.0,
            degrad_hi / baseline_mean * 100.0,
        );
        println!(
            "  CI excludes zero: {} (a CI including zero would mean this degradation is not \
             statistically distinguishable from no degradation at all, at this corpus size)",
            degrad_lo > 0.0 || degrad_hi < 0.0
        );
    }
}
