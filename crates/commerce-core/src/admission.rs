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

use roaring::RoaringBitmap;

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

/// Issue #14 P3-E03: a second, independent admission check for the
/// single largest rejection reason `admit` finds (P3-E02, real data:
/// 76.89% of all real traffic) -- residual free text left over after
/// structural resolution. Rather than requiring `residual_lexical` to be
/// *empty*, this requires every residual token to be *safely verifiable*:
/// present as an exact whole-word token somewhere in this catalog's own
/// indexed text (`CatalogIndex::lexical_and_candidates`, Round 1's own
/// token-postings index -- no delegate call, no BM25 ranking, no
/// generic lexical retrieval rebuilt), AND the resulting *combined*
/// (structural AND lexical) candidate set small enough to stay within
/// `max_lexical_narrowed_candidates` -- the exact same "small candidate
/// set is safe without a ranking signal" principle `admit`'s own
/// `max_candidates` cap already relies on, applied to a second signal.
///
/// A residual token that never appears *anywhere* in this catalog's
/// indexed text at all (a typo, an out-of-vocabulary word, a plural/
/// stemming mismatch this exact-token index cannot bridge) makes the
/// whole query ineligible here (`None`) rather than "safely narrows to
/// zero candidates": those are different claims -- the former means this
/// mechanism genuinely cannot interpret the term at all and Solr (whose
/// own analyzer may stem/fuzzy-match it) is strictly more likely to find
/// something, while the latter would silently admit a query to a
/// guaranteed-empty native result when falling back could have helped.
///
/// The identical reasoning applies when every individual token *is*
/// known but their AND-combination (optionally further ANDed with an
/// existing structural constraint) is empty: no single product carries
/// every token together, or none of the lexically-matching products also
/// satisfy the structural constraint. A real-data P3-E03 measurement
/// (`docs/experiments/PHASE3_LOG.md`) found this was not a rare edge case
/// -- 24.9% of otherwise-eligible queries hit exactly this, and a quarter
/// of those had Solr find a real relevant result the native path would
/// have silently returned nothing for. `count == 0` is therefore rejected
/// explicitly below rather than left to pass the cap check trivially (0
/// is never greater than any cap).
///
/// Additive, not integrated into `admit`/`AdmissionPolicy`: this keeps
/// the original, already-validated (P3-E00/E01/E02) admission contract
/// completely unchanged for existing callers. Callers should try `admit`
/// first and only reach for this on a `Reject(RejectReason::UnresolvedResidual)`
/// outcome (calling it on an ambiguous or already-structurally-admitted
/// query is meaningless, though not unsafe -- it reads the index, never
/// mutates anything).
pub fn admit_lexically_narrowed(
    query: &CommerceQuery,
    index: &CatalogIndex,
    max_lexical_narrowed_candidates: usize,
) -> Option<(RoaringBitmap, u64)> {
    if !query.ambiguous.is_empty() || query.residual_lexical.is_empty() {
        return None;
    }
    let residual_tokens: Vec<String> = query
        .residual_lexical
        .iter()
        .flat_map(|phrase| phrase.split_whitespace().map(str::to_lowercase))
        .collect();
    if residual_tokens.is_empty()
        || residual_tokens.iter().any(|t| {
            index
                .lexical_and_candidates(std::slice::from_ref(t))
                .is_empty()
        })
    {
        return None;
    }
    let lexical_bitmap = index.lexical_and_candidates(&residual_tokens);
    let combined = if query.constraints.is_empty() {
        lexical_bitmap
    } else {
        lexical_bitmap & index.indexed_candidates(&query.constraints)
    };
    let count = combined.len();
    if count == 0 || count as usize > max_lexical_narrowed_candidates {
        return None;
    }
    Some((combined, count))
}

/// Execute a query admitted via [`admit_lexically_narrowed`]. `narrow_by`
/// must be the exact bitmap that function returned -- this does not
/// recompute it, matching `execute_admitted`'s own "no internal re-check"
/// division of labor between routing and execution.
pub fn execute_lexically_narrowed(
    index: &CatalogIndex,
    query: &CommerceQuery,
    narrow_by: &RoaringBitmap,
    catalog: &Catalog,
    k: usize,
) -> Vec<crate::index::RankedHit> {
    index.execute_ranked_narrowed_by(query, narrow_by, catalog, k)
}

/// Issue #14 P3-E03/P3-E05: `admit_lexically_narrowed` measured on the
/// *entire* `UnresolvedResidual` population was REJECTed on real data --
/// every swept cap failed every one of Issue #14's four relevance
/// budgets, because independent per-token presence-in-title verification
/// is a weak precision signal with no ranking function behind it
/// (`docs/experiments/PHASE3_LOG.md` P3-E03). A follow-up diagnostic
/// (P3-E04) found the failure was not uniform: queries that *also* carry
/// an existing structural constraint (`query.constraints` non-empty)
/// alongside the residual text showed a consistently smaller NDCG delta
/// and a 2-3x lower false-positive rate than pure-lexical-only queries --
/// the structural half acts as an independent precision anchor, the same
/// kind P3-E02 found tolerates having no ranking signal reasonably well.
/// P3-E05's real-corpus + live-Solr measurement confirmed this: at
/// `max_lexical_narrowed_candidates=20`, this restricted policy reaches
/// 4.94% whole-corpus coverage while staying within Issue #14's 2%
/// relevance budget -- roughly 6x P3-E02's entire structural-only
/// coverage (0.81%).
///
/// This function is the hardened, tested version of that policy: unlike
/// `admit_lexically_narrowed`, which admits a pure-lexical-only query
/// just as readily as one with an existing structural constraint, this
/// rejects outright (`None`) whenever `query.constraints` is empty --
/// never falling back to "safely narrows on tokens alone," which is
/// exactly the case P3-E03 measured and rejected. Everything else is
/// unchanged from `admit_lexically_narrowed`; this exists so a future
/// caller cannot accidentally reproduce P3-E03's rejected behavior by
/// calling the wrong function, matching this module's own pattern of
/// encoding a safety boundary in the type/function contract rather than
/// leaving it to caller discipline.
pub fn admit_structurally_anchored_lexical(
    query: &CommerceQuery,
    index: &CatalogIndex,
    max_lexical_narrowed_candidates: usize,
) -> Option<(RoaringBitmap, u64)> {
    if query.constraints.is_empty() {
        return None;
    }
    admit_lexically_narrowed(query, index, max_lexical_narrowed_candidates)
}

/// Issue #14 P3-E08/P3-E09: a third mechanism, disjoint from both `admit`
/// and `admit_structurally_anchored_lexical`. P3-E08's diagnostic (real
/// data, `docs/experiments/PHASE3_LOG.md`) found the pure-lexical-only
/// population `admit_structurally_anchored_lexical` deliberately excludes
/// (no existing structural constraint) is not uniformly unsafe: queries
/// whose *entire* residual is a single token have a strikingly better
/// precision profile than 2-token or 3+-token residuals, and get
/// *better*, not worse, as the candidate cap loosens up to a point --
/// the opposite of every other admission mechanism measured so far.
/// P3-E09's real-corpus measurement confirmed this at the whole-workload
/// level: this population clears Issue #14's 2% relevance budget at
/// *every* swept cap, including effectively unlimited (200,000) --
/// 99.88% of the eligible population admits within budget, contributing
/// 3.66% whole-corpus coverage on its own, fully disjoint from the other
/// two KEPT mechanisms.
///
/// A single residual token is often a highly specific, low-ambiguity
/// term (a model name, a rare descriptor) that narrows the catalog on
/// its own almost as precisely as a structural constraint does; a
/// multi-token residual is more often a generic descriptive phrase
/// (P3-E03's own false-positive examples: "without full grille bar", "24
/// volt electric plug") whose independent-token AND-narrowing does not
/// reliably track the query's actual intent. This function encodes that
/// distinction directly: rejects outright (`None`) whenever the residual
/// does not tokenize to exactly one token, then delegates to
/// `admit_lexically_narrowed` unchanged. Deliberately does not also
/// require `query.constraints` to be empty -- unlike
/// `admit_structurally_anchored_lexical`, a single-token residual is
/// safe on its own merits regardless of whether a structural constraint
/// also happens to be present; a caller composing multiple admission
/// mechanisms should try them in a fixed order (e.g. `admit`, then
/// `admit_structurally_anchored_lexical`, then this) so a query already
/// admitted upstream is never re-evaluated here.
pub fn admit_single_token_lexical(
    query: &CommerceQuery,
    index: &CatalogIndex,
    max_lexical_narrowed_candidates: usize,
) -> Option<(RoaringBitmap, u64)> {
    let residual_token_count = query
        .residual_lexical
        .iter()
        .flat_map(|phrase| phrase.split_whitespace())
        .count();
    if residual_token_count != 1 {
        return None;
    }
    admit_lexically_narrowed(query, index, max_lexical_narrowed_candidates)
}
