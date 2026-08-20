//! Issue #14 (Phase 3): the safe fast-path admission decision. Phase 2
//! (`plan::plan`) always routed *something* through commerce-native --
//! `FastPath`, a structural-narrow-then-delegate-rank `Hybrid`, or a
//! delegate-search-then-verify `Punt` -- and found the latter two
//! consistently slower than a single mature Solr call on the real
//! traffic-dominant classes (`docs/experiments/PHASE2_LOG.md` P2-E17),
//! largely because they paid for commerce-native's own planning *and* an
//! embedded lexical delegate call, never neither.
//!
//! Phase 3's architecture is different in kind, not degree: a query is
//! either safe and complete enough to answer entirely from the native
//! structural index (`Admit`), or it is forwarded to Solr completely
//! unmodified, exactly as if commerce-native did not exist (`Reject`) --
//! never both. `admit` is the single decision point; it must be cheap
//! (no delegate call, no index execution beyond what a selectivity check
//! needs) since its cost is paid on *every* query, including every
//! rejected one, and Issue #14's invariant 1 requires the miss path to
//! stay close to the Solr baseline.
//!
//! "Safe" here means: every span in the query resolved to *something*
//! (no `residual_lexical` left over commerce-native would otherwise
//! silently ignore), no ambiguity was collapsed, at least one real
//! structural constraint exists (a query with none has nothing to narrow
//! by and is definitionally unsafe to answer natively), and the resulting
//! candidate set is small enough that `execute_ranked`'s lack of any real
//! ranking signal (P2-E17: `compile_lexicon`'s baseline lexicon never
//! populates `query.preferences`, so ties break on ascending
//! `(product_id, variant_id)`, not relevance) cannot matter much -- a
//! small candidate set has little room for a wrong top-K, a large one has
//! a lot. `AdmissionPolicy::max_candidates` is the one tunable knob this
//! phase's coverage-frontier sweep (Issue #14 RQ2) varies.

use crate::domain::Catalog;
use crate::index::CatalogIndex;
use crate::ir::CommerceQuery;

/// Why a query was rejected -- kept even though `Reject` also carries no
/// data of its own, so callers/eval harnesses can report a breakdown of
/// *why* real traffic falls back, not just how much of it does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RejectReason {
    /// `query.ambiguous` is non-empty: some phrase resolved to more than
    /// one candidate and was deliberately left unresolved rather than
    /// guessed. Never safe to admit -- there is no single interpretation
    /// to execute natively.
    Ambiguous,
    /// `query.residual_lexical` is non-empty: some part of the query was
    /// never resolved to structure at all. Native execution would
    /// silently ignore it, which is exactly the "not complete" case this
    /// admission check exists to catch.
    UnresolvedResidual,
    /// `query.constraints` is empty: nothing to narrow by. A query with
    /// zero structural constraints has no basis for `FastPath` at all
    /// (this is `plan::plan`'s own unconditional-`Punt` case, carried
    /// forward as unconditional-`Reject` here).
    NoStructuralConstraint,
    /// The structural candidate set exceeds `AdmissionPolicy::max_candidates`.
    /// Carries the real measured count so callers can report how far over
    /// the cap a rejected query was, not just that it was rejected.
    NotSelectiveEnough { candidates: u64 },
}

/// The admission decision for one compiled query, plus enough detail
/// (the real candidate count on both branches, where available) for an
/// eval harness to build a coverage/selectivity report without
/// re-deriving it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdmissionDecision {
    Admit { candidates: u64 },
    Reject(RejectReason),
}

impl AdmissionDecision {
    pub fn is_admit(&self) -> bool {
        matches!(self, AdmissionDecision::Admit { .. })
    }
}

/// Phase 3's one tunable admission knob. Deliberately a single field for
/// now -- Issue #14 RQ2 sweeps this to trace the coverage/relevance
/// frontier; additional knobs (a confidence threshold, a variant-scope
/// requirement) are added only if a specific rejected-query-class
/// experiment finds evidence one is needed (P3-E03+'s own discipline),
/// not spun up speculatively ahead of that evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AdmissionPolicy {
    /// The largest structural candidate-set size still considered safe to
    /// admit. Inclusive: a query resolving to exactly this many
    /// candidates is admitted, one more is not.
    pub max_candidates: usize,
}

/// Decide whether `query` is safe and complete enough for native
/// `FastPath` execution, or must fall back to Solr untouched. Cheap by
/// construction: at most one `indexed_candidates` bitmap build (skipped
/// entirely when the query is already rejected on ambiguity/residual/
/// no-constraint grounds, which needs no index access at all), and never
/// a delegate call.
pub fn admit(
    query: &CommerceQuery,
    index: &CatalogIndex,
    policy: &AdmissionPolicy,
) -> AdmissionDecision {
    if !query.ambiguous.is_empty() {
        return AdmissionDecision::Reject(RejectReason::Ambiguous);
    }
    if !query.residual_lexical.is_empty() {
        return AdmissionDecision::Reject(RejectReason::UnresolvedResidual);
    }
    if query.constraints.is_empty() {
        return AdmissionDecision::Reject(RejectReason::NoStructuralConstraint);
    }
    let candidates = index.indexed_candidates(&query.constraints).len();
    if candidates as usize > policy.max_candidates {
        return AdmissionDecision::Reject(RejectReason::NotSelectiveEnough { candidates });
    }
    AdmissionDecision::Admit { candidates }
}

/// Execute an admitted query natively. Callers must only call this after
/// `admit` returns `Admit` -- there is no internal re-check, matching
/// `plan::execute_planned`'s own division of labor between routing and
/// execution. Reuses `CatalogIndex::execute_ranked` unchanged: Phase 3
/// does not rebuild ranking (Issue #14's own rule), and measuring the
/// *existing* ranking behavior honestly on admitted queries is exactly
/// what this phase's relevance-budget evidence needs.
pub fn execute_admitted(
    index: &CatalogIndex,
    query: &CommerceQuery,
    catalog: &Catalog,
    k: usize,
) -> Vec<crate::index::RankedHit> {
    index.execute_ranked(query, catalog, k)
}
