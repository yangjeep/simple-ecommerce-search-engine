//! Issue #14/#18 P3-E09: real whole-workload verdict for a third,
//! disjoint admission mechanism P3-E08's diagnostic surfaced -- residual-
//! lexical queries with exactly ONE residual token, regardless of
//! whether a structural constraint also exists. P3-E08 found this
//! sub-population (which excludes anything P3-E05's structurally-anchored
//! mechanism already covers, since that requires a structural constraint)
//! has a strikingly better precision profile than 2-token or 3+-token
//! residuals: at cap=20, delta=-0.0731 with only 1.8% false-positive rate
//! (5/279) -- lower than P3-E05's own anchored mechanism at the same cap
//! (5.14%).
//!
//! Requires NO new Solr querying: every input is already-persisted,
//! already-measured per-query data. P3-E03's `eligible_queries_raw.csv`
//! already carries each eligible query's REAL native NDCG (computed by
//! actually executing `execute_lexically_narrowed` during that run, not
//! an approximation) and its real Solr NDCG for the same query; P3-E06's
//! `whole_corpus_solr_ndcg.csv` supplies every other (non-admitted)
//! query's own real Solr score for the whole-workload computation. This
//! binary is pure re-aggregation of existing evidence, following the
//! same "isolated marginal contribution" methodology P3-E03/P3-E05 used
//! (every query this mechanism does not admit -- including ones the other
//! two KEPT mechanisms would separately admit -- scores as a Solr
//! fallback here, so this is a fair standalone read of the new lever
//! before folding it into a P3-E06-style combined measurement).
//!
//! Usage: cargo run --release -p phase3-eval --bin p3e09_single_token_lexical_eval
//!        [p3e03_eligible_csv] [p3e05_eligible_csv] [p3e06_whole_corpus_csv]

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

const SWEEP: &[usize] = &[
    1, 2, 3, 5, 10, 20, 30, 50, 75, 100, 150, 250, 500, 1_000, 2_500, 5_000, 10_000, 50_000,
    200_000,
];

struct Row {
    combined_count: u64,
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

fn main() {
    let mut args = std::env::args().skip(1);
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
        solr_ndcg.insert(col_u64(line, 0), col_f64(line, 1));
    }
    let total = solr_ndcg.len();
    let solr_only_ndcg_mean: f64 = solr_ndcg.values().sum::<f64>() / total as f64;
    println!("  {total} queries loaded; whole-workload pure-Solr-only baseline NDCG@10: {solr_only_ndcg_mean:.4}");

    println!("loading structurally-anchored qids from {p3e05_csv:?} (to exclude -- already a disjoint, separately-measured mechanism)...");
    let anchored_qids: HashSet<u64> = std::fs::read_to_string(&p3e05_csv)
        .unwrap_or_else(|e| panic!("failed to read {p3e05_csv:?}: {e}"))
        .lines()
        .skip(1)
        .filter(|l| !l.is_empty())
        .map(|l| col_u64(l, 0))
        .collect();
    println!(
        "  {} structurally-anchored qids excluded",
        anchored_qids.len()
    );

    println!("loading P3-E03's eligible population, filtering to single-residual-token pure-lexical-only from {p3e03_csv:?}...");
    let mut eligible: HashMap<u64, Row> = HashMap::new();
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
        let residual_token_count = col_usize(line, 2);
        if residual_token_count != 1 {
            continue;
        }
        eligible.insert(
            qid,
            Row {
                combined_count: col_u64(line, 1),
                native_ndcg: col_f64(line, 3),
            },
        );
    }
    println!(
        "  {} single-residual-token, non-anchored eligible queries ({:.2}% of whole corpus, cap-independent)",
        eligible.len(),
        eligible.len() as f64 / total as f64 * 100.0
    );

    println!("\n=== P3-E09 single-token pure-lexical coverage/relevance frontier (isolated marginal contribution) ===");
    println!(
        "{:>10} {:>10} {:>9} {:>10} {:>10} {:>10} {:>12} {:>10} {:>10}",
        "cap",
        "admitted",
        "cov%_elig",
        "cov%_all",
        "native_ndcg",
        "solr_ndcg_sub",
        "ndcg_delta",
        "whole_wl_ndcg",
        "wl_degrad"
    );
    let mut frontier_csv = String::from(
        "max_candidates,admitted,coverage_pct_of_eligible,coverage_pct_of_whole_corpus,native_ndcg_mean,solr_ndcg_on_admitted_mean,ndcg_delta_on_admitted,whole_workload_ndcg,whole_workload_degradation,false_positive_admissions\n",
    );
    for &cap in SWEEP {
        let admitted: Vec<(&u64, &Row)> = eligible
            .iter()
            .filter(|(_, r)| r.combined_count as usize <= cap)
            .collect();
        let admitted_count = admitted.len();
        let coverage_pct_of_eligible = admitted_count as f64 / eligible.len() as f64 * 100.0;
        let coverage_pct_of_whole = admitted_count as f64 / total as f64 * 100.0;

        let native_ndcg_sum: f64 = admitted.iter().map(|(_, r)| r.native_ndcg).sum();
        let native_ndcg_mean = if admitted_count > 0 {
            native_ndcg_sum / admitted_count as f64
        } else {
            0.0
        };
        let solr_on_admitted_sum: f64 = admitted.iter().map(|(qid, _)| solr_ndcg[qid]).sum();
        let solr_on_admitted_mean = if admitted_count > 0 {
            solr_on_admitted_sum / admitted_count as f64
        } else {
            0.0
        };
        let ndcg_delta_on_admitted = native_ndcg_mean - solr_on_admitted_mean;

        let admitted_qids: HashSet<u64> = admitted.iter().map(|(&qid, _)| qid).collect();
        let rest_solr_sum: f64 = solr_ndcg
            .iter()
            .filter(|(qid, _)| !admitted_qids.contains(qid))
            .map(|(_, n)| n)
            .sum();
        let whole_workload_ndcg = (native_ndcg_sum + rest_solr_sum) / total as f64;
        let whole_workload_degradation = solr_only_ndcg_mean - whole_workload_ndcg;

        let false_positive_admissions = admitted
            .iter()
            .filter(|(qid, r)| r.native_ndcg == 0.0 && solr_ndcg[qid] > 0.0)
            .count();

        println!(
            "{:>10} {:>10} {:>8.2}% {:>8.2}% {:>10.4} {:>10.4} {:>+10.4} {:>12.4} {:>+10.4}",
            cap,
            admitted_count,
            coverage_pct_of_eligible,
            coverage_pct_of_whole,
            native_ndcg_mean,
            solr_on_admitted_mean,
            ndcg_delta_on_admitted,
            whole_workload_ndcg,
            whole_workload_degradation,
        );
        frontier_csv.push_str(&format!(
            "{cap},{admitted_count},{coverage_pct_of_eligible},{coverage_pct_of_whole},{native_ndcg_mean},{solr_on_admitted_mean},{ndcg_delta_on_admitted},{whole_workload_ndcg},{whole_workload_degradation},{false_positive_admissions}\n"
        ));
    }
    let artifacts_dir = PathBuf::from("dataset_cache/p3e09_artifacts");
    std::fs::create_dir_all(&artifacts_dir).ok();
    std::fs::write(artifacts_dir.join("frontier_sweep.csv"), &frontier_csv).ok();

    println!("\n=== relevance-budget calibration (Issue #14 RQ2, single-token pure-lexical mechanism) ===");
    for budget_pct in [0.0, 0.5, 1.0, 2.0] {
        let mut best: Option<(usize, usize, f64)> = None;
        for &cap in SWEEP {
            let admitted: Vec<(&u64, &Row)> = eligible
                .iter()
                .filter(|(_, r)| r.combined_count as usize <= cap)
                .collect();
            let admitted_count = admitted.len();
            let native_ndcg_sum: f64 = admitted.iter().map(|(_, r)| r.native_ndcg).sum();
            let admitted_qids: HashSet<u64> = admitted.iter().map(|(&qid, _)| qid).collect();
            let rest_solr_sum: f64 = solr_ndcg
                .iter()
                .filter(|(qid, _)| !admitted_qids.contains(qid))
                .map(|(_, n)| n)
                .sum();
            let whole_workload_ndcg = (native_ndcg_sum + rest_solr_sum) / total as f64;
            let degradation_pct =
                (solr_only_ndcg_mean - whole_workload_ndcg) / solr_only_ndcg_mean * 100.0;
            if degradation_pct <= budget_pct {
                let coverage = admitted_count as f64 / total as f64 * 100.0;
                if best.is_none_or(|(_, c, _)| admitted_count > c) {
                    best = Some((cap, admitted_count, coverage));
                }
            }
        }
        match best {
            Some((cap, count, coverage)) => println!(
                "  budget<={budget_pct:.1}%: best cap={cap}, coverage={count}/{total} ({coverage:.2}% of whole corpus)"
            ),
            None => println!(
                "  budget<={budget_pct:.1}%: no swept cap value stays within this budget"
            ),
        }
    }
    println!("\nartifacts written to {}", artifacts_dir.display());
}
