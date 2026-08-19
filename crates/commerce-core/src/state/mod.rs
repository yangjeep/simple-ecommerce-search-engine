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
            let unchanged = prev.availability == delta.availability
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
}
