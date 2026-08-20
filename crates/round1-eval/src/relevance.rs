//! Shared NDCG@k/Recall@k/MRR scoring, factored out of Phase 2's
//! `phase2-eval::p1d_physical_advantage_eval` (P2-E12's HashMap-iteration-
//! order floating-point noise fix, `docs/experiments/PHASE2_LOG.md`) so
//! later phases compute relevance the same, already-deterministic way
//! rather than re-deriving it and potentially reintroducing that exact
//! noise source.

use std::collections::BTreeMap;

use crate::data::EsciLabel;

fn relevance_gain(label: EsciLabel) -> f64 {
    match label {
        EsciLabel::Exact => 3.0,
        EsciLabel::Substitute => 2.0,
        EsciLabel::Complement => 1.0,
        EsciLabel::Irrelevant => 0.0,
    }
}

/// Deterministic (no `HashMap` iteration involved): `hits`/`judged` are
/// both already-ordered inputs (`&[String]`/`BTreeMap`), so summation
/// order is fixed call to call -- P2-E12's fix, carried forward here
/// rather than re-broken by a fresh per-phase reimplementation.
pub fn ndcg_recall_mrr(
    hits: &[String],
    judged: &BTreeMap<String, EsciLabel>,
    k: usize,
) -> (f64, f64, f64) {
    let relevant_total = judged.values().filter(|l| l.is_relevant()).count();
    if relevant_total == 0 {
        return (0.0, 0.0, 0.0);
    }
    let top: Vec<&str> = hits.iter().take(k).map(String::as_str).collect();
    let is_relevant_hit = |pid: &str| judged.get(pid).is_some_and(|l| l.is_relevant());

    let dcg: f64 = top
        .iter()
        .enumerate()
        .map(|(i, &pid)| {
            let gain = judged.get(pid).map(|&l| relevance_gain(l)).unwrap_or(0.0);
            gain / (i as f64 + 2.0).log2()
        })
        .sum();
    let mut ideal_gains: Vec<f64> = judged.values().map(|&l| relevance_gain(l)).collect();
    ideal_gains.sort_by(|a, b| b.total_cmp(a));
    let idcg: f64 = ideal_gains
        .iter()
        .take(k)
        .enumerate()
        .map(|(i, &g)| g / (i as f64 + 2.0).log2())
        .sum();
    let ndcg = if idcg > 0.0 { dcg / idcg } else { 0.0 };

    let hit_count = top.iter().filter(|&&pid| is_relevant_hit(pid)).count();
    let recall = hit_count as f64 / relevant_total as f64;

    let rr = top
        .iter()
        .position(|&pid| is_relevant_hit(pid))
        .map(|pos| 1.0 / (pos as f64 + 1.0))
        .unwrap_or(0.0);

    (ndcg, recall, rr)
}
