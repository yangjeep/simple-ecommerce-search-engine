//! Treatment B (`docs/experiments/ISSUE45_PROTOCOL.md` section 5): a
//! deliberately naive multi-run consensus baseline. Every field is voted
//! on directly from the raw proposals' own stated values -- no catalog
//! evidence, no validator, no engine-capability audit (this treatment
//! does not know `RelationshipIndex` doesn't exist in `commerce_core`;
//! if a plurality of raw runs propose `Relationship`, this treatment
//! promotes it, an intentional, disclosed safety gap this treatment
//! exists to expose).
//!
//! Kept in its own module, structurally separate from
//! `e2c_canonicalizer.rs`, so it can never accidentally share R1/R6/R7's
//! evidence-aware logic -- a real risk that would quietly make this
//! treatment not naive anymore, defeating the reason it exists (Issue
//! #45: "test whether a real canonicalizer beats naive voting").

use crate::e2b_schema::{
    Operator, PhysicalPrimitive, Scope, SemanticRole, Significance, ValueType,
};
use crate::e2c_schema::{
    CandidateDescriptor, CanonicalDescriptor, CanonicalOutcome, RunProvenance,
    CANONICAL_SCHEMA_VERSION,
};

/// Ties broken by first-run-order -- a genuinely arbitrary, non-evidence-
/// aware tiebreak, disclosed as such (the protocol's own section 5).
fn plurality_first_wins<T: Copy + PartialEq>(values: &[T]) -> T {
    let mut best_count = 0usize;
    let mut best = values[0];
    for &candidate in values {
        let count = values.iter().filter(|&&v| v == candidate).count();
        if count > best_count {
            best_count = count;
            best = candidate;
        }
    }
    best
}

pub fn majority_vote(runs: &[(u32, CandidateDescriptor)], real_key: &str) -> CanonicalOutcome {
    let mut contributing_runs: Vec<u32> = runs.iter().map(|(idx, _)| *idx).collect();
    contributing_runs.sort_unstable();
    let non_abstain: Vec<&CandidateDescriptor> = runs
        .iter()
        .filter(|(_, d)| !d.abstain)
        .map(|(_, d)| d)
        .collect();

    if non_abstain.is_empty() {
        return CanonicalOutcome::Abstain {
            real_key: real_key.to_string(),
            reason: "every raw proposal abstained".to_string(),
            contributing_runs,
        };
    }

    let roles: Vec<SemanticRole> = non_abstain.iter().map(|d| d.semantic_role).collect();
    let role = plurality_first_wins(&roles);

    let value_types: Vec<ValueType> = non_abstain.iter().map(|d| d.value_type).collect();
    let value_type = plurality_first_wins(&value_types);

    let scopes: Vec<Scope> = non_abstain.iter().map(|d| d.scope).collect();
    let scope = plurality_first_wins(&scopes);

    // Voted DIRECTLY, unlike Treatment C/D's R1 -- this is the whole
    // point of Treatment B: primitive is not forced to track role.
    let primitives: Vec<PhysicalPrimitive> = non_abstain
        .iter()
        .map(|d| d.candidate_physical_primitive)
        .collect();
    let primitive = plurality_first_wins(&primitives);

    let sigs: Vec<Significance> = non_abstain
        .iter()
        .map(|d| d.retrieval_significance)
        .collect();
    let retrieval_significance = plurality_first_wins(&sigs);

    let mut aliases: Vec<String> = non_abstain
        .iter()
        .flat_map(|d| d.aliases.iter().cloned())
        .collect();
    aliases.sort();
    aliases.dedup();

    let mut operators: Vec<Operator> = non_abstain
        .iter()
        .flat_map(|d| d.supported_operators.iter().copied())
        .collect();
    operators.sort_by_key(|o| *o as u8);
    operators.dedup();

    let agreeing = non_abstain
        .iter()
        .filter(|d| d.semantic_role == role)
        .count();
    let confidence = agreeing as f64 / non_abstain.len() as f64;

    // Sorted by run_index for readability/reproducibility -- Treatment
    // B's own DECISION is legitimately order-sensitive on a genuine tie
    // (disclosed first-run-order tiebreak), unlike C/D; only this
    // bookkeeping list is normalized.
    let mut provenance: Vec<RunProvenance> = runs
        .iter()
        .map(|(idx, d)| RunProvenance {
            run_index: *idx,
            semantic_role: d.semantic_role,
            candidate_physical_primitive: d.candidate_physical_primitive,
            confidence: d.confidence,
            abstained: d.abstain,
        })
        .collect();
    provenance.sort_by_key(|p| p.run_index);

    CanonicalOutcome::Promoted(CanonicalDescriptor {
        schema_version: CANONICAL_SCHEMA_VERSION,
        real_key: real_key.to_string(),
        semantic_role: role,
        value_type,
        scope,
        supported_operators: operators,
        aliases,
        retrieval_significance,
        canonical_physical_primitive: primitive,
        confidence,
        provenance,
        decision_reasons: vec![
            "Treatment B: naive per-field plurality vote, first-run-order tiebreak, no catalog evidence, no validator, no engine-capability audit".to_string(),
        ],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::e2b_schema::{
        Operator as Op, PhysicalPrimitive as PP, Scope as Sc, Significance as Sig, ValueType as VT,
    };

    fn base(role: SemanticRole, prim: PP) -> CandidateDescriptor {
        CandidateDescriptor {
            key: "k".to_string(),
            real_key: None,
            semantic_role: role,
            value_type: VT::String,
            scope: Sc::Product,
            supported_operators: vec![Op::Eq],
            aliases: vec![],
            relationship_semantics: None,
            retrieval_significance: Sig::RetrievalSignificant,
            candidate_physical_primitive: prim,
            confidence: 0.5,
            evidence: "test".to_string(),
            abstain: false,
        }
    }

    /// Treatment B's own disclosed unsafe-promotion risk: a plurality of
    /// raw runs proposing Relationship gets promoted, unlike Treatments
    /// C/D (R7 hard-blocks this regardless of vote count).
    #[test]
    fn majority_vote_can_promote_relationship_unlike_canonicalizer() {
        let mut relationship = base(SemanticRole::Relationship, PP::None);
        relationship.scope = Sc::Relationship;
        let runs = vec![
            (1, relationship.clone()),
            (2, relationship.clone()),
            (3, relationship.clone()),
            (4, base(SemanticRole::Enum, PP::BitmapEnum)),
            (5, base(SemanticRole::FreeText, PP::LexicalPostings)),
        ];
        let outcome = majority_vote(&runs, "color");
        let d = outcome
            .promoted()
            .expect("Treatment B promotes the plurality winner unconditionally");
        assert_eq!(d.semantic_role, SemanticRole::Relationship);
    }

    /// Primitive is voted directly, not derived from role -- the
    /// opposite of R1.
    #[test]
    fn majority_vote_does_not_force_primitive_to_track_role() {
        let runs = vec![
            (1, base(SemanticRole::Enum, PP::LexicalPostings)),
            (2, base(SemanticRole::Enum, PP::LexicalPostings)),
            (3, base(SemanticRole::Enum, PP::BitmapEnum)),
        ];
        let outcome = majority_vote(&runs, "k");
        let d = outcome.promoted().unwrap();
        assert_eq!(d.semantic_role, SemanticRole::Enum);
        assert_eq!(
            d.canonical_physical_primitive,
            PP::LexicalPostings,
            "B votes primitive directly (2/3 LexicalPostings), unlike R1's role->primitive function"
        );
    }
}
