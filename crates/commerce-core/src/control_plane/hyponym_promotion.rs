//! Gate 5 promotion lifecycle for `cold_start::profile::product_type_hyponym_groups`'
//! candidate relations.
//!
//! `product_type_hyponym_groups` is a pure, syntactic candidate generator
//! (a real "RIB" in Issue #55's own terms: an unvalidated whole-word
//! superset relation, nothing more) -- but until this module existed,
//! `compile_lexicon` installed *every* candidate it produced as a hard
//! `ProductTypeAny` serving route, unconditionally. That gap is exactly
//! why `"beds" -> "cat beds"` / `"dog beds & mats"` -- a confirmed,
//! disclosed cross-family false positive
//! (`docs/decisions/ISSUE55_HYPONYM_LEAF_ONLY_DECISION.md`,
//! `docs/decisions/ISSUE55_HYPONYM_REACHABILITY_AUDIT_DECISION.md`) --
//! shipped as live default behavior with no gate at all. This module is
//! the gate: only a relation with a recorded `Promoted` verdict may ever
//! become a live `ProductTypeAny` route ("FIB" semantics); everything
//! else falls back to safe per-id `ProductType` matching, exactly the
//! severity asymmetry Issue #55 itself states ("a promotion error is
//! substantially more serious than leaving a relation unresolved").
//!
//! Deliberately reuses [`super::RuleProvenance`]/[`super::RuleStatus`]
//! from `implication.rs` rather than inventing a parallel enum -- a
//! hyponym relation's lifecycle (`Candidate` -> `Promoted`/`Withdrawn`,
//! evidenced by `Catalog`/`QueryLog`/`Behavioral`/`Model`/`Manual`
//! provenance) is the same shape as an implication rule's, just relating
//! two product-type names instead of a trigger phrase to implied facts.

use std::collections::BTreeSet;

use super::implication::{RuleProvenance, RuleStatus};

/// One candidate (broader, narrower) product-type hyponym relation and
/// its promotion lifecycle state. Names are lowercased on construction to
/// match `CatalogProfile::product_type_names`'s own key normalization.
#[derive(Debug, Clone, PartialEq)]
pub struct HyponymRelation {
    pub broader: String,
    pub narrower: String,
    pub provenance: RuleProvenance,
    pub confidence: f64,
    pub status: RuleStatus,
}

impl HyponymRelation {
    /// A new, unvalidated candidate relation (`status: Candidate`). Use
    /// [`Self::promote`]/[`Self::withdraw`] once adjudication evidence
    /// exists.
    pub fn candidate(
        broader: &str,
        narrower: &str,
        provenance: RuleProvenance,
        confidence: f64,
    ) -> Self {
        HyponymRelation {
            broader: broader.to_lowercase(),
            narrower: narrower.to_lowercase(),
            provenance,
            confidence,
            status: RuleStatus::Candidate,
        }
    }

    pub fn promote(mut self) -> Self {
        self.status = RuleStatus::Promoted;
        self
    }

    pub fn withdraw(mut self) -> Self {
        self.status = RuleStatus::Withdrawn;
        self
    }
}

/// A compiled, versioned set of **promoted relations only** -- mirrors
/// [`super::ImplicationTable::compile`]'s own structural guarantee: a
/// `Candidate` or `Withdrawn` relation can never reach [`Self::contains`].
/// `Default` is the empty set (nothing promoted), which is deliberately
/// the safe fallback `compile_lexicon` now uses: a catalog with no
/// adjudicated promotions gets plain per-id `ProductType` matching only,
/// never an unvalidated `ProductTypeAny` expansion.
#[derive(Debug, Clone, Default)]
pub struct PromotedHyponyms {
    version: u32,
    pairs: BTreeSet<(String, String)>,
}

impl PromotedHyponyms {
    /// Compiles a set of relations, silently dropping any `Candidate` or
    /// `Withdrawn` relation -- an unvalidated or retracted relation must be
    /// structurally incapable of reaching `contains`, not merely excluded
    /// by convention at each call site.
    pub fn compile(version: u32, relations: impl IntoIterator<Item = HyponymRelation>) -> Self {
        let pairs = relations
            .into_iter()
            .filter(|r| r.status == RuleStatus::Promoted)
            .map(|r| (r.broader, r.narrower))
            .collect();
        PromotedHyponyms { version, pairs }
    }

    pub fn contains(&self, broader: &str, narrower: &str) -> bool {
        self.pairs
            .contains(&(broader.to_lowercase(), narrower.to_lowercase()))
    }

    pub fn version(&self) -> u32 {
        self.version
    }

    pub fn len(&self) -> usize {
        self.pairs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.pairs.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_candidate_relation_is_never_in_the_compiled_promoted_set() {
        let relation =
            HyponymRelation::candidate("recliners", "gray recliners", RuleProvenance::Catalog, 0.9);
        let table = PromotedHyponyms::compile(1, [relation]);
        assert!(!table.contains("recliners", "gray recliners"));
        assert!(table.is_empty());
    }

    #[test]
    fn a_promoted_relation_is_reachable_via_contains() {
        let relation =
            HyponymRelation::candidate("recliners", "gray recliners", RuleProvenance::Catalog, 0.9)
                .promote();
        let table = PromotedHyponyms::compile(1, [relation]);
        assert!(table.contains("recliners", "gray recliners"));
        assert!(
            table.contains("RECLINERS", "Gray Recliners"),
            "lookup is case-insensitive, matching the lowercase-on-construction normalization"
        );
        assert_eq!(table.len(), 1);
    }

    #[test]
    fn a_withdrawn_relation_is_never_applied_even_after_having_been_promoted() {
        let relation = HyponymRelation::candidate("beds", "cat beds", RuleProvenance::Catalog, 0.9)
            .promote()
            .withdraw();
        let table = PromotedHyponyms::compile(1, [relation]);
        assert!(!table.contains("beds", "cat beds"));
    }

    #[test]
    fn the_default_promoted_set_is_empty_the_safe_fallback() {
        let table = PromotedHyponyms::default();
        assert!(table.is_empty());
        assert!(!table.contains("beds", "cat beds"));
        assert!(!table.contains("recliners", "gray recliners"));
    }

    #[test]
    fn unrelated_promoted_pairs_do_not_leak_into_an_unrelated_query() {
        let relation =
            HyponymRelation::candidate("recliners", "gray recliners", RuleProvenance::Catalog, 0.9)
                .promote();
        let table = PromotedHyponyms::compile(1, [relation]);
        assert!(!table.contains("beds", "cat beds"));
        assert!(!table.contains("recliners", "cat beds"));
        assert!(!table.contains("beds", "gray recliners"));
    }
}
