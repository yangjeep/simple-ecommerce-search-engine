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
    /// A non-entity hard constraint resolved (e.g. `compile()`'s
    /// hard-coded numeric `"size N"`/`"under"`/`"over"` keyword branches)
    /// and the query routed structurally. Distinct from
    /// `VocabularyGapDemotedToPunt`: nothing here went through the P9-E05
    /// demotion path at all -- an adversarial review found the original
    /// classifier conflated this case with the demotion case, since both
    /// leave `has_entity` false, and the fix (checking `constraints`
    /// non-emptiness before falling through to the demotion check) is
    /// this variant's own reason for existing.
    NonEntityConstraintResolved,
    /// Same non-entity hard constraint as [`Self::NonEntityConstraintResolved`],
    /// but the query still routed to `Punt` (e.g. non-selective).
    NonEntityConstraintPunted,
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

    // Deliberate priority: a query can resolve a real entity constraint
    // *and* separately carry an unrelated unresolved ambiguous span (a
    // different phrase entirely). Ambiguity disclosure takes priority
    // over reporting routing success in that case -- an adversarial
    // review flagged this ordering as worth documenting explicitly, since
    // it is easy to misread as an oversight; it is intentional, and
    // neither E2 nor E3's real runs ever produced this combination
    // (`ambiguous_span_present` was 0% in both), so it remains untested
    // against real data.
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
    // No entity, not ambiguous, but a hard constraint is still present --
    // e.g. compile()'s "size N"/"under"/"over" keyword branches, which
    // resolve directly into `constraints` and never pass through the
    // P9-E05 demotion path (`lexicon_attribute_matches`) at all. A second,
    // independent adversarial review (this project's own governance for
    // Issue #42) caught the original version of this function folding
    // this case into `VocabularyGapDemotedToPunt` below purely because
    // both share `has_entity == false` -- confirmed as a real defect by
    // rerunning `e3_mixed_category_eval` directly: its `size_schema_conflict`
    // queries route FastPath (a hard Numeric constraint, non-entity,
    // structurally routed) yet were reported under a class named
    // "demoted to Punt," contradicting the routing table two sections
    // above it in the same printout.
    if !compiled.constraints.is_empty() {
        return if structurally_routed {
            FailureClass::NonEntityConstraintResolved
        } else {
            FailureClass::NonEntityConstraintPunted
        };
    }
    // No entity, not ambiguous, no hard constraint at all: either
    // genuinely nothing resolved, or a lone attribute-only match(es) that
    // P9-E05 demoted to preferences.
    if compiled.preferences.is_empty() {
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
            FailureClass::NonEntityConstraintResolved => "non_entity_constraint_resolved",
            FailureClass::NonEntityConstraintPunted => "non_entity_constraint_punted",
            FailureClass::AmbiguousSpanPresent => "ambiguous_span_present",
            FailureClass::EntityResolvedButPunted => "entity_resolved_but_punted",
        };
        f.write_str(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use commerce_core::domain::{Constraint, NumericOp};
    use commerce_core::ir::{AmbiguousSpan, CommerceQuery, Preference};

    fn query_with(
        constraints: Vec<ResolvedConstraint>,
        preferences: usize,
        ambiguous: usize,
    ) -> CommerceQuery {
        CommerceQuery {
            constraints,
            preferences: (0..preferences)
                .map(|_| Preference::Boost {
                    attribute: "x".to_string(),
                    value: "y".to_string(),
                    weight: 1.0,
                })
                .collect(),
            ambiguous: (0..ambiguous)
                .map(|_| AmbiguousSpan {
                    text: "z".to_string(),
                    candidates: vec![],
                })
                .collect(),
            residual_lexical: vec![],
        }
    }

    fn numeric_constraint() -> ResolvedConstraint {
        ResolvedConstraint::Attribute(Constraint::Numeric {
            attribute: "size".to_string(),
            op: NumericOp::Eq,
            value: 34.0,
        })
    }

    /// Regression test for the adversarial-review-confirmed defect: a
    /// non-entity hard constraint (e.g. compile()'s "size N" keyword
    /// branch) that routes FastPath must NOT be classified as
    /// `VocabularyGapDemotedToPunt` -- nothing was demoted, and the query
    /// was not punted.
    #[test]
    fn non_entity_hard_constraint_routed_fastpath_is_not_reported_as_demoted_to_punt() {
        let compiled = query_with(vec![numeric_constraint()], 0, 0);
        let class = classify(&compiled, ExecutionOutcome::FastPath);
        assert_eq!(class, FailureClass::NonEntityConstraintResolved);
        assert_ne!(class, FailureClass::VocabularyGapDemotedToPunt);
    }

    #[test]
    fn non_entity_hard_constraint_routed_punt_is_its_own_class() {
        let compiled = query_with(vec![numeric_constraint()], 0, 0);
        let class = classify(&compiled, ExecutionOutcome::Punt);
        assert_eq!(class, FailureClass::NonEntityConstraintPunted);
    }

    #[test]
    fn a_real_demoted_preference_with_no_hard_constraint_is_vocabulary_gap() {
        let compiled = query_with(vec![], 1, 0);
        let class = classify(&compiled, ExecutionOutcome::Punt);
        assert_eq!(class, FailureClass::VocabularyGapDemotedToPunt);
    }

    #[test]
    fn nothing_resolved_at_all_is_no_structural_signal() {
        let compiled = query_with(vec![], 0, 0);
        let class = classify(&compiled, ExecutionOutcome::Punt);
        assert_eq!(class, FailureClass::NoStructuralSignalPunt);
    }
}
