//! Issue #9's real question, end-to-end: does swapping `HeuristicCanonicalizer`
//! for the shipping `FrequencyOnlyCanonicalizer`/raw `min_enum_frequency`
//! brand gate recover meaningful Semantic FIB coverage on real queries
//! without sacrificing measured precision/recall -- not just the
//! classification-level precision/recall against the 209-candidate
//! adjudication corpus (`brand_canonicalizer_eval.rs`), but the actual
//! downstream FIB/precision/recall numbers P2-E02/P2-E05
//! (`docs/experiments/PHASE2_LOG.md`) already established as this
//! project's real evidence metric. CLAUDE.md: "Do not claim an
//! architectural win from microbenchmarks alone when end-to-end evidence
//! is available."
//!
//! Reuses `round1_eval::classify`'s existing measurement machinery
//! unmodified, exactly like `canonicalization_eval.rs` (P2-E02) -- this
//! experiment is entirely about which mechanism decides brand-vocabulary
//! trust, not about changing how FIB/precision/recall are measured.
//!
//! Scope, stated explicitly: only brand-vocabulary inclusion is swapped
//! (`compile_lexicon_with_brand_canonicalizer`); enum-value (color/size/
//! etc) filtering stays on the same raw `min_enum_frequency` gate in both
//! arms, because the adjudication ground truth this evaluates against
//! only covers brand vocabulary (`docs/research/brand-adjudication-rubric.md`).
//!
//! Usage: cargo run --release -p phase2-eval --bin canonicalizer_fib_eval
//!        [catalog.jsonl] [queries.jsonl]

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::Instant;

use commerce_core::cold_start::{
    compile_lexicon, compile_lexicon_with_brand_canonicalizer, CatalogProfile,
    FrequencyOnlyCanonicalizer, HeuristicCanonicalizer,
};
use round1_eval::classify::{self, AggregationRule, ClassCounts, QueryClass};
use round1_eval::{catalog, data};

/// A handful of representative titles per lowercased brand name, collected
/// in one deterministic pass over the real catalog (encounter order) --
/// the same bounded-evidence shape the adjudication corpus itself used
/// (`build_brand_adjudication_corpus.py`'s `representative_products`,
/// capped there too), not the full per-brand product set.
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

    println!("loading + ingesting real catalog...");
    let products = data::load_catalog(&catalog_path);
    let ingested = catalog::build_catalog(&products);
    println!("{} real products ingested", ingested.catalog.products.len());

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
        "{} distinct real queries; comparing FrequencyOnlyCanonicalizer (== existing raw min_enum_frequency gate) vs HeuristicCanonicalizer for brand vocabulary, enum-value filtering unchanged in both arms...\n",
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
            "{label:>28}  fib={:>6.1}%  ambig={:>6.1}%  punt={:>6.1}%  precision={:>6.1}%  recall_ES={:>6.1}%  recall_Ex={:>6.1}%  ({elapsed:.1}s compile+profile)",
            fib_rate * 100.0,
            counts.fraction(QueryClass::Ambiguous) * 100.0,
            counts.fraction(QueryClass::UnresolvedPunt) * 100.0,
            precision.precision() * 100.0,
            precision.filter_recall() * 100.0,
            precision.exact_recall() * 100.0,
        );
    };

    for &threshold in &[3usize, 10, 25, 50] {
        println!("--- min_enum_frequency / min_frequency_for_trust = {threshold} ---");

        let t0 = Instant::now();
        let freq_lexicon = compile_lexicon(&profile, threshold);
        run(
            "FrequencyOnlyCanonicalizer",
            freq_lexicon,
            t0.elapsed().as_secs_f64(),
        );

        let t0 = Instant::now();
        let heuristic = HeuristicCanonicalizer {
            min_frequency_for_trust: threshold,
        };
        let heuristic_lexicon =
            compile_lexicon_with_brand_canonicalizer(&profile, threshold, &heuristic, lookup);
        run(
            "HeuristicCanonicalizer",
            heuristic_lexicon,
            t0.elapsed().as_secs_f64(),
        );

        // Sanity check: FrequencyOnlyCanonicalizer at this threshold must
        // reproduce compile_lexicon's own raw-threshold brand gate exactly
        // (`canonicalize.rs`'s own doc comment claims this; verify it here
        // against real 1.2M-product data, not just the 209-candidate unit
        // tests).
        let t0 = Instant::now();
        let freq = FrequencyOnlyCanonicalizer {
            min_frequency: threshold,
        };
        let freq_via_canonicalizer =
            compile_lexicon_with_brand_canonicalizer(&profile, threshold, &freq, lookup);
        run(
            "  (sanity: via canonicalizer)",
            freq_via_canonicalizer,
            t0.elapsed().as_secs_f64(),
        );
        println!();
    }

    println!("(P2-E02 baseline, threshold=1 (unfiltered): FIB=55.4%, ambiguity=38.4%, punt=2.5%, precision=94.5%, recall_ES=4.3%, recall_Exact=5.0%)");
}
