//! R1-E06: does the Gate 5 control-plane loop (observe -> propose ->
//! replay -> promote/reject) reduce punt rate on *real* unresolved
//! vocabulary from the real query corpus, without regressing precision?
//! Reuses `commerce_core::control_plane` unmodified — this experiment is
//! entirely about whether the mechanism generalizes past Phase 0's toy
//! fixture, not about changing it.
//!
//! Usage: cargo run --release -p round1-eval --bin control_plane_real
//!        [catalog.jsonl] [queries.jsonl]

use std::collections::HashMap;
use std::path::PathBuf;

use commerce_core::cold_start::{compile_lexicon, CatalogProfile};
use commerce_core::control_plane::{observe_residual_terms, try_promote, FixtureModelProvider};
use commerce_core::domain::Constraint;
use commerce_core::ir::{
    measure_coverage, Candidate, ResolvedConstraint, SemanticContext, StructuralConstraint,
};
use round1_eval::{catalog, data};

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

    println!("loading + indexing real catalog...");
    let products = data::load_catalog(&catalog_path);
    let ingested = catalog::build_catalog(&products);
    let profile = CatalogProfile::build(&ingested.catalog, &ingested.brands, &[], &[]);
    let lexicon = compile_lexicon(&profile);
    let context = SemanticContext::new(
        1,
        "R1-E06: real catalog-derived lexicon (Gate 6, unmodified)",
        lexicon,
    );

    println!("loading real queries...");
    let judgments = data::load_queries(&queries_path);
    let mut query_text_by_id: HashMap<u64, &str> = HashMap::new();
    for j in &judgments {
        query_text_by_id
            .entry(j.query_id)
            .or_insert(j.query.as_str());
    }
    let mut all_queries: Vec<&str> = query_text_by_id.values().copied().collect();
    all_queries.sort_unstable();

    let baseline = measure_coverage(&all_queries, context.lexicon());
    println!();
    println!("=== Baseline coverage (v1, real catalog-derived lexicon, 22,458 real queries) ===");
    println!(
        "  fully_resolved={} / {} ({:.1}%)  ambiguous={}  residual={}",
        baseline.fully_resolved,
        baseline.total_queries,
        baseline.fraction_fully_resolved() * 100.0,
        baseline.had_ambiguity,
        baseline.had_residual
    );

    let observations = observe_residual_terms(&all_queries, context.lexicon());
    println!();
    println!(
        "=== Top 20 real unresolved (residual) terms, by frequency across 22,458 real queries ==="
    );
    for obs in observations.iter().take(20) {
        println!("  {:<20} frequency={}", obs.term, obs.frequency);
    }
    println!(
        "  ({} distinct residual terms observed in total)",
        observations.len()
    );

    // A provider backed by real catalog evidence: propose a Brand mapping
    // for a residual term only if that exact normalized string is a real
    // brand name in the catalog (i.e. a residual term that a *smarter*
    // cold-start canonicalizer — the one R1-E02b's finding says is
    // missing — would have caught). This is deliberately conservative:
    // it does not propose color mappings at all, since R1-E02/E02b found
    // the color field itself is frequently not genuine categorical data,
    // and proposing more noise into a noisy field is exactly the
    // "aggressive guessing... is a regression" failure mode to avoid.
    let brand_by_normalized: HashMap<&str, _> = ingested
        .brands
        .iter()
        .map(|b| (b.name.as_str(), b.id))
        .collect();
    let mut mappings = Vec::new();
    for obs in &observations {
        if let Some(&brand_id) = brand_by_normalized.get(obs.term.as_str()) {
            mappings.push((
                obs.term.clone(),
                Candidate::constraint(
                    ResolvedConstraint::Structural(StructuralConstraint::Brand(brand_id)),
                    1.0,
                ),
            ));
        }
    }
    println!();
    println!(
        "=== Evidence-backed provider: {} of the top {} residual terms are real catalog brand names ===",
        mappings.len(),
        observations.len().min(usize::MAX)
    );
    for (term, _) in mappings.iter().take(15) {
        println!("  {term}");
    }

    let provider = FixtureModelProvider::new(
        mappings
            .iter()
            .map(|(t, c)| (Box::leak(t.clone().into_boxed_str()) as &str, c.clone())),
    );

    match try_promote(
        &context,
        &all_queries,
        &provider,
        "v2: evidence-backed brand mappings from real residual terms",
    ) {
        Ok(promoted) => {
            println!();
            println!(
                "=== Promotion: ACCEPTED -> version {} ===",
                promoted.version
            );
            let after = measure_coverage(&all_queries, promoted.lexicon());
            println!(
                "  v1: fully_resolved={} ({:.1}%)  ->  v2: fully_resolved={} ({:.1}%)",
                baseline.fully_resolved,
                baseline.fraction_fully_resolved() * 100.0,
                after.fully_resolved,
                after.fraction_fully_resolved() * 100.0
            );
            println!(
                "  punt-relevant residual count: v1={}  v2={}",
                baseline.had_residual, after.had_residual
            );
        }
        Err(rejected) => {
            println!();
            println!("=== Promotion: REJECTED ===");
            println!(
                "  candidate fully_resolved={}  baseline fully_resolved={}  regressions={}",
                rejected.candidate.fully_resolved,
                rejected.baseline.fully_resolved,
                rejected.regressions.len()
            );
            if !rejected.regressions.is_empty() {
                println!(
                    "  sample regressions: {:?}",
                    &rejected.regressions[..rejected.regressions.len().min(5)]
                );
            }
        }
    }

    // Also test the naive provider the brief warns against: propose a
    // Brand mapping for the single most frequent residual term
    // regardless of whether it's real catalog evidence, to see whether
    // the replay gate correctly rejects an unfounded guess.
    if let Some(top) = observations.first() {
        println!();
        println!("=== Control experiment: naive unfounded guess for the single most frequent residual term ({:?}) ===", top.term);
        let guess_provider = FixtureModelProvider::new([(
            Box::leak(top.term.clone().into_boxed_str()) as &str,
            Candidate::constraint(
                ResolvedConstraint::Attribute(Constraint::Boolean {
                    attribute: "waterproof".to_string(),
                    value: true,
                }),
                0.5,
            ),
        )]);
        match try_promote(&context, &all_queries, &guess_provider, "v2-naive: unfounded guess") {
            Ok(promoted) => {
                let after = measure_coverage(&all_queries, promoted.lexicon());
                println!("  ACCEPTED (unexpected) -> version {}", promoted.version);
                println!(
                    "  fully_resolved: v1={} ({:.1}%) -> v2={} ({:.1}%)  [+{} queries newly \"resolved\" to a semantically nonsensical constraint]",
                    baseline.fully_resolved,
                    baseline.fraction_fully_resolved() * 100.0,
                    after.fully_resolved,
                    after.fraction_fully_resolved() * 100.0,
                    after.fully_resolved - baseline.fully_resolved
                );
            }
            Err(rejected) => println!(
                "  REJECTED as expected: candidate fully_resolved={} vs baseline={}, regressions={}",
                rejected.candidate.fully_resolved,
                rejected.baseline.fully_resolved,
                rejected.regressions.len()
            ),
        }
    }
}
