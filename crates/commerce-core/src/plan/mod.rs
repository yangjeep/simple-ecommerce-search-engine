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

use crate::domain::{Catalog, ProductId, VariantId};
use crate::index::CatalogIndex;
use crate::ir::CommerceQuery;

/// One free-text ranking result from a [`LexicalDelegate`], before
/// `commerce_core` re-verifies it. `score` is delegate-defined (e.g.
/// Tantivy's BM25 score) and only meaningful for ordering within one
/// delegate's own results, not compared across delegates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LexicalHit {
    pub product: ProductId,
    pub score: f64,
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
pub fn execute_planned(
    query: &CommerceQuery,
    catalog: &Catalog,
    index: &CatalogIndex,
    delegate: Option<&dyn LexicalDelegate>,
    k: usize,
    policy: &PlannerPolicy,
) -> (PlannedQuery, Vec<PlannedHit>) {
    let planned = plan(query, index, catalog.products.len(), policy);
    let oversampled_limit = k * policy.delegate_oversample;
    let hits = match planned.outcome {
        ExecutionOutcome::FastPath => index
            .execute_ranked(query, catalog, k)
            .into_iter()
            .map(|h| PlannedHit {
                product: h.product,
                variant: h.variant,
                score: h.score,
            })
            .collect(),
        ExecutionOutcome::Hybrid => {
            let candidate_ords = index.indexed_candidates(&query.constraints);
            let restrict_to = index.candidate_product_ids(&candidate_ords);
            let raw = delegate
                .map(|d| {
                    d.search(
                        &query.residual_lexical,
                        Some(&restrict_to),
                        oversampled_limit,
                    )
                })
                .unwrap_or_default();
            verify_and_truncate(raw, Some(&restrict_to), query, catalog, index, k)
        }
        ExecutionOutcome::Punt => {
            let raw = delegate
                .map(|d| d.search(&query.residual_lexical, None, oversampled_limit))
                .unwrap_or_default();
            verify_and_truncate(raw, None, query, catalog, index, k)
        }
    };
    (planned, hits)
}

/// Re-verify every delegate hit against `commerce_core`'s own constraint
/// semantics before it is ever returned. `restrict_to`, when present, is
/// re-checked here too rather than trusted from the delegate call: a
/// delegate that ignores `restrict_to` (accidentally or otherwise) cannot
/// produce an incorrect result, only a wasted one. A product may have
/// multiple variants; the first variant that satisfies every constraint is
/// used (matching `CommerceQuery::execute`'s per-variant, not
/// per-product, matching discipline) — a product with no satisfying
/// variant is dropped even if the delegate ranked it highly.
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
        let Some(variant) = product
            .variants
            .iter()
            .find(|v| query.matches_variant(product, v))
        else {
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
        attributes, BrandId, CategoryId, Inventory, Price, Product, ProductTypeId, Variant,
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
            },
            LexicalHit {
                product: ProductId(2),
                score: 2.0, // ranked higher, but outside restrict_to
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
}
