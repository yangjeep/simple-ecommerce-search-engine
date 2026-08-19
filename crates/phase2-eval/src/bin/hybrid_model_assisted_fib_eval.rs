//! Issue #9, P2-E10: does the model-assisted canonicalizer arm's real
//! classification-level win (P2-E09) survive contact with real end-to-end
//! FIB/precision/recall measurement, the way P2-E08 already tested for
//! `HeuristicCanonicalizer` -- and found it does NOT (heuristic wins on
//! classification, loses on real recall)?
//!
//! **The tractability problem, and how this experiment solves it**:
//! `FrequencyOnlyCanonicalizer`/`HeuristicCanonicalizer` are cheap,
//! deterministic code that decide every one of the real ~206,227 distinct
//! catalog brands in milliseconds. The model-assisted arm's real form is
//! per-value agent judgments -- classifying the full vocabulary would mean
//! ~206,227 individual judgments, ruled out by CLAUDE.md's own cold-start
//! discipline ("do not perform one LLM call per SKU/value at scale") and
//! this environment's lack of a live, cheap model API. P2-E09 solved this
//! for the classification-level metric by scoring against a 209-candidate
//! sample. This experiment solves it for the END-TO-END metric with a
//! different, real technique: a brand string that is below the
//! canonicalization threshold AND never appears anywhere in the real
//! 22,458-query judged set cannot possibly change any measured FIB/
//! precision/recall number, no matter how it's classified -- so restricting
//! model-assisted judgment to a real sample of brands drawn from exactly
//! that "could matter" set (`scripts/phase2/build_query_relevant_brand_sample.py`,
//! 500 of the 7,532 real below-threshold brands whose exact string appears
//! in some real query, seed=7) is not cherry-picking, it targets the
//! population actually relevant to this measurement.
//!
//! **The fairness problem this experiment's design solves**: model-assisted
//! only has real judgments for 500 of the ~199,582 real below-threshold
//! brands. A canonicalizer that silently defaults every OTHER brand to
//! "not trusted" would be systematically biased toward lower FIB coverage
//! than `FrequencyOnlyCanonicalizer` (which decides every brand), an unfair
//! comparison. `HybridModelAssistedCanonicalizer` fixes this: it uses the
//! real model-assisted judgment for the 500 sampled brands, and falls back
//! to the EXACT SAME decision `FrequencyOnlyCanonicalizer` would make (at
//! the same threshold) for every other brand -- so both arms decide every
//! brand identically except the 500 where model-assisted's real judgment
//! is substituted in. This isolates the causal effect of model-assisted's
//! decisions on exactly the brands that could matter for this query set,
//! against an otherwise-identical baseline.
//!
//! **Evidence class**: real. Catalog, queries, and the 500-brand sample's
//! selection criterion are all real. Model-assisted judgments for the 500
//! sampled brands are five independent agent-labeled batches (100 each),
//! merged and order-verified against the sample file.
//!
//! Usage: cargo run --release -p phase2-eval --bin hybrid_model_assisted_fib_eval
//!        [catalog.jsonl] [queries.jsonl] [sample.jsonl] [model_assisted.json]

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::Instant;

use commerce_core::cold_start::{
    compile_lexicon, compile_lexicon_with_brand_canonicalizer, CanonicalizationEvidence,
    CatalogProfile, FrequencyOnlyCanonicalizer, VocabularyCanonicalizer, VocabularyClass,
};
use round1_eval::classify::{self, AggregationRule, ClassCounts, QueryClass};
use round1_eval::{catalog, data};
use serde::Deserialize;

#[derive(Deserialize)]
struct ModelAssistedRow {
    brand_normalized: String,
    label: String,
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

/// Real model-assisted judgment for a real, tractably-sized sample of
/// query-relevant brands; falls back to the exact `FrequencyOnlyCanonicalizer`
/// decision (at the same threshold) for every brand outside that sample,
/// so both arms in this experiment decide every one of the real ~206,227
/// distinct brands, differing ONLY on the 500 sampled ones -- a fair,
/// isolated comparison rather than a partial-coverage strawman.
struct HybridModelAssistedCanonicalizer {
    overrides: HashMap<String, VocabularyClass>,
    fallback_threshold: usize,
}

impl VocabularyCanonicalizer for HybridModelAssistedCanonicalizer {
    fn classify(&self, evidence: &CanonicalizationEvidence) -> VocabularyClass {
        if let Some(&class) = self.overrides.get(evidence.value) {
            return class;
        }
        FrequencyOnlyCanonicalizer {
            min_frequency: self.fallback_threshold,
        }
        .classify(evidence)
    }
}

fn representative_titles_by_brand(
    ingested_catalog: &commerce_core::domain::Catalog,
    brands: &[commerce_core::domain::Brand],
) -> HashMap<String, Vec<String>> {
    let brand_name_by_id: HashMap<commerce_core::domain::BrandId, String> = brands
        .iter()
        .map(|b| (b.id, b.name.to_lowercase()))
        .collect();
    let mut titles: HashMap<String, Vec<String>> = HashMap::new();
    const MAX_TITLES_PER_BRAND: usize = 3;
    for product in &ingested_catalog.products {
        let Some(name) = brand_name_by_id.get(&product.brand) else {
            continue;
        };
        let entry = titles.entry(name.clone()).or_default();
        if entry.len() < MAX_TITLES_PER_BRAND {
            entry.push(product.title.clone());
        }
    }
    titles
}

fn main() {
    let mut args = std::env::args().skip(1);
    let catalog_path = args
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("dataset_cache/export/catalog.jsonl"));
    let queries_path = args
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("dataset_cache/export/queries.jsonl"));
    let sample_path = args
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("dataset_cache/export/brand_query_relevant_sample.jsonl"));
    let model_assisted_path = args.next().map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from("dataset_cache/export/brand_query_relevant_model_assisted.json")
    });

    println!("loading + ingesting real catalog...");
    let products = data::load_catalog(&catalog_path);
    let ingested = catalog::build_catalog(&products);
    println!("{} real products ingested", ingested.catalog.products.len());

    let sample_brands: Vec<String> = std::fs::read_to_string(&sample_path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", sample_path.display()))
        .lines()
        .map(|line| {
            let v: serde_json::Value = serde_json::from_str(line).expect("bad sample row");
            v["brand_normalized"].as_str().unwrap().to_string()
        })
        .collect();
    let model_assisted_rows: Vec<ModelAssistedRow> = serde_json::from_str(
        &std::fs::read_to_string(&model_assisted_path)
            .unwrap_or_else(|e| panic!("failed to read {}: {e}", model_assisted_path.display())),
    )
    .expect("bad model-assisted json");
    assert_eq!(
        sample_brands.len(),
        model_assisted_rows.len(),
        "sample and model-assisted judgments must cover the same candidate set"
    );
    let sample_set: HashSet<&str> = sample_brands.iter().map(String::as_str).collect();
    for row in &model_assisted_rows {
        assert!(
            sample_set.contains(row.brand_normalized.as_str()),
            "model-assisted judgment for {} is not in the sample file",
            row.brand_normalized
        );
    }
    println!(
        "{} real brands sampled (below-threshold, appear in real query text) with real model-assisted judgments",
        sample_brands.len()
    );
    let trusted_in_sample = model_assisted_rows
        .iter()
        .filter(|r| parse_class(&r.label).trusted_as_structural())
        .count();
    println!(
        "  model-assisted trusts {trusted_in_sample}/{} ({:.1}%) of the sample as structural",
        model_assisted_rows.len(),
        100.0 * trusted_in_sample as f64 / model_assisted_rows.len() as f64
    );

    let profile = CatalogProfile::build(&ingested.catalog, &ingested.brands, &[], &[]);
    let titles_by_brand = representative_titles_by_brand(&ingested.catalog, &ingested.brands);
    let lookup =
        |name: &str| -> Vec<String> { titles_by_brand.get(name).cloned().unwrap_or_default() };

    println!("loading real queries + judgments...");
    let judgments = data::load_queries(&queries_path);
    let mut query_text_by_id: HashMap<u64, &str> = HashMap::new();
    let mut judgments_by_query: HashMap<u64, Vec<&data::JudgedExample>> = HashMap::new();
    for j in &judgments {
        query_text_by_id
            .entry(j.query_id)
            .or_insert(j.query.as_str());
        judgments_by_query.entry(j.query_id).or_default().push(j);
    }
    let mut query_ids: Vec<u64> = query_text_by_id.keys().copied().collect();
    query_ids.sort_unstable();
    let known_ids: HashSet<&str> = ingested
        .asin_to_product_id
        .keys()
        .map(String::as_str)
        .collect();

    println!(
        "{} distinct real queries; comparing FrequencyOnlyCanonicalizer vs. hybrid (model-assisted override on 500 real query-relevant brands, frequency-only fallback elsewhere)...\n",
        query_ids.len()
    );

    let run = |label: &str, lexicon: commerce_core::ir::SemanticLexicon, elapsed: f64| {
        let mut counts = ClassCounts::default();
        let mut compiled_by_query = HashMap::with_capacity(query_ids.len());
        for query_id in &query_ids {
            let text = query_text_by_id[query_id];
            let compiled = commerce_core::ir::compile(text, &lexicon);
            let class = classify::classify(text, &compiled, &known_ids);
            counts.record(class);
            compiled_by_query.insert(*query_id, (class, compiled));
        }
        let precision = classify::measure_precision(
            &ingested.catalog,
            &ingested.asin_to_product_id,
            &judgments_by_query,
            &compiled_by_query,
            AggregationRule::ExistingAnd,
        );
        let fib_rate = counts.fraction(QueryClass::StructuralOnly)
            + counts.fraction(QueryClass::StructuralPlusLexical)
            + counts.fraction(QueryClass::ExactIdLookup);
        println!(
            "{label:>32}  fib={:>6.2}%  ambig={:>6.2}%  punt={:>6.2}%  precision={:>6.1}%  recall_ES={:>6.2}%  recall_Ex={:>6.2}%  ({elapsed:.1}s)",
            fib_rate * 100.0,
            counts.fraction(QueryClass::Ambiguous) * 100.0,
            counts.fraction(QueryClass::UnresolvedPunt) * 100.0,
            precision.precision() * 100.0,
            precision.filter_recall() * 100.0,
            precision.exact_recall() * 100.0,
        );
    };

    let overrides: HashMap<String, VocabularyClass> = model_assisted_rows
        .iter()
        .map(|r| (r.brand_normalized.clone(), parse_class(&r.label)))
        .collect();

    for &threshold in &[3usize, 10, 25, 50] {
        println!("--- min_enum_frequency = {threshold} ---");

        let t0 = Instant::now();
        let freq_lexicon = compile_lexicon(&profile, threshold);
        run(
            "FrequencyOnlyCanonicalizer (baseline)",
            freq_lexicon,
            t0.elapsed().as_secs_f64(),
        );

        let t0 = Instant::now();
        let hybrid = HybridModelAssistedCanonicalizer {
            overrides: overrides.clone(),
            fallback_threshold: threshold,
        };
        let hybrid_lexicon =
            compile_lexicon_with_brand_canonicalizer(&profile, threshold, &hybrid, lookup);
        run(
            "Hybrid model-assisted (500 real)",
            hybrid_lexicon,
            t0.elapsed().as_secs_f64(),
        );
        println!();
    }

    println!("(P2-E08 reference at threshold=25: FrequencyOnly fib=21.3% precision=90.5% recall_ES=31.7% recall_Ex=35.6%; Heuristic (full coverage) fib=45.3% precision=91.7% recall_ES=13.8% recall_Ex=15.8%)");
}
