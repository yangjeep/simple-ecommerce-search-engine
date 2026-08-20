//! Issue #8: the commerce-state fast path. Separates two update classes
//! CLAUDE.md and Issue #8 both name explicitly: slow-changing *semantic*
//! state (brand, taxonomy, attribute meaning -- `cold_start`/`control_plane`'s
//! job, may justify expensive/model-assisted compilation) from
//! fast-changing *operational* state (variant availability -- this
//! module's job, must be cheap, deterministic, and query-visible
//! immediately, with no reindexing).
//!
//! Havenask/IndexLib archaeology (`docs/research/havenask-realtime-update-archaeology.md`)
//! found a mature system independently converges on the same physical
//! idea for exactly this class of field: true in-place mutation (a bit
//! flip for a bitmap/filterable term, a fixed-offset write for a scalar
//! attribute), bypassing the full document/segment reindex path entirely,
//! gated by an explicit "is this field allowed the cheap path" mechanism
//! (`AddToUpdateDocumentRewriter`). This module is the same algorithmic
//! pattern -- in-place `RoaringBitmap` mutation over the exact ordinal
//! scheme `CatalogIndex` already uses -- implemented natively rather than
//! reproduced from Havenask's C++ (clean-room, per Issue #5's own
//! constraint), scoped to our typed `VariantId` domain rather than a
//! generic field-update mechanism.
//!
//! `CommerceStateOverlay` is deliberately independent of `CatalogIndex`:
//! the immutable structural index knows nothing about mutable state, and
//! this module knows nothing about brand/category/taxonomy semantics --
//! composition happens only in [`execute_with_overlay`]. Neither this
//! module nor its callers ever invoke a model/LLM (CLAUDE.md: "No
//! LLM/model call in the default query hot path") -- a delta's `source`
//! and `authority` are opaque provenance the engine stores for conflict
//! resolution, never interpreted or looked up against any external
//! system. The engine does not know what Shopify, SFCC, a beacon, or a
//! merchant webhook is, per Issue #8's own scope boundary.

use std::collections::HashMap;

use roaring::RoaringBitmap;

use crate::domain::{Availability, Catalog, Constraint, ProductId, VariantId};
use crate::index::CatalogIndex;
use crate::ir::{CommerceQuery, ResolvedConstraint};

/// Whether the delta producing this state is a raw, unverified signal
/// (a beacon/webhook observation) or a definitive, authoritative
/// reconciliation. Precedence rule (see [`CommerceStateOverlay::apply`]):
/// an `Authoritative` delta always applies, correcting a possibly-wrong
/// `Observed` guess regardless of timestamp ordering; within the same
/// tier, later `observed_at` wins and an older one arriving late is
/// rejected as stale. This is a deliberate design choice, not one dictated
/// by evidence (Issue #8 leaves the exact precedence unspecified) --
/// recorded here so it's an explicit decision, not an accident.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Authority {
    Observed,
    Authoritative,
}

/// One commerce-state change for one variant. The engine's entire
/// contract with the outside world (Issue #8's `apply_variant_state_delta`):
/// external platforms/webhooks/beacons/reconciliation jobs produce these;
/// the engine only makes an accepted delta visible to retrieval, cheaply.
#[derive(Debug, Clone, PartialEq)]
pub struct VariantStateDelta {
    pub product_id: ProductId,
    pub variant_id: VariantId,
    pub availability: Availability,
    /// Deliberately out of scope for this first implementation (Issue #8:
    /// "Prefer the smallest primitive that proves or rejects the thesis" /
    /// "only a subset belongs in the first implementation") -- accepted
    /// and stored if present, but not yet wired into query execution.
    /// Whether inventory quantity needs its own typed mutable column
    /// (evidence suggests yes, since it's a magnitude not a flag, unlike
    /// availability) is an open question for a follow-up experiment, not
    /// answered here.
    pub inventory_units: Option<u32>,
    /// A logical clock the caller supplies (never wall-clock time read
    /// inside `commerce_core` itself, keeping this deterministic and
    /// testable) -- must be comparable/orderable across deltas for the
    /// same variant.
    pub observed_at: u64,
    /// Opaque provenance label. The engine never interprets its meaning
    /// beyond storing it for inspection -- ordering/authority is decided
    /// by `observed_at`/`authority` alone, not by which `source` string
    /// this is.
    pub source: String,
    pub authority: Authority,
}

/// What happened when a delta was applied -- so a caller (and this
/// module's own tests) can distinguish a real state change from a
/// deliberately-rejected stale/duplicate one, rather than treating every
/// call as unconditionally successful.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplyOutcome {
    /// Changed the tracked state.
    Applied,
    /// Same delta (same availability/inventory, same or covered
    /// `observed_at`) as what's already tracked -- a no-op, not an error.
    /// Duplicate/idempotent delivery (Issue #8's required correctness
    /// case) must be safe to replay.
    Idempotent,
    /// Rejected: an `Observed`-tier delta with an `observed_at` older than
    /// the currently-tracked state for this variant, and not overridden
    /// by an `Authoritative` tier. The tracked state is unchanged.
    Stale,
}

/// Per-variant bookkeeping needed to decide whether a new delta should
/// apply, replayed on process restart from whatever durable log a real
/// deployment keeps (this module defines the in-memory structure and
/// precedence rule; persistence/replay is explicitly out of this issue's
/// scope boundary -- external systems own ingestion).
#[derive(Debug, Clone, Copy, PartialEq)]
struct TrackedState {
    observed_at: u64,
    authority: Authority,
    availability: Availability,
    inventory_units: Option<u32>,
}

/// The mutable commerce-state overlay: one process-lifetime structure,
/// built once from a [`CatalogIndex`] (to learn the ordinal scheme) and
/// then mutated in place per accepted [`VariantStateDelta`] -- no
/// reindexing, no `CatalogIndex` rebuild, matching Havenask's own
/// validated in-place-mutation pattern for exactly this class of field.
#[derive(Debug)]
pub struct CommerceStateOverlay {
    /// Ordinals (the same scheme `CatalogIndex::indexed_candidates`
    /// returns) currently considered available. A bit flip, not a
    /// document rewrite -- the direct analogue of Havenask's
    /// `BitmapLeafReader::TryUpdateInOriginalBitmap` in-place bit set/reset.
    available_ordinals: RoaringBitmap,
    tracked: HashMap<VariantId, TrackedState>,
}

impl CommerceStateOverlay {
    /// Seed the overlay from `index`/`catalog`'s own initial inventory
    /// state (`Variant.inventory.status`), so a freshly-built overlay
    /// reflects the catalog snapshot's own truth before any delta is ever
    /// applied -- there is no "unknown" state for a variant the index
    /// already knows about.
    pub fn build(index: &CatalogIndex, catalog: &Catalog) -> Self {
        let mut available_ordinals = RoaringBitmap::new();
        let mut tracked = HashMap::new();
        for product in &catalog.products {
            for variant in &product.variants {
                let Some(ordinal) = index.ordinal_of(variant.id) else {
                    continue;
                };
                let availability = variant.inventory.status;
                if availability == Availability::InStock {
                    available_ordinals.insert(ordinal);
                }
                tracked.insert(
                    variant.id,
                    TrackedState {
                        observed_at: 0,
                        // The catalog snapshot's own inventory field is
                        // just ingested data as of build time, not a
                        // deliberate reconciliation event -- seeding it
                        // as `Authoritative` would permanently block any
                        // future real `Observed`-tier delta for that
                        // variant, since a lower tier can never override
                        // a higher one regardless of timestamp (see
                        // `apply`). Caught by this module's own test
                        // suite: the first version seeded `Authoritative`
                        // and every subsequent `Observed` delta in every
                        // other test was silently rejected as `Stale`.
                        authority: Authority::Observed,
                        availability,
                        inventory_units: Some(variant.inventory.available_units),
                    },
                );
            }
        }
        CommerceStateOverlay {
            available_ordinals,
            tracked,
        }
    }

    /// Apply one delta. O(1): a hash-map lookup/insert plus at most one
    /// `RoaringBitmap` bit set/reset -- no scan, no rebuild, regardless of
    /// catalog size. Returns which of [`ApplyOutcome`]'s three cases this
    /// call was, so idempotent replay and stale-event rejection are
    /// observable, not silently indistinguishable from a real change.
    pub fn apply(&mut self, index: &CatalogIndex, delta: &VariantStateDelta) -> ApplyOutcome {
        let previous = self.tracked.get(&delta.variant_id).copied();

        if let Some(prev) = previous {
            // A strictly lower authority tier can never override the
            // tracked state, regardless of its own observed_at -- once
            // authoritative truth is established, no mere observation
            // should be able to downgrade it just by claiming a later
            // timestamp. Caught by this module's own test suite: the
            // first version of this check only compared timestamps within
            // the lower-tier branch instead of rejecting it outright, so
            // a later Observed delta could still overwrite an earlier
            // Authoritative one.
            let lower_tier = delta.authority < prev.authority;
            let same_tier_and_older =
                delta.authority == prev.authority && delta.observed_at < prev.observed_at;
            if lower_tier || same_tier_and_older {
                return ApplyOutcome::Stale;
            }
            // `authority` must be part of this comparison, not just the
            // payload -- caught by this module's own adversarial
            // concurrent-convergence test (see
            // `concurrent_multi_writer_multi_reader_converges_and_never_observes_corrupted_state`)
            // and isolated by
            // `authoritative_confirmation_of_an_already_correct_observed_guess_must_still_promote_authority`:
            // an Authoritative delta that happens to confirm an already-
            // correct Observed-tier payload is a real state change (the
            // tracked *authority* tier changes, even though availability/
            // inventory do not) and must not be reported Idempotent --
            // doing so skipped updating `self.tracked`, silently leaving
            // the authority tier un-promoted, so a later Observed-tier
            // delta could still wrongly override what should have been a
            // locked-in authoritative fact.
            let unchanged = prev.authority == delta.authority
                && prev.availability == delta.availability
                && prev.inventory_units == delta.inventory_units
                && delta.observed_at <= prev.observed_at;
            if unchanged {
                return ApplyOutcome::Idempotent;
            }
        }

        if let Some(ordinal) = index.ordinal_of(delta.variant_id) {
            match delta.availability {
                Availability::InStock => {
                    self.available_ordinals.insert(ordinal);
                }
                Availability::OutOfStock | Availability::Backorder => {
                    self.available_ordinals.remove(ordinal);
                }
            }
        }
        self.tracked.insert(
            delta.variant_id,
            TrackedState {
                observed_at: delta.observed_at,
                authority: delta.authority,
                availability: delta.availability,
                inventory_units: delta.inventory_units,
            },
        );
        ApplyOutcome::Applied
    }

    /// Current tracked availability for `variant_id`, if the overlay knows
    /// about it at all (built from the catalog, or updated by a delta
    /// since).
    pub fn availability(&self, variant_id: VariantId) -> Option<Availability> {
        self.tracked.get(&variant_id).map(|t| t.availability)
    }

    pub fn inventory_units(&self, variant_id: VariantId) -> Option<u32> {
        self.tracked
            .get(&variant_id)
            .and_then(|t| t.inventory_units)
    }

    /// The live "available" ordinal set, for composing with
    /// [`CatalogIndex::indexed_candidates`] -- exposed read-only so a
    /// caller can never mutate overlay state except through [`Self::apply`].
    pub fn available_ordinals(&self) -> &RoaringBitmap {
        &self.available_ordinals
    }
}

/// `semantic candidate bitmap AND live availability bitmap`, then
/// verify/narrow-then-verify exactly like [`CatalogIndex::execute`] --
/// Issue #8's own target execution shape. Deliberately a freestanding
/// function, not a method on `CatalogIndex` or `CommerceStateOverlay`:
/// neither structure depends on the other, only this composition point
/// does, keeping the immutable index and the mutable overlay independently
/// testable and independently reasoned about.
///
/// An `availability = InStock` constraint in `query` (if present) is
/// explicitly a *hard filter request*, separate from `overlay`'s own
/// live-state gating: both must agree an ordinal is available for it to
/// survive, so a caller cannot bypass the live overlay by simply omitting
/// an availability constraint from the query -- the overlay is always
/// consulted, matching Issue #8's correctness requirement that a variant
/// query must never accidentally return an unavailable variant "because
/// another variant remains available."
pub fn execute_with_overlay(
    index: &CatalogIndex,
    overlay: &CommerceStateOverlay,
    query: &CommerceQuery,
    catalog: &Catalog,
) -> Vec<(ProductId, VariantId)> {
    let structural_candidates = index.indexed_candidates(&query.constraints);
    let candidates = structural_candidates & overlay.available_ordinals();

    let text_constraints: Vec<&Constraint> = query
        .constraints
        .iter()
        .filter_map(|c| match c {
            ResolvedConstraint::Attribute(inner @ Constraint::Text { .. }) => Some(inner),
            _ => None,
        })
        .collect();

    let mut hits = Vec::new();
    for ordinal in candidates.iter() {
        let Some(variant_id) = index.variant_id_at(ordinal) else {
            continue;
        };
        let Some((product, variant)) = index.lookup_variant(catalog, variant_id) else {
            continue;
        };
        if text_constraints.is_empty() {
            hits.push((product.id, variant.id));
            continue;
        }
        let attrs = crate::domain::effective_attributes(product, variant);
        if text_constraints.iter().all(|c| c.matches(&attrs)) {
            hits.push((product.id, variant.id));
        }
    }
    hits
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{
        attributes, AttributeValue, BrandId, CategoryId, Inventory, Price, Product, ProductId,
        ProductTypeId, Variant, VariantId,
    };
    use crate::ir::StructuralConstraint;

    /// Issue #8's headline correctness case: Product X with
    /// Black/9=OOS, Black/10=IN_STOCK, White/9=IN_STOCK. A query for
    /// color=black AND size=9 must not return Product X merely because
    /// another variant remains available.
    fn product_x() -> Catalog {
        let variant = |id: u64, color: &str, size: f64, available: bool| Variant {
            id: VariantId(id),
            attributes: attributes([
                ("color", AttributeValue::Enum(color.to_string())),
                ("size", AttributeValue::Numeric(size)),
            ]),
            price: Price::usd(1_000),
            inventory: if available {
                Inventory::in_stock(5)
            } else {
                Inventory::out_of_stock()
            },
        };
        Catalog {
            products: vec![Product {
                id: ProductId(1),
                product_type: ProductTypeId(1),
                brand: BrandId(1),
                category: CategoryId(1),
                title: "Product X".to_string(),
                attributes: attributes([]),
                variants: vec![
                    variant(101, "Black", 9.0, false),
                    variant(102, "Black", 10.0, true),
                    variant(103, "White", 9.0, true),
                ],
            }],
        }
    }

    fn query_black_size_9() -> CommerceQuery {
        CommerceQuery {
            constraints: vec![
                ResolvedConstraint::Attribute(Constraint::Enum {
                    attribute: "color".to_string(),
                    value: "Black".to_string(),
                }),
                ResolvedConstraint::Attribute(Constraint::Numeric {
                    attribute: "size".to_string(),
                    op: crate::domain::NumericOp::Eq,
                    value: 9.0,
                }),
            ],
            preferences: vec![],
            ambiguous: vec![],
            residual_lexical: vec![],
        }
    }

    #[test]
    fn variant_scoped_oos_does_not_leak_across_sibling_variants() {
        let catalog = product_x();
        let index = CatalogIndex::build(&catalog);
        let overlay = CommerceStateOverlay::build(&index, &catalog);

        let hits = execute_with_overlay(&index, &overlay, &query_black_size_9(), &catalog);
        assert!(
            hits.is_empty(),
            "Black/size 9 is OOS on this product; it must not be returned just because \
             Black/size 10 or White/size 9 remain available"
        );

        let other_hits = execute_with_overlay(
            &index,
            &overlay,
            &CommerceQuery {
                constraints: vec![
                    ResolvedConstraint::Attribute(Constraint::Enum {
                        attribute: "color".to_string(),
                        value: "Black".to_string(),
                    }),
                    ResolvedConstraint::Attribute(Constraint::Numeric {
                        attribute: "size".to_string(),
                        op: crate::domain::NumericOp::Eq,
                        value: 10.0,
                    }),
                ],
                preferences: vec![],
                ambiguous: vec![],
                residual_lexical: vec![],
            },
            &catalog,
        );
        assert_eq!(
            other_hits,
            vec![(ProductId(1), VariantId(102))],
            "the in-stock sibling variant must still be returned"
        );
    }

    #[test]
    fn oos_to_in_stock_reversal_is_immediately_visible() {
        let catalog = product_x();
        let index = CatalogIndex::build(&catalog);
        let mut overlay = CommerceStateOverlay::build(&index, &catalog);

        assert!(execute_with_overlay(&index, &overlay, &query_black_size_9(), &catalog).is_empty());

        let outcome = overlay.apply(
            &index,
            &VariantStateDelta {
                product_id: ProductId(1),
                variant_id: VariantId(101),
                availability: Availability::InStock,
                inventory_units: Some(3),
                observed_at: 1,
                source: "test-webhook".to_string(),
                authority: Authority::Observed,
            },
        );
        assert_eq!(outcome, ApplyOutcome::Applied);

        let hits = execute_with_overlay(&index, &overlay, &query_black_size_9(), &catalog);
        assert_eq!(
            hits,
            vec![(ProductId(1), VariantId(101))],
            "the reversal must be visible immediately, with no rebuild"
        );
    }

    #[test]
    fn duplicate_events_are_idempotent() {
        let catalog = product_x();
        let index = CatalogIndex::build(&catalog);
        let mut overlay = CommerceStateOverlay::build(&index, &catalog);

        let delta = VariantStateDelta {
            product_id: ProductId(1),
            variant_id: VariantId(101),
            availability: Availability::InStock,
            inventory_units: Some(3),
            observed_at: 1,
            source: "test-webhook".to_string(),
            authority: Authority::Observed,
        };
        assert_eq!(overlay.apply(&index, &delta), ApplyOutcome::Applied);
        assert_eq!(
            overlay.apply(&index, &delta),
            ApplyOutcome::Idempotent,
            "replaying the exact same delta must be a safe no-op, not an error or a double-apply"
        );
        assert_eq!(
            overlay.availability(VariantId(101)),
            Some(Availability::InStock)
        );
    }

    #[test]
    fn a_stale_event_arriving_after_newer_state_is_rejected() {
        let catalog = product_x();
        let index = CatalogIndex::build(&catalog);
        let mut overlay = CommerceStateOverlay::build(&index, &catalog);

        overlay.apply(
            &index,
            &VariantStateDelta {
                product_id: ProductId(1),
                variant_id: VariantId(101),
                availability: Availability::InStock,
                inventory_units: None,
                observed_at: 10,
                source: "webhook".to_string(),
                authority: Authority::Observed,
            },
        );
        let outcome = overlay.apply(
            &index,
            &VariantStateDelta {
                product_id: ProductId(1),
                variant_id: VariantId(101),
                availability: Availability::OutOfStock,
                inventory_units: None,
                observed_at: 5, // older than the already-applied observed_at=10
                source: "late-webhook".to_string(),
                authority: Authority::Observed,
            },
        );
        assert_eq!(outcome, ApplyOutcome::Stale);
        assert_eq!(
            overlay.availability(VariantId(101)),
            Some(Availability::InStock),
            "the stale event must not overwrite the newer tracked state"
        );
    }

    #[test]
    fn authoritative_reconciliation_overrides_an_observed_guess_regardless_of_timestamp() {
        let catalog = product_x();
        let index = CatalogIndex::build(&catalog);
        let mut overlay = CommerceStateOverlay::build(&index, &catalog);

        overlay.apply(
            &index,
            &VariantStateDelta {
                product_id: ProductId(1),
                variant_id: VariantId(101),
                availability: Availability::InStock,
                inventory_units: None,
                observed_at: 100, // a "later" but merely observed, possibly-wrong guess
                source: "beacon".to_string(),
                authority: Authority::Observed,
            },
        );
        let outcome = overlay.apply(
            &index,
            &VariantStateDelta {
                product_id: ProductId(1),
                variant_id: VariantId(101),
                availability: Availability::OutOfStock,
                inventory_units: None,
                observed_at: 50, // earlier timestamp, but authoritative
                source: "platform-reconciliation".to_string(),
                authority: Authority::Authoritative,
            },
        );
        assert_eq!(
            outcome,
            ApplyOutcome::Applied,
            "an authoritative reconciliation must correct an observed-only guess \
             even when its own observed_at is not the latest"
        );
        assert_eq!(
            overlay.availability(VariantId(101)),
            Some(Availability::OutOfStock)
        );

        // A later Observed-tier delta must NOT be able to override the
        // now-authoritative state.
        let downgraded = overlay.apply(
            &index,
            &VariantStateDelta {
                product_id: ProductId(1),
                variant_id: VariantId(101),
                availability: Availability::InStock,
                inventory_units: None,
                observed_at: 200,
                source: "beacon".to_string(),
                authority: Authority::Observed,
            },
        );
        assert_eq!(
            downgraded,
            ApplyOutcome::Stale,
            "a merely-observed signal must not override authoritative reconciliation"
        );
        assert_eq!(
            overlay.availability(VariantId(101)),
            Some(Availability::OutOfStock)
        );
    }

    #[test]
    fn product_with_all_variants_oos_returns_nothing() {
        let catalog = product_x();
        let index = CatalogIndex::build(&catalog);
        let mut overlay = CommerceStateOverlay::build(&index, &catalog);
        for (variant_id, ts) in [(VariantId(102), 1u64), (VariantId(103), 2u64)] {
            overlay.apply(
                &index,
                &VariantStateDelta {
                    product_id: ProductId(1),
                    variant_id,
                    availability: Availability::OutOfStock,
                    inventory_units: None,
                    observed_at: ts,
                    source: "test".to_string(),
                    authority: Authority::Authoritative,
                },
            );
        }
        let query = CommerceQuery {
            constraints: vec![ResolvedConstraint::Structural(StructuralConstraint::Brand(
                BrandId(1),
            ))],
            preferences: vec![],
            ambiguous: vec![],
            residual_lexical: vec![],
        };
        let hits = execute_with_overlay(&index, &overlay, &query, &catalog);
        assert!(
            hits.is_empty(),
            "every variant is OOS; the product must not appear at all"
        );
    }

    #[test]
    fn multiple_rapid_updates_converge_to_the_last_applied_state() {
        let catalog = product_x();
        let index = CatalogIndex::build(&catalog);
        let mut overlay = CommerceStateOverlay::build(&index, &catalog);
        for i in 1..=20u64 {
            let availability = if i % 2 == 0 {
                Availability::InStock
            } else {
                Availability::OutOfStock
            };
            overlay.apply(
                &index,
                &VariantStateDelta {
                    product_id: ProductId(1),
                    variant_id: VariantId(101),
                    availability,
                    inventory_units: None,
                    observed_at: i,
                    source: "burst".to_string(),
                    authority: Authority::Observed,
                },
            );
        }
        assert_eq!(
            overlay.availability(VariantId(101)),
            Some(Availability::InStock),
            "20 rapid alternating updates must converge to exactly the last one applied (i=20, even -> InStock)"
        );
    }

    /// RED evidence, found via the concurrency test below, isolated to a
    /// minimal single-threaded reproduction: an `Authoritative` delta
    /// whose payload happens to already match the currently-tracked
    /// `Observed` state (a real, plausible case -- e.g. a reconciliation
    /// job confirms what a beacon had already guessed correctly) is
    /// wrongly classified `Idempotent` by the `unchanged` fast path, which
    /// compares only availability/inventory/observed_at -- NOT authority.
    /// Because `Idempotent` returns before `self.tracked` is updated, the
    /// authority upgrade is silently dropped: `tracked.authority` stays
    /// `Observed` even though an authoritative confirmation was just
    /// accepted. A subsequent `Observed`-tier delta with a *different*
    /// payload is then wrongly accepted (compared against the never-
    /// promoted `Observed` tier instead of the true `Authoritative` one),
    /// overriding a state that should have been locked in. This single-
    /// threaded reproduction exists so the bug is provable independent of
    /// any concurrency/timing question -- it is a pure precedence-logic
    /// defect.
    #[test]
    fn authoritative_confirmation_of_an_already_correct_observed_guess_must_still_promote_authority(
    ) {
        let catalog = product_x();
        let index = CatalogIndex::build(&catalog);
        let mut overlay = CommerceStateOverlay::build(&index, &catalog);

        // Step 1: an Observed-tier guess, OutOfStock.
        overlay.apply(
            &index,
            &VariantStateDelta {
                product_id: ProductId(1),
                variant_id: VariantId(101),
                availability: Availability::OutOfStock,
                inventory_units: None,
                observed_at: 10,
                source: "beacon".to_string(),
                authority: Authority::Observed,
            },
        );
        assert_eq!(
            overlay.availability(VariantId(101)),
            Some(Availability::OutOfStock)
        );

        // Step 2: an Authoritative reconciliation that happens to CONFIRM
        // the same OutOfStock payload, with an observed_at that is not
        // newer. This must be recognized as an authority upgrade -- the
        // tracked state's authority must become Authoritative -- even
        // though the payload itself did not change.
        let confirm_outcome = overlay.apply(
            &index,
            &VariantStateDelta {
                product_id: ProductId(1),
                variant_id: VariantId(101),
                availability: Availability::OutOfStock, // same payload as tracked
                inventory_units: None,
                observed_at: 5, // older than the tracked observed_at=10
                source: "platform-reconciliation".to_string(),
                authority: Authority::Authoritative,
            },
        );
        assert_eq!(
            confirm_outcome,
            ApplyOutcome::Applied,
            "an authority upgrade is a real state change (to the tracked authority tier) even \
             when the availability payload happens to already match -- it must not be reported \
             as Idempotent, which would silently drop the upgrade"
        );

        // Step 3: a later, ordinary Observed-tier delta with a NEWER
        // observed_at, disagreeing with the now-authoritative state. This
        // must be rejected as Stale -- the authority upgrade from step 2
        // must have taken effect, not been silently lost.
        let downgrade_outcome = overlay.apply(
            &index,
            &VariantStateDelta {
                product_id: ProductId(1),
                variant_id: VariantId(101),
                availability: Availability::InStock,
                inventory_units: None,
                observed_at: 20, // newer than everything so far
                source: "beacon".to_string(),
                authority: Authority::Observed,
            },
        );
        assert_eq!(
            downgrade_outcome,
            ApplyOutcome::Stale,
            "a merely-observed signal, however recent, must not override a confirmed \
             authoritative state -- if this is Applied instead, the authority upgrade in step 2 \
             was silently dropped"
        );
        assert_eq!(
            overlay.availability(VariantId(101)),
            Some(Availability::OutOfStock),
            "the authoritatively-confirmed state must still be in effect"
        );
    }

    /// Issue #8's own named correctness case, "concurrent reads during
    /// mutation," was previously only exercised for throughput/latency
    /// (`crates/realtime-eval/src/bin/variant_state_overlay_eval.rs`), not
    /// asserted as a correctness property -- a real gap, closed here with
    /// two properties a real concurrent deployment actually needs:
    ///
    /// 1. **Convergence is order-independent.** `apply`'s precedence rule
    ///    (authority tier, then `observed_at`) is designed to be a
    ///    commutative, associative, idempotent merge -- the same shape as
    ///    a last-writer-wins CRDT register. Applying the exact same
    ///    multiset of deltas for many variants through many concurrent
    ///    writer threads racing on one `RwLock`-protected overlay, in an
    ///    unpredictable interleaving, must converge to the exact same
    ///    final state as applying that multiset sequentially in a fixed
    ///    order -- proven here against a real sequential "oracle" run, not
    ///    merely inspected.
    /// 2. **No torn/corrupted reads.** Reader threads run continuously
    ///    throughout the concurrent writer burst; every value they observe
    ///    must be a real, valid `Availability`. This is structurally
    ///    guaranteed by Rust's type system plus `RwLock`'s mutual
    ///    exclusion (a writer holds `&mut self` only while holding the
    ///    write lock, so a reader can never observe a partially-applied
    ///    delta) -- asserted explicitly here as documented, verified
    ///    behavior rather than left as an implicit assumption.
    #[test]
    fn concurrent_multi_writer_multi_reader_converges_and_never_observes_corrupted_state() {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::{Arc, RwLock};
        use std::thread;

        const N_VARIANTS: usize = 100;
        const DELTAS_PER_VARIANT: u64 = 200;

        let variant_ids: Vec<VariantId> = (0..N_VARIANTS as u64).map(VariantId).collect();
        let catalog = Catalog {
            products: variant_ids
                .iter()
                .map(|&vid| Product {
                    id: ProductId(vid.0),
                    product_type: ProductTypeId(1),
                    brand: BrandId(1),
                    category: CategoryId(1),
                    title: format!("Product {}", vid.0),
                    attributes: attributes([]),
                    variants: vec![Variant {
                        id: vid,
                        attributes: attributes([]),
                        price: Price::usd(1_000),
                        inventory: Inventory::in_stock(1),
                    }],
                })
                .collect(),
        };
        let index = CatalogIndex::build(&catalog);

        // A fixed, deterministic multiset of deltas per variant: a mix of
        // Observed and Authoritative tiers with a scrambled (non-monotonic
        // in generation order) observed_at sequence, so "highest
        // observed_at wins, but Authoritative always beats Observed" is
        // genuinely exercised, not trivially satisfied by generation
        // order.
        let mut all_deltas: Vec<VariantStateDelta> = Vec::new();
        for &vid in &variant_ids {
            for i in 0..DELTAS_PER_VARIANT {
                let observed_at = (i.wrapping_mul(37).wrapping_add(vid.0.wrapping_mul(7)))
                    % (DELTAS_PER_VARIANT * 2);
                let authority = if i % 5 == 0 {
                    Authority::Authoritative
                } else {
                    Authority::Observed
                };
                let availability = if i % 2 == 0 {
                    Availability::InStock
                } else {
                    Availability::OutOfStock
                };
                all_deltas.push(VariantStateDelta {
                    product_id: ProductId(vid.0),
                    variant_id: vid,
                    availability,
                    inventory_units: None,
                    observed_at,
                    source: "concurrency-test".to_string(),
                    authority,
                });
            }
        }

        // Oracle: apply the exact same deltas sequentially, single-
        // threaded, in generation order, and record each variant's
        // expected final availability. Because the precedence rule is
        // designed to be order-independent, this is a valid reference
        // regardless of what order the concurrent run actually applies
        // deltas in.
        let mut oracle_overlay = CommerceStateOverlay::build(&index, &catalog);
        for delta in &all_deltas {
            oracle_overlay.apply(&index, delta);
        }
        let expected: HashMap<VariantId, Availability> = variant_ids
            .iter()
            .map(|&vid| (vid, oracle_overlay.availability(vid).unwrap()))
            .collect();

        // Concurrent run: reorder the SAME deltas into a different,
        // deterministic-but-non-sequential order (so the physical
        // application order genuinely differs from the oracle's), then
        // stride 8 writer threads across that order so deltas for the same
        // variant land on different threads and race against each other in
        // real, unpredictable wall-clock interleaving.
        let mut shuffled = all_deltas.clone();
        shuffled.sort_by_key(|d| d.variant_id.0.wrapping_mul(2654435761) ^ d.observed_at);

        let shared = Arc::new(RwLock::new(CommerceStateOverlay::build(&index, &catalog)));
        let index_arc = Arc::new(index);
        let work: Arc<Vec<VariantStateDelta>> = Arc::new(shuffled);
        let stop = Arc::new(AtomicBool::new(false));

        let writer_handles: Vec<_> = (0..8)
            .map(|t: usize| {
                let shared = Arc::clone(&shared);
                let index_arc = Arc::clone(&index_arc);
                let work = Arc::clone(&work);
                thread::spawn(move || {
                    let mut i = t;
                    while i < work.len() {
                        let mut guard = shared.write().expect("writer lock");
                        guard.apply(&index_arc, &work[i]);
                        drop(guard);
                        i += 8;
                    }
                })
            })
            .collect();

        let reader_handles: Vec<_> = (0..4)
            .map(|_| {
                let shared = Arc::clone(&shared);
                let stop = Arc::clone(&stop);
                let reader_variant_ids = variant_ids.clone();
                thread::spawn(move || {
                    let mut reads = 0usize;
                    while !stop.load(Ordering::Relaxed) {
                        for &vid in &reader_variant_ids {
                            let guard = shared.read().expect("reader lock");
                            let value = guard.availability(vid);
                            drop(guard);
                            assert!(
                                matches!(
                                    value,
                                    Some(Availability::InStock)
                                        | Some(Availability::OutOfStock)
                                        | Some(Availability::Backorder)
                                ),
                                "read during concurrent mutation must always observe a real, \
                                 valid state, got {value:?}"
                            );
                            reads += 1;
                        }
                    }
                    reads
                })
            })
            .collect();

        for h in writer_handles {
            h.join().expect("writer thread panicked");
        }
        stop.store(true, Ordering::Relaxed);
        let mut total_reads = 0usize;
        for h in reader_handles {
            total_reads += h.join().expect("reader thread panicked");
        }
        assert!(
            total_reads > 0,
            "reader threads must have actually run concurrently with the writers, not merely \
             raced to completion before any read happened"
        );

        let final_overlay = Arc::try_unwrap(shared)
            .unwrap_or_else(|_| panic!("all threads joined, overlay must be uniquely owned"))
            .into_inner()
            .expect("no poisoned lock");
        for &vid in &variant_ids {
            assert_eq!(
                final_overlay.availability(vid),
                Some(expected[&vid]),
                "variant {vid:?}: concurrent multi-writer application (arbitrary interleaving) \
                 must converge to the exact same final state as sequential oracle application -- \
                 the precedence rule must be a true order-independent merge, not merely \
                 'usually right'"
            );
        }
    }
}
