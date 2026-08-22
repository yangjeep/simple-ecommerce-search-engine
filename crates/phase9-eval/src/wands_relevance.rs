//! NDCG@k/Recall@k/MRR for WANDS's real 3-way relevance label
//! (`Exact`/`Partial`/`Irrelevant`, confirmed via direct scan of
//! `dataset_cache/wands/label.csv`: 25,614/146,633/61,201 across 233,448
//! judgments over 480 queries) -- distinct from
//! `round1_eval::relevance::ndcg_recall_mrr`, which is hard-wired to
//! `round1_eval::data::EsciLabel`'s 4-way scale and has no notion of
//! WANDS's labels. The DCG/IDCG/recall/MRR math itself is the same
//! well-known formula `round1_eval::relevance` already implements
//! correctly (including its P2-E12 deterministic-summation-order fix,
//! carried forward here) -- this module exists because the *label ->
//! gain* mapping is dataset-specific, not because the scoring formula
//! needed to change.

use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WandsLabel {
    Exact,
    Partial,
    Irrelevant,
}

impl WandsLabel {
    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "Exact" => Some(Self::Exact),
            "Partial" => Some(Self::Partial),
            "Irrelevant" => Some(Self::Irrelevant),
            _ => None,
        }
    }

    /// The WANDS paper's own 3-grade relevance scale (Exact=2, Partial=1,
    /// Irrelevant=0), not an invented gain mapping.
    fn gain(self) -> f64 {
        match self {
            Self::Exact => 2.0,
            Self::Partial => 1.0,
            Self::Irrelevant => 0.0,
        }
    }

    pub fn is_relevant(self) -> bool {
        !matches!(self, Self::Irrelevant)
    }
}

/// Deterministic (no `HashMap` iteration involved): `hits`/`judged` are
/// both already-ordered inputs (`&[String]`/`BTreeMap`), matching
/// `round1_eval::relevance::ndcg_recall_mrr`'s own P2-E12 discipline.
pub fn ndcg_recall_mrr(
    hits: &[String],
    judged: &BTreeMap<String, WandsLabel>,
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
            let gain = judged.get(pid).map(|&l| l.gain()).unwrap_or(0.0);
            gain / (i as f64 + 2.0).log2()
        })
        .sum();
    let mut ideal_gains: Vec<f64> = judged.values().map(|&l| l.gain()).collect();
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

#[cfg(test)]
mod tests {
    use super::*;

    fn judged(pairs: &[(&str, WandsLabel)]) -> BTreeMap<String, WandsLabel> {
        pairs.iter().map(|(k, v)| (k.to_string(), *v)).collect()
    }

    #[test]
    fn parse_rejects_unknown_labels() {
        assert_eq!(WandsLabel::parse("Exact"), Some(WandsLabel::Exact));
        assert_eq!(WandsLabel::parse("Partial"), Some(WandsLabel::Partial));
        assert_eq!(
            WandsLabel::parse("Irrelevant"),
            Some(WandsLabel::Irrelevant)
        );
        assert_eq!(WandsLabel::parse("Substitute"), None, "not a WANDS label");
    }

    #[test]
    fn perfect_ranking_scores_ndcg_one() {
        let j = judged(&[
            ("a", WandsLabel::Exact),
            ("b", WandsLabel::Partial),
            ("c", WandsLabel::Irrelevant),
        ]);
        let hits = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let (ndcg, recall, rr) = ndcg_recall_mrr(&hits, &j, 10);
        assert!(
            (ndcg - 1.0).abs() < 1e-9,
            "perfect order should be ndcg=1.0: {ndcg}"
        );
        assert!((recall - 1.0).abs() < 1e-9);
        assert!((rr - 1.0).abs() < 1e-9);
    }

    #[test]
    fn no_relevant_judgments_returns_all_zero_not_nan() {
        let j = judged(&[("a", WandsLabel::Irrelevant)]);
        let hits = vec!["a".to_string()];
        assert_eq!(ndcg_recall_mrr(&hits, &j, 10), (0.0, 0.0, 0.0));
    }

    #[test]
    fn worst_ordering_scores_lower_than_best_ordering() {
        let j = judged(&[
            ("a", WandsLabel::Exact),
            ("b", WandsLabel::Partial),
            ("c", WandsLabel::Irrelevant),
        ]);
        let best = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let worst = vec!["c".to_string(), "b".to_string(), "a".to_string()];
        let (best_ndcg, _, _) = ndcg_recall_mrr(&best, &j, 10);
        let (worst_ndcg, _, _) = ndcg_recall_mrr(&worst, &j, 10);
        assert!(
            worst_ndcg < best_ndcg,
            "{worst_ndcg} should be < {best_ndcg}"
        );
    }

    #[test]
    fn unjudged_hit_contributes_zero_gain_not_a_panic() {
        let j = judged(&[("a", WandsLabel::Exact)]);
        let hits = vec!["unjudged".to_string(), "a".to_string()];
        let (ndcg, recall, rr) = ndcg_recall_mrr(&hits, &j, 10);
        assert!(ndcg > 0.0 && ndcg < 1.0, "{ndcg}");
        assert!((recall - 1.0).abs() < 1e-9);
        assert!((rr - 0.5).abs() < 1e-9, "a is relevant at rank 2: {rr}");
    }
}
