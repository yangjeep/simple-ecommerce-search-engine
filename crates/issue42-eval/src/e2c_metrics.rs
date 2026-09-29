//! E2c metrics (`docs/experiments/ISSUE45_PROTOCOL.md` sections 9-10):
//! grouping raw runs by real key, the leave-one-out canonical-stability
//! design, pairwise agreement, safety (unsafe-accepted count), and
//! retrieval-significant recall.

use std::collections::BTreeMap;

use crate::e2b_pipeline::resolve_real_key;
use crate::e2b_schema::{LlmPassOutput, SemanticRole, Significance};
use crate::e2b_workload::UnifiedFieldStats;
use crate::e2c_canonicalizer::canonicalize;
use crate::e2c_majority_vote::majority_vote;
use crate::e2c_schema::{CandidateDescriptor, CanonicalOutcome};

#[derive(Debug, Clone, Copy)]
pub enum Treatment {
    B,
    C,
    D,
}

/// Groups one configuration's raw runs by real key, resolving
/// shown-key -> real-key exactly as `e2b_pipeline::build_baselines_2_and_3`
/// already does (same mapping, same function, never reimplemented).
pub fn group_by_real_key(
    config: &str,
    runs: &[LlmPassOutput],
    anon: &BTreeMap<String, String>,
    noisy: &BTreeMap<String, String>,
) -> BTreeMap<String, Vec<(u32, CandidateDescriptor)>> {
    let mut out: BTreeMap<String, Vec<(u32, CandidateDescriptor)>> = BTreeMap::new();
    for run in runs {
        for d in &run.descriptors {
            let real_key = resolve_real_key(config, &d.key, anon, noisy).to_string();
            let mut resolved = d.clone();
            resolved.real_key = Some(real_key.clone());
            out.entry(real_key)
                .or_default()
                .push((run.run_index, resolved));
        }
    }
    out
}

/// Leave-one-out canonicalizations: for N raw runs, produces N outcomes,
/// each canonicalized from all-but-one run -- the structural analogue of
/// E2b's own raw pairwise comparison, letting a canonicalizer's own
/// output stability be measured the same way (section 9).
#[allow(clippy::too_many_arguments)]
pub fn leave_one_out_outcomes(
    treatment: Treatment,
    runs_for_key: &[(u32, CandidateDescriptor)],
    real_key: &str,
    stats: Option<&UnifiedFieldStats>,
    wands_queries: &[String],
    has_real_variant_grouping: bool,
) -> Vec<CanonicalOutcome> {
    (0..runs_for_key.len())
        .map(|drop_idx| {
            let subset: Vec<(u32, CandidateDescriptor)> = runs_for_key
                .iter()
                .enumerate()
                .filter(|(i, _)| *i != drop_idx)
                .map(|(_, r)| r.clone())
                .collect();
            match treatment {
                Treatment::B => majority_vote(&subset, real_key),
                Treatment::C | Treatment::D => match stats {
                    Some(s) => canonicalize(
                        &subset,
                        real_key,
                        s,
                        wands_queries,
                        has_real_variant_grouping,
                        matches!(treatment, Treatment::D),
                    ),
                    None => CanonicalOutcome::Abstain {
                        real_key: real_key.to_string(),
                        reason: "no measured stats available for this real key".to_string(),
                        contributing_runs: subset.iter().map(|(i, _)| *i).collect(),
                    },
                },
            }
        })
        .collect()
}

#[derive(Debug, Clone, Default)]
pub struct StabilityCounts {
    pub role_agree: usize,
    pub type_agree: usize,
    pub scope_agree: usize,
    pub primitive_agree: usize,
    pub full_agree: usize,
    pub total_pairs: usize,
}

impl StabilityCounts {
    pub fn add(&mut self, other: &StabilityCounts) {
        self.role_agree += other.role_agree;
        self.type_agree += other.type_agree;
        self.scope_agree += other.scope_agree;
        self.primitive_agree += other.primitive_agree;
        self.full_agree += other.full_agree;
        self.total_pairs += other.total_pairs;
    }

    pub fn rate(numerator: usize, denominator: usize) -> f64 {
        if denominator == 0 {
            1.0
        } else {
            numerator as f64 / denominator as f64
        }
    }
}

/// Pairwise agreement over a set of outcomes (5 leave-one-out outcomes ->
/// C(5,2)=10 pairs, matching E2b's own raw pairwise design at the same
/// sample size). Two `Abstain` outcomes count as full agreement on every
/// axis (a consistent, safe result); a `Promoted` vs `Abstain` pair
/// disagrees on every axis; two `Promoted` outcomes are compared field by
/// field.
pub fn pairwise_stability(outcomes: &[CanonicalOutcome]) -> StabilityCounts {
    let mut r = StabilityCounts::default();
    for i in 0..outcomes.len() {
        for j in (i + 1)..outcomes.len() {
            r.total_pairs += 1;
            match (&outcomes[i], &outcomes[j]) {
                (CanonicalOutcome::Promoted(a), CanonicalOutcome::Promoted(b)) => {
                    if a.semantic_role == b.semantic_role {
                        r.role_agree += 1;
                    }
                    if a.value_type == b.value_type {
                        r.type_agree += 1;
                    }
                    if a.scope == b.scope {
                        r.scope_agree += 1;
                    }
                    if a.canonical_physical_primitive == b.canonical_physical_primitive {
                        r.primitive_agree += 1;
                    }
                    if a.semantic_role == b.semantic_role
                        && a.value_type == b.value_type
                        && a.scope == b.scope
                        && a.canonical_physical_primitive == b.canonical_physical_primitive
                    {
                        r.full_agree += 1;
                    }
                }
                (CanonicalOutcome::Abstain { .. }, CanonicalOutcome::Abstain { .. }) => {
                    r.role_agree += 1;
                    r.type_agree += 1;
                    r.scope_agree += 1;
                    r.primitive_agree += 1;
                    r.full_agree += 1;
                }
                _ => {}
            }
        }
    }
    r
}

/// An accepted/promoted descriptor whose oracle-confirmed real role is
/// Identifier or Relationship, but which was NOT promoted with that same
/// role -- the cross-item-identity conflation R3/R5's own identifier-
/// serving-primitive work exists to prevent (a genuine identifier or
/// relationship field silently accepted as an ordinary structural
/// attribute). A genuine identifier/relationship correctly promoted
/// *as* Identifier/Relationship (e.g. cleared through R5's own real
/// classifier gate) is the safe, intended outcome R5/R7 exist to
/// produce, not an unsafe one. Matches E2b's own corrected definition.
///
/// **Issue #47 (E2d) confirmed defect, fixed here**: the original
/// filter (`|(key, _)| ...`) discarded the promoted role entirely and
/// flagged every promoted oracle-Identifier/Relationship key regardless
/// of whether it was actually promoted with a mismatched role --
/// contradicting this function's own doc comment and this file's own
/// `unsafe_accepted_is_zero_when_promoted_role_matches_oracle_or_oracle_is_not_identity`
/// test name (which never actually exercised the Identifier/Relationship
/// branch, so it passed without verifying its own stated intent).
/// `e2c_canonicalization_eval.rs`'s own established usage pre-filters to
/// `is_structural` (Enum/Numeric/Boolean) before ever calling this
/// function, so E2c's own already-published "zero unsafe" numbers were
/// never exposed to this defect (verified by rerunning both existing
/// E2c binaries after this fix -- byte-identical output, preserved in
/// `docs/research/artifacts/i47_e2d_e2c_regression_check_run1/`). The
/// defect surfaced only in Issue #47/E2d's own broader safety scope
/// (which does not pre-filter to structural-only, deliberately, to
/// exercise Identifier/Relationship promotions too).
pub fn unsafe_accepted_count(
    promoted_keys_and_roles: &[(String, SemanticRole)],
    oracle_by_key: &BTreeMap<String, SemanticRole>,
) -> usize {
    unsafe_accepted_keys(promoted_keys_and_roles, oracle_by_key).len()
}

/// The actual keys `unsafe_accepted_count` counts, for callers that need
/// to report *which* keys were flagged (e.g. an adversarial-review
/// transparency field), not merely how many. Both this function and
/// `unsafe_accepted_count` share this one predicate -- deliberately not
/// duplicated at any call site, per this repo's own "do not
/// independently reimplement safety logic that could silently drift"
/// discipline (`e2b_pipeline.rs`'s own `resolve_real_key` reuse is the
/// precedent this follows).
pub fn unsafe_accepted_keys(
    promoted_keys_and_roles: &[(String, SemanticRole)],
    oracle_by_key: &BTreeMap<String, SemanticRole>,
) -> Vec<String> {
    promoted_keys_and_roles
        .iter()
        .filter(|(key, role)| {
            matches!(
                oracle_by_key.get(key),
                Some(SemanticRole::Identifier) | Some(SemanticRole::Relationship)
            ) && oracle_by_key.get(key) != Some(role)
        })
        .map(|(key, _)| key.clone())
        .collect()
}

/// Oracle-labeled retrieval-significant real keys that end up `Promoted`
/// -- matches E2b's own corrected recall definition (filtered by real
/// acceptance, not mere key presence).
pub fn retrieval_significant_recall(
    promoted_keys: &std::collections::BTreeSet<String>,
    oracle_all: &[crate::e2b_schema::Descriptor],
) -> f64 {
    let significant: Vec<&crate::e2b_schema::Descriptor> = oracle_all
        .iter()
        .filter(|d| d.retrieval_significance == Significance::RetrievalSignificant)
        .collect();
    if significant.is_empty() {
        return 1.0;
    }
    let recovered = significant
        .iter()
        .filter(|d| promoted_keys.contains(&d.key))
        .count();
    recovered as f64 / significant.len() as f64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::e2b_schema::{Operator, PhysicalPrimitive, Scope, Significance as Sig, ValueType};
    use crate::e2c_schema::CanonicalDescriptor;

    fn promoted(role: SemanticRole, prim: PhysicalPrimitive) -> CanonicalOutcome {
        CanonicalOutcome::Promoted(CanonicalDescriptor {
            schema_version: 1,
            real_key: "k".to_string(),
            semantic_role: role,
            value_type: ValueType::String,
            scope: Scope::Product,
            supported_operators: vec![Operator::Eq],
            aliases: vec![],
            retrieval_significance: Sig::RetrievalSignificant,
            canonical_physical_primitive: prim,
            confidence: 1.0,
            provenance: vec![],
            decision_reasons: vec![],
        })
    }

    #[test]
    fn identical_promoted_outcomes_agree_on_every_axis() {
        let outcomes = vec![
            promoted(SemanticRole::Enum, PhysicalPrimitive::BitmapEnum),
            promoted(SemanticRole::Enum, PhysicalPrimitive::BitmapEnum),
            promoted(SemanticRole::Enum, PhysicalPrimitive::BitmapEnum),
        ];
        let r = pairwise_stability(&outcomes);
        assert_eq!(r.total_pairs, 3);
        assert_eq!(r.role_agree, 3);
        assert_eq!(r.full_agree, 3);
    }

    #[test]
    fn abstain_pairs_count_as_full_agreement() {
        let outcomes = vec![
            CanonicalOutcome::Abstain {
                real_key: "k".to_string(),
                reason: "x".to_string(),
                contributing_runs: vec![1, 2],
            },
            CanonicalOutcome::Abstain {
                real_key: "k".to_string(),
                reason: "y".to_string(),
                contributing_runs: vec![1, 3],
            },
        ];
        let r = pairwise_stability(&outcomes);
        assert_eq!(r.full_agree, 1);
        assert_eq!(r.total_pairs, 1);
    }

    #[test]
    fn promoted_vs_abstain_disagrees_on_every_axis() {
        let outcomes = vec![
            promoted(SemanticRole::Enum, PhysicalPrimitive::BitmapEnum),
            CanonicalOutcome::Abstain {
                real_key: "k".to_string(),
                reason: "x".to_string(),
                contributing_runs: vec![1],
            },
        ];
        let r = pairwise_stability(&outcomes);
        assert_eq!(r.full_agree, 0);
        assert_eq!(r.role_agree, 0);
    }

    #[test]
    fn unsafe_accepted_flags_oracle_identifier_promoted_as_something_else() {
        let promoted = vec![("samplepartnumber".to_string(), SemanticRole::Enum)];
        let mut oracle = BTreeMap::new();
        oracle.insert("samplepartnumber".to_string(), SemanticRole::Identifier);
        assert_eq!(unsafe_accepted_count(&promoted, &oracle), 1);
    }

    #[test]
    fn unsafe_accepted_is_zero_when_promoted_role_matches_oracle_or_oracle_is_not_identity() {
        let promoted = vec![("color".to_string(), SemanticRole::Enum)];
        let mut oracle = BTreeMap::new();
        oracle.insert("color".to_string(), SemanticRole::Enum);
        assert_eq!(unsafe_accepted_count(&promoted, &oracle), 0);
    }

    /// **Issue #47 (E2d) confirmed defect, found while building the
    /// adaptive controller's own safety accounting**: this test's own
    /// sibling above (`unsafe_accepted_is_zero_when_promoted_role_matches_oracle_or_oracle_is_not_identity`)
    /// is named to assert "zero when promoted role matches oracle,"
    /// exactly matching this function's own doc comment ("an
    /// accepted/promoted descriptor whose oracle-confirmed real role is
    /// Identifier/Relationship" being the unsafe case -- i.e. a genuine
    /// identifier/relationship field silently promoted as something
    /// else, the "cross-item-identity conflation R3's own identifier-
    /// serving-primitive work exists to PREVENT"). But that sibling
    /// test's own example ("color", oracle=Enum) never actually
    /// exercises the Identifier/Relationship branch at all -- it passes
    /// by coincidence, not by verifying the role-match half of its own
    /// name. The real implementation's filter closure
    /// (`|(key, _)| ...`) discards the promoted role entirely and only
    /// checks whether *oracle* says Identifier/Relationship, flagging
    /// EVERY promoted oracle-Identifier/Relationship key as "unsafe" --
    /// including one correctly, safely promoted AS Identifier through
    /// R5's own real classifier gate, which is exactly the SAFE outcome
    /// R5 exists to produce (`ISSUE45_DECISION.md`'s own "R5 blocks
    /// compatibledrainassemblypartnumber's Identifier claim... regardless
    /// of vote count" names blocking/demotion as the failure R5 prevents;
    /// a real identifier that correctly clears R5's bar and gets promoted
    /// as `Identifier` is the intended, safe result, not an unsafe one).
    /// This reproduces that exact scenario: a genuine identifier
    /// (`part_number`-shaped, oracle role Identifier) correctly promoted
    /// with the SAME role -- must not be flagged unsafe, but the
    /// pre-fix implementation flags it anyway because it never compares
    /// the promoted role to oracle at all.
    #[test]
    fn unsafe_accepted_does_not_flag_an_identifier_correctly_promoted_as_identifier() {
        let promoted = vec![("part_number".to_string(), SemanticRole::Identifier)];
        let mut oracle = BTreeMap::new();
        oracle.insert("part_number".to_string(), SemanticRole::Identifier);
        assert_eq!(
            unsafe_accepted_count(&promoted, &oracle),
            0,
            "a genuine identifier correctly promoted as Identifier (matching oracle exactly, \
             via R5's real classifier gate) is the safe, intended outcome, not an unsafe one"
        );
    }

    /// The true positive this function exists to catch, preserved
    /// alongside the fix: a genuine identifier/relationship silently
    /// promoted as an ordinary structural role IS unsafe.
    #[test]
    fn unsafe_accepted_still_flags_an_identifier_promoted_as_something_else() {
        let promoted = vec![("part_number".to_string(), SemanticRole::Enum)];
        let mut oracle = BTreeMap::new();
        oracle.insert("part_number".to_string(), SemanticRole::Identifier);
        assert_eq!(unsafe_accepted_count(&promoted, &oracle), 1);
    }

    /// **Adversarial-review-requested coverage gap, closed**: the fix's
    /// own regression tests only exercised Identifier<->Enum mismatches;
    /// the Identifier<->Relationship cross-conflation the fix's own doc
    /// comment claims to catch (both are "Identifier or Relationship" in
    /// the `matches!` guard) was never independently verified in either
    /// direction. Both directions checked here.
    #[test]
    fn unsafe_accepted_flags_relationship_identifier_cross_conflation_both_directions() {
        let mut oracle = BTreeMap::new();
        oracle.insert("compatible_part".to_string(), SemanticRole::Relationship);
        let promoted_as_identifier =
            vec![("compatible_part".to_string(), SemanticRole::Identifier)];
        assert_eq!(
            unsafe_accepted_count(&promoted_as_identifier, &oracle),
            1,
            "a genuine Relationship field promoted as Identifier is still a cross-item-identity \
             conflation risk, not a safe substitution"
        );

        let mut oracle2 = BTreeMap::new();
        oracle2.insert("part_number".to_string(), SemanticRole::Identifier);
        let promoted_as_relationship =
            vec![("part_number".to_string(), SemanticRole::Relationship)];
        assert_eq!(
            unsafe_accepted_count(&promoted_as_relationship, &oracle2),
            1,
            "a genuine Identifier field promoted as Relationship is also flagged, not silently \
             treated as an acceptable substitution between the two protected roles"
        );
    }

    #[test]
    fn unsafe_accepted_keys_returns_exactly_the_keys_the_count_counts() {
        let promoted = vec![
            ("part_number".to_string(), SemanticRole::Enum),
            ("color".to_string(), SemanticRole::Enum),
        ];
        let mut oracle = BTreeMap::new();
        oracle.insert("part_number".to_string(), SemanticRole::Identifier);
        oracle.insert("color".to_string(), SemanticRole::Enum);
        let keys = unsafe_accepted_keys(&promoted, &oracle);
        assert_eq!(keys, vec!["part_number".to_string()]);
        assert_eq!(keys.len(), unsafe_accepted_count(&promoted, &oracle));
    }
}
