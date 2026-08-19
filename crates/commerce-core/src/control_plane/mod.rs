//! Gate 5: the offline/slow control plane. Nothing in `ir` or `index`
//! (the hot query path) calls anything in this module — it exists to
//! *evolve* a [`crate::ir::SemanticContext`] between deployments, not to
//! serve a query. See `docs/adr/0005-control-plane-prototype.md`.

mod observe;
mod provider;
mod replay;

pub use observe::{observe_residual_terms, Observation};
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
