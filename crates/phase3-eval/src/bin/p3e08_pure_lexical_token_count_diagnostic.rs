//! Issue #14/#18 P3-E08 diagnostic (no Solr call needed -- reuses P3-E03's
//! and P3-E05's already-measured per-query data): P3-E04/P3-E05 found and
//! recovered the structurally-anchored slice of P3-E03's REJECTed
//! population. The remaining pure-lexical-only slice (no existing
//! structural constraint alongside the residual text) is still REJECTed
//! in aggregate -- but per Issue #18's Workstream A mining loop ("keep
//! mining the highest-volume rejected class until additional safe
//! coverage attempts are exhausted or a clear structural boundary is
//! established"), the aggregate REJECT should not be accepted as final
//! without checking whether *this* population has its own internal
//! structure worth exploiting before moving on.
//!
//! Hypothesis under test: a single-token residual (often a specific,
//! low-ambiguity term -- a model name, a rare descriptor) is a stronger
//! precision signal than a multi-token residual (more often a generic
//! descriptive phrase, e.g. "without full grille bar", "24 volt electric
//! plug" -- P3-E03's own cited false-positive examples). If true, a
//! residual-token-count cap (admit only 1-token residuals) might recover
//! some further safe coverage the aggregate measurement obscures; if the
//! precision profile is flat across token counts, that is real evidence
//! this specific axis is exhausted and the pure-lexical-only population's
//! REJECT should stand without further narrow-casting attempts on it.
//!
//! Method: reads P3-E03's full eligible population (`eligible_queries_raw.csv`,
//! which already carries `residual_token_count` per query) and P3-E05's
//! eligible population (to identify which qids are structurally-anchored
//! and must be excluded here -- this diagnostic is about the *pure-lexical-only*
//! remainder specifically, not double-counting P3-E05's already-recovered
//! slice). Buckets the remainder by `residual_token_count` (1, 2, 3+) at
//! the same four representative cap points P3-E03/P3-E04 used.
//!
//! Usage: cargo run --release -p phase3-eval --bin p3e08_pure_lexical_token_count_diagnostic
//!        [p3e03_eligible_csv] [p3e05_eligible_csv]

use std::collections::HashSet;
use std::path::PathBuf;

struct Row {
    combined_count: u64,
    residual_token_count: usize,
    native_ndcg: f64,
    solr_ndcg: f64,
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
fn col_u64_first(line: &str) -> u64 {
    col_u64(line, 0)
}

fn main() {
    let mut args = std::env::args().skip(1);
    let p3e03_csv = args.next().map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from("docs/research/artifacts/p3e03_run1/eligible_queries_raw.csv")
    });
    let p3e05_csv = args.next().map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from("docs/research/artifacts/p3e05_run1/eligible_queries_raw.csv")
    });

    println!("loading structurally-anchored qids from {p3e05_csv:?}...");
    let anchored_qids: HashSet<u64> = std::fs::read_to_string(&p3e05_csv)
        .unwrap_or_else(|e| panic!("failed to read {p3e05_csv:?}: {e}"))
        .lines()
        .skip(1)
        .filter(|l| !l.is_empty())
        .map(col_u64_first)
        .collect();
    println!(
        "  {} structurally-anchored qids loaded",
        anchored_qids.len()
    );

    println!("loading P3-E03's full eligible population from {p3e03_csv:?}...");
    let mut pure_lexical: Vec<Row> = Vec::new();
    let mut skipped_anchored = 0usize;
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
            skipped_anchored += 1;
            continue;
        }
        pure_lexical.push(Row {
            combined_count: col_u64(line, 1),
            residual_token_count: col_usize(line, 2),
            native_ndcg: col_f64(line, 3),
            solr_ndcg: col_f64(line, 6),
        });
    }
    println!(
        "  {} pure-lexical-only rows ({} excluded as structurally-anchored, matching P3-E05's own eligible count)",
        pure_lexical.len(),
        skipped_anchored
    );

    println!("\n=== P3-E08 diagnostic: pure-lexical-only, segmented by residual token count ===");
    println!(
        "{:>10} {:>14} {:>10} {:>12} {:>12} {:>10} {:>10}",
        "cap", "token_count", "admitted", "native_ndcg", "solr_ndcg", "delta", "false_pos"
    );
    for &cap in &[1u64, 20, 250, u64::MAX] {
        for bucket in ["1", "2", "3+"] {
            let admitted: Vec<&Row> = pure_lexical
                .iter()
                .filter(|r| {
                    r.combined_count <= cap
                        && match bucket {
                            "1" => r.residual_token_count == 1,
                            "2" => r.residual_token_count == 2,
                            _ => r.residual_token_count >= 3,
                        }
                })
                .collect();
            let n = admitted.len();
            let native_mean = if n > 0 {
                admitted.iter().map(|r| r.native_ndcg).sum::<f64>() / n as f64
            } else {
                0.0
            };
            let solr_mean = if n > 0 {
                admitted.iter().map(|r| r.solr_ndcg).sum::<f64>() / n as f64
            } else {
                0.0
            };
            let false_pos = admitted
                .iter()
                .filter(|r| r.native_ndcg == 0.0 && r.solr_ndcg > 0.0)
                .count();
            let cap_label = if cap == u64::MAX {
                "unlimited".to_string()
            } else {
                cap.to_string()
            };
            println!(
                "{:>10} {:>14} {:>10} {:>12.4} {:>12.4} {:>+10.4} {:>9}/{}",
                cap_label,
                bucket,
                n,
                native_mean,
                solr_mean,
                native_mean - solr_mean,
                false_pos,
                n
            );
        }
    }
}
