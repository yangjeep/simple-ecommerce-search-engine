//! Issue #14/#18 P3-E10: the three-way combined safe-offload Pareto
//! frontier. P3-E06 measured the combined system of two mechanisms
//! (structural `admit()` + `admit_structurally_anchored_lexical`) and
//! found their contributions genuinely additive once disjointness was
//! explicitly verified rather than assumed. P3-E09 established a third,
//! independently-KEPT, disjoint mechanism (`admit_single_token_lexical`).
//! Three isolated "clears budget" results do not automatically imply
//! their sum clears budget when combined -- this measures the combined
//! system directly, exactly as P3-E06 insisted on doing for two
//! mechanisms.
//!
//! Requires NO new Solr querying: every input is already-persisted,
//! already-measured per-query data (P3-E02's/P3-E05's
//! `eligible_queries_raw.csv`, P3-E03's for the single-token population
//! -- re-derived with the same filter P3-E08/P3-E09 used -- and P3-E06's
//! whole-corpus `whole_corpus_solr_ndcg.csv`).
//!
//! Native latency is NOT re-measured here: `admit_single_token_lexical`
//! executes via the identical `execute_lexically_narrowed`/
//! `execute_ranked_narrowed_by` path already measured at ~0.0011-0.0015ms
//! on similarly small candidate sets in P3-E02/P3-E05, and this
//! experiment's own eligible population (824 queries, cap-independently)
//! is of the same shape. Re-running a fresh latency campaign against the
//! same execution function under materially the same conditions would be
//! very unlikely to reveal new information; this choice is stated
//! explicitly here rather than silently presented as a fresh measurement.
//!
//! Usage: cargo run --release -p phase3-eval --bin p3e10_three_way_combined_frontier
//!        [p3e02_csv] [p3e03_csv] [p3e05_csv] [p3e06_whole_corpus_csv]

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

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
    let solr_ndcg: HashMap<u64, f64> = std::fs::read_to_string(&p3e06_csv)
        .unwrap_or_else(|e| panic!("failed to read {p3e06_csv:?}: {e}"))
        .lines()
        .skip(1)
        .filter(|l| !l.is_empty())
        .map(|l| (col_u64(l, 0), col_f64(l, 1)))
        .collect();
    let total = solr_ndcg.len();
    let solr_only_ndcg_mean: f64 = solr_ndcg.values().sum::<f64>() / total as f64;
    println!("  {total} queries loaded; whole-workload pure-Solr-only baseline NDCG@10: {solr_only_ndcg_mean:.4}");

    println!("loading structural admit()-eligible population from {p3e02_csv:?}...");
    // P3-E02's own CSV columns: qid,candidates,native_ndcg,...
    let structural = load_rows(&p3e02_csv, 1, 2);
    println!("  {} structurally eligible queries", structural.len());

    println!("loading structurally-anchored lexical eligible population from {p3e05_csv:?}...");
    // P3-E05's own CSV columns: qid,combined_count,native_ndcg,...
    let anchored = load_rows(&p3e05_csv, 1, 2);
    println!("  {} anchored-lexical eligible queries", anchored.len());

    println!("deriving single-token pure-lexical eligible population from {p3e03_csv:?} (same filter P3-E08/P3-E09 used: residual_token_count==1, excluding anchored qids)...");
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
    println!(
        "  {} single-token pure-lexical eligible queries",
        single_token.len()
    );

    println!("\ndisjointness check across all three eligible populations...");
    let structural_qids: HashSet<u64> = structural.keys().copied().collect();
    let single_token_qids: HashSet<u64> = single_token.keys().copied().collect();
    let structural_anchored_overlap = structural_qids.intersection(&anchored_qids).count();
    let structural_single_overlap = structural_qids.intersection(&single_token_qids).count();
    let anchored_single_overlap = anchored_qids.intersection(&single_token_qids).count();
    println!(
        "  structural x anchored overlap: {structural_anchored_overlap}, structural x single-token overlap: {structural_single_overlap}, anchored x single-token overlap: {anchored_single_overlap}"
    );
    assert_eq!(
        structural_anchored_overlap + structural_single_overlap + anchored_single_overlap,
        0,
        "all three eligible populations must be pairwise disjoint by construction -- a nonzero \
         overlap here means the combined-coverage arithmetic below would double-count queries"
    );
    println!("  confirmed: all three populations are pairwise disjoint");

    // A small, representative grid: each mechanism's own two most
    // information-dense cap values from its own P3-E0{2,5,9} budget
    // calibration (a tight-budget point and a looser one), mirroring
    // P3-E06's own grid-sweep methodology rather than an exhaustive
    // cross-product.
    let structural_caps = [50u64, 250];
    let anchored_caps = [1u64, 20];
    let single_token_caps = [20u64, 200_000];

    println!("\n=== P3-E10 three-way combined safe-offload Pareto frontier ===");
    println!(
        "{:>8} {:>8} {:>10} {:>10} {:>10} {:>10} {:>8} {:>12} {:>10}",
        "s_cap",
        "a_cap",
        "t_cap",
        "structural",
        "anchored",
        "single_tok",
        "cov%",
        "whole_ndcg",
        "degrad"
    );
    let mut csv = String::from(
        "structural_cap,anchored_cap,single_token_cap,structural_admitted,anchored_admitted,single_token_admitted,coverage_pct,whole_workload_ndcg,whole_workload_degradation\n",
    );
    for &s_cap in &structural_caps {
        for &a_cap in &anchored_caps {
            for &t_cap in &single_token_caps {
                let s_admitted: Vec<(&u64, &Row)> = structural
                    .iter()
                    .filter(|(_, r)| r.cap_metric <= s_cap)
                    .collect();
                let a_admitted: Vec<(&u64, &Row)> = anchored
                    .iter()
                    .filter(|(_, r)| r.cap_metric <= a_cap)
                    .collect();
                let t_admitted: Vec<(&u64, &Row)> = single_token
                    .iter()
                    .filter(|(_, r)| r.cap_metric <= t_cap)
                    .collect();

                let mut admitted_qids: HashSet<u64> = HashSet::new();
                let mut native_ndcg_sum = 0.0;
                for (&qid, r) in s_admitted
                    .iter()
                    .chain(a_admitted.iter())
                    .chain(t_admitted.iter())
                {
                    admitted_qids.insert(qid);
                    native_ndcg_sum += r.native_ndcg;
                }
                let admitted_count = admitted_qids.len();
                let coverage_pct = admitted_count as f64 / total as f64 * 100.0;

                let rest_solr_sum: f64 = solr_ndcg
                    .iter()
                    .filter(|(qid, _)| !admitted_qids.contains(qid))
                    .map(|(_, n)| n)
                    .sum();
                let whole_workload_ndcg = (native_ndcg_sum + rest_solr_sum) / total as f64;
                let whole_workload_degradation = solr_only_ndcg_mean - whole_workload_ndcg;

                println!(
                    "{:>8} {:>8} {:>10} {:>10} {:>10} {:>10} {:>7.2}% {:>12.4} {:>+10.4}",
                    s_cap,
                    a_cap,
                    t_cap,
                    s_admitted.len(),
                    a_admitted.len(),
                    t_admitted.len(),
                    coverage_pct,
                    whole_workload_ndcg,
                    whole_workload_degradation
                );
                csv.push_str(&format!(
                    "{s_cap},{a_cap},{t_cap},{},{},{},{coverage_pct},{whole_workload_ndcg},{whole_workload_degradation}\n",
                    s_admitted.len(),
                    a_admitted.len(),
                    t_admitted.len()
                ));
            }
        }
    }
    let artifacts_dir = PathBuf::from("dataset_cache/p3e10_artifacts");
    std::fs::create_dir_all(&artifacts_dir).ok();
    std::fs::write(artifacts_dir.join("combined_frontier.csv"), &csv).ok();
    println!("\nartifacts written to {}", artifacts_dir.display());
}
