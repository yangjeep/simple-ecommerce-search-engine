use commerce_core::fixtures::{shoe_semantic_context, REPRESENTATIVE_QUERY_SET};
use commerce_core::ir::{compile, measure_coverage};

#[test]
fn semantic_context_carries_version_and_source() {
    let ctx = shoe_semantic_context();
    assert_eq!(ctx.version, 1);
    assert!(!ctx.source.is_empty());
}

#[test]
fn aliases_resolve_to_the_same_canonical_id_as_the_canonical_phrase() {
    let ctx = shoe_semantic_context();
    let canonical = compile("running shoes", ctx.lexicon());
    let sneakers = compile("sneakers", ctx.lexicon());
    let trainers = compile("trainers", ctx.lexicon());
    assert_eq!(canonical.constraints, sneakers.constraints);
    assert_eq!(canonical.constraints, trainers.constraints);
    assert!(sneakers.ambiguous.is_empty());
    assert!(trainers.ambiguous.is_empty());
}

/// Gate 4's explicit metric: what fraction of a representative query set
/// resolves without model inference. `REPRESENTATIVE_QUERY_SET` is
/// constructed with a known-exact outcome per query (see its doc comment)
/// so this test is a real measurement, not a threshold assertion — the
/// point is to record the actual number, not to make the test pass by
/// picking a lenient bound.
#[test]
fn measured_structural_coverage_matches_the_constructed_query_set() {
    let ctx = shoe_semantic_context();
    let report = measure_coverage(REPRESENTATIVE_QUERY_SET, ctx.lexicon());

    // Was fully_resolved=12/had_residual=6/fraction=0.6 (E004). Issue #6
    // P1-B (`docs/experiments/PHASE2_LOG.md` P2-E11) fixed
    // `ir::query::apply_candidates` so a phrase resolving to *only* a
    // soft `Preference` also stays in `residual_lexical` -- a Preference
    // must never make a lexical delegate blind to the phrase that
    // produced it (found via a real-data relevance regression, not
    // hypothesized). Two of `REPRESENTATIVE_QUERY_SET`'s queries resolve
    // purely to preference-only terms and correctly carry residual text
    // now, moving from "fully resolved" to "had residual." This is a
    // real, intended behavior change, not test drift -- `measure_coverage`'s
    // own "no ambiguity AND no residual" definition is unchanged.
    assert_eq!(report.total_queries, 20);
    assert_eq!(report.fully_resolved, 10, "{report:?}");
    assert_eq!(report.had_ambiguity, 2, "{report:?}");
    assert_eq!(report.had_residual, 8, "{report:?}");
    assert!((report.fraction_fully_resolved() - 0.5).abs() < 1e-9);
}
