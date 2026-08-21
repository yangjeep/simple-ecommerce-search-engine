//! Issue #14/#18 P3-E11 diagnostic (no Solr call needed): the first look
//! this campaign has taken at the AMBIGUOUS rejection reason -- 22.29% of
//! all real traffic (P3-E01), the second-largest rejection reason after
//! unresolved residual, and structurally distinct (a phrase that resolved
//! to *multiple* candidate interpretations, not one that failed to
//! resolve at all).
//!
//! Before building anything, checks whether ambiguity has an exploitable
//! internal structure. `Candidate::confidence` exists in the type
//! (`crates/commerce-core/src/ir/lexicon.rs`) but is deliberately unused
//! by `compile()` for auto-disambiguation, and -- checked directly against
//! `cold_start::profile::compile_lexicon`, the actual lexicon every real-
//! data Phase 2/3 experiment has used -- every candidate it ever
//! constructs is hard-coded to `confidence: 1.0`. There is no real
//! confidence signal to exploit in the lexicon this project has actually
//! benchmarked against; a "pick the higher-confidence candidate" idea is
//! a dead end here without first changing lexicon compilation itself.
//!
//! What *does* exist independent of the lexicon: each candidate's own
//! real catalog frequency (how many products/variants actually carry
//! that value), directly queryable via `CatalogIndex::indexed_candidates`
//! on a single-element constraint slice. This diagnostic checks whether
//! ambiguous spans tend to have one candidate that dominates the others
//! by catalog frequency -- if shoppers' phrasing usually resolves to the
//! catalog's own dominant/common reading rather than a rare one, that
//! would be a real, catalog-grounded (not confidence-placeholder) signal
//! worth a full admission-mechanism experiment. If the frequency
//! distribution across candidates is flat/uninformative, that is a real
//! negative finding too, worth recording before moving to Issue #16's own
//! learned-implication territory (which explicitly targets exactly this
//! problem with catalog+query evidence rather than static frequency
//! alone).
//!
//! Usage: cargo run --release -p phase3-eval --bin p3e11_ambiguous_frequency_diagnostic
//!        [catalog.jsonl] [queries.jsonl]

use std::collections::BTreeMap;
use std::path::PathBuf;

use commerce_core::admission::{admit, AdmissionDecision, AdmissionPolicy, RejectReason};
use commerce_core::cold_start::{compile_lexicon, CatalogProfile};
use commerce_core::index::CatalogIndex;
use commerce_core::ir::compile;
use commerce_core::ir::lexicon::ResolvedTerm;
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

    let unlimited_policy = AdmissionPolicy {
        max_candidates: usize::MAX,
    };

    let mut ambiguous_count = 0usize;
    let mut single_span = 0usize;
    let mut multi_span = 0usize;
    let mut two_candidate_spans = 0usize;
    let mut three_plus_candidate_spans = 0usize;
    let mut preference_only_spans = 0usize;
    let mut confidences_all_tied = 0usize;
    let mut confidences_vary = 0usize;
    // For single-span, all-Constraint-candidate ambiguous queries only
    // (the simplest, most tractable subclass): frequency-ratio buckets.
    let mut dominant_10x = 0usize;
    let mut dominant_2x = 0usize;
    let mut flat = 0usize;
    let mut resolved_candidate_set_le_250 = 0usize;
    let mut tractable_subclass_total = 0usize;

    for (&_qid, text) in &judged_by_query {
        let compiled = compile(text, &lexicon);
        let AdmissionDecision::Reject(reason) = admit(&compiled, &index, &unlimited_policy) else {
            continue;
        };
        if reason != RejectReason::Ambiguous {
            continue;
        }
        ambiguous_count += 1;

        if compiled.ambiguous.len() == 1 {
            single_span += 1;
        } else {
            multi_span += 1;
        }

        for span in &compiled.ambiguous {
            match span.candidates.len() {
                2 => two_candidate_spans += 1,
                n if n >= 3 => three_plus_candidate_spans += 1,
                _ => {}
            }
            let confidences: Vec<f64> = span.candidates.iter().map(|c| c.confidence).collect();
            if confidences.windows(2).all(|w| (w[0] - w[1]).abs() < 1e-9) {
                confidences_all_tied += 1;
            } else {
                confidences_vary += 1;
            }
            if span
                .candidates
                .iter()
                .all(|c| matches!(c.resolved, ResolvedTerm::Preference(_)))
            {
                preference_only_spans += 1;
            }
        }

        // The tractable subclass: exactly one ambiguous span, every
        // candidate a real hard Constraint (not a Preference), so
        // "pick the most frequent candidate" produces a fully-resolved,
        // admissible query to actually measure.
        if compiled.ambiguous.len() == 1 {
            let span = &compiled.ambiguous[0];
            let constraint_candidates: Vec<_> = span
                .candidates
                .iter()
                .filter_map(|c| match &c.resolved {
                    ResolvedTerm::Constraint(rc) => Some(rc.clone()),
                    ResolvedTerm::Preference(_) => None,
                })
                .collect();
            if constraint_candidates.len() == span.candidates.len()
                && constraint_candidates.len() >= 2
            {
                tractable_subclass_total += 1;
                let mut freqs: Vec<u64> = constraint_candidates
                    .iter()
                    .map(|c| index.indexed_candidates(std::slice::from_ref(c)).len())
                    .collect();
                freqs.sort_unstable_by(|a, b| b.cmp(a));
                let top = freqs[0];
                let second = freqs[1];
                if second == 0 || top as f64 / second as f64 >= 10.0 {
                    dominant_10x += 1;
                } else if top as f64 / second as f64 >= 2.0 {
                    dominant_2x += 1;
                } else {
                    flat += 1;
                }

                // What would the resolved candidate-set size look like if
                // we picked the top-frequency candidate and combined it
                // with this query's own existing structural constraints?
                let winner_idx = constraint_candidates
                    .iter()
                    .enumerate()
                    .map(|(i, c)| (i, index.indexed_candidates(std::slice::from_ref(c)).len()))
                    .max_by_key(|&(_, f)| f)
                    .map(|(i, _)| i)
                    .unwrap();
                let mut resolved_constraints = compiled.constraints.clone();
                resolved_constraints.push(constraint_candidates[winner_idx].clone());
                let resolved_count = index.indexed_candidates(&resolved_constraints).len();
                if resolved_count > 0 && resolved_count as usize <= 250 {
                    resolved_candidate_set_le_250 += 1;
                }
            }
        }
    }

    println!("\n=== P3-E11 diagnostic: ambiguous-query internal structure, real data ===");
    println!("rejected for Ambiguous: {ambiguous_count}/{total}");
    println!("  single-span: {single_span}, multi-span: {multi_span}");
    println!(
        "  span candidate counts: 2-candidate spans={two_candidate_spans}, 3+-candidate spans={three_plus_candidate_spans}"
    );
    println!(
        "  spans where every candidate's confidence is tied: {confidences_all_tied}, spans where confidence varies: {confidences_vary}"
    );
    println!("  spans where every candidate is Preference-only (no hard constraint at all): {preference_only_spans}");
    println!(
        "\n  tractable subclass (exactly 1 ambiguous span, every candidate a real Constraint, >=2 candidates): {tractable_subclass_total}/{ambiguous_count}"
    );
    println!(
        "    catalog-frequency dominance: top>=10x second: {dominant_10x}, top 2-10x second: {dominant_2x}, flat (<2x): {flat}"
    );
    println!(
        "    of these, picking the highest-frequency candidate yields a combined candidate set (with existing constraints) that is nonzero and <=250: {resolved_candidate_set_le_250}/{tractable_subclass_total} ({:.2}% of all ambiguous-rejected traffic, {:.2}% of the whole corpus)",
        resolved_candidate_set_le_250 as f64 / ambiguous_count.max(1) as f64 * 100.0,
        resolved_candidate_set_le_250 as f64 / total as f64 * 100.0
    );
}
