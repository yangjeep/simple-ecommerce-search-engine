use commerce_core::control_plane::{
    observe_residual_terms, try_promote, FixtureModelProvider, ModelProvider, Observation,
    Proposal, ReplayResult,
};
use commerce_core::domain::{BrandId, Constraint};
use commerce_core::fixtures::{shoe_semantic_context, REPRESENTATIVE_QUERY_SET};
use commerce_core::ir::{Candidate, ResolvedConstraint, StructuralConstraint};

#[test]
fn observes_every_residual_term_from_the_representative_query_set() {
    let ctx = shoe_semantic_context();
    let observations = observe_residual_terms(REPRESENTATIVE_QUERY_SET, ctx.lexicon());
    let terms: Vec<&str> = observations.iter().map(|o| o.term.as_str()).collect();

    // From tests/coverage.rs's classification: exactly these 9 terms are
    // out-of-vocabulary across the 6 residual queries, each appearing once.
    assert_eq!(
        terms,
        vec!["adidas", "balance", "blue", "fit", "new", "shoes", "trail", "vegan", "wide"]
    );
    assert!(
        observations.iter().all(|o| o.frequency == 1),
        "{observations:?}"
    );
}

fn adidas_and_blue_provider() -> FixtureModelProvider {
    FixtureModelProvider::new([
        (
            "adidas",
            Candidate::constraint(
                ResolvedConstraint::Structural(StructuralConstraint::Brand(BrandId(2))),
                0.8,
            ),
        ),
        (
            "blue",
            Candidate::constraint(
                ResolvedConstraint::Attribute(Constraint::Enum {
                    attribute: "color".to_string(),
                    value: "Blue".to_string(),
                }),
                0.8,
            ),
        ),
    ])
}

#[test]
fn candidate_mappings_are_promoted_when_replay_improves_coverage_without_regressions() {
    let ctx = shoe_semantic_context();
    let provider = adidas_and_blue_provider();

    let promoted = try_promote(
        &ctx,
        REPRESENTATIVE_QUERY_SET,
        &provider,
        "v2: +adidas,+blue (Gate 5 test)",
    )
    .expect("should promote");

    assert_eq!(promoted.version, 2);

    // Both previously-residual queries about adidas/blue must now be fully
    // resolvable, and everything that resolved before must still resolve.
    let before = commerce_core::ir::compile("Adidas running shoes", ctx.lexicon());
    let after = commerce_core::ir::compile("Adidas running shoes", promoted.lexicon());
    assert!(!before.residual_lexical.is_empty());
    assert!(after.residual_lexical.is_empty() && after.ambiguous.is_empty());

    let after_blue = commerce_core::ir::compile("blue running shoes", promoted.lexicon());
    assert!(after_blue.residual_lexical.is_empty() && after_blue.ambiguous.is_empty());

    let report = commerce_core::ir::measure_coverage(REPRESENTATIVE_QUERY_SET, promoted.lexicon());
    assert_eq!(report.fully_resolved, 14, "{report:?}"); // 12 baseline + adidas + blue
}

#[test]
fn no_proposals_means_nothing_is_promoted() {
    let ctx = shoe_semantic_context();
    let empty_provider = FixtureModelProvider::new([]);

    let result = try_promote(&ctx, REPRESENTATIVE_QUERY_SET, &empty_provider, "v2: no-op");
    let rejected = result.expect_err("should not promote with zero accepted proposals");
    assert_eq!(
        rejected.candidate.fully_resolved,
        rejected.baseline.fully_resolved
    );
    assert!(rejected.regressions.is_empty());
}

/// Focused unit test of the promotion gate itself, independent of
/// `compile`: a candidate that improves aggregate coverage but regresses
/// even one previously-resolved query must still be rejected.
#[test]
fn promotion_gate_rejects_any_regression_even_with_a_net_aggregate_gain() {
    use commerce_core::ir::CoverageReport;

    let baseline = CoverageReport {
        total_queries: 20,
        fully_resolved: 12,
        had_ambiguity: 2,
        had_residual: 6,
    };
    let candidate_with_regression = CoverageReport {
        total_queries: 20,
        fully_resolved: 15, // net +3, looks like a clear win in aggregate...
        had_ambiguity: 2,
        had_residual: 3,
    };
    let result = ReplayResult {
        baseline: baseline.clone(),
        candidate: candidate_with_regression,
        regressions: vec!["red running shoes size 8".to_string()], // ...but one query broke.
        newly_resolved: vec![
            "a".to_string(),
            "b".to_string(),
            "c".to_string(),
            "d".to_string(),
        ],
    };
    assert!(!result.passes_promotion_gate());

    let clean_result = ReplayResult {
        regressions: Vec::new(),
        ..result
    };
    assert!(clean_result.passes_promotion_gate());
}

/// A provider that always declines is a legitimate `ModelProvider` and
/// must not panic or be treated specially by the pipeline.
struct AlwaysDeclineProvider;
impl ModelProvider for AlwaysDeclineProvider {
    fn propose(&self, _observation: &Observation) -> Option<Proposal> {
        None
    }
}

#[test]
fn a_provider_that_always_declines_never_promotes() {
    let ctx = shoe_semantic_context();
    let result = try_promote(&ctx, REPRESENTATIVE_QUERY_SET, &AlwaysDeclineProvider, "v2");
    assert!(result.is_err());
}
