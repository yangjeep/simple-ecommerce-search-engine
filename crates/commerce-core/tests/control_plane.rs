use commerce_core::control_plane::{
    observe_residual_terms, try_promote, try_promote_with_precision, FixtureJudgmentOracle,
    FixtureModelProvider, Judgment, ModelProvider, Observation, PromotionRejection, Proposal,
    ReplayResult,
};
use commerce_core::domain::{BrandId, Constraint};
use commerce_core::fixtures::{shoe_semantic_context, REPRESENTATIVE_QUERY_SET};
use commerce_core::ir::{Candidate, ResolvedConstraint, StructuralConstraint};

#[test]
fn observes_every_residual_term_from_the_representative_query_set() {
    let ctx = shoe_semantic_context();
    let observations = observe_residual_terms(REPRESENTATIVE_QUERY_SET, ctx.lexicon());
    let terms: Vec<&str> = observations.iter().map(|o| o.term.as_str()).collect();

    // Was 9 terms (E004). Then 11: Issue #6 P1-B (`docs/experiments/PHASE2_LOG.md`
    // P2-E11) fixed `apply_candidates` so a phrase resolving to *only* a
    // soft `Preference` -- "cushioned", "breathable" -- also stays in
    // `residual_lexical` (a Preference must never hide its own phrase
    // from lexical search). `observe_residual_terms` scans exactly that
    // field, so both now legitimately appear -- they are in-vocabulary
    // (matched a real lexicon entry, unlike the other 9 out-of-vocabulary
    // terms here), but this function's job is "what residual text exists
    // to search on," which correctly includes them now.
    //
    // Now 12: Phase 9 (Issue #34) fixed `compile`'s resolution-priority
    // defect (`docs/experiments/PHASE9_LOG.md` P9-E05,
    // `PHASE9_DECISION.md`) so a lexicon-derived hard attribute constraint
    // with no corroborating entity constraint anywhere in the query is
    // demoted to a `Preference` and kept searchable, exactly like the
    // P1-B fix already did for genuinely soft candidates. "New Balance
    // waterproof shoes" in this fixture set resolves no entity at all
    // ("New Balance" is out-of-vocabulary, bare "shoes" is never a
    // registered phrase -- only "running shoes" is), so "waterproof" used
    // to compile to an unconfirmed hard filter and is now correctly
    // demoted, adding it to what this function observes as residual text.
    assert_eq!(
        terms,
        vec![
            "adidas",
            "balance",
            "blue",
            "breathable",
            "cushioned",
            "fit",
            "new",
            "shoes",
            "trail",
            "vegan",
            "waterproof",
            "wide"
        ]
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
    // Was 14 (12 baseline + adidas + blue). Baseline dropped 12 -> 10 per
    // P2-E11 (see `observes_every_residual_term_from_the_representative_query_set`
    // above); adidas/blue promotion still adds exactly +2 on top of
    // whatever the baseline legitimately is.
    assert_eq!(report.fully_resolved, 12, "{report:?}"); // 10 baseline + adidas + blue
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

/// Round 1 R1-E06's real-data finding, reproduced deterministically: a
/// provider proposing a mapping for a previously-unseen term to a
/// completely unrelated constraint is accepted by the original,
/// coverage-only `try_promote` gate (regression-free by construction for
/// any never-before-seen term, per R1-E06's mechanism -- adding it cannot
/// break any query that didn't already contain it, and the term did not
/// exist in the lexicon before, so there's nothing for it to regress).
/// This is not a hypothetical: R1-E06 found this exact failure mode
/// against a real 22,458-query corpus. `docs/experiments/ROUND1_LOG.md`'s
/// own "Regression check" section for R1-E06 named this test as the
/// concrete, not-yet-built follow-up; this is it.
#[test]
fn coverage_only_gate_accepts_a_nonsensical_mapping_for_a_never_before_seen_term() {
    let ctx = shoe_semantic_context();
    // "vegan" is residual against REPRESENTATIVE_QUERY_SET (the fixture's
    // own "vegan running shoes" query) and has no plausible connection to
    // waterproofing -- the same shape as R1-E06's real "shirts"/"and" ->
    // waterproof=true control experiment.
    let nonsensical_provider = FixtureModelProvider::new([(
        "vegan",
        Candidate::constraint(
            ResolvedConstraint::Attribute(Constraint::Boolean {
                attribute: "waterproof".to_string(),
                value: true,
            }),
            0.5,
        ),
    )]);

    let promoted = try_promote(
        &ctx,
        REPRESENTATIVE_QUERY_SET,
        &nonsensical_provider,
        "v2-nonsensical",
    );
    assert!(
        promoted.is_ok(),
        "reproducing R1-E06: the coverage-only gate accepts this, which is exactly the safety gap being fixed"
    );
}

/// The fix: `try_promote_with_precision` rejects the same nonsensical
/// mapping above once given a `PrecisionOracle` that judges "vegan running
/// shoes" (the one query the mapping newly resolves) as matching products
/// that are not actually relevant -- i.e., the waterproof=true constraint
/// finds *something* (a real risk: an unfounded mapping can still
/// coincidentally match products), but none of what it finds is what a
/// human would call relevant to "vegan running shoes".
#[test]
fn precision_gate_rejects_the_same_nonsensical_mapping_the_coverage_gate_accepted() {
    let ctx = shoe_semantic_context();
    let nonsensical_provider = FixtureModelProvider::new([(
        "vegan",
        Candidate::constraint(
            ResolvedConstraint::Attribute(Constraint::Boolean {
                attribute: "waterproof".to_string(),
                value: true,
            }),
            0.5,
        ),
    )]);
    // Real-world-shaped evidence: 5 products are genuinely relevant to
    // "vegan running shoes", but the unfounded mapping's waterproof=true
    // filter matches 40 products in the (hypothetical, real-scale)
    // catalog and only 2 of them happen to be relevant -- an imprecise
    // resolution, not a zero-recall one (see the next test for that case).
    let oracle = FixtureJudgmentOracle::new([(
        "vegan running shoes",
        Judgment {
            judged_relevant_total: 5,
            filtered_total: 40,
            filtered_relevant: 2,
        },
    )]);

    let result = try_promote_with_precision(
        &ctx,
        REPRESENTATIVE_QUERY_SET,
        &nonsensical_provider,
        &oracle,
        0.5, // min_precision
        "v2-nonsensical",
    );

    match result {
        Err(PromotionRejection::PrecisionGateFailed(failure)) => {
            assert_eq!(failure.precision.queries_judged, 1);
            assert_eq!(
                failure.precision.queries_below_threshold,
                vec!["vegan running shoes".to_string()]
            );
        }
        other => panic!("expected PrecisionGateFailed, got {other:?}"),
    }
}

/// A real loophole found while validating this gate against real ESCI
/// data (P2-E03, `docs/experiments/PHASE2_LOG.md`): a resolution whose
/// constraint kind an oracle's execution engine doesn't even implement
/// matches *zero* products, not irrelevant ones. An earlier version of
/// this gate only checked precision when `filtered_total > 0`, so a
/// zero-match resolution trivially "passed" -- exactly what happened when
/// R1-E06's real naive "shirts" -> waterproof=true mapping was replayed
/// through the gate for the first time: waterproof is a Boolean
/// constraint the real-data harness's execution engine doesn't evaluate
/// at all, so every judged product returned zero matches, and the
/// precision-only check let it through. `Judgment::fails` now also
/// rejects when relevant products are known to exist for a query but the
/// resolution matches none of them at all, independent of precision.
#[test]
fn precision_gate_rejects_a_resolution_that_matches_nothing_despite_known_relevant_products() {
    let ctx = shoe_semantic_context();
    let nonsensical_provider = FixtureModelProvider::new([(
        "vegan",
        Candidate::constraint(
            ResolvedConstraint::Attribute(Constraint::Boolean {
                attribute: "waterproof".to_string(),
                value: true,
            }),
            0.5,
        ),
    )]);
    // 3 products are genuinely relevant to "vegan running shoes", but the
    // resolution's filter matches literally nothing (filtered_total: 0) --
    // the exact real-data shape this test reproduces.
    let oracle = FixtureJudgmentOracle::new([(
        "vegan running shoes",
        Judgment {
            judged_relevant_total: 3,
            filtered_total: 0,
            filtered_relevant: 0,
        },
    )]);

    let result = try_promote_with_precision(
        &ctx,
        REPRESENTATIVE_QUERY_SET,
        &nonsensical_provider,
        &oracle,
        0.5,
        "v2-nonsensical-zero-match",
    );

    match result {
        Err(PromotionRejection::PrecisionGateFailed(failure)) => {
            assert_eq!(
                failure.precision.queries_below_threshold,
                vec!["vegan running shoes".to_string()]
            );
        }
        other => panic!(
            "a zero-match resolution with known relevant products must be rejected, got {other:?}"
        ),
    }
}

/// The precision gate must not block a *genuine* fix just because no
/// judgment evidence exists for it -- absence of evidence is not evidence
/// of a problem. Reuses `adidas_and_blue_provider`'s known-good mapping
/// (already proven to promote cleanly under the coverage-only gate above)
/// with an oracle that has no data at all for either newly-resolved query.
#[test]
fn precision_gate_does_not_block_a_genuine_fix_when_no_judgment_evidence_exists() {
    let ctx = shoe_semantic_context();
    let provider = adidas_and_blue_provider();
    let empty_oracle = FixtureJudgmentOracle::new([]);

    let promoted = try_promote_with_precision(
        &ctx,
        REPRESENTATIVE_QUERY_SET,
        &provider,
        &empty_oracle,
        0.5,
        "v2: +adidas,+blue, precision-checked",
    )
    .expect("no judgment evidence must not block promotion");
    assert_eq!(promoted.version, 2);
}
