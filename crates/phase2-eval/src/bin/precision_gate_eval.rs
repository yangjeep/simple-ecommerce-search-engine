//! P2-E03: does `try_promote_with_precision` (the fix for R1-E06's
//! structural safety gap) actually reject R1-E06's real control experiment
//! -- a naive mapping from the most frequent real residual term to an
//! unrelated `waterproof=true` constraint -- when given a
//! `PrecisionOracle` backed by real ESCI judgments, on the exact same real
//! catalog/query corpus R1-E06 used?
//!
//! Usage: cargo run --release -p phase2-eval --bin precision_gate_eval
//!        [catalog.jsonl] [queries.jsonl]

use std::collections::HashMap;
use std::path::PathBuf;

use commerce_core::cold_start::{compile_lexicon, CatalogProfile};
use commerce_core::control_plane::{
    observe_residual_terms, try_promote, try_promote_with_precision, FixtureModelProvider,
    Judgment, PrecisionOracle, PromotionRejection,
};
use commerce_core::domain::{Constraint, ProductId};
use commerce_core::ir::{Candidate, ResolvedConstraint, SemanticContext};
use round1_eval::classify::product_satisfies_and;
use round1_eval::data::EsciLabel;
use round1_eval::{catalog, data};

struct RealJudgmentOracle<'a> {
    catalog: &'a commerce_core::domain::Catalog,
    judgments_by_text: HashMap<&'a str, Vec<(ProductId, EsciLabel)>>,
}

impl PrecisionOracle for RealJudgmentOracle<'_> {
    fn judge(&self, query: &str, constraints: &[ResolvedConstraint]) -> Option<Judgment> {
        let judged = self.judgments_by_text.get(query)?;
        let mut judged_relevant_total = 0;
        let mut filtered_total = 0;
        let mut filtered_relevant = 0;
        for &(product_id, label) in judged {
            let relevant = label.is_relevant();
            if relevant {
                judged_relevant_total += 1;
            }
            if product_satisfies_and(self.catalog, product_id, constraints) {
                filtered_total += 1;
                if relevant {
                    filtered_relevant += 1;
                }
            }
        }
        Some(Judgment {
            judged_relevant_total,
            filtered_total,
            filtered_relevant,
        })
    }
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

    println!("loading + indexing real catalog...");
    let products = data::load_catalog(&catalog_path);
    let ingested = catalog::build_catalog(&products);
    // min_enum_frequency=1: same unfiltered lexicon R1-E06 used, so this
    // entry isolates the precision-gate fix as the only variable relative
    // to R1-E06's recorded result (P2-E02's canonicalization fix is a
    // separate, already-validated improvement).
    let profile = CatalogProfile::build(&ingested.catalog, &ingested.brands, &[], &[]);
    let lexicon = compile_lexicon(&profile, 1);
    let context = SemanticContext::new(1, "P2-E03: real catalog-derived lexicon", lexicon);

    println!("loading real queries + judgments...");
    let judgments = data::load_queries(&queries_path);
    let mut query_text_by_id: HashMap<u64, &str> = HashMap::new();
    let mut judgments_by_text: HashMap<&str, Vec<(ProductId, EsciLabel)>> = HashMap::new();
    for j in &judgments {
        query_text_by_id
            .entry(j.query_id)
            .or_insert(j.query.as_str());
        if let Some(&product_id) = ingested.asin_to_product_id.get(&j.product_id) {
            judgments_by_text
                .entry(j.query.as_str())
                .or_default()
                .push((product_id, j.label));
        }
    }
    let mut all_queries: Vec<&str> = query_text_by_id.values().copied().collect();
    all_queries.sort_unstable();

    let observations = observe_residual_terms(&all_queries, context.lexicon());
    let Some(top) = observations.first() else {
        println!("no residual terms observed; nothing to reproduce");
        return;
    };

    println!();
    println!(
        "=== Reproducing R1-E06's control experiment: naive unfounded guess for {:?} -> waterproof=true ===",
        top.term
    );
    let naive_provider = FixtureModelProvider::new([(
        Box::leak(top.term.clone().into_boxed_str()) as &str,
        Candidate::constraint(
            ResolvedConstraint::Attribute(Constraint::Boolean {
                attribute: "waterproof".to_string(),
                value: true,
            }),
            0.5,
        ),
    )]);

    println!();
    println!("--- Original coverage-only gate (try_promote, R1-E06's original behavior) ---");
    match try_promote(&context, &all_queries, &naive_provider, "v2-naive") {
        Ok(promoted) => println!(
            "  ACCEPTED -> version {} (reproducing R1-E06: the coverage-only gate has no way to catch this)",
            promoted.version
        ),
        Err(rejected) => println!(
            "  REJECTED (unexpected -- R1-E06 found this was accepted): {rejected:?}"
        ),
    }

    println!();
    println!("--- New precision-aware gate (try_promote_with_precision, real ESCI judgments as the oracle) ---");
    let oracle = RealJudgmentOracle {
        catalog: &ingested.catalog,
        judgments_by_text,
    };
    match try_promote_with_precision(
        &context,
        &all_queries,
        &naive_provider,
        &oracle,
        0.5,
        "v2-naive-precision-checked",
    ) {
        Ok(promoted) => println!(
            "  ACCEPTED -> version {} (fix did NOT catch this -- would need investigation)",
            promoted.version
        ),
        Err(PromotionRejection::CoverageGateFailed(r)) => println!(
            "  REJECTED at the coverage gate (candidate={}, baseline={}, regressions={})",
            r.candidate.fully_resolved,
            r.baseline.fully_resolved,
            r.regressions.len()
        ),
        Err(PromotionRejection::PrecisionGateFailed(failure)) => {
            println!("  REJECTED at the precision gate -- the fix works:");
            println!(
                "    newly_resolved queries: {}",
                failure.replay.newly_resolved.len()
            );
            println!(
                "    queries judged by real evidence: {}",
                failure.precision.queries_judged
            );
            println!(
                "    queries below min_precision (0.5): {:?}",
                failure.precision.queries_below_threshold
            );
        }
    }
}
