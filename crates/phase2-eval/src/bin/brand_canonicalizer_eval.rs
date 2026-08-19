//! Issue #9: does `HeuristicCanonicalizer` recover meaningful trusted-brand
//! coverage over `FrequencyOnlyCanonicalizer` (the literal shipping
//! `min_enum_frequency` gate, wrapped unmodified as a
//! `VocabularyCanonicalizer` -- `commerce_core::cold_start::canonicalize`)
//! without sacrificing precision, measured against real reconciled human
//! (three-independent-pass) ground truth?
//!
//! **Evidence class**: real. The 209-candidate corpus
//! (`dataset_cache/export/brand_adjudication_corpus.jsonl`) is sampled
//! (`scripts/phase2/build_brand_adjudication_corpus.py`, deterministic,
//! seed=7) from the real 1,215,854-product ESCI catalog's real excluded/
//! near-frontier brand vocabulary. Ground truth
//! (`dataset_cache/export/brand_adjudication_ground_truth.jsonl`) is
//! reconciled (`scripts/phase2/reconcile_brand_adjudication.py`) from three
//! independent labeling passes per
//! `docs/research/brand-adjudication-rubric.md`'s protocol.
//!
//! **Independence**: the two canonicalizers under evaluation here are
//! deterministic code with no relationship to how the ground truth was
//! produced. (The separate model-assisted arm, evaluated elsewhere, is
//! *not* independent of the ground-truth labels in the same way -- see the
//! rubric's disclosed same-model-family limitation.)
//!
//! Usage: cargo run --release -p phase2-eval --bin brand_canonicalizer_eval
//!        [corpus.jsonl] [ground_truth.jsonl]

use std::collections::HashMap;
use std::path::PathBuf;

use commerce_core::cold_start::{
    CanonicalizationEvidence, FrequencyOnlyCanonicalizer, HeuristicCanonicalizer,
    VocabularyCanonicalizer,
};
use serde::Deserialize;

#[derive(Deserialize)]
struct RepresentativeProduct {
    title: String,
}

#[derive(Deserialize)]
struct CorpusRow {
    brand_normalized: String,
    real_occurrence_count: usize,
    representative_products: Vec<RepresentativeProduct>,
}

#[derive(Deserialize)]
struct GroundTruthRow {
    brand_normalized: String,
    final_label: String,
    confidence: String,
}

fn trusted(label: &str) -> bool {
    matches!(
        label,
        "canonical_known_entity_or_alias" | "legitimate_new_entity"
    )
}

fn load_jsonl_lines(path: &PathBuf) -> Vec<String> {
    std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()))
        .lines()
        .map(str::to_string)
        .collect()
}

struct Scored {
    tp: usize,
    fp: usize,
    fn_: usize,
    tn: usize,
}

impl Scored {
    fn new() -> Self {
        Scored {
            tp: 0,
            fp: 0,
            fn_: 0,
            tn: 0,
        }
    }
    fn record(&mut self, predicted_trusted: bool, actual_trusted: bool) {
        match (predicted_trusted, actual_trusted) {
            (true, true) => self.tp += 1,
            (true, false) => self.fp += 1,
            (false, true) => self.fn_ += 1,
            (false, false) => self.tn += 1,
        }
    }
    fn precision(&self) -> f64 {
        if self.tp + self.fp == 0 {
            0.0
        } else {
            self.tp as f64 / (self.tp + self.fp) as f64
        }
    }
    fn recall(&self) -> f64 {
        if self.tp + self.fn_ == 0 {
            0.0
        } else {
            self.tp as f64 / (self.tp + self.fn_) as f64
        }
    }
    fn f1(&self) -> f64 {
        let (p, r) = (self.precision(), self.recall());
        if p + r == 0.0 {
            0.0
        } else {
            2.0 * p * r / (p + r)
        }
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let corpus_path = args
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("dataset_cache/export/brand_adjudication_corpus.jsonl"));
    let ground_truth_path = args.next().map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from("dataset_cache/export/brand_adjudication_ground_truth.jsonl")
    });

    let corpus_rows: Vec<CorpusRow> = load_jsonl_lines(&corpus_path)
        .iter()
        .map(|line| serde_json::from_str(line).expect("bad corpus row"))
        .collect();
    let ground_truth_rows: Vec<GroundTruthRow> = load_jsonl_lines(&ground_truth_path)
        .iter()
        .map(|line| serde_json::from_str(line).expect("bad ground truth row"))
        .collect();

    let gt_by_brand: HashMap<&str, &GroundTruthRow> = ground_truth_rows
        .iter()
        .map(|r| (r.brand_normalized.as_str(), r))
        .collect();
    assert_eq!(
        corpus_rows.len(),
        ground_truth_rows.len(),
        "corpus and ground truth must cover the same candidate set"
    );

    let total_trusted = ground_truth_rows
        .iter()
        .filter(|r| trusted(&r.final_label))
        .count();
    println!(
        "{} candidates; ground truth says {} ({:.1}%) should be trusted_as_structural",
        corpus_rows.len(),
        total_trusted,
        100.0 * total_trusted as f64 / corpus_rows.len() as f64
    );

    println!(
        "\n{:>10}  {:>26}  {:>26}",
        "threshold", "FrequencyOnlyCanonicalizer", "HeuristicCanonicalizer"
    );
    println!(
        "{:>10}  {:>8} {:>8} {:>8}  {:>8} {:>8} {:>8}",
        "", "prec", "recall", "f1", "prec", "recall", "f1"
    );

    for &threshold in &[1usize, 2, 3, 5, 10, 25, 50, 100, 250] {
        let freq = FrequencyOnlyCanonicalizer {
            min_frequency: threshold,
        };
        let heuristic = HeuristicCanonicalizer {
            min_frequency_for_trust: threshold,
        };

        let mut freq_scored = Scored::new();
        let mut heuristic_scored = Scored::new();

        for row in &corpus_rows {
            let gt = gt_by_brand
                .get(row.brand_normalized.as_str())
                .unwrap_or_else(|| panic!("no ground truth for {}", row.brand_normalized));
            let actual_trusted = trusted(&gt.final_label);

            let titles: Vec<String> = row
                .representative_products
                .iter()
                .map(|p| p.title.clone())
                .collect();
            let evidence = CanonicalizationEvidence {
                value: &row.brand_normalized,
                occurrence_count: row.real_occurrence_count,
                representative_titles: &titles,
            };

            freq_scored.record(
                freq.classify(&evidence).trusted_as_structural(),
                actual_trusted,
            );
            heuristic_scored.record(
                heuristic.classify(&evidence).trusted_as_structural(),
                actual_trusted,
            );
        }

        println!(
            "{:>10}  {:>7.1}% {:>7.1}% {:>7.1}%  {:>7.1}% {:>7.1}% {:>7.1}%",
            threshold,
            freq_scored.precision() * 100.0,
            freq_scored.recall() * 100.0,
            freq_scored.f1() * 100.0,
            heuristic_scored.precision() * 100.0,
            heuristic_scored.recall() * 100.0,
            heuristic_scored.f1() * 100.0,
        );
    }

    // Break down accuracy by ground-truth confidence tier at one
    // representative threshold (25, P2-E05's measured real recall-peak
    // frontier) -- do the canonicalizers do better on candidates the human
    // passes unanimously agreed on than on genuinely contested ones?
    println!("\n--- Accuracy by ground-truth confidence tier (threshold=25) ---");
    let freq = FrequencyOnlyCanonicalizer { min_frequency: 25 };
    let heuristic = HeuristicCanonicalizer {
        min_frequency_for_trust: 25,
    };
    for tier in ["unanimous", "majority", "no_majority"] {
        let mut freq_scored = Scored::new();
        let mut heuristic_scored = Scored::new();
        let mut n = 0;
        for row in &corpus_rows {
            let gt = &gt_by_brand[row.brand_normalized.as_str()];
            if gt.confidence != tier {
                continue;
            }
            n += 1;
            let actual_trusted = trusted(&gt.final_label);
            let titles: Vec<String> = row
                .representative_products
                .iter()
                .map(|p| p.title.clone())
                .collect();
            let evidence = CanonicalizationEvidence {
                value: &row.brand_normalized,
                occurrence_count: row.real_occurrence_count,
                representative_titles: &titles,
            };
            freq_scored.record(
                freq.classify(&evidence).trusted_as_structural(),
                actual_trusted,
            );
            heuristic_scored.record(
                heuristic.classify(&evidence).trusted_as_structural(),
                actual_trusted,
            );
        }
        let freq_acc = (freq_scored.tp + freq_scored.tn) as f64 / n.max(1) as f64;
        let heuristic_acc = (heuristic_scored.tp + heuristic_scored.tn) as f64 / n.max(1) as f64;
        println!(
            "  {tier}: n={n}  freq_only_accuracy={:.1}%  heuristic_accuracy={:.1}%",
            freq_acc * 100.0,
            heuristic_acc * 100.0
        );
    }
}
