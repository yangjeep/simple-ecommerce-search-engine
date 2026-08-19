//! Gate 5: the offline/slow control plane. Nothing in `ir` or `index`
//! (the hot query path) calls anything in this module — it exists to
//! *evolve* a [`crate::ir::SemanticContext`] between deployments, not to
//! serve a query. See `docs/adr/0005-control-plane-prototype.md`.

mod observe;
mod precision;
mod provider;
mod replay;

pub use observe::{observe_residual_terms, Observation};
pub use precision::{
    check_precision, FixtureJudgmentOracle, Judgment, PrecisionCheck, PrecisionGateFailure,
    PrecisionOracle, PromotionRejection,
};
pub use provider::{FixtureModelProvider, ModelProvider, Proposal};
pub use replay::{replay, ReplayResult};

use crate::ir::{SemanticContext, SemanticLexicon};

/// Observe residual terms, ask `provider` for a proposal for each
/// (most-frequent first), and fold every accepted proposal into a clone
/// of `context`'s lexicon. A term the provider declines (`propose`
/// returns `None`) is left unresolved.
pub fn propose_candidates(
    context: &SemanticContext,
    queries: &[&str],
    provider: &dyn ModelProvider,
) -> (SemanticLexicon, Vec<Proposal>) {
    let observations = observe_residual_terms(queries, context.lexicon());
    let mut candidate_lexicon = context.lexicon().clone();
    let mut accepted = Vec::new();
    for observation in &observations {
        if let Some(proposal) = provider.propose(observation) {
            candidate_lexicon.insert(&proposal.term, vec![proposal.candidate.clone()]);
            accepted.push(proposal);
        }
    }
    (candidate_lexicon, accepted)
}

/// The full observe -> propose -> replay -> promote/reject loop. Returns
/// `Ok(new_context)` (version + 1) only when replay shows a strict,
/// regression-free coverage improvement; otherwise returns the
/// `ReplayResult` that explains why promotion was rejected, so the caller
/// can inspect exactly what failed rather than just "no."
pub fn try_promote(
    context: &SemanticContext,
    queries: &[&str],
    provider: &dyn ModelProvider,
    new_source: &'static str,
) -> Result<SemanticContext, ReplayResult> {
    let (candidate_lexicon, accepted) = propose_candidates(context, queries, provider);
    let result = replay(queries, context.lexicon(), &candidate_lexicon);
    if accepted.is_empty() || !result.passes_promotion_gate() {
        return Err(result);
    }
    Ok(SemanticContext::new(
        context.version + 1,
        new_source,
        candidate_lexicon,
    ))
}

/// The same observe -> propose -> replay loop as [`try_promote`], plus a
/// second, independent gate: every query the candidate lexicon newly
/// resolves must also pass a [`PrecisionOracle`] check, not merely improve
/// coverage. Round 1 R1-E06 (`docs/experiments/ROUND1_LOG.md`) found
/// `try_promote`'s coverage-only gate structurally cannot reject a
/// nonsensical mapping for any previously-unseen term — this is the fix,
/// additive (does not change `try_promote`'s existing behavior/tests) so
/// existing callers that don't have judgment evidence available keep
/// working exactly as before.
pub fn try_promote_with_precision(
    context: &SemanticContext,
    queries: &[&str],
    provider: &dyn ModelProvider,
    oracle: &dyn PrecisionOracle,
    min_precision: f64,
    new_source: &'static str,
) -> Result<SemanticContext, PromotionRejection> {
    let (candidate_lexicon, accepted) = propose_candidates(context, queries, provider);
    let result = replay(queries, context.lexicon(), &candidate_lexicon);
    if accepted.is_empty() || !result.passes_promotion_gate() {
        return Err(PromotionRejection::CoverageGateFailed(result));
    }
    let precision = check_precision(
        &result.newly_resolved,
        &candidate_lexicon,
        oracle,
        min_precision,
    );
    if !precision.passes() {
        return Err(PromotionRejection::PrecisionGateFailed(Box::new(
            PrecisionGateFailure {
                replay: result,
                precision,
            },
        )));
    }
    Ok(SemanticContext::new(
        context.version + 1,
        new_source,
        candidate_lexicon,
    ))
}
