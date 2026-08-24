//! Treatments C (deterministic canonicalizer + validator) and D
//! (conservative canonicalizer with abstention) --
//! `docs/experiments/ISSUE45_PROTOCOL.md` section 4 (rules R1-R11) and
//! section 5 (treatment definitions). Both treatments share this single
//! implementation (D layers one stricter admission bar on top of C's own
//! role resolution, per the protocol's own text: "R3/R4/R6/R7... are
//! unchanged between C and D"), unlike Treatment B (`e2c_majority_vote.rs`),
//! which is deliberately kept in its own module so it can never
//! accidentally share this file's evidence-aware logic.

use crate::e2b_schema::{
    Operator, PhysicalPrimitive, Scope, SemanticRole, Significance, ValueType,
};
use crate::e2b_validator::{cross_run_type_conflict, validate};
use crate::e2b_workload::UnifiedFieldStats;
use crate::e2c_schema::{
    CandidateDescriptor, CanonicalDescriptor, CanonicalOutcome, RunProvenance,
    CANONICAL_SCHEMA_VERSION,
};

/// R2's own tie-break precedence, ascending: on a plurality tie, the
/// earlier (lower-index) role in this list wins -- always the more
/// conservative one.
const ROLE_PRECEDENCE: [SemanticRole; 7] = [
    SemanticRole::Ignore,
    SemanticRole::FreeText,
    SemanticRole::Enum,
    SemanticRole::Numeric,
    SemanticRole::Boolean,
    SemanticRole::Identifier,
    SemanticRole::Relationship,
];

const SIGNIFICANCE_PRECEDENCE: [Significance; 3] = [
    Significance::Ignore,
    Significance::RankingOnly,
    Significance::RetrievalSignificant,
];

/// R3's bounded-cardinality constant: chosen to cover exactly the
/// `S/M/L/XL`-shaped and `10mm/12mm/14mm`-shaped cases
/// `docs/research/artifacts/i45_e2c_disagreement_taxonomy_run1/` found,
/// without reaching WANDS's own legitimate high-cardinality Enum fields
/// (`color` at 4,686 distinct).
const R3_BOUNDED_CARDINALITY: usize = 50;
const R3_LOW_PARSE_RATE: f64 = 0.5;
const R3_HIGH_PARSE_RATE: f64 = 0.9;

/// R1: physical primitive is a deterministic function of role, never a
/// free choice -- matching exactly how `commerce_core::index::CatalogIndex::build`
/// already derives structure from `AttributeValue` kind today.
pub fn role_to_primitive(role: SemanticRole) -> PhysicalPrimitive {
    match role {
        SemanticRole::Enum | SemanticRole::Boolean => PhysicalPrimitive::BitmapEnum,
        SemanticRole::Numeric => PhysicalPrimitive::NumericRange,
        SemanticRole::Identifier => PhysicalPrimitive::IdentifierDictionary,
        SemanticRole::FreeText => PhysicalPrimitive::LexicalPostings,
        SemanticRole::Relationship | SemanticRole::Ignore => PhysicalPrimitive::None,
    }
}

fn operators_for_primitive(prim: PhysicalPrimitive) -> Vec<Operator> {
    match prim {
        PhysicalPrimitive::BitmapEnum => vec![Operator::Eq, Operator::Contains],
        PhysicalPrimitive::NumericRange => vec![Operator::Eq, Operator::Range],
        PhysicalPrimitive::IdentifierDictionary => vec![Operator::ExactLookup],
        PhysicalPrimitive::LexicalPostings => vec![Operator::Contains],
        PhysicalPrimitive::None => vec![],
    }
}

fn value_type_for_role(role: SemanticRole) -> ValueType {
    match role {
        SemanticRole::Numeric => ValueType::Number,
        SemanticRole::Boolean => ValueType::Boolean,
        _ => ValueType::String,
    }
}

fn plurality<T: Copy + PartialEq>(values: &[T], precedence_ascending: &[T]) -> (T, usize) {
    let mut best = precedence_ascending[0];
    let mut best_count = 0usize;
    for &candidate in precedence_ascending {
        let count = values.iter().filter(|&&v| v == candidate).count();
        if count > best_count {
            best_count = count;
            best = candidate;
        }
    }
    (best, best_count)
}

/// R2: plurality vote of `semantic_role` among non-abstaining raw
/// proposals. Returns `(role, count_for_role)`.
fn plurality_role(non_abstain: &[&CandidateDescriptor]) -> (SemanticRole, usize) {
    let roles: Vec<SemanticRole> = non_abstain.iter().map(|d| d.semantic_role).collect();
    plurality(&roles, &ROLE_PRECEDENCE)
}

fn plurality_significance(non_abstain: &[&CandidateDescriptor]) -> Significance {
    let sigs: Vec<Significance> = non_abstain
        .iter()
        .map(|d| d.retrieval_significance)
        .collect();
    plurality(&sigs, &SIGNIFICANCE_PRECEDENCE).0
}

/// R6's extensibility hook for a dataset that *does* have real per-row
/// Variant identity (never exercised by WANDS/automotive as ingested;
/// exists only so the rule set is not silently unable to handle a future
/// real-Variant dataset). Not evidence-derived here beyond a plain vote,
/// since no such dataset feeds this checkpoint's own measurement.
fn plurality_scope(non_abstain: &[&CandidateDescriptor]) -> Scope {
    let scopes: Vec<Scope> = non_abstain.iter().map(|d| d.scope).collect();
    plurality(
        &scopes,
        &[Scope::Product, Scope::Variant, Scope::Relationship],
    )
    .0
}

/// Treatments C and D, shared implementation. `conservative = false` is
/// Treatment C; `conservative = true` is Treatment D (adds the majority,
/// not merely plurality, admission bar for a structural role -- see
/// `docs/experiments/ISSUE45_PROTOCOL.md` section 5).
///
/// `has_real_variant_grouping` is a dataset-structural fact (never a
/// per-field vote): `false` for WANDS/automotive as ingested by this
/// checkpoint (R6) -- both are audited to have no real per-row Variant
/// identity to measure scope against.
pub fn canonicalize(
    runs: &[(u32, CandidateDescriptor)],
    real_key: &str,
    stats: &UnifiedFieldStats,
    wands_queries: &[String],
    has_real_variant_grouping: bool,
    conservative: bool,
) -> CanonicalOutcome {
    let mut contributing_runs: Vec<u32> = runs.iter().map(|(idx, _)| *idx).collect();
    contributing_runs.sort_unstable();
    // Sorted by run_index: provenance is a bookkeeping record, not part
    // of the decision itself, and GO gate criterion 7 requires
    // canonicalization to be independent of input-vector order --
    // caught by this checkpoint's own order-independence test before it
    // was fixed here (RED before GREEN).
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

    let mut reasons = Vec::new();

    // R2: plurality vote.
    let (mut role, role_count) = plurality_role(&non_abstain);
    reasons.push(format!(
        "R2: plurality role={role:?} ({role_count}/{} non-abstaining proposals)",
        non_abstain.len()
    ));

    // R3, integrated with R9: whenever a real cross-run categorical
    // (Enum/Boolean) vs Numeric conflict exists among the raw proposals
    // -- the exact condition `cross_run_type_conflict` (E2b's own,
    // reused, never reimplemented) flags for at least one pair -- R3
    // tries to resolve it from real measured evidence first. Only when
    // R3's own two evidence conditions BOTH fail to apply (a genuinely
    // ambiguous middle zone: parse rate between the low/high bars, or
    // cardinality too high to safely call it a bounded Enum) does R9
    // fire as the defense-in-depth safety net and force an abstain --
    // R9 must run AFTER R3 gets a chance to resolve the same condition,
    // or R3 could never fire at all (an ordering bug this checkpoint's
    // own adversarial-fixture testing caught before any measurement ran).
    let any_conflict = (0..runs.len())
        .any(|i| ((i + 1)..runs.len()).any(|j| cross_run_type_conflict(&runs[i].1, &runs[j].1)));
    if any_conflict {
        let mut resolved = false;
        if stats.numeric_parseable_fraction < R3_LOW_PARSE_RATE
            && stats.distinct_values <= R3_BOUNDED_CARDINALITY
        {
            role = SemanticRole::Enum;
            resolved = true;
            reasons.push(format!(
                "R3: forced Enum (numeric_parseable_fraction={:.3} < {R3_LOW_PARSE_RATE}, distinct_values={} <= {R3_BOUNDED_CARDINALITY})",
                stats.numeric_parseable_fraction, stats.distinct_values
            ));
        } else if stats.numeric_parseable_fraction >= R3_HIGH_PARSE_RATE {
            role = SemanticRole::Numeric;
            resolved = true;
            reasons.push(format!(
                "R3: forced Numeric (numeric_parseable_fraction={:.3} >= {R3_HIGH_PARSE_RATE})",
                stats.numeric_parseable_fraction
            ));
        }
        if !resolved {
            return CanonicalOutcome::Abstain {
                real_key: real_key.to_string(),
                reason: format!(
                    "R9: unresolved cross-run categorical-vs-numeric conflict, R3 evidence inconclusive (numeric_parseable_fraction={:.3}, distinct_values={})",
                    stats.numeric_parseable_fraction, stats.distinct_values
                ),
                contributing_runs,
            };
        }
    }

    // Treatment D's own stricter bar: a structural role must be a true
    // majority (>50%) of non-abstaining raw proposals, not merely a
    // plurality, applied to whatever role R2/R3 finally resolved to.
    if conservative
        && matches!(
            role,
            SemanticRole::Enum
                | SemanticRole::Numeric
                | SemanticRole::Boolean
                | SemanticRole::Identifier
        )
    {
        let agreeing = non_abstain
            .iter()
            .filter(|d| d.semantic_role == role)
            .count();
        if (agreeing as f64) <= (non_abstain.len() as f64) / 2.0 {
            return CanonicalOutcome::Abstain {
                real_key: real_key.to_string(),
                reason: format!(
                    "D: role {role:?} has only {agreeing}/{} raw-proposal support, not a true majority -- conservative treatment demotes to abstain",
                    non_abstain.len()
                ),
                contributing_runs,
            };
        }
    }

    // R5: Identifier promotion requires the same statistical bar the
    // real production IdentifierClassifier already uses.
    if role == SemanticRole::Identifier {
        let clears_uniqueness =
            stats.uniqueness_ratio >= commerce_core::index::MIN_UNIQUENESS_RATIO;
        let clears_sample_size =
            stats.occurrences >= commerce_core::index::MIN_IDENTIFIER_SAMPLE_SIZE;
        if !clears_uniqueness || !clears_sample_size {
            reasons.push(format!(
                "R5: Identifier does not clear IdentifierClassifier's own bar (uniqueness_ratio={:.4}, occurrences={})",
                stats.uniqueness_ratio, stats.occurrences
            ));
            if stats.distinct_values <= R3_BOUNDED_CARDINALITY {
                role = SemanticRole::Enum;
                reasons.push("R5: demoted to Enum (bounded cardinality)".to_string());
            } else {
                return CanonicalOutcome::Abstain {
                    real_key: real_key.to_string(),
                    reason: format!(
                        "R5: Identifier rejected by IdentifierClassifier's own bar and cardinality ({} distinct) too high to safely demote to Enum",
                        stats.distinct_values
                    ),
                    contributing_runs,
                };
            }
        }
    }

    // R7: Relationship is demoted/abstained, never promoted -- no
    // RelationshipIndex primitive exists anywhere in commerce_core
    // (audited, docs/experiments/ISSUE45_PROTOCOL.md section 3). A hard
    // rule, not a threshold: applies regardless of vote count.
    if role == SemanticRole::Relationship {
        return CanonicalOutcome::Abstain {
            real_key: real_key.to_string(),
            reason: "R7: Relationship role/scope cannot be promoted -- no serving primitive exists in commerce_core (audited)".to_string(),
            contributing_runs,
        };
    }

    // R6: scope defaults deterministically to the dataset's own real
    // structure, never to a per-run vote, for a dataset with no real
    // per-row Variant identity to measure against.
    let scope = if has_real_variant_grouping {
        plurality_scope(&non_abstain)
    } else {
        Scope::Product
    };
    reasons.push(format!(
        "R6: scope={scope:?} (has_real_variant_grouping={has_real_variant_grouping})"
    ));

    // R1: physical primitive as a deterministic function of role.
    let mut primitive = role_to_primitive(role);
    let mut retrieval_significance = plurality_significance(&non_abstain);

    // R4: zero/near-zero variance overrides everything to
    // non-discriminating, regardless of role.
    if stats.distinct_values <= 1 {
        primitive = PhysicalPrimitive::None;
        retrieval_significance = Significance::Ignore;
        reasons.push(format!(
            "R4: zero-variance override (distinct_values={}) -> primitive=None, retrieval_significance=Ignore",
            stats.distinct_values
        ));
    }

    let value_type = value_type_for_role(role);
    let supported_operators = operators_for_primitive(primitive);

    // R10: aliases are a deduplicated union, never a single source.
    let mut aliases: Vec<String> = non_abstain
        .iter()
        .flat_map(|d| d.aliases.iter().cloned())
        .collect();
    aliases.sort();
    aliases.dedup();

    // R11: confidence is recomputed as a real agreement fraction, never
    // copied from any single raw proposal's self-reported value.
    let agreeing = non_abstain
        .iter()
        .filter(|d| d.semantic_role == role)
        .count();
    let confidence = agreeing as f64 / non_abstain.len() as f64;

    let synthetic = CandidateDescriptor {
        key: real_key.to_string(),
        real_key: Some(real_key.to_string()),
        semantic_role: role,
        value_type,
        scope,
        supported_operators: supported_operators.clone(),
        aliases: aliases.clone(),
        relationship_semantics: None,
        retrieval_significance,
        candidate_physical_primitive: primitive,
        confidence,
        evidence: reasons.join("; "),
        abstain: false,
    };

    // R8: the deterministic validator is applied to the CANONICAL
    // descriptor, not each raw one -- reusing the exact, already-governed
    // e2b_validator::validate, never a parallel validator.
    let validation = validate(&synthetic, stats, wands_queries);
    if !validation.accepted {
        return CanonicalOutcome::Abstain {
            real_key: real_key.to_string(),
            reason: format!(
                "R8: e2b_validator::validate rejected the canonical descriptor: {:?}",
                validation.findings
            ),
            contributing_runs,
        };
    }
    reasons.push(format!(
        "R8: validator accepted (findings: {:?})",
        validation.findings
    ));

    CanonicalOutcome::Promoted(CanonicalDescriptor {
        schema_version: CANONICAL_SCHEMA_VERSION,
        real_key: real_key.to_string(),
        semantic_role: role,
        value_type,
        scope,
        supported_operators,
        aliases,
        retrieval_significance,
        canonical_physical_primitive: primitive,
        confidence,
        provenance,
        decision_reasons: reasons,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::e2b_schema::{
        Operator as Op, PhysicalPrimitive as PP, Scope as Sc, Significance as Sig,
    };

    fn base(role: SemanticRole, prim: PP, conf: f64) -> CandidateDescriptor {
        CandidateDescriptor {
            key: "k".to_string(),
            real_key: None,
            semantic_role: role,
            value_type: value_type_for_role(role),
            scope: Sc::Product,
            supported_operators: vec![Op::Eq],
            aliases: vec![],
            relationship_semantics: None,
            retrieval_significance: Sig::RetrievalSignificant,
            candidate_physical_primitive: prim,
            confidence: conf,
            evidence: "test".to_string(),
            abstain: false,
        }
    }

    /// `samples` matters only when the resolved role ends up `Numeric`
    /// (`value_type::Number`) -- `e2b_validator::validate`'s own
    /// parseability check rejects a Number-typed descriptor whose real
    /// sampled values are not numeric strings, so a test whose
    /// canonicalization is expected to resolve to `Numeric` and be
    /// promoted must pass numeric-looking samples here.
    fn stats(
        distinct: usize,
        occurrences: usize,
        uniqueness: f64,
        parse_rate: f64,
    ) -> UnifiedFieldStats {
        stats_with_samples(distinct, occurrences, uniqueness, parse_rate, &["a"])
    }

    fn stats_with_samples(
        distinct: usize,
        occurrences: usize,
        uniqueness: f64,
        parse_rate: f64,
        samples: &[&str],
    ) -> UnifiedFieldStats {
        UnifiedFieldStats {
            key: "k".to_string(),
            occurrences,
            distinct_values: distinct,
            uniqueness_ratio: uniqueness,
            numeric_parseable_fraction: parse_rate,
            mean_value_length: 5.0,
            variant_scoped: None,
            sample_values: samples.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn r1_primitive_is_a_pure_function_of_role() {
        assert_eq!(role_to_primitive(SemanticRole::Enum), PP::BitmapEnum);
        assert_eq!(role_to_primitive(SemanticRole::Boolean), PP::BitmapEnum);
        assert_eq!(role_to_primitive(SemanticRole::Numeric), PP::NumericRange);
        assert_eq!(
            role_to_primitive(SemanticRole::Identifier),
            PP::IdentifierDictionary
        );
        assert_eq!(
            role_to_primitive(SemanticRole::FreeText),
            PP::LexicalPostings
        );
        assert_eq!(role_to_primitive(SemanticRole::Relationship), PP::None);
        assert_eq!(role_to_primitive(SemanticRole::Ignore), PP::None);
    }

    /// The basecolor/finish/upholsterycolor pattern: role agrees (Enum)
    /// but raw primitive flip-flops bitmap_enum/lexical_postings at
    /// identical stats -- R1 must resolve this deterministically to
    /// BitmapEnum regardless of which primitive any individual run
    /// proposed.
    #[test]
    fn primitive_selection_ambiguity_resolved_by_r1_not_by_raw_primitive_votes() {
        let runs = vec![
            (1, base(SemanticRole::Enum, PP::BitmapEnum, 0.5)),
            (2, base(SemanticRole::Enum, PP::LexicalPostings, 0.5)),
            (3, base(SemanticRole::Enum, PP::BitmapEnum, 0.5)),
            (4, base(SemanticRole::Enum, PP::LexicalPostings, 0.5)),
            (5, base(SemanticRole::Enum, PP::BitmapEnum, 0.5)),
        ];
        let s = stats(1613, 8145, 0.198, 0.0);
        let outcome = canonicalize(&runs, "basecolor", &s, &[], false, false);
        let d = outcome.promoted().expect("must promote");
        assert_eq!(d.canonical_physical_primitive, PP::BitmapEnum);
    }

    /// thread_size: 4/5 runs Enum, 1/5 Numeric, numeric_parseable_fraction=0
    /// (unit-suffixed values defeat naive parsing), distinct_values=3 --
    /// R3 forces Enum regardless of the lone Numeric vote.
    #[test]
    fn thread_size_shaped_case_resolves_to_enum_via_r3() {
        let runs = vec![
            (1, base(SemanticRole::Enum, PP::BitmapEnum, 0.68)),
            (2, base(SemanticRole::Numeric, PP::NumericRange, 0.75)),
            (3, base(SemanticRole::Enum, PP::BitmapEnum, 0.8)),
            (4, base(SemanticRole::Enum, PP::BitmapEnum, 0.8)),
            (5, base(SemanticRole::Enum, PP::BitmapEnum, 0.75)),
        ];
        let s = stats(3, 150, 0.02, 0.0);
        let outcome = canonicalize(&runs, "thread_size", &s, &[], false, false);
        let d = outcome.promoted().expect("must promote");
        assert_eq!(d.semantic_role, SemanticRole::Enum);
        assert_eq!(d.canonical_physical_primitive, PP::BitmapEnum);
    }

    /// voltage: role agrees Numeric, but distinct_values=1 (a real
    /// constant) -- R4 must force primitive=None/Ignore regardless of
    /// role, matching the real disagreement (2 runs proposed
    /// NumericRange, 3 proposed None, all five agreeing on the fact,
    /// disagreeing only on the conclusion).
    #[test]
    fn zero_variance_forces_none_primitive_via_r4() {
        let runs: Vec<_> = (1..=5)
            .map(|i| (i, base(SemanticRole::Numeric, PP::NumericRange, 0.7)))
            .collect();
        let s = stats_with_samples(1, 150, 0.0067, 1.0, &["12"]);
        let outcome = canonicalize(&runs, "voltage", &s, &[], false, false);
        let d = outcome.promoted().expect("must promote");
        assert_eq!(d.canonical_physical_primitive, PP::None);
        assert_eq!(d.retrieval_significance, Sig::Ignore);
    }

    /// color's hallucination case: a lone run proposes Relationship off a
    /// junk placeholder value -- R7 must hard-block promotion even
    /// though it is only 1 of 5 votes (belt-and-suspenders: plurality
    /// alone would already reject it, but R7 does not depend on vote
    /// count at all).
    #[test]
    fn relationship_proposal_never_promotes_even_as_plurality_winner() {
        let mut relationship_run = base(SemanticRole::Relationship, PP::None, 0.4);
        relationship_run.scope = Sc::Relationship;
        relationship_run.relationship_semantics = Some("cross-reference".to_string());
        let runs = vec![
            (1, relationship_run.clone()),
            (2, relationship_run.clone()),
            (3, relationship_run.clone()),
            (4, base(SemanticRole::Enum, PP::BitmapEnum, 0.3)),
            (5, base(SemanticRole::FreeText, PP::LexicalPostings, 0.3)),
        ];
        let s = stats(4686, 26295, 0.178, 0.0);
        let outcome = canonicalize(&runs, "color", &s, &[], false, false);
        assert!(
            !outcome.is_promoted(),
            "Relationship must never be promoted, even as a 3/5 plurality winner"
        );
    }

    /// A genuinely unresolved case: contradictory evidence (small numeric-
    /// looking sample vs. low aggregate numeric_parseable_fraction) with
    /// Identifier proposals split abstain/accept -- must abstain, never
    /// fabricate an answer (compatibledrainassemblypartnumber-shaped).
    #[test]
    fn contradictory_evidence_identifier_case_can_abstain() {
        let mut low_conf = base(SemanticRole::Identifier, PP::IdentifierDictionary, 0.15);
        low_conf.abstain = true;
        let runs = vec![
            (
                1,
                base(SemanticRole::Identifier, PP::IdentifierDictionary, 0.3),
            ),
            (2, low_conf.clone()),
            (3, low_conf.clone()),
        ];
        // uniqueness_ratio 0.48 fails the real 0.95 production bar, and
        // distinct_values=78 exceeds R3_BOUNDED_CARDINALITY -- must
        // abstain, not silently demote.
        let s = stats(78, 161, 0.48, 0.13);
        let outcome = canonicalize(
            &runs,
            "compatibledrainassemblypartnumber",
            &s,
            &[],
            false,
            false,
        );
        assert!(!outcome.is_promoted());
    }

    /// Scope defaults to Product for a dataset with no real per-row
    /// Variant identity (R6), overriding a per-run vote entirely.
    #[test]
    fn scope_defaults_to_product_when_no_real_variant_grouping() {
        let mut variant_vote = base(SemanticRole::Numeric, PP::NumericRange, 0.8);
        variant_vote.scope = Sc::Variant;
        let runs = vec![
            (1, variant_vote.clone()),
            (2, variant_vote.clone()),
            (3, base(SemanticRole::Numeric, PP::NumericRange, 0.8)),
        ];
        let s = stats_with_samples(2006, 29674, 0.0676, 1.0, &[".2", ".375", ".5"]);
        let outcome = canonicalize(&runs, "overalldepth-fronttoback", &s, &[], false, false);
        let d = outcome.promoted().expect("must promote");
        assert_eq!(d.scope, Sc::Product);
    }

    /// Treatment D: a role with only plurality (not majority) support
    /// must abstain, where Treatment C would still promote it. 5 votes:
    /// Enum wins plurality at 2/5 (Boolean/FreeText/Ignore at 1 each) --
    /// a real plurality, but not a majority.
    #[test]
    fn conservative_treatment_abstains_below_true_majority() {
        let runs = vec![
            (1, base(SemanticRole::Enum, PP::BitmapEnum, 0.5)),
            (2, base(SemanticRole::Enum, PP::BitmapEnum, 0.5)),
            (3, base(SemanticRole::Boolean, PP::BitmapEnum, 0.4)),
            (4, base(SemanticRole::FreeText, PP::LexicalPostings, 0.3)),
            (5, base(SemanticRole::Ignore, PP::None, 0.15)),
        ];
        let s = stats(200, 500, 0.4, 0.0);
        let outcome_c = canonicalize(&runs, "k", &s, &[], false, false);
        let outcome_d = canonicalize(&runs, "k", &s, &[], false, true);
        assert!(
            outcome_c.is_promoted(),
            "C: Enum is a real 2/5 plurality winner"
        );
        assert!(
            !outcome_d.is_promoted(),
            "D: 2/5 non-abstaining proposals is not a true majority"
        );
    }

    /// Canonicalization output must not depend on input-vector order
    /// (GO gate criterion 7: no hidden last-writer/majority winner).
    #[test]
    fn canonicalization_output_does_not_depend_on_run_order() {
        let runs = vec![
            (1, base(SemanticRole::Enum, PP::BitmapEnum, 0.6)),
            (2, base(SemanticRole::Enum, PP::LexicalPostings, 0.5)),
            (3, base(SemanticRole::Numeric, PP::NumericRange, 0.4)),
        ];
        let mut shuffled = runs.clone();
        shuffled.reverse();
        let s = stats(9, 10223, 0.00088, 0.0);
        let a = canonicalize(&runs, "k", &s, &[], false, false);
        let b = canonicalize(&shuffled, "k", &s, &[], false, false);
        assert_eq!(a, b, "canonicalization must be order-independent");
    }
}
