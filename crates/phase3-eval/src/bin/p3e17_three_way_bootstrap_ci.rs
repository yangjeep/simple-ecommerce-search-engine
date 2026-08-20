//! Issue #14 P3-E17: bootstrap confidence intervals for P3-E16's
//! fine-grained three-way combined operating points.
//!
//! P3-E07 supplied paired-bootstrap CIs for the two-mechanism operating
//! points P3-E06 promoted. P3-E16 found materially better three-mechanism
//! operating points via an exact algebraic re-aggregation of already-
//! published per-mechanism cap sweeps, but deliberately did not attach a
//! CI to them (a genuinely new measurement, not free algebra, was
//! explicitly flagged as follow-up work rather than smuggled into a
//! point-estimate-only experiment). This is that follow-up: extends
//! P3-E07's exact paired-bootstrap methodology from two admission
//! mechanisms to three, over the three promoted P3-E16 operating points.
//!
//! No new Solr querying: every input is already-persisted, already-
//! measured per-query data (P3-E02's/P3-E05's `eligible_queries_raw.csv`,
//! P3-E03's for the single-token population -- re-derived with the same
//! filter P3-E08/P3-E09/P3-E10 established -- and P3-E06's whole-corpus
//! `whole_corpus_solr_ndcg.csv`).
//!
//! Usage: cargo run --release -p phase3-eval --bin p3e17_three_way_bootstrap_ci
//!        [p3e02_csv] [p3e03_csv] [p3e05_csv] [p3e06_whole_corpus_csv]

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;

const RESAMPLES: usize = 5000;
const ALPHA: f64 = 0.05;
const SEED: u64 = 7;

struct Row {
    cap_metric: u64,
    native_ndcg: f64,
}

fn col_u64(line: &str, idx: usize) -> u64 {
    line.split(',').nth(idx).unwrap().parse().unwrap()
}
fn col_usize(line: &str, idx: usize) -> usize {
    line.split(',').nth(idx).unwrap().parse().unwrap()
}
fn col_f64(line: &str, idx: usize) -> f64 {
    line.split(',').nth(idx).unwrap().parse().unwrap()
}

fn load_rows(path: &PathBuf, cap_col: usize, ndcg_col: usize) -> HashMap<u64, Row> {
    std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("failed to read {path:?}: {e}"))
        .lines()
        .skip(1)
        .filter(|l| !l.is_empty())
        .map(|l| {
            (
                col_u64(l, 0),
                Row {
                    cap_metric: col_u64(l, cap_col),
                    native_ndcg: col_f64(l, ndcg_col),
                },
            )
        })
        .collect()
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
    let p3e03_csv = args.next().map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from("docs/research/artifacts/p3e03_run1/eligible_queries_raw.csv")
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
        let qid = col_u64(line, 0);
        solr_ndcg.insert(qid, col_f64(line, 1));
    }
    let total = solr_ndcg.len();
    let baseline_mean = solr_ndcg.values().sum::<f64>() / total as f64;
    println!("  {total} queries loaded; whole-workload pure-Solr-only baseline NDCG@10: {baseline_mean:.4}");

    println!("loading structurally-eligible queries from {p3e02_csv:?}...");
    let structural = load_rows(&p3e02_csv, 1, 2);
    println!("  {} queries", structural.len());

    println!(
        "loading structurally-anchored lexically-narrowed eligible queries from {p3e05_csv:?}..."
    );
    let anchored = load_rows(&p3e05_csv, 1, 2);
    println!("  {} queries", anchored.len());

    println!("deriving single-token pure-lexical eligible population from {p3e03_csv:?} (residual_token_count==1, excluding anchored qids)...");
    let anchored_qids: HashSet<u64> = anchored.keys().copied().collect();
    let mut single_token: HashMap<u64, Row> = HashMap::new();
    for line in std::fs::read_to_string(&p3e03_csv)
        .unwrap_or_else(|e| panic!("failed to read {p3e03_csv:?}: {e}"))
        .lines()
        .skip(1)
    {
        if line.is_empty() {
            continue;
        }
        let qid = col_u64(line, 0);
        if anchored_qids.contains(&qid) {
            continue;
        }
        if col_usize(line, 2) != 1 {
            continue;
        }
        single_token.insert(
            qid,
            Row {
                cap_metric: col_u64(line, 1),
                native_ndcg: col_f64(line, 3),
            },
        );
    }
    println!("  {} queries", single_token.len());

    // The three P3-E16 promoted operating points, one per RQ2 budget.
    let operating_points: &[(&str, u64, u64, u64)] = &[
        ("budget<=0.5% promoted point", 2, 1, 2),
        ("budget<=1.0% promoted point", 50, 1, 20),
        ("budget<=2.0% promoted point", 2, 20, 10),
    ];

    // Sorted, not raw HashMap iteration order -- P3-E07's own determinism
    // lesson (HashMap's default hasher is randomized per-process).
    let mut qids: Vec<u64> = solr_ndcg.keys().copied().collect();
    qids.sort_unstable();

    for &(label, s_cap, a_cap, t_cap) in operating_points {
        println!(
            "\n=== {label}: structural_cap={s_cap}, anchored_cap={a_cap}, single_token_cap={t_cap} ==="
        );
        let mut policy_values = Vec::with_capacity(total);
        let mut baseline_values_paired = Vec::with_capacity(total);
        let mut admitted = 0usize;
        for &qid in &qids {
            let solr_v = solr_ndcg[&qid];
            baseline_values_paired.push(solr_v);
            if let Some(row) = structural.get(&qid) {
                if row.cap_metric <= s_cap {
                    policy_values.push(row.native_ndcg);
                    admitted += 1;
                    continue;
                }
            }
            if let Some(row) = anchored.get(&qid) {
                if row.cap_metric <= a_cap {
                    policy_values.push(row.native_ndcg);
                    admitted += 1;
                    continue;
                }
            }
            if let Some(row) = single_token.get(&qid) {
                if row.cap_metric <= t_cap {
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
