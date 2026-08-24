//! Hand-authored, deterministic adversarial fixtures --
//! `docs/experiments/ISSUE45_PROTOCOL.md` section 7. Not LLM-sourced:
//! each fixture's expected safe outcome is asserted directly, matching
//! CLAUDE.md's "add a failing test/benchmark first where practical."
//! Several of the required cases (Enum-vs-Numeric ambiguity, a
//! relationship-like field that must not collapse into an ordinary
//! attribute, semantically equivalent proposals choosing different but
//! operationally equivalent primitives, a genuinely unresolved case,
//! order-independence) are already covered directly in
//! `e2c_canonicalizer.rs`'s own test module (co-located with the rules
//! they exercise); this module covers the remaining required cases.

#[cfg(test)]
mod tests {
    use crate::e2b_schema::{
        Operator as Op, PhysicalPrimitive as PP, Scope as Sc, SemanticRole as SR,
        Significance as Sig, ValueType as VT,
    };
    use crate::e2b_workload::UnifiedFieldStats;
    use crate::e2c_canonicalizer::canonicalize;
    use crate::e2c_majority_vote::majority_vote;
    use crate::e2c_schema::CandidateDescriptor;

    fn descriptor(role: SR, prim: PP, aliases: &[&str], evidence: &str) -> CandidateDescriptor {
        CandidateDescriptor {
            key: "k".to_string(),
            real_key: None,
            semantic_role: role,
            value_type: match role {
                SR::Numeric => VT::Number,
                SR::Boolean => VT::Boolean,
                _ => VT::String,
            },
            scope: Sc::Product,
            supported_operators: vec![Op::Eq],
            aliases: aliases.iter().map(|s| s.to_string()).collect(),
            relationship_semantics: None,
            retrieval_significance: Sig::RetrievalSignificant,
            candidate_physical_primitive: prim,
            confidence: 0.6,
            evidence: evidence.to_string(),
            abstain: false,
        }
    }

    fn stats(
        distinct: usize,
        occurrences: usize,
        uniqueness: f64,
        parse_rate: f64,
    ) -> UnifiedFieldStats {
        UnifiedFieldStats {
            key: "k".to_string(),
            occurrences,
            distinct_values: distinct,
            uniqueness_ratio: uniqueness,
            numeric_parseable_fraction: parse_rate,
            mean_value_length: 5.0,
            variant_scoped: None,
            sample_values: vec!["a".to_string()],
        }
    }

    /// Case 1: same concept, wildly different LLM labels/wording and
    /// evidence phrasing -- consistent role/stats. Canonicalization must
    /// still converge to one stable answer and R10 must merge every
    /// run's aliases into a superset, never discard one run's real query
    /// phrases just because another run phrased its own evidence
    /// differently.
    #[test]
    fn synonym_wording_does_not_prevent_convergence_and_aliases_are_unioned() {
        let runs = vec![
            (
                1,
                descriptor(SR::Enum, PP::BitmapEnum, &["blue"], "Color / Enum / Bitmap"),
            ),
            (
                2,
                descriptor(
                    SR::Enum,
                    PP::BitmapEnum,
                    &["cerulean"],
                    "ProductColor, a bounded categorical swatch",
                ),
            ),
            (
                3,
                descriptor(
                    SR::Enum,
                    PP::BitmapEnum,
                    &["navy"],
                    "AppearanceAttribute(color): discrete finish vocabulary",
                ),
            ),
        ];
        let s = stats(50, 1000, 0.05, 0.0);
        let outcome = canonicalize(&runs, "color", &s, &[], false, false);
        let d = outcome
            .promoted()
            .expect("consistent role/stats must promote");
        assert_eq!(d.semantic_role, SR::Enum);
        let mut aliases = d.aliases.clone();
        aliases.sort();
        assert_eq!(
            aliases,
            vec![
                "blue".to_string(),
                "cerulean".to_string(),
                "navy".to_string()
            ]
        );
    }

    /// Case 2: same real key name canonicalized independently in two
    /// different (config, stats) contexts must never cross-contaminate
    /// -- each call is a pure function of its own arguments.
    #[test]
    fn same_key_name_different_real_type_across_contexts_does_not_cross_contaminate() {
        let apparel_runs = vec![
            (
                1,
                descriptor(
                    SR::Enum,
                    PP::BitmapEnum,
                    &["size 8"],
                    "apparel size, bounded enum",
                ),
            ),
            (
                2,
                descriptor(
                    SR::Enum,
                    PP::BitmapEnum,
                    &["size 10"],
                    "apparel size, bounded enum",
                ),
            ),
        ];
        let apparel_stats = stats(12, 5000, 0.0024, 0.1);
        let apparel_outcome =
            canonicalize(&apparel_runs, "size", &apparel_stats, &[], false, false);

        let automotive_runs = vec![
            (
                1,
                descriptor(
                    SR::Numeric,
                    PP::NumericRange,
                    &["size 34"],
                    "bolt diameter in mm",
                ),
            ),
            (
                2,
                descriptor(
                    SR::Numeric,
                    PP::NumericRange,
                    &["size 34"],
                    "bolt diameter in mm",
                ),
            ),
        ];
        let automotive_stats = UnifiedFieldStats {
            sample_values: vec!["34".to_string(), "36".to_string()],
            ..stats(400, 5000, 0.08, 0.99)
        };
        let automotive_outcome = canonicalize(
            &automotive_runs,
            "size",
            &automotive_stats,
            &[],
            false,
            false,
        );

        assert_eq!(apparel_outcome.promoted().unwrap().semantic_role, SR::Enum);
        assert_eq!(
            automotive_outcome.promoted().unwrap().semantic_role,
            SR::Numeric
        );
    }

    /// Case 4: Identifier-vs-high-cardinality-Enum ambiguity. Real stats
    /// fail the production IdentifierClassifier bar (uniqueness_ratio
    /// well under 0.95) AND cardinality is too high to safely demote to
    /// Enum (> R3's bounded-cardinality constant) -- must abstain, never
    /// silently promote as either.
    #[test]
    fn identifier_vs_high_cardinality_enum_ambiguity_abstains_when_neither_is_safe() {
        let runs = vec![
            (
                1,
                descriptor(
                    SR::Identifier,
                    PP::IdentifierDictionary,
                    &[],
                    "looks unique",
                ),
            ),
            (
                2,
                descriptor(
                    SR::Identifier,
                    PP::IdentifierDictionary,
                    &[],
                    "looks unique",
                ),
            ),
        ];
        // uniqueness_ratio 0.60 fails the 0.95 production bar;
        // distinct_values 900 exceeds the 50-item bounded-cardinality
        // ceiling R5 uses to decide a safe Enum demotion.
        let s = stats(900, 1500, 0.60, 0.02);
        let outcome = canonicalize(&runs, "sku_like_field", &s, &[], false, false);
        assert!(
            !outcome.is_promoted(),
            "neither Identifier nor a safe Enum demotion applies -- must abstain"
        );
    }

    /// Case 5: R6's own extensibility hook -- a dataset that DOES have
    /// real per-row Variant identity (has_real_variant_grouping=true)
    /// falls back to a real evidence-based scope vote instead of the
    /// Product-only WANDS/automotive default. This proves R6's default
    /// is a disclosed dataset-structural choice, not a hardcoded
    /// inability to represent Variant scope at all.
    #[test]
    fn scope_ambiguity_with_real_variant_grouping_uses_a_real_vote_not_the_product_default() {
        let mut variant_vote = descriptor(SR::Enum, PP::BitmapEnum, &[], "varies per SKU");
        variant_vote.scope = Sc::Variant;
        let runs = vec![
            (1, variant_vote.clone()),
            (2, variant_vote.clone()),
            (
                3,
                descriptor(SR::Enum, PP::BitmapEnum, &[], "varies per SKU"),
            ),
        ];
        let s = stats(6, 900, 0.007, 0.0);
        let outcome = canonicalize(&runs, "fabric_swatch", &s, &[], true, false);
        let d = outcome.promoted().unwrap();
        assert_eq!(
            d.scope,
            Sc::Variant,
            "with has_real_variant_grouping=true, scope is a real 2/3 vote, not forced to Product"
        );
    }

    /// Case 6: a sparse (low occurrence/density) but retrieval-significant
    /// attribute must not be rejected merely for being sparse -- the
    /// canonicalizer has no density-based rejection rule, only the
    /// checks R1-R11 actually specify.
    #[test]
    fn sparse_but_retrieval_significant_attribute_still_promotes() {
        let runs = vec![
            (
                1,
                descriptor(
                    SR::Enum,
                    PP::BitmapEnum,
                    &["organic cotton"],
                    "rare but real",
                ),
            ),
            (
                2,
                descriptor(
                    SR::Enum,
                    PP::BitmapEnum,
                    &["organic cotton"],
                    "rare but real",
                ),
            ),
        ];
        // Only 12 occurrences across a large catalog -- sparse, still a
        // clean bounded enum by shape.
        let s = stats(2, 12, 0.167, 0.0);
        let outcome = canonicalize(&runs, "organic", &s, &[], false, false);
        assert!(
            outcome.is_promoted(),
            "sparsity alone is not a rejection reason"
        );
    }

    /// Case 7: a misleading field name (e.g. an anonymized/noisy shown
    /// key) must not change the outcome -- canonicalization never
    /// inspects the shown key name or evidence text, only the real
    /// measured stats and the votes themselves.
    #[test]
    fn misleading_field_name_does_not_change_the_outcome() {
        let honestly_named_runs = vec![
            (
                1,
                descriptor(SR::Numeric, PP::NumericRange, &[], "product weight"),
            ),
            (
                2,
                descriptor(SR::Numeric, PP::NumericRange, &[], "product weight"),
            ),
        ];
        let mislead_runs = vec![
            (
                1,
                descriptor(
                    SR::Numeric,
                    PP::NumericRange,
                    &[],
                    "definitely not weight, trust me",
                ),
            ),
            (
                2,
                descriptor(
                    SR::Numeric,
                    PP::NumericRange,
                    &[],
                    "definitely not weight, trust me",
                ),
            ),
        ];
        let s = stats(500, 5000, 0.1, 0.999);
        let stats_samples = UnifiedFieldStats {
            sample_values: vec!["1.5".to_string()],
            ..s
        };
        let a = canonicalize(
            &honestly_named_runs,
            "overallproductweight",
            &stats_samples,
            &[],
            false,
            false,
        );
        let b = canonicalize(
            &mislead_runs,
            "overallproductweight",
            &stats_samples,
            &[],
            false,
            false,
        );
        assert_eq!(
            a.promoted()
                .map(|d| (d.semantic_role, d.canonical_physical_primitive)),
            b.promoted()
                .map(|d| (d.semantic_role, d.canonical_physical_primitive)),
            "evidence text/naming never drives the canonical decision"
        );
    }

    /// Case 8: units/quantities with inconsistent formatting
    /// (warrantylength-shaped real case: mixed short/long string values,
    /// near-zero numeric_parseable_fraction despite a duration-implying
    /// name, no run actually proposes Numeric for it in the real raw
    /// data -- the boundary contested is Enum vs FreeText, resolved by
    /// ordinary plurality once R1 makes primitive track role, not by
    /// R3, which only ever engages a real Enum-vs-Numeric role split).
    #[test]
    fn inconsistent_unit_formatting_resolves_by_plurality_without_needing_r3() {
        let runs = vec![
            (
                1,
                descriptor(SR::Enum, PP::LexicalPostings, &["1 year"], "bucketed"),
            ),
            (
                2,
                descriptor(SR::Enum, PP::BitmapEnum, &["lifetime"], "small closed set"),
            ),
            (
                3,
                descriptor(
                    SR::FreeText,
                    PP::LexicalPostings,
                    &["full sentence"],
                    "some values are long sentences",
                ),
            ),
        ];
        let s = stats(112, 21320, 0.0053, 0.00028);
        let outcome = canonicalize(&runs, "warrantylength", &s, &[], false, false);
        let d = outcome.promoted().expect("must resolve, not abstain");
        assert_eq!(d.semantic_role, SR::Enum);
        assert_eq!(
            d.canonical_physical_primitive,
            PP::BitmapEnum,
            "R1 makes primitive a function of the resolved role, regardless of any run's own primitive vote"
        );
    }

    /// Cardinality too high for R3's bounded-Enum rule to apply, and a
    /// real Enum-vs-Numeric role conflict genuinely exists in the raw
    /// votes: R3's two evidence conditions both fail to engage (parse
    /// rate is low, but cardinality exceeds the bound), so R9's
    /// defense-in-depth abstain fires -- a conservative, safe outcome
    /// for a case this checkpoint's own rules do not claim to resolve.
    #[test]
    fn enum_vs_numeric_conflict_above_the_bounded_cardinality_ceiling_abstains() {
        let runs = vec![
            (
                1,
                descriptor(SR::Enum, PP::BitmapEnum, &[], "many short codes"),
            ),
            (
                2,
                descriptor(SR::Numeric, PP::NumericRange, &[], "could be a code number"),
            ),
        ];
        let s = stats(500, 21320, 0.023, 0.02);
        let outcome = canonicalize(
            &runs,
            "ambiguous_high_cardinality_code",
            &s,
            &[],
            false,
            false,
        );
        assert!(
            !outcome.is_promoted(),
            "neither of R3's two evidence conditions applies above the bounded-cardinality ceiling -- must abstain, not guess"
        );
    }

    /// Case 10: proposals agree with each other but are contradicted by
    /// real catalog statistics -- all 3 raw runs confidently and
    /// unanimously propose Identifier, but the real uniqueness_ratio
    /// (0.10) is nowhere near the production bar. Unanimous raw
    /// agreement must not override measured evidence.
    #[test]
    fn unanimous_agreement_does_not_override_contradicting_real_statistics() {
        let runs = vec![
            (
                1,
                descriptor(SR::Identifier, PP::IdentifierDictionary, &[], "confident"),
            ),
            (
                2,
                descriptor(SR::Identifier, PP::IdentifierDictionary, &[], "confident"),
            ),
            (
                3,
                descriptor(SR::Identifier, PP::IdentifierDictionary, &[], "confident"),
            ),
        ];
        // uniqueness_ratio 0.10, occurrences well above the sample-size
        // bar, but nowhere near unique -- clearly not a real identifier
        // no matter how many runs agree.
        let s = stats(500, 5000, 0.10, 0.01);
        let outcome = canonicalize(&runs, "misjudged_field", &s, &[], false, false);
        assert!(
            !matches!(outcome.promoted().map(|d| d.semantic_role), Some(SR::Identifier)),
            "unanimous but evidence-contradicted Identifier votes must not be promoted as Identifier"
        );
    }

    /// Case 11: majority vote would be unsafe -- side-by-side comparison
    /// of Treatment B (promotes the Relationship plurality winner
    /// unconditionally) against Treatment C (R7 hard-blocks it). This is
    /// the concrete case Issue #45's own falsification criteria and GO
    /// gate criterion 1 (zero unsafe accepted) exist to catch.
    #[test]
    fn majority_vote_promotes_what_the_canonicalizer_correctly_refuses() {
        let mut relationship = descriptor(SR::Relationship, PP::None, &[], "cross-reference");
        relationship.scope = Sc::Relationship;
        relationship.relationship_semantics = Some("linked part".to_string());
        let runs = vec![
            (1, relationship.clone()),
            (2, relationship.clone()),
            (3, relationship.clone()),
            (4, descriptor(SR::Enum, PP::BitmapEnum, &[], "alt reading")),
        ];
        let s = stats(78, 161, 0.4845, 0.13);
        let b_outcome = majority_vote(&runs, "compatibledrainassemblypartnumber");
        let c_outcome = canonicalize(
            &runs,
            "compatibledrainassemblypartnumber",
            &s,
            &[],
            false,
            false,
        );

        assert_eq!(
            b_outcome.promoted().map(|d| d.semantic_role),
            Some(SR::Relationship),
            "Treatment B is naive and promotes the 3/4 plurality winner unconditionally"
        );
        assert!(
            !c_outcome.is_promoted(),
            "Treatment C's R7 hard-blocks Relationship promotion regardless of vote share"
        );
    }
}
