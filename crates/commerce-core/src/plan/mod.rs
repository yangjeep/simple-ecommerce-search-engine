//! Issue #6 priority 5: the structural-plus-delegated-lexical execution
//! contract ADR 0008 committed to but did not build. P2-E01
//! (`docs/experiments/PHASE2_LOG.md`) validated that a delegated lexical
//! engine (Tantivy) recovers real relevance quality *standalone*; it did
//! not validate how that delegate composes with `commerce_core`'s own
//! structural/facet index at query time. This module is that composition.
//!
//! `commerce_core` defines the mechanism and owns correctness, exactly like
//! `control_plane::provider::ModelProvider` and
//! `control_plane::precision::PrecisionOracle`: a delegate is a trait, not
//! a dependency. No lexical-engine crate (Tantivy or otherwise) is a
//! dependency of `commerce_core` — the first real implementation of
//! [`LexicalDelegate`] lives in `phase2-eval`, wrapping the same Tantivy
//! index P2-E01 already validated.
//!
//! Three execution outcomes, matching Issue #5/#6's own naming:
//!
//! - **FastPath**: the compiled query resolved entirely to structural
//!   constraints (no residual free-text terms). The delegate is never
//!   called — this is the entire point of Gate 3's specialization, and the
//!   one workload class Round 1 found a real, uncontested physical
//!   advantage for (R1-E05).
//! - **Hybrid**: at least one structural constraint exists *and* it is
//!   genuinely selective (R1-E05's finding: the physical advantage is
//!   conditional on selectivity, not automatic). The structural index
//!   narrows the candidate set first; the delegate ranks free text only
//!   within that narrowed set.
//! - **Punt**: either no structural constraint exists at all, or the ones
//!   present are not selective (a near-universal bitmap — R1-E05's
//!   ~36,700x worst case). Rather than materialize a huge candidate
//!   bitmap only to discard most of it, the structural layer punts
//!   entirely: the delegate searches the whole corpus, and any structural
//!   constraints are re-verified against only the delegate's small
//!   returned hit set instead. This is the actual architectural answer
//!   R1-E05's finding demanded: never pay bitmap-intersection cost
//!   proportional to a non-selective candidate set.
//!
//! Regardless of outcome, `commerce_core` re-verifies every hard
//! constraint against every returned hit itself (`CommerceQuery::matches_variant`)
//! before returning it — a delegate is trusted for ranking/recall, never
//! for correctness (CLAUDE.md: "Product/variant correctness is
//! non-negotiable").

use std::collections::BTreeSet;

use crate::domain::{Catalog, ProductId, ProductTypeId, VariantId};
use crate::index::CatalogIndex;
use crate::ir::{CommerceQuery, ResolvedConstraint, StructuralConstraint};

mod residual;
pub use residual::{ResidualClass, ResidualPolicy};

/// One free-text ranking result from a [`LexicalDelegate`], before
/// `commerce_core` re-verifies it. `score` is delegate-defined (e.g.
/// Tantivy's BM25 score) and only meaningful for ordering within one
/// delegate's own results, not compared across delegates.
///
/// `variant` is an additive field (Issue #42 R3,
/// `docs/experiments/ISSUE42_LOG.md#i42-r3`, `docs/adr/0013-identifier-serving-primitive.md`):
/// the specific variant a delegate's match resolved to, if it can name
/// one. Every delegate in this workspace today constructs `variant: None`
/// -- none can resolve a specific variant, only a product -- so this is
/// purely a widening of what a *future* delegate can express, not a
/// behavior change for any existing one. When `Some`, it is a recall hint
/// only, never a correctness bypass: `verify_and_truncate` still
/// re-checks `query.matches_variant` against the named variant, and never
/// falls back to a different variant of the same product if it fails
/// (this module's own doc comment: a delegate is trusted for
/// ranking/recall, never for correctness).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LexicalHit {
    pub product: ProductId,
    pub score: f64,
    pub variant: Option<VariantId>,
}

/// The mechanism `commerce_core` requires of a delegated lexical/ranking
/// engine. `commerce_core` has no idea what implements this (Tantivy,
/// something else) and no dependency on it — mirrors
/// `control_plane::provider::ModelProvider` exactly.
pub trait LexicalDelegate {
    /// Rank up to `limit` candidates for `terms` (typically
    /// `query.residual_lexical`), optionally restricted to `restrict_to`
    /// (a structural pre-filter, present only for [`ExecutionOutcome::Hybrid`]).
    /// Implementations are free to over-return (e.g. an oversampled top-N
    /// before restriction) — `execute_planned` re-filters and truncates the
    /// result regardless, so a delegate cannot violate correctness by
    /// ignoring `restrict_to`, only waste its own effort.
    fn search(
        &self,
        terms: &[String],
        restrict_to: Option<&BTreeSet<ProductId>>,
        limit: usize,
    ) -> Vec<LexicalHit>;
}

/// Tuning knobs for [`plan`]/[`execute_planned`] that are not derived from
/// the query or the catalog itself — the kind of thing a real deployment
/// would eventually want to vary per vertical or per merchant rather than
/// compile in as a constant. Deliberately a named, typed policy rather
/// than bare parameters threaded through function signatures: this is the
/// extension point a future finding (e.g. from the `searchspring/search-api`
/// commerce-archaeology workstream, Issue #5 section 12) that turns out to
/// be a genuine merchant-specific or vertical-specific behavior should
/// extend — add a field here, not a new function parameter or a
/// conditional branch in `plan`/`execute_planned`.
///
/// No [`Default`] impl: neither field has an evidence-backed recommended
/// value yet (P2-E05, `docs/experiments/PHASE2_LOG.md`, found
/// `selectivity_threshold` did not even affect routing on the real
/// catalog once the actual root cause — unfiltered brand vocabulary —
/// was the confound; a real default requires re-measuring against the
/// corrected lexicon, not asserting one here ahead of that evidence).
/// Callers must construct one explicitly.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlannerPolicy {
    /// The maximum `structural_candidates / catalog_size` fraction still
    /// considered "genuinely selective" (R1-E05's own vocabulary) — at or
    /// under this, a query with a structural constraint routes to
    /// [`ExecutionOutcome::Hybrid`]; above it, to
    /// [`ExecutionOutcome::Punt`] (R1-E05 found non-selective structural
    /// bitmap intersection collapses performance, so `Punt` avoids ever
    /// materializing one).
    pub selectivity_threshold: f64,
    /// How large a multiple of the caller's requested `k` to ask a
    /// [`LexicalDelegate`] to over-return, so that `execute_planned`'s
    /// re-verification against `commerce_core`'s own constraints (which
    /// can drop delegate hits) still leaves enough candidates to reach
    /// `k` when that many genuinely exist. Not a correctness parameter —
    /// verification is exact regardless of this value — only a
    /// recall/cost tradeoff for how hard the delegate searches.
    pub delegate_oversample: usize,
}

/// Which of the three execution outcomes a compiled query was routed to,
/// and why (`selectivity`/`structural_candidates` explain a `Hybrid` vs.
/// `Punt` choice; both are `None` for `FastPath`, which never computes a
/// structural candidate set at all).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ExecutionOutcome {
    FastPath,
    Hybrid,
    Punt,
}

/// The routing decision for one compiled query against one index/catalog
/// size, before execution. Exposed separately from [`execute_planned`] so
/// callers (e.g. an experiment harness) can measure routing distribution
/// without paying for a full delegate call.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlannedQuery {
    pub outcome: ExecutionOutcome,
    /// `structural_candidates / catalog_size` — `None` for `FastPath`
    /// (no candidate set is computed) and for `Punt` via the "no
    /// structural constraint at all" path (also no candidate set computed).
    /// Present for `Punt` via the "non-selective" path, so a caller can see
    /// how close the query came to the threshold.
    pub selectivity: Option<f64>,
}

/// Route a compiled query to one of the three execution outcomes, per
/// `policy.selectivity_threshold` — the caller supplies the policy rather
/// than this module hardcoding a value, since the right threshold is an
/// empirical question this module's own evaluation harness
/// (`phase2-eval`) is what measures.
pub fn plan(
    query: &CommerceQuery,
    index: &CatalogIndex,
    catalog_size: usize,
    policy: &PlannerPolicy,
) -> PlannedQuery {
    if query.residual_lexical.is_empty() {
        return PlannedQuery {
            outcome: ExecutionOutcome::FastPath,
            selectivity: None,
        };
    }
    if query.constraints.is_empty() {
        return PlannedQuery {
            outcome: ExecutionOutcome::Punt,
            selectivity: None,
        };
    }
    let candidates = index.indexed_candidates(&query.constraints);
    let selectivity = if catalog_size == 0 {
        0.0
    } else {
        candidates.len() as f64 / catalog_size as f64
    };
    let outcome = if selectivity <= policy.selectivity_threshold {
        ExecutionOutcome::Hybrid
    } else {
        ExecutionOutcome::Punt
    };
    PlannedQuery {
        outcome,
        selectivity: Some(selectivity),
    }
}

/// One final, `commerce_core`-verified hit: every hard constraint in
/// `query.constraints` has been checked against this exact
/// `(product, variant)` pair, regardless of which outcome produced it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlannedHit {
    pub product: ProductId,
    pub variant: VariantId,
    pub score: f64,
}

/// Route and execute a compiled query. `delegate` is `None` when no
/// lexical engine is wired up at all (e.g. `commerce_core`'s own test
/// suite, which has no Tantivy dependency): `FastPath` still works fully,
/// `Hybrid`/`Punt` degrade to structural-only-or-empty rather than panic.
///
/// `residual_policy` is `None` for every caller unaffected by Issue #42 R2
/// (`docs/experiments/ISSUE42_LOG.md#i42-r2`, `docs/adr/0012-residual-lexical-policy.md`)
/// -- an additive, opt-in parameter, not a required one: passing `None`
/// reproduces byte-identical prior behavior in every case. When `Some`, a
/// `Hybrid`/`Punt` outcome whose delegate returns zero raw hits for a
/// non-empty residual can bypass the strict veto and fall back to the
/// structural candidate set instead of returning empty -- but only when a
/// corroborating `ProductType` constraint is present *and* every residual
/// token classifies [`ResidualClass::Preferred`] (see
/// [`residual_fallback_hits`]). Any `Required` token, or no corroborating
/// `ProductType` constraint at all, keeps today's exact behavior.
pub fn execute_planned(
    query: &CommerceQuery,
    catalog: &Catalog,
    index: &CatalogIndex,
    delegate: Option<&dyn LexicalDelegate>,
    k: usize,
    policy: &PlannerPolicy,
    residual_policy: Option<&ResidualPolicy>,
) -> (PlannedQuery, Vec<PlannedHit>) {
    let planned = plan(query, index, catalog.products.len(), policy);
    let oversampled_limit = k * policy.delegate_oversample;
    let hits = match planned.outcome {
        ExecutionOutcome::FastPath => {
            ranked_hits_to_planned(index.execute_ranked(query, catalog, k))
        }
        ExecutionOutcome::Hybrid => {
            let candidate_ords = index.indexed_candidates(&query.constraints);
            let restrict_to = index.candidate_product_ids(&candidate_ords);
            let identifier = identifier_hits(query, catalog, index, Some(&restrict_to), k);
            if !identifier.is_empty() {
                identifier
            } else {
                let raw = delegate
                    .map(|d| {
                        d.search(
                            &query.residual_lexical,
                            Some(&restrict_to),
                            oversampled_limit,
                        )
                    })
                    .unwrap_or_default();
                match residual_fallback_hits(&raw, query, catalog, index, k, residual_policy) {
                    Some(hits) => hits,
                    None => verify_and_truncate(raw, Some(&restrict_to), query, catalog, index, k),
                }
            }
        }
        ExecutionOutcome::Punt => {
            let identifier = identifier_hits(query, catalog, index, None, k);
            if !identifier.is_empty() {
                identifier
            } else {
                // Issue #6 P1-D / P2-E16 (`docs/experiments/PHASE2_LOG.md`): a
                // real P1-D benchmark measured `lexical_first` (36.8% of all
                // real traffic, 100% Punt-via-no-structural-constraint) paying
                // for a `k * policy.delegate_oversample` (e.g. 200 instead of
                // 10) delegate call for no benefit -- when `query.constraints`
                // is empty, `verify_and_truncate`'s constraint check
                // (`CommerceQuery::matches_variant`, an `.all()` over an empty
                // iterator) is vacuously true for every hit, so no delegate hit
                // can ever be rejected on constraint grounds and oversampling
                // cannot change which `k` hits end up returned -- it only
                // forces the delegate to do more work (e.g. more stored-field
                // fetches) for a result that's provably identical to asking for
                // `k` directly. Oversampling still applies whenever
                // `restrict_to` is present (`Hybrid`, reached only when
                // `query.constraints` is non-empty) or a real constraint could
                // still reject a hit, where headroom is genuinely needed.
                let limit = if query.constraints.is_empty() {
                    k
                } else {
                    oversampled_limit
                };
                let raw = delegate
                    .map(|d| d.search(&query.residual_lexical, None, limit))
                    .unwrap_or_default();
                match residual_fallback_hits(&raw, query, catalog, index, k, residual_policy) {
                    Some(hits) => hits,
                    None => verify_and_truncate(raw, None, query, catalog, index, k),
                }
            }
        }
    };
    (planned, hits)
}

/// Issue #42 R3's identifier serving primitive
/// (`docs/experiments/ISSUE42_LOG.md#i42-r3`, `docs/adr/0013-identifier-serving-primitive.md`):
/// an exact identifier-dictionary lookup against every token in
/// `query.residual_lexical`, tried by `execute_planned`'s `Hybrid`/`Punt`
/// arms *before* the delegate is ever called. An exact identifier match
/// (e.g. a part number) is a strictly higher-precision signal than general
/// lexical ranking, and CLAUDE.md's Mission is explicit that "structural
/// retrieval is primary where semantics are known" -- so a non-empty
/// result here means the delegate is skipped entirely for this query.
///
/// Every candidate is still independently re-verified via
/// `query.matches_variant` (and, when `restrict_to` is `Some` -- the
/// `Hybrid` case -- membership in it too) before being returned: an
/// identifier match is a recall hint, never a bypass of `commerce_core`'s
/// own correctness ownership (this module's own doc comment). Returns an
/// empty `Vec` when no token in `query.residual_lexical` has any
/// identifier-dictionary entry at all (including when this catalog has no
/// accepted identifier fields whatsoever), so the caller falls through to
/// today's existing delegate-based logic completely unchanged.
fn identifier_hits(
    query: &CommerceQuery,
    catalog: &Catalog,
    index: &CatalogIndex,
    restrict_to: Option<&BTreeSet<ProductId>>,
    k: usize,
) -> Vec<PlannedHit> {
    let mut union: BTreeSet<(ProductId, VariantId)> = BTreeSet::new();
    for token in &query.residual_lexical {
        union.extend(index.identifier_lookup(token));
    }

    let mut out = Vec::new();
    for (product_id, variant_id) in union {
        if out.len() >= k {
            break;
        }
        if let Some(allowed) = restrict_to {
            if !allowed.contains(&product_id) {
                continue;
            }
        }
        let Some((product, variant)) = index.lookup_variant(catalog, variant_id) else {
            continue;
        };
        if !query.matches_variant(product, variant) {
            continue;
        }
        out.push(PlannedHit {
            product: product_id,
            variant: variant_id,
            score: 1.0,
        });
    }
    out
}

/// The exact `RankedHit` -> `PlannedHit` transform `execute_planned`'s
/// `FastPath` arm and [`residual_fallback_hits`]'s structural fallback both
/// need -- kept as one function so the two call sites cannot silently
/// diverge in shape.
fn ranked_hits_to_planned(hits: Vec<crate::index::RankedHit>) -> Vec<PlannedHit> {
    hits.into_iter()
        .map(|h| PlannedHit {
            product: h.product,
            variant: h.variant,
            score: h.score,
        })
        .collect()
}

/// The bare `ProductTypeId` a query's own structural constraints name, if
/// any -- the "corroborating entity" [`residual_fallback_hits`] requires
/// before it will ever consult a [`ResidualPolicy`]. Deliberately does not
/// inspect anything about the constraint's *narrowness* (e.g. a compound
/// `ProductType` + `Enum` constraint's real, possibly smaller candidate
/// set): that distinction belongs to `ResidualPolicy::classify`'s own
/// cross-type-breadth signal, not to this lookup
/// (`docs/adr/0012-residual-lexical-policy.md`).
fn corroborating_product_type(query: &CommerceQuery) -> Option<ProductTypeId> {
    query.constraints.iter().find_map(|c| match c {
        ResolvedConstraint::Structural(StructuralConstraint::ProductType(id)) => Some(*id),
        _ => None,
    })
}

/// Issue #42 R2's evidence-backed residual fallback
/// (`docs/experiments/ISSUE42_LOG.md#i42-r2`, `docs/adr/0012-residual-lexical-policy.md`):
/// applies only when every one of four conditions holds --
///
/// 1. `raw` (the delegate's own hits for this `Hybrid`/`Punt` outcome) is
///    empty;
/// 2. `residual_policy` is `Some` (the caller opted in);
/// 3. [`corroborating_product_type`] finds a `ProductType` constraint in
///    `query.constraints`;
/// 4. every token in `query.residual_lexical` classifies
///    [`ResidualClass::Preferred`] -- `query.residual_lexical` is
///    guaranteed non-empty here, since `plan()` routes an empty-residual
///    query to `FastPath`, which never reaches this function's callers.
///
/// Returns `Some` with the structural candidate set (ranked via
/// `execute_ranked`, the same signal `FastPath` uses) when the fallback
/// applies; `None` otherwise, so the caller proceeds with today's exact
/// `verify_and_truncate` call -- any `Required` token, any missing
/// corroborating `ProductType`, or `residual_policy: None` all keep
/// current behavior unchanged.
fn residual_fallback_hits(
    raw: &[LexicalHit],
    query: &CommerceQuery,
    catalog: &Catalog,
    index: &CatalogIndex,
    k: usize,
    residual_policy: Option<&ResidualPolicy>,
) -> Option<Vec<PlannedHit>> {
    if !raw.is_empty() {
        return None;
    }
    let residual_policy = residual_policy?;
    corroborating_product_type(query)?;
    let all_preferred = query
        .residual_lexical
        .iter()
        .all(|token| residual_policy.classify(token) == ResidualClass::Preferred);
    if !all_preferred {
        return None;
    }
    Some(ranked_hits_to_planned(
        index.execute_ranked(query, catalog, k),
    ))
}

/// Re-verify every delegate hit against `commerce_core`'s own constraint
/// semantics before it is ever returned. `restrict_to`, when present, is
/// re-checked here too rather than trusted from the delegate call: a
/// delegate that ignores `restrict_to` (accidentally or otherwise) cannot
/// produce an incorrect result, only a wasted one. A product may have
/// multiple variants; when `hit.variant` names one (Issue #42 R3,
/// `docs/experiments/ISSUE42_LOG.md#i42-r3`), that variant is preferred --
/// but only if it also satisfies every constraint, never unconditionally
/// (a named variant is a recall hint, never a bypass of this function's
/// own correctness ownership). When `hit.variant` names a variant that
/// fails a constraint, the hit is dropped entirely; it never falls back to
/// a different, constraint-satisfying variant of the same product, since
/// that would silently substitute a variant the delegate never actually
/// matched. When `hit.variant` is `None` (every delegate in this workspace
/// today), the first variant that satisfies every constraint is used
/// (matching `CommerceQuery::execute`'s per-variant, not per-product,
/// matching discipline) — a product with no satisfying variant is dropped
/// even if the delegate ranked it highly.
///
/// `pub(crate)` (not private) specifically so `tests` below can exercise
/// `restrict_to` in isolation from `query.constraints` — worth recording
/// why. `execute_planned`'s one current caller always derives `restrict_to`
/// from `query.constraints` itself (`indexed_candidates(&query.constraints)`),
/// and `matches_variant` below already checks every constraint in that same
/// set, so *in that one call pattern* the `restrict_to` membership check
/// can never independently change the outcome — it is provably redundant
/// with the constraint check today. This was found, not assumed: an
/// attempted RED-evidence demonstration for a `restrict_to`-focused test
/// (temporarily removing the membership check) stayed GREEN, because the
/// test's query's only constraint already excluded the same product the
/// removed check would have. Deleting the check would have been the wrong
/// fix: `restrict_to` is kept as an independent parameter deliberately, as
/// the extension point for a future restriction *not* derivable from
/// `query.constraints` alone — e.g. a merchandising policy restricting to
/// a curated collection, or an in-stock-only filter applied above the
/// query layer (Issue #5 section 12's merchandising-policy category).
/// `tests::restrict_to_independently_excludes_a_constraint_satisfying_hit`
/// exercises exactly that scenario directly, since `execute_planned`'s
/// current single call pattern cannot.
///
/// Looks products up via `index.lookup_product` (an O(1) hash-map lookup)
/// rather than a linear `catalog.products.iter().find(...)` scan. The
/// first version of this function used the linear scan and was only
/// caught as a real, severe problem (not a hypothetical one) when
/// P2-E05's real-data run against the full 1.2M-product catalog hung
/// rather than completing in the few seconds every other real-data
/// experiment in this project has taken -- every delegate hit was paying
/// up to O(catalog size) just to be verified, and a query loop over
/// thousands of real queries each verifying `k * DELEGATE_OVERSAMPLE`
/// candidates made that cost dominate everything else by orders of
/// magnitude. Recorded here, not silently fixed, per this project's
/// "record failed experiments" discipline (`docs/experiments/PHASE2_LOG.md`
/// P2-E03 set the precedent for a real-data-caught bug in new code being
/// reported, not smoothed over).
pub(crate) fn verify_and_truncate(
    raw: Vec<LexicalHit>,
    restrict_to: Option<&BTreeSet<ProductId>>,
    query: &CommerceQuery,
    catalog: &Catalog,
    index: &CatalogIndex,
    k: usize,
) -> Vec<PlannedHit> {
    let mut out = Vec::with_capacity(k);
    for hit in raw {
        if out.len() >= k {
            break;
        }
        if let Some(allowed) = restrict_to {
            if !allowed.contains(&hit.product) {
                continue;
            }
        }
        let Some(product) = index.lookup_product(catalog, hit.product) else {
            continue;
        };
        let variant = match hit.variant {
            Some(vid) => product
                .variants
                .iter()
                .find(|v| v.id == vid && query.matches_variant(product, v)),
            None => product
                .variants
                .iter()
                .find(|v| query.matches_variant(product, v)),
        };
        let Some(variant) = variant else {
            continue;
        };
        out.push(PlannedHit {
            product: product.id,
            variant: variant.id,
            score: hit.score,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{
        attributes, AttributeValue, BrandId, CategoryId, Constraint, Inventory, Price, Product,
        ProductTypeId, Variant,
    };

    fn two_product_catalog() -> Catalog {
        let make = |id: u64| Product {
            id: ProductId(id),
            product_type: ProductTypeId(1),
            brand: BrandId(1),
            category: CategoryId(1),
            title: format!("Product {id}"),
            attributes: attributes([]),
            variants: vec![Variant {
                id: VariantId(100 + id),
                attributes: attributes([]),
                price: Price::usd(1_000),
                inventory: Inventory::in_stock(1),
            }],
        };
        Catalog {
            products: vec![make(1), make(2)],
        }
    }

    /// Directly exercises `verify_and_truncate`'s `restrict_to` parameter
    /// independent of `query.constraints`, proving it does real,
    /// independent filtering work when a caller supplies a restriction not
    /// derivable from the query's own constraints -- something
    /// `execute_planned`'s current single call pattern (`restrict_to`
    /// always derived from `query.constraints`) cannot demonstrate, since
    /// `matches_variant` alone already subsumes a same-derivation
    /// `restrict_to` there. See the doc comment on `verify_and_truncate`
    /// for the full context: an earlier, less specific version of this
    /// test (`tests/plan.rs`'s `verify_and_truncate_drops_a_delegate_hit_outside_restrict_to`)
    /// stayed GREEN even with the `restrict_to` check temporarily deleted,
    /// because that test's query had a structural constraint that already
    /// excluded the same product for an unrelated reason -- a false
    /// negative in RED-evidence terms, corrected by this test.
    #[test]
    fn restrict_to_independently_excludes_a_constraint_satisfying_hit() {
        let catalog = two_product_catalog();
        let index = CatalogIndex::build(&catalog);
        // No constraints at all: both products trivially satisfy
        // `matches_variant` (an empty `.all(...)` is vacuously true), so
        // only `restrict_to` can be responsible for excluding product 2.
        let query = CommerceQuery {
            constraints: vec![],
            preferences: vec![],
            ambiguous: vec![],
            residual_lexical: vec!["anything".to_string()],
        };
        let restrict_to: BTreeSet<ProductId> = BTreeSet::from([ProductId(1)]);
        let raw = vec![
            LexicalHit {
                product: ProductId(1),
                score: 1.0,
                variant: None,
            },
            LexicalHit {
                product: ProductId(2),
                score: 2.0, // ranked higher, but outside restrict_to
                variant: None,
            },
        ];

        let hits = verify_and_truncate(raw, Some(&restrict_to), &query, &catalog, &index, 10);

        assert_eq!(
            hits.iter().map(|h| h.product).collect::<Vec<_>>(),
            vec![ProductId(1)],
            "product 2 satisfies every (empty) query constraint and was ranked \
             higher by the delegate, yet must still be excluded by restrict_to alone"
        );
    }

    // Issue #42 R2 (`docs/experiments/ISSUE42_LOG.md#i42-r2`,
    // `docs/adr/0012-residual-lexical-policy.md`): regression coverage for
    // `execute_planned`'s new, additive `residual_policy` parameter and the
    // `residual_fallback_hits` gate it feeds. Uses
    // `crate::fixtures::residual_policy_catalog` (the same shape as
    // `issue42-eval::r2_workload::build()`, ported in rather than depended
    // on) plus a stub delegate that always returns zero raw hits, so every
    // test isolates the fallback's own four-condition gate rather than
    // real delegate behavior.

    use crate::cold_start::{compile_lexicon, CatalogProfile};
    use crate::fixtures::residual_policy_catalog;

    const RESIDUAL_TEST_POLICY: PlannerPolicy = PlannerPolicy {
        selectivity_threshold: 0.05,
        delegate_oversample: 20,
    };

    /// Always returns zero hits, regardless of terms/restriction -- isolates
    /// `residual_fallback_hits`'s own gate from real delegate behavior.
    struct EmptyDelegate;

    impl LexicalDelegate for EmptyDelegate {
        fn search(
            &self,
            _terms: &[String],
            _restrict_to: Option<&BTreeSet<ProductId>>,
            _limit: usize,
        ) -> Vec<LexicalHit> {
            Vec::new()
        }
    }

    fn residual_test_setup() -> (
        crate::fixtures::ResidualPolicyCatalog,
        crate::ir::SemanticLexicon,
        CatalogIndex,
    ) {
        let fixture = residual_policy_catalog();
        let profile = CatalogProfile::build(
            &fixture.catalog,
            &fixture.brands,
            &fixture.product_types,
            &fixture.categories,
        );
        let lexicon = compile_lexicon(&profile, 1);
        let index = CatalogIndex::build(&fixture.catalog);
        (fixture, lexicon, index)
    }

    /// Case (a): every residual token Preferred, zero raw delegate hits, a
    /// corroborating `ProductType` constraint present -- must recover the
    /// structural candidate set instead of staying at zero.
    #[test]
    fn residual_fallback_recovers_structural_set_when_every_residual_token_is_preferred() {
        let (fixture, lexicon, index) = residual_test_setup();
        let residual_policy = ResidualPolicy::compile(&fixture.catalog);
        let query = crate::ir::compile("furniture sofas", &lexicon);
        assert_eq!(
            query.constraints,
            vec![ResolvedConstraint::Structural(
                StructuralConstraint::ProductType(fixture.sofas)
            )],
            "this test's premise is a single corroborating ProductType(Sofas) constraint; got {:?}",
            query.constraints
        );
        assert_eq!(query.residual_lexical, vec!["furniture".to_string()]);
        let delegate = EmptyDelegate;

        let (_planned, hits) = execute_planned(
            &query,
            &fixture.catalog,
            &index,
            Some(&delegate),
            10,
            &RESIDUAL_TEST_POLICY,
            Some(&residual_policy),
        );

        assert!(
            !hits.is_empty(),
            "an all-Preferred residual with zero raw delegate hits and a corroborating \
             ProductType constraint must fall back to the structural candidate set, not stay at \
             zero"
        );
        assert!(
            hits.iter()
                .all(|h| h.product == ProductId(1) || h.product == ProductId(2)),
            "the recovered set must be exactly the Sofas structural candidates, got {:?}",
            hits.iter().map(|h| h.product).collect::<Vec<_>>()
        );
    }

    /// Case (b): the exact same query/setup as (a), but `residual_policy:
    /// None` -- must reproduce today's exact (empty) output, proving the
    /// new parameter is truly additive/opt-in.
    #[test]
    fn residual_policy_none_reproduces_todays_exact_empty_output() {
        let (fixture, lexicon, index) = residual_test_setup();
        let query = crate::ir::compile("furniture sofas", &lexicon);
        let delegate = EmptyDelegate;

        let (_planned, hits) = execute_planned(
            &query,
            &fixture.catalog,
            &index,
            Some(&delegate),
            10,
            &RESIDUAL_TEST_POLICY,
            None,
        );

        assert!(
            hits.is_empty(),
            "residual_policy: None must reproduce today's exact empty output for the same query \
             that (a) recovers with Some(&residual_policy) -- proving the parameter is additive"
        );
    }

    /// Case (c): a Required (narrowly-observed, genuinely specific)
    /// residual token, even with a corroborating `ProductType` constraint
    /// and `Some(&residual_policy)` -- must still veto to empty. The single
    /// most important correctness case: a real specific attribute value
    /// must still exclude a query it does not belong to.
    #[test]
    fn required_residual_token_still_vetoes_with_a_corroborating_product_type() {
        let (fixture, lexicon, index) = residual_test_setup();
        let residual_policy = ResidualPolicy::compile(&fixture.catalog);
        let query = crate::ir::compile("velvet boots", &lexicon);
        assert_eq!(
            query.constraints,
            vec![ResolvedConstraint::Structural(
                StructuralConstraint::ProductType(fixture.boots)
            )],
            "this test's premise is a single corroborating ProductType(Boots) constraint; got {:?}",
            query.constraints
        );
        assert_eq!(query.residual_lexical, vec!["velvet".to_string()]);
        assert_eq!(
            residual_policy.classify("velvet"),
            ResidualClass::Required,
            "'velvet' is observed under exactly one product type (Sofas) in this fixture -- below \
             the cross-type-breadth threshold -- so it must classify Required even for a Boots \
             query"
        );
        let delegate = EmptyDelegate;

        let (_planned, hits) = execute_planned(
            &query,
            &fixture.catalog,
            &index,
            Some(&delegate),
            10,
            &RESIDUAL_TEST_POLICY,
            Some(&residual_policy),
        );

        assert!(
            hits.is_empty(),
            "a Required residual token must still veto the query to empty even with a \
             corroborating ProductType constraint present, got {:?}",
            hits.iter().map(|h| h.product).collect::<Vec<_>>()
        );
    }

    /// Case (d): every residual token Preferred and `Some(&residual_policy)`,
    /// but the query's only structural constraint is `Brand`, not
    /// `ProductType` -- must still veto to empty. The fallback requires a
    /// corroborating `ProductType` specifically, not any structural
    /// constraint.
    #[test]
    fn fallback_requires_a_corroborating_product_type_not_any_structural_constraint() {
        let (fixture, lexicon, index) = residual_test_setup();
        let residual_policy = ResidualPolicy::compile(&fixture.catalog);
        let query = crate::ir::compile("acme furniture", &lexicon);
        assert_eq!(
            query.constraints,
            vec![ResolvedConstraint::Structural(StructuralConstraint::Brand(
                BrandId(1)
            ))],
            "this test's premise is a Brand constraint with no ProductType constraint anywhere; \
             got {:?}",
            query.constraints
        );
        assert_eq!(query.residual_lexical, vec!["furniture".to_string()]);
        assert_eq!(
            residual_policy.classify("furniture"),
            ResidualClass::Preferred
        );
        let delegate = EmptyDelegate;

        let (_planned, hits) = execute_planned(
            &query,
            &fixture.catalog,
            &index,
            Some(&delegate),
            10,
            &RESIDUAL_TEST_POLICY,
            Some(&residual_policy),
        );

        assert!(
            hits.is_empty(),
            "a Brand constraint alone (no corroborating ProductType) must not trigger the \
             residual fallback, even when every residual token is Preferred, got {:?}",
            hits.iter().map(|h| h.product).collect::<Vec<_>>()
        );
    }

    /// Second correction round (a fresh adversarial review of this
    /// production merge, before it was committed): the original
    /// `residual_policy_catalog` fixture gave every product `attributes([])`,
    /// so no test in this module could ever compile a *compound*
    /// `ProductType(Sofas) AND Enum(color=...)` constraint at all -- exactly
    /// the shape Issue #42 R2's own second correction round needed to catch
    /// a real false positive (`docs/experiments/ISSUE42_LOG.md#i42-r2`,
    /// `docs/adr/0012-residual-lexical-policy.md`). This test proves the
    /// fallback's structural recovery (`residual_fallback_hits`'s call to
    /// `index.execute_ranked`) respects a *compound* constraint's full,
    /// narrowed candidate set -- not just the bare `ProductType` --
    /// something no prior commerce-core test exercised through
    /// `execute_planned` at all, compound or otherwise: an all-`Preferred`
    /// residual word ("furniture") plus a compound `ProductType(Sofas) AND
    /// Enum(color=Blue)` constraint must recover ONLY the Blue Leather Sofa
    /// (P1), never the Purple Velvet Sofa (P2), even though P2 also
    /// satisfies the bare `ProductType(Sofas)` constraint alone.
    #[test]
    fn residual_fallback_respects_a_compound_constraints_full_narrowed_candidate_set() {
        let (fixture, lexicon, index) = residual_test_setup();
        let residual_policy = ResidualPolicy::compile(&fixture.catalog);
        let query = crate::ir::compile("blue furniture sofas", &lexicon);
        assert_eq!(
            query.constraints.len(),
            2,
            "this test's whole premise is a compound structural constraint (ProductType AND \
             color); got {:?}",
            query.constraints
        );
        assert_eq!(query.residual_lexical, vec!["furniture".to_string()]);
        assert_eq!(
            residual_policy.classify("furniture"),
            ResidualClass::Preferred,
            "'furniture' is the fixture's own benign, broadly-observed cross-type word"
        );
        let delegate = EmptyDelegate;

        let (_planned, hits) = execute_planned(
            &query,
            &fixture.catalog,
            &index,
            Some(&delegate),
            10,
            &RESIDUAL_TEST_POLICY,
            Some(&residual_policy),
        );

        assert_eq!(
            hits.iter().map(|h| h.product).collect::<Vec<_>>(),
            vec![ProductId(1)],
            "the fallback must recover exactly the compound constraint's real candidate set \
             (Blue Leather Sofa only) -- never P2 (Purple Velvet Sofa), which satisfies the bare \
             ProductType(Sofas) constraint alone but not the compound color=Blue constraint; got \
             {:?}",
            hits.iter().map(|h| h.product).collect::<Vec<_>>()
        );
    }

    // Issue #42 R3 (`docs/experiments/ISSUE42_LOG.md#i42-r3`,
    // `docs/adr/0013-identifier-serving-primitive.md`): regression coverage
    // for `identifier_hits` and its wiring into `execute_planned`'s
    // `Hybrid`/`Punt` arms, and for `verify_and_truncate`'s new
    // named-variant resolution in `LexicalHit::variant`.

    /// A catalog with well over 100 occurrences (`index::identifier::MIN_IDENTIFIER_SAMPLE_SIZE`)
    /// of a variant-scoped, all-distinct `serial_number` field -- clearing
    /// every `IdentifierClassifier` gate -- plus one target product with
    /// two variants sharing the same `ProductType` (so both trivially
    /// satisfy a `ProductType` structural constraint, since `ProductType`
    /// is a per-product, not per-variant, field) but distinct
    /// `serial_number` values. The exact shape needed to prove
    /// `execute_planned` resolves an identifier query to the SPECIFIC
    /// variant its own identifier names, not "whichever variant happens to
    /// satisfy the query's structural constraints first" (today's
    /// pre-R3 behavior, and still `verify_and_truncate`'s own fallback for
    /// any hit with `variant: None`).
    fn identifier_disambiguation_catalog() -> (Catalog, ProductId, VariantId, VariantId) {
        const FILLER_TYPE: u32 = 1;
        const TARGET_TYPE: u32 = 2;

        let filler_count = 110u64; // well over the classifier's 100-occurrence minimum
        let mut products: Vec<Product> = (0..filler_count)
            .map(|i| Product {
                id: ProductId(i),
                product_type: ProductTypeId(FILLER_TYPE),
                brand: BrandId(1),
                category: CategoryId(1),
                title: format!("Filler {i}"),
                attributes: attributes([]),
                variants: vec![Variant {
                    id: VariantId(1_000 + i),
                    attributes: attributes([(
                        "serial_number",
                        AttributeValue::Text(format!("FILL-{i:06}")),
                    )]),
                    price: Price::usd(1_000),
                    inventory: Inventory::in_stock(1),
                }],
            })
            .collect();

        let target_product = ProductId(9_000);
        let variant_1 = VariantId(90_001);
        let variant_2 = VariantId(90_002);
        products.push(Product {
            id: target_product,
            product_type: ProductTypeId(TARGET_TYPE),
            brand: BrandId(1),
            category: CategoryId(1),
            title: "Target Product".to_string(),
            attributes: attributes([]),
            variants: vec![
                Variant {
                    id: variant_1,
                    attributes: attributes([(
                        "serial_number",
                        AttributeValue::Text("TARGET-AAA".to_string()),
                    )]),
                    price: Price::usd(2_000),
                    inventory: Inventory::in_stock(1),
                },
                Variant {
                    id: variant_2,
                    attributes: attributes([(
                        "serial_number",
                        AttributeValue::Text("TARGET-BBB".to_string()),
                    )]),
                    price: Price::usd(2_000),
                    inventory: Inventory::in_stock(1),
                },
            ],
        });

        (Catalog { products }, target_product, variant_1, variant_2)
    }

    #[test]
    fn execute_planned_resolves_an_identifier_query_to_the_specific_named_variant() {
        let (catalog, target_product, variant_1, variant_2) = identifier_disambiguation_catalog();
        let index = CatalogIndex::build(&catalog);
        assert_eq!(
            index.identifier_field_count(),
            1,
            "this test's premise is that serial_number clears every IdentifierClassifier gate"
        );

        let query = CommerceQuery {
            constraints: vec![ResolvedConstraint::Structural(
                StructuralConstraint::ProductType(ProductTypeId(2)),
            )],
            preferences: vec![],
            ambiguous: vec![],
            residual_lexical: vec!["TARGET-BBB".to_string()],
        };
        // A delegate that finds nothing -- proving the result below comes
        // from the identifier lookup, not from any delegate behavior.
        let delegate = EmptyDelegate;

        let (planned, hits) = execute_planned(
            &query,
            &catalog,
            &index,
            Some(&delegate),
            10,
            &RESIDUAL_TEST_POLICY,
            None,
        );

        assert_eq!(planned.outcome, ExecutionOutcome::Hybrid);
        assert_eq!(
            hits.len(),
            1,
            "the identifier lookup must resolve to exactly one hit, got {hits:?}"
        );
        assert_eq!(hits[0].product, target_product);
        assert_eq!(
            hits[0].variant, variant_2,
            "must resolve to the SECOND variant (the one the query's identifier actually names), \
             not the first variant that merely happens to also satisfy the ProductType \
             constraint -- today's pre-R3 arbitrary-first-variant behavior"
        );
        assert_ne!(hits[0].variant, variant_1);
    }

    /// A delegate hit whose `variant: Some(vid)` names a variant that does
    /// NOT satisfy `query.matches_variant` must be rejected outright by
    /// `verify_and_truncate` -- never silently substituted with a
    /// different, constraint-satisfying variant of the same product, even
    /// though one exists.
    #[test]
    fn verify_and_truncate_rejects_a_named_variant_that_fails_a_constraint_without_falling_back() {
        let catalog = crate::fixtures::variant_safety_catalog();
        let index = CatalogIndex::build(&catalog);
        let query = CommerceQuery {
            constraints: vec![ResolvedConstraint::Attribute(Constraint::Enum {
                attribute: "color".to_string(),
                value: "Black".to_string(),
            })],
            preferences: vec![],
            ambiguous: vec![],
            residual_lexical: vec!["anything".to_string()],
        };
        // VariantId(101) (Black/size 8) satisfies this constraint;
        // VariantId(102) (Red/size 9) does not. The (simulated) delegate
        // names 102 specifically.
        let raw = vec![LexicalHit {
            product: ProductId(1),
            score: 1.0,
            variant: Some(VariantId(102)),
        }];

        let hits = verify_and_truncate(raw, None, &query, &catalog, &index, 10);

        assert!(
            hits.is_empty(),
            "a named variant that fails a constraint must be dropped entirely, never \
             substituted with a different variant of the same product, got {hits:?}"
        );
    }

    /// A residual token with no match in any identifier dictionary at all
    /// (including a catalog with zero accepted identifier fields) must
    /// fall straight through to today's existing delegate-based logic,
    /// unchanged -- proving `identifier_hits` cannot accidentally
    /// short-circuit a genuine miss into an empty result the delegate
    /// would otherwise have recovered.
    #[test]
    fn a_residual_token_with_no_identifier_match_falls_through_to_the_delegate_path_unchanged() {
        let catalog = two_product_catalog();
        let index = CatalogIndex::build(&catalog);
        assert_eq!(
            index.identifier_field_count(),
            0,
            "this test's premise is a catalog with no accepted identifier fields at all"
        );

        let query = CommerceQuery {
            constraints: vec![],
            preferences: vec![],
            ambiguous: vec![],
            residual_lexical: vec!["nonexistent-identifier".to_string()],
        };
        let delegate = MockOneHitDelegate(LexicalHit {
            product: ProductId(1),
            score: 1.0,
            variant: None,
        });

        let (planned, hits) = execute_planned(
            &query,
            &catalog,
            &index,
            Some(&delegate),
            10,
            &RESIDUAL_TEST_POLICY,
            None,
        );

        assert_eq!(planned.outcome, ExecutionOutcome::Punt);
        assert_eq!(
            hits.iter().map(|h| h.product).collect::<Vec<_>>(),
            vec![ProductId(1)],
            "a miss in every identifier dictionary must fall through to the existing \
             delegate-based path unchanged, not silently short-circuit to empty"
        );
    }

    /// A minimal delegate that always returns one fixed hit, regardless of
    /// terms/restriction -- used only to prove the delegate path still
    /// runs (and its hit is still returned) when the identifier lookup
    /// finds nothing.
    struct MockOneHitDelegate(LexicalHit);

    impl LexicalDelegate for MockOneHitDelegate {
        fn search(
            &self,
            _terms: &[String],
            _restrict_to: Option<&BTreeSet<ProductId>>,
            _limit: usize,
        ) -> Vec<LexicalHit> {
            vec![self.0]
        }
    }
}
