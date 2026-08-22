//! A shared, generator-agnostic classification of *why* a compiled query
//! ended up the way it did, used by both the E2 and E3 experiment
//! binaries so their failure-taxonomy tables use the same categories.

use commerce_core::ir::{CommerceQuery, ResolvedConstraint, StructuralConstraint};
use commerce_core::plan::ExecutionOutcome;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum FailureClass {
    /// A real entity constraint (`Brand`/`BrandAny`/`ProductType`/`Category`)
    /// was resolved and the query routed structurally (`FastPath`/`Hybrid`).
    EntityResolvedStructural,
    /// No entity, no attribute constraint either (everything ambiguous or
    /// residual) -- `Punt`, and there was nothing to demote in the first
    /// place. Not a failure: this is the honest "no structural signal
    /// exists" case.
    NoStructuralSignalPunt,
    /// The query's *only* candidate constraints were lexicon-derived
    /// attribute matches with no corroborating entity -- the P9-E05
    /// demotion rule fired, correctly avoiding a coincidental wrong hard
    /// filter, at the cost of losing structural admission for this query.
    VocabularyGapDemotedToPunt,
    /// At least one span had more than one candidate reading.
    AmbiguousSpanPresent,
    /// An entity constraint was resolved but the query still routed to
    /// `Punt` (should not normally happen given `plan()`'s own contract --
    /// flagged separately if ever observed).
    EntityResolvedButPunted,
}

pub fn classify(compiled: &CommerceQuery, outcome: ExecutionOutcome) -> FailureClass {
    let has_entity = compiled.constraints.iter().any(|c| {
        matches!(
            c,
            ResolvedConstraint::Structural(
                StructuralConstraint::Brand(_)
                    | StructuralConstraint::BrandAny(_)
                    | StructuralConstraint::ProductType(_)
                    | StructuralConstraint::Category(_)
            )
        )
    });
    let structurally_routed = matches!(
        outcome,
        ExecutionOutcome::FastPath | ExecutionOutcome::Hybrid
    );

    if !compiled.ambiguous.is_empty() {
        return FailureClass::AmbiguousSpanPresent;
    }
    if has_entity {
        return if structurally_routed {
            FailureClass::EntityResolvedStructural
        } else {
            FailureClass::EntityResolvedButPunted
        };
    }
    // No entity, not ambiguous: either genuinely nothing resolved, or a
    // lone attribute-only match(es) that P9-E05 demoted to preferences.
    if compiled.preferences.is_empty() && compiled.constraints.is_empty() {
        FailureClass::NoStructuralSignalPunt
    } else {
        FailureClass::VocabularyGapDemotedToPunt
    }
}

impl std::fmt::Display for FailureClass {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            FailureClass::EntityResolvedStructural => "entity_resolved_structural",
            FailureClass::NoStructuralSignalPunt => "no_structural_signal_punt",
            FailureClass::VocabularyGapDemotedToPunt => "vocabulary_gap_demoted_to_punt",
            FailureClass::AmbiguousSpanPresent => "ambiguous_span_present",
            FailureClass::EntityResolvedButPunted => "entity_resolved_but_punted",
        };
        f.write_str(s)
    }
}
