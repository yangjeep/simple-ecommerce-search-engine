//! Issue #9: does `HeuristicCanonicalizer` recover meaningful trusted-brand
//! coverage over `FrequencyOnlyCanonicalizer` (the literal shipping
//! `min_enum_frequency` gate, wrapped unmodified as a
//! `VocabularyCanonicalizer` -- `commerce_core::cold_start::canonicalize`)
//! without sacrificing precision, measured against real reconciled human
//! (three-independent-pass) ground truth -- and where does a third,
//! model-assisted arm land on the same metric?
//!
//! **Evidence class**: real. The 209-candidate corpus
//! (`dataset_cache/export/brand_adjudication_corpus.jsonl`) is sampled
//! (`scripts/phase2/build_brand_adjudication_corpus.py`, deterministic,
//! seed=7) from the real 1,215,854-product ESCI catalog's real excluded/
//! near-frontier brand vocabulary. Ground truth
//! (`dataset_cache/export/brand_adjudication_ground_truth.jsonl`) is
//! reconciled (`scripts/phase2/reconcile_brand_adjudication.py`) from three
//! independent labeling passes per
//! `docs/research/brand-adjudication-rubric.md`'s protocol. The
//! model-assisted arm (`dataset_cache/export/brand_adjudication_model_assisted.json`)
//! is a fourth, independently-run agent pass over the same 209 candidates,
//! held out of the ground-truth reconciliation itself (which uses only the
//! three passes above) -- so it is scored here as a genuine system-under-
//! test, not one of the raters that produced its own ground truth.
//!
//! **Independence**: `FrequencyOnlyCanonicalizer`/`HeuristicCanonicalizer`
//! are deterministic code with no relationship to how the ground truth was
//! produced. The model-assisted arm is a held-out pass, but **not**
//! independent in the stronger sense that matters for validity: it and
//! every ground-truth-forming pass are produced by the same underlying
//! model family in this environment (no distinct-vendor model or human
//! panel was available) -- `docs/research/brand-adjudication-rubric.md`'s
//! own disclosed limitation, repeated here because it directly qualifies
//! every number this specific arm produces below.
//!
//! **A real, disclosed scope limitation on the model-assisted arm**: it
//! classifies only the 209 sampled candidates, not the full real
//! 206,227-distinct-brand vocabulary `compile_lexicon` would see in
//! production -- labeling the full vocabulary would mean one agent call
//! per distinct brand string, which CLAUDE.md's own "do not perform one
//! LLM call per SKU/value at scale" cold-start discipline and this
//! environment's lack of a live, cheap model API both rule out. This arm
//! is therefore only evaluated on the classification-level metric below
//! (P2-E07's question), not wired into `compile_lexicon_with_brand_canonicalizer`
//! for a P2-E08-style full end-to-end FIB/recall sweep the way
//! `HeuristicCanonicalizer` was -- a concrete environmental blocker, not an
//! oversight, and recorded as such in `docs/experiments/PHASE2_LOG.md`.
//!
//! Usage: cargo run --release -p phase2-eval --bin brand_canonicalizer_eval
//!        [corpus.jsonl] [ground_truth.jsonl] [model_assisted.json]

use std::collections::HashMap;
use std::path::PathBuf;

use commerce_core::cold_start::{
    CanonicalizationEvidence, FrequencyOnlyCanonicalizer, HeuristicCanonicalizer,
    VocabularyCanonicalizer, VocabularyClass,
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

#[derive(Deserialize)]
struct ModelAssistedRow {
    brand_normalized: String,
    label: String,
}

fn trusted(label: &str) -> bool {
    matches!(
        label,
        "canonical_known_entity_or_alias" | "legitimate_new_entity"
    )
}

fn parse_class(label: &str) -> VocabularyClass {
    match label {
        "canonical_known_entity_or_alias" => VocabularyClass::CanonicalKnownEntityOrAlias,
        "legitimate_new_entity" => VocabularyClass::LegitimateNewEntity,
        "lexical_only_not_structural" => VocabularyClass::LexicalOnlyNotStructural,
        "ambiguous_insufficient_evidence" => VocabularyClass::AmbiguousInsufficientEvidence,
        "junk_malformed_wrong_field" => VocabularyClass::JunkMalformedWrongField,
        other => panic!("unknown vocabulary class label: {other}"),
    }
}

/// The model-assisted arm's output is a fixed classification per candidate
/// (an independent agent pass already ran offline, per-value, over the
/// bounded 209-candidate corpus -- not a callable, general-vocabulary
/// mechanism) -- so this simply wraps that fixed lookup as a
/// `VocabularyCanonicalizer`, ignoring `occurrence_count`/
/// `representative_titles` (the agent already saw and used that same
/// evidence when it produced the fixed label). This is the real shape a
/// "compiled control-plane artifact" takes for a model-assisted strategy
/// in this architecture (`control_plane::provider::ModelProvider`'s own
/// propose/replay/promote pattern: the model runs offline, the hot path
/// only ever consults the compiled result) -- scoped here to the 209
/// candidates this evaluation actually has real model output for.
struct ModelAssistedCanonicalizer {
    labels: HashMap<String, VocabularyClass>,
}

impl VocabularyCanonicalizer for ModelAssistedCanonicalizer {
    fn classify(&self, evidence: &CanonicalizationEvidence) -> VocabularyClass {
        self.labels
            .get(evidence.value)
            .copied()
            .unwrap_or(VocabularyClass::AmbiguousInsufficientEvidence)
    }
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
    let model_assisted_path = args.next().map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from("dataset_cache/export/brand_adjudication_model_assisted.json")
    });

    let corpus_rows: Vec<CorpusRow> = load_jsonl_lines(&corpus_path)
        .iter()
        .map(|line| serde_json::from_str(line).expect("bad corpus row"))
        .collect();
    let ground_truth_rows: Vec<GroundTruthRow> = load_jsonl_lines(&ground_truth_path)
        .iter()
        .map(|line| serde_json::from_str(line).expect("bad ground truth row"))
        .collect();
    let model_assisted_rows: Vec<ModelAssistedRow> = serde_json::from_str(
        &std::fs::read_to_string(&model_assisted_path)
            .unwrap_or_else(|e| panic!("failed to read {}: {e}", model_assisted_path.display())),
    )
    .expect("bad model-assisted json");

    let gt_by_brand: HashMap<&str, &GroundTruthRow> = ground_truth_rows
        .iter()
        .map(|r| (r.brand_normalized.as_str(), r))
        .collect();
    assert_eq!(
        corpus_rows.len(),
        ground_truth_rows.len(),
        "corpus and ground truth must cover the same candidate set"
    );
    assert_eq!(
        corpus_rows.len(),
        model_assisted_rows.len(),
        "corpus and model-assisted arm must cover the same candidate set"
    );
    let model_assisted = ModelAssistedCanonicalizer {
        labels: model_assisted_rows
            .iter()
            .map(|r| (r.brand_normalized.clone(), parse_class(&r.label)))
            .collect(),
    };

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

    // --- Model-assisted arm: a single fixed classification per candidate
    // (no threshold to sweep -- the agent already saw occurrence_count and
    // representative_titles when it produced each label), scored on the
    // exact same binary trusted_as_structural target as the two
    // deterministic arms above.
    println!("\n--- Model-assisted arm (fixed per-candidate labels, no threshold) ---");
    let mut model_scored = Scored::new();
    let mut exact_5way_matches = 0usize;
    for row in &corpus_rows {
        let gt = gt_by_brand[row.brand_normalized.as_str()];
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
        let predicted_class = model_assisted.classify(&evidence);
        model_scored.record(predicted_class.trusted_as_structural(), actual_trusted);
        if parse_class(&gt.final_label) == predicted_class {
            exact_5way_matches += 1;
        }
    }
    println!(
        "  trusted_as_structural: precision={:.1}% recall={:.1}% f1={:.1}%",
        model_scored.precision() * 100.0,
        model_scored.recall() * 100.0,
        model_scored.f1() * 100.0
    );
    println!(
        "  exact 5-class agreement with ground truth: {}/{} ({:.1}%)",
        exact_5way_matches,
        corpus_rows.len(),
        100.0 * exact_5way_matches as f64 / corpus_rows.len() as f64
    );

    // Break down accuracy by ground-truth confidence tier at one
    // representative threshold (25, P2-E05's measured real recall-peak
    // frontier) -- do the canonicalizers do better on candidates the human
    // passes unanimously agreed on than on genuinely contested ones?
    println!("\n--- Accuracy by ground-truth confidence tier (threshold=25 for the two deterministic arms) ---");
    let freq = FrequencyOnlyCanonicalizer { min_frequency: 25 };
    let heuristic = HeuristicCanonicalizer {
        min_frequency_for_trust: 25,
    };
    for tier in ["unanimous", "majority", "no_majority"] {
        let mut freq_scored = Scored::new();
        let mut heuristic_scored = Scored::new();
        let mut model_tier_scored = Scored::new();
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
            model_tier_scored.record(
                model_assisted.classify(&evidence).trusted_as_structural(),
                actual_trusted,
            );
        }
        let freq_acc = (freq_scored.tp + freq_scored.tn) as f64 / n.max(1) as f64;
        let heuristic_acc = (heuristic_scored.tp + heuristic_scored.tn) as f64 / n.max(1) as f64;
        let model_acc = (model_tier_scored.tp + model_tier_scored.tn) as f64 / n.max(1) as f64;
        println!(
            "  {tier}: n={n}  freq_only_accuracy={:.1}%  heuristic_accuracy={:.1}%  model_assisted_accuracy={:.1}%",
            freq_acc * 100.0,
            heuristic_acc * 100.0,
            model_acc * 100.0
        );
    }
}
