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
/// Identifier or Relationship -- the cross-item-identity conflation R3's
/// own identifier-serving-primitive work exists to prevent. Matches
/// E2b's own corrected definition exactly.
pub fn unsafe_accepted_count(
    promoted_keys_and_roles: &[(String, SemanticRole)],
    oracle_by_key: &BTreeMap<String, SemanticRole>,
) -> usize {
    promoted_keys_and_roles
        .iter()
        .filter(|(key, _)| {
            matches!(
                oracle_by_key.get(key),
                Some(SemanticRole::Identifier) | Some(SemanticRole::Relationship)
            )
        })
        .count()
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
}
