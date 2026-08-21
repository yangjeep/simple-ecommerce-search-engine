//! Issue #14/#18 P3-E16: fine-grained three-way combined Pareto frontier.
//!
//! P3-E10 swept a coarse 2x2x2 grid (each mechanism's own two most
//! information-dense cap values) and explicitly flagged, in its own
//! Decision section, that "a finer cap search ... would likely find a
//! combined point closer to the true Pareto-optimal frontier ... since
//! the additive relationship makes the tradeoff surface exactly
//! characterizable from each mechanism's own already-measured per-cap
//! degradation without further Solr querying." This experiment is that
//! finer search.
//!
//! No new Solr querying, no new native execution, no new binary logic
//! beyond arithmetic: it is a pure re-aggregation of each mechanism's own
//! already-persisted, full 19-point `frontier_sweep.csv` (P3-E02, P3-E05,
//! P3-E09), justified by an exact algebraic identity rather than an
//! empirical spot-check. Since the three eligible populations are
//! pairwise disjoint by construction (verified explicitly in P3-E06/
//! P3-E10), each mechanism's own reported `whole_workload_degradation`
//! at a given cap already equals `(sum_solr_on_admitted -
//! sum_native_on_admitted) / total_queries` -- a quantity that does not
//! depend on what any *other* mechanism admits. Summing three such
//! isolated degradation values (and three disjoint admitted counts)
//! therefore reproduces the exact combined measurement P3-E10 would get
//! from a live combined run, for any cap triple, not just the 8 points
//! P3-E10 actually measured. This lets the full 19x19x19 = 6,859-point
//! grid (every cap value each mechanism's own budget-calibration sweep
//! already used) be searched in-process for the true Pareto-optimal
//! coverage at each of Issue #14's RQ2 budgets, instead of the coarse
//! grid's coarse answer.
//!
//! Usage: cargo run --release -p phase3-eval --bin p3e16_finegrained_three_way_frontier
//!        [p3e02_csv] [p3e05_csv] [p3e09_csv] [p3e06_whole_corpus_csv]

use std::path::PathBuf;

#[derive(Clone, Copy, Debug)]
struct CapPoint {
    cap: u64,
    admitted: u64,
    degradation: f64,
}

fn col_u64(line: &str, idx: usize) -> u64 {
    line.split(',').nth(idx).unwrap().parse().unwrap()
}
fn col_f64(line: &str, idx: usize) -> f64 {
    line.split(',').nth(idx).unwrap().parse().unwrap()
}

fn load_frontier(path: &PathBuf, admitted_col: usize, degradation_col: usize) -> Vec<CapPoint> {
    std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("failed to read {path:?}: {e}"))
        .lines()
        .skip(1)
        .filter(|l| !l.is_empty())
        .map(|l| CapPoint {
            cap: col_u64(l, 0),
            admitted: col_u64(l, admitted_col),
            degradation: col_f64(l, degradation_col),
        })
        .collect()
}

struct BestAtBudget {
    budget_pct: f64,
    best: Option<(CapPoint, CapPoint, CapPoint, u64, f64, f64)>,
}

fn main() {
    let mut args = std::env::args().skip(1);
    let p3e02_csv = args
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("docs/research/artifacts/p3e02_run1/frontier_sweep.csv"));
    let p3e05_csv = args
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("docs/research/artifacts/p3e05_run1/frontier_sweep.csv"));
    let p3e09_csv = args
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("docs/research/artifacts/p3e09_run1/frontier_sweep.csv"));
    let p3e06_csv = args.next().map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from("docs/research/artifacts/p3e06_run1/whole_corpus_solr_ndcg.csv")
    });

    println!("loading whole-corpus Solr baseline from {p3e06_csv:?}...");
    let solr_ndcg_sum: f64 = std::fs::read_to_string(&p3e06_csv)
        .unwrap_or_else(|e| panic!("failed to read {p3e06_csv:?}: {e}"))
        .lines()
        .skip(1)
        .filter(|l| !l.is_empty())
        .map(|l| col_f64(l, 1))
        .sum();
    let total: u64 = std::fs::read_to_string(&p3e06_csv)
        .unwrap()
        .lines()
        .skip(1)
        .filter(|l| !l.is_empty())
        .count() as u64;
    let solr_only_ndcg_mean = solr_ndcg_sum / total as f64;
    println!(
        "  {total} queries loaded; whole-workload pure-Solr-only baseline NDCG@10: {solr_only_ndcg_mean:.4}"
    );

    println!("loading structural admit() frontier from {p3e02_csv:?}...");
    let structural = load_frontier(&p3e02_csv, 1, 7);
    println!("  {} cap points", structural.len());

    println!("loading structurally-anchored lexical frontier from {p3e05_csv:?}...");
    let anchored = load_frontier(&p3e05_csv, 1, 8);
    println!("  {} cap points", anchored.len());

    println!("loading single-token lexical frontier from {p3e09_csv:?}...");
    let single_token = load_frontier(&p3e09_csv, 1, 8);
    println!("  {} cap points", single_token.len());

    let total_grid_points = structural.len() * anchored.len() * single_token.len();
    println!(
        "\n=== P3-E16 fine-grained three-way combined Pareto frontier: {} x {} x {} = {} grid points ===",
        structural.len(),
        anchored.len(),
        single_token.len(),
        total_grid_points
    );

    let mut csv = String::from(
        "structural_cap,anchored_cap,single_token_cap,structural_admitted,anchored_admitted,single_token_admitted,combined_admitted,coverage_pct,combined_degradation,degradation_relative_pct\n",
    );

    let budgets = [0.5f64, 1.0, 2.0];
    let mut best: Vec<BestAtBudget> = budgets
        .iter()
        .map(|&b| BestAtBudget {
            budget_pct: b,
            best: None,
        })
        .collect();

    for &s in &structural {
        for &a in &anchored {
            for &t in &single_token {
                let combined_admitted = s.admitted + a.admitted + t.admitted;
                let coverage_pct = combined_admitted as f64 / total as f64 * 100.0;
                let combined_degradation = s.degradation + a.degradation + t.degradation;
                let relative_pct = combined_degradation / solr_only_ndcg_mean * 100.0;

                csv.push_str(&format!(
                    "{},{},{},{},{},{},{combined_admitted},{coverage_pct},{combined_degradation},{relative_pct}\n",
                    s.cap, a.cap, t.cap, s.admitted, a.admitted, t.admitted
                ));

                for entry in best.iter_mut() {
                    if relative_pct <= entry.budget_pct {
                        let is_better = match entry.best {
                            None => true,
                            Some((_, _, _, best_admitted, _, _)) => {
                                combined_admitted > best_admitted
                            }
                        };
                        if is_better {
                            entry.best =
                                Some((s, a, t, combined_admitted, coverage_pct, relative_pct));
                        }
                    }
                }
            }
        }
    }

    println!("\n--- Pareto-optimal point per RQ2 budget (max coverage subject to relative degradation <= budget) ---");
    for entry in &best {
        match entry.best {
            Some((s, a, t, admitted, coverage_pct, relative_pct)) => {
                println!(
                    "budget<={:>4.1}%: structural_cap={:<7} anchored_cap={:<7} single_token_cap={:<7} \
                     combined_admitted={:<6} coverage={:>6.2}% degradation={:>5.2}% relative",
                    entry.budget_pct, s.cap, a.cap, t.cap, admitted, coverage_pct, relative_pct
                );
            }
            None => println!(
                "budget<={:>4.1}%: no grid point clears this budget",
                entry.budget_pct
            ),
        }
    }

    let artifacts_dir = PathBuf::from("dataset_cache/p3e16_artifacts");
    std::fs::create_dir_all(&artifacts_dir).ok();
    std::fs::write(
        artifacts_dir.join("finegrained_combined_frontier.csv"),
        &csv,
    )
    .ok();
    println!(
        "\nfull {}-point grid written to {}",
        total_grid_points,
        artifacts_dir
            .join("finegrained_combined_frontier.csv")
            .display()
    );
}
