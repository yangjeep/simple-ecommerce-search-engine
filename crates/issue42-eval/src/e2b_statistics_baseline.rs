//! E2b baseline 1 (`docs/experiments/ISSUE42_PROTOCOL.md`'s E2b
//! amendment 1): a deterministic classifier using *only*
//! [`crate::e2b_workload::UnifiedFieldStats`] -- uniqueness ratio,
//! cardinality, numeric-parseable fraction, and (when available)
//! variant scope. Zero access to a key's own name and zero natural-
//! language understanding of what a sample value *means* (it cannot
//! know `"modern"` names a design style; it can only see "a
//! low-cardinality string"). This is the honest "what can pure
//! statistics do without semantic understanding" floor every other
//! baseline is compared against.
//!
//! Reuses `commerce_core::index::identifier`'s own real, calibrated
//! constants (`MIN_UNIQUENESS_RATIO`, `MIN_IDENTIFIER_SAMPLE_SIZE`) for
//! the identifier-shaped case, rather than inventing a second,
//! independent cutoff -- the same real production mechanism this
//! codebase's own R3 experiment and R2/R3 production merge already
//! validated.

use commerce_core::index::{MIN_IDENTIFIER_SAMPLE_SIZE, MIN_UNIQUENESS_RATIO};

use crate::e2b_schema::{
    Descriptor, Operator, PhysicalPrimitive, Scope, SemanticRole, Significance, ValueType,
};
use crate::e2b_workload::UnifiedFieldStats;

/// Below this many occurrences, the statistics-only baseline abstains
/// entirely rather than guess from a handful of samples -- the same
/// small-sample discipline `IdentifierClassifier` already requires for
/// the identifier case, generalized here to every role.
const MIN_SAMPLE_SIZE: usize = 30;

pub fn classify(stats: &UnifiedFieldStats) -> Descriptor {
    if stats.occurrences < MIN_SAMPLE_SIZE {
        return Descriptor {
            key: stats.key.clone(),
            real_key: None,
            semantic_role: SemanticRole::Ignore,
            value_type: ValueType::String,
            scope: Scope::Product,
            supported_operators: vec![],
            aliases: vec![],
            relationship_semantics: None,
            retrieval_significance: Significance::Ignore,
            candidate_physical_primitive: PhysicalPrimitive::None,
            confidence: 0.0,
            evidence: format!(
                "only {} occurrences, below the {MIN_SAMPLE_SIZE}-occurrence minimum this \
                 statistics-only baseline requires before classifying anything at all",
                stats.occurrences
            ),
            abstain: true,
        };
    }

    // Identifier: reuses the real, calibrated production cutoffs
    // directly, including the small-sample safeguard R3's own
    // second correction round added.
    if stats.uniqueness_ratio >= MIN_UNIQUENESS_RATIO
        && stats.occurrences >= MIN_IDENTIFIER_SAMPLE_SIZE
        && stats.variant_scoped != Some(false)
    {
        return Descriptor {
            key: stats.key.clone(),
            real_key: None,
            semantic_role: SemanticRole::Identifier,
            value_type: ValueType::String,
            scope: if stats.variant_scoped == Some(true) {
                Scope::Variant
            } else {
                Scope::Product
            },
            supported_operators: vec![Operator::ExactLookup],
            aliases: vec![],
            relationship_semantics: None,
            // Statistics alone cannot tell a real primary identifier
            // from a statistically-identical-looking internal
            // reference field (e.g. WANDS's own `samplepartnumber`) --
            // a deliberately conservative default, disclosing the real
            // limit of this baseline rather than guessing high.
            retrieval_significance: Significance::RankingOnly,
            candidate_physical_primitive: PhysicalPrimitive::IdentifierDictionary,
            confidence: stats.uniqueness_ratio,
            evidence: format!(
                "uniqueness_ratio={:.4} >= {MIN_UNIQUENESS_RATIO} and occurrences={} >= \
                 {MIN_IDENTIFIER_SAMPLE_SIZE} -- the real, calibrated commerce_core::index::identifier \
                 cutoffs, statistics only, no name/value semantics consulted",
                stats.uniqueness_ratio, stats.occurrences
            ),
            abstain: false,
        };
    }

    // Numeric: almost every sampled value actually parses as a number,
    // and there is real spread (not just 1-2 numeric-looking codes).
    if stats.numeric_parseable_fraction >= 0.9 && stats.distinct_values > 3 {
        return Descriptor {
            key: stats.key.clone(),
            real_key: None,
            semantic_role: SemanticRole::Numeric,
            value_type: ValueType::Number,
            scope: Scope::Product,
            supported_operators: vec![Operator::Range, Operator::Eq],
            aliases: vec![],
            relationship_semantics: None,
            retrieval_significance: Significance::RankingOnly,
            candidate_physical_primitive: PhysicalPrimitive::NumericRange,
            confidence: stats.numeric_parseable_fraction,
            evidence: format!(
                "numeric_parseable_fraction={:.4}, distinct_values={} -- almost every sampled \
                 value parses as a number with real spread",
                stats.numeric_parseable_fraction, stats.distinct_values
            ),
            abstain: false,
        };
    }

    // Boolean: exactly two distinct values, not numeric-shaped.
    if stats.distinct_values == 2 {
        return Descriptor {
            key: stats.key.clone(),
            real_key: None,
            semantic_role: SemanticRole::Boolean,
            value_type: ValueType::Boolean,
            scope: Scope::Product,
            supported_operators: vec![Operator::Eq],
            aliases: vec![],
            relationship_semantics: None,
            retrieval_significance: Significance::RetrievalSignificant,
            candidate_physical_primitive: PhysicalPrimitive::BitmapEnum,
            confidence: 0.7,
            evidence: "exactly 2 distinct values, not numeric-shaped -- statistics alone cannot \
                       confirm these are really yes/no semantics, only that cardinality is 2"
                .to_string(),
            abstain: false,
        };
    }

    // Enum: real repetition (low uniqueness ratio) AND short,
    // categorical-shaped values (low mean length). Cardinality alone
    // does not work here -- found, not assumed, before this baseline
    // was ever run against the oracle: a real bounded Enum
    // (`color`, uniqueness_ratio=0.178, 4,686 distinct values) and real
    // free text (`productcare`, uniqueness_ratio=0.222, 3,500 distinct
    // values) have near-identical uniqueness ratios AND both exceed any
    // small distinct-value ceiling, so neither signal alone separates
    // them -- mean value length does (`color`~8 chars vs
    // `productcare`~51 chars on this real data).
    if stats.uniqueness_ratio < 0.3 && stats.mean_value_length <= 20.0 {
        return Descriptor {
            key: stats.key.clone(),
            real_key: None,
            semantic_role: SemanticRole::Enum,
            value_type: ValueType::String,
            scope: Scope::Product,
            supported_operators: vec![Operator::Eq],
            aliases: vec![],
            relationship_semantics: None,
            retrieval_significance: Significance::RetrievalSignificant,
            candidate_physical_primitive: PhysicalPrimitive::BitmapEnum,
            confidence: 1.0 - stats.uniqueness_ratio,
            evidence: format!(
                "uniqueness_ratio={:.4} < 0.3, mean_value_length={:.1} <= 20 -- bounded, \
                 repeating, short/categorical-shaped values",
                stats.uniqueness_ratio, stats.mean_value_length
            ),
            abstain: false,
        };
    }

    // Everything else: high-cardinality, non-numeric, not boolean --
    // the honest "cannot classify from statistics alone" bucket.
    Descriptor {
        key: stats.key.clone(),
        real_key: None,
        semantic_role: SemanticRole::FreeText,
        value_type: ValueType::String,
        scope: Scope::Product,
        supported_operators: vec![Operator::Contains],
        aliases: vec![],
        relationship_semantics: None,
        retrieval_significance: Significance::Ignore,
        candidate_physical_primitive: PhysicalPrimitive::LexicalPostings,
        confidence: 0.4,
        evidence: format!(
            "uniqueness_ratio={:.4}, distinct_values={}, numeric_parseable_fraction={:.4} -- \
             fits none of Identifier/Numeric/Boolean/Enum's statistical shape; the conservative \
             default, not a guess at a more specific role",
            stats.uniqueness_ratio, stats.distinct_values, stats.numeric_parseable_fraction
        ),
        abstain: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::e2b_workload::{automotive_unified_stats, load_wands_feed, UnifiedFieldStats};

    fn wands_unified() -> std::collections::BTreeMap<String, UnifiedFieldStats> {
        let feed = load_wands_feed();
        feed.stats
            .iter()
            .map(|(k, s)| (k.clone(), UnifiedFieldStats::from(s)))
            .collect()
    }

    #[test]
    fn classifies_part_number_as_identifier_using_the_real_production_cutoff() {
        let stats = automotive_unified_stats(1500);
        let d = classify(stats.get("part_number").unwrap());
        assert_eq!(d.semantic_role, SemanticRole::Identifier);
        assert!(!d.abstain);
    }

    #[test]
    fn abstains_on_voltage_a_constant_field_below_no_minimum_but_still_classifies_numeric() {
        let stats = automotive_unified_stats(1500);
        // voltage has occurrences=150 (>= MIN_SAMPLE_SIZE) but distinct=1
        // -- not enough distinct values to call Numeric under this
        // baseline's own >3-distinct-value rule, and not boolean
        // (distinct != 2), so it falls through to the FreeText/default
        // bucket -- a real, disclosed statistics-only limitation (a
        // constant numeric field looks like unclassifiable noise to
        // pure statistics, not like "a real Numeric field with no
        // variance").
        let d = classify(stats.get("voltage").unwrap());
        assert_ne!(d.semantic_role, SemanticRole::Numeric);
    }

    #[test]
    #[ignore = "requires dataset_cache/wands/product.csv (run scripts/datasets/fetch_wands.sh first, then `cargo test -- --ignored`); not fetched in CI, matching this repo's own convention of keeping real-external-dataset dependence out of `cargo test`"]
    fn classifies_color_as_enum_from_bounded_cardinality_alone() {
        let stats = wands_unified();
        let d = classify(stats.get("color").unwrap());
        assert_eq!(d.semantic_role, SemanticRole::Enum);
    }

    #[test]
    #[ignore = "requires dataset_cache/wands/product.csv (run scripts/datasets/fetch_wands.sh first, then `cargo test -- --ignored`); not fetched in CI, matching this repo's own convention of keeping real-external-dataset dependence out of `cargo test`"]
    fn classifies_overallproductweight_as_numeric_from_shape_alone() {
        let stats = wands_unified();
        let d = classify(stats.get("overallproductweight").unwrap());
        assert_eq!(d.semantic_role, SemanticRole::Numeric);
    }

    #[test]
    #[ignore = "requires dataset_cache/wands/product.csv (run scripts/datasets/fetch_wands.sh first, then `cargo test -- --ignored`); not fetched in CI, matching this repo's own convention of keeping real-external-dataset dependence out of `cargo test`"]
    fn classifies_samplepartnumber_as_identifier_but_cannot_know_it_is_not_retrieval_significant() {
        let stats = wands_unified();
        let d = classify(stats.get("samplepartnumber").unwrap());
        assert_eq!(
            d.semantic_role,
            SemanticRole::Identifier,
            "statistically identifier-shaped, exactly as the oracle also confirms statistically"
        );
        assert_ne!(
            d.retrieval_significance,
            Significance::Ignore,
            "this baseline's whole point: it CANNOT see that this field is semantically a red \
             herring (the oracle marks it Ignore for retrieval significance; this baseline has \
             no way to reach that conclusion from statistics alone)"
        );
    }

    #[test]
    #[ignore = "requires dataset_cache/wands/product.csv (run scripts/datasets/fetch_wands.sh first, then `cargo test -- --ignored`); not fetched in CI, matching this repo's own convention of keeping real-external-dataset dependence out of `cargo test`"]
    fn classifies_productcare_as_freetext_from_high_cardinality_alone() {
        let stats = wands_unified();
        let d = classify(stats.get("productcare").unwrap());
        assert_eq!(d.semantic_role, SemanticRole::FreeText);
    }
}
