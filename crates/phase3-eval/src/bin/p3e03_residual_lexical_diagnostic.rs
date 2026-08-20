//! Issue #14 P3-E03 diagnostic (not yet an implementation): before
//! building anything, check whether "safely admit a query with non-empty
//! `residual_lexical` when every residual token is verifiable via the
//! native `lexical_postings` token index (no delegate call -- Round 1's
//! own `CatalogIndex::lexical_and_candidates`), narrowing the combined
//! structural+lexical candidate set enough to stay safe" is a promising
//! direction at all, on real data -- per this project's own "diagnose
//! before build" discipline (P2-E11/P2-E15's own precedent).
//!
//! For every real query rejected in P3-E02 for `UnresolvedResidual`,
//! reports: how many residual tokens it has, whether every one of them
//! is a real word found *anywhere* in the lexical_postings index at all
//! (vs. a stopword-shaped miss, a typo, or a term this catalog's titles
//! never contain), and what candidate-set size results from combining
//! the query's own structural constraints (if any) with an AND-narrowing
//! over its residual tokens.
//!
//! Usage: cargo run --release -p phase3-eval --bin p3e03_residual_lexical_diagnostic
//!        [catalog.jsonl] [queries.jsonl]

use std::collections::BTreeMap;
use std::path::PathBuf;

use commerce_core::admission::{admit, AdmissionDecision, AdmissionPolicy};
use commerce_core::cold_start::{compile_lexicon, CatalogProfile};
use commerce_core::index::CatalogIndex;
use commerce_core::ir::compile;
use round1_eval::catalog;
use round1_eval::data;

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

    println!("building commerce_core structural index...");
    let index = CatalogIndex::build(&ingested.catalog);

    let profile = CatalogProfile::build(&ingested.catalog, &ingested.brands, &[], &[]);
    let lexicon = compile_lexicon(&profile, 25);

    println!("loading real queries + judgments...");
    let judgments = data::load_queries(&queries_path);
    let mut judged_by_query: BTreeMap<u64, String> = BTreeMap::new();
    {
        let mut has_relevant: BTreeMap<u64, bool> = BTreeMap::new();
        for j in &judgments {
            judged_by_query
                .entry(j.query_id)
                .or_insert_with(|| j.query.clone());
            *has_relevant.entry(j.query_id).or_insert(false) |= j.label.is_relevant();
        }
        judged_by_query.retain(|qid, _| has_relevant.get(qid).copied().unwrap_or(false));
    }
    let total = judged_by_query.len();
    println!("{total} distinct real judged queries with at least one relevant label loaded");

    let policy = AdmissionPolicy {
        max_candidates: usize::MAX,
    };
    let mut residual_1_word = 0usize;
    let mut residual_2_words = 0usize;
    let mut residual_3plus_words = 0usize;
    let mut all_tokens_known = 0usize;
    let mut some_tokens_unknown = 0usize;
    let mut combined_le_50 = 0usize;
    let mut combined_le_250 = 0usize;
    let mut combined_zero = 0usize;
    let mut combined_huge = 0usize;
    let mut rejected_residual_count = 0usize;

    for (&_qid, text) in &judged_by_query {
        let compiled = compile(text, &lexicon);
        let AdmissionDecision::Reject(reason) = admit(&compiled, &index, &policy) else {
            continue;
        };
        if !matches!(
            reason,
            commerce_core::admission::RejectReason::UnresolvedResidual
        ) {
            continue;
        }
        rejected_residual_count += 1;

        let residual_tokens: Vec<String> = compiled
            .residual_lexical
            .iter()
            .flat_map(|phrase| phrase.split_whitespace().map(str::to_lowercase))
            .collect();
        match residual_tokens.len() {
            0 => {} // shouldn't happen given non-empty residual_lexical, but guard anyway
            1 => residual_1_word += 1,
            2 => residual_2_words += 1,
            _ => residual_3plus_words += 1,
        }

        let known_tokens = residual_tokens
            .iter()
            .filter(|t| !index.lexical_and_candidates(std::slice::from_ref(t)).is_empty())
            .count();
        if known_tokens == residual_tokens.len() {
            all_tokens_known += 1;
        } else {
            some_tokens_unknown += 1;
        }

        let lexical_bitmap = index.lexical_and_candidates(&residual_tokens);
        let combined = if compiled.constraints.is_empty() {
            lexical_bitmap
        } else {
            lexical_bitmap & index.indexed_candidates(&compiled.constraints)
        };
        let combined_count = combined.len();
        match combined_count {
            0 => combined_zero += 1,
            n if n <= 50 => combined_le_50 += 1,
            n if n <= 250 => combined_le_250 += 1,
            _ => combined_huge += 1,
        }
    }

    println!("\n=== P3-E03 diagnostic: residual-lexical opportunity, real data ===");
    println!("rejected for UnresolvedResidual: {rejected_residual_count}/{total}");
    println!(
        "  residual token count: 1-word={residual_1_word} 2-word={residual_2_words} 3+word={residual_3plus_words}"
    );
    println!(
        "  every residual token found *somewhere* in lexical_postings: {all_tokens_known}/{rejected_residual_count}"
    );
    println!(
        "  at least one residual token never appears in any product's title/text: {some_tokens_unknown}/{rejected_residual_count}"
    );
    println!("  combined (structural AND lexical-token) candidate-set size distribution:");
    println!(
        "    0 candidates: {combined_zero}, 1-50: {combined_le_50}, 51-250: {combined_le_250}, >250: {combined_huge}"
    );
    let promising = combined_le_50 + combined_le_250;
    println!(
        "  queries that WOULD become newly safe-admissible under a combined structural+lexical cap<=250: {promising}/{rejected_residual_count} ({:.2}% of all rejected-for-residual traffic, {:.2}% of the whole corpus)",
        promising as f64 / rejected_residual_count as f64 * 100.0,
        promising as f64 / total as f64 * 100.0
    );
}
