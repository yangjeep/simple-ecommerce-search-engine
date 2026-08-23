//! E2b's deterministic validator (`docs/experiments/ISSUE42_PROTOCOL.md`'s
//! E2b amendment 1): checks a proposed [`Descriptor`] against the
//! *actually measured* [`UnifiedFieldStats`] for its real key -- never
//! against `e2b_oracle`, mirroring `commerce_core`'s own "never trust
//! the delegate for correctness" doctrine applied to the control plane
//! instead of a lexical delegate. LLM proposals are hypotheses; this is
//! what stands between a hypothesis and anything a real build would act
//! on.

use commerce_core::index::{compute_field_stats, IdentifierClassifier, MIN_IDENTIFIER_SAMPLE_SIZE};

use crate::e2b_schema::{Descriptor, Scope, SemanticRole, ValueType};
use crate::e2b_workload::UnifiedFieldStats;

/// One validator finding against a single proposed [`Descriptor`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidatorFinding {
    /// A real, confirmed problem serious enough that the descriptor must
    /// not be accepted as proposed -- an "unsafe accepted structural
    /// classification" if it were shipped anyway.
    Reject(String),
    /// A real concern, not disqualifying on its own -- reported,
    /// confidence lowered, not silently accepted at face value.
    Downgrade(String),
}

#[derive(Debug, Clone)]
pub struct ValidationResult {
    pub accepted: bool,
    pub findings: Vec<ValidatorFinding>,
    /// distinct_values * average sampled value byte length -- a real,
    /// reported memory estimate, not gated on (matching R3's own
    /// "RSS/index-size... not disqualifying on its own" precedent).
    pub memory_estimate_bytes: usize,
    /// Whether any of this descriptor's own proposed `aliases` appears
    /// as a real substring of any of WANDS's own 480 real shopper
    /// queries.
    pub workload_evidence_found: bool,
}

fn parses_as_number(v: &str) -> bool {
    v.parse::<f64>().is_ok()
}

/// Every `supported_operators` entry this repository's own
/// `commerce_core::domain::Constraint`/`StructuralConstraint` can
/// actually express today. `ExactLookup` is expressible only via R3's
/// own `IdentifierDictionary`/`CatalogIndex::identifier_lookup` --
/// real, but a distinct mechanism from `Constraint`'s own operators.
fn operator_has_real_serving_semantics(op: crate::e2b_schema::Operator) -> bool {
    use crate::e2b_schema::Operator::*;
    matches!(op, Eq | Range | Contains | ExactLookup)
}

pub fn validate(
    proposal: &Descriptor,
    stats: &UnifiedFieldStats,
    wands_queries: &[String],
) -> ValidationResult {
    let mut findings = Vec::new();

    // Parseability: a Number-typed proposal's real sampled values must
    // actually parse as numbers.
    if proposal.value_type == ValueType::Number {
        let non_numeric: Vec<&String> = stats
            .sample_values
            .iter()
            .filter(|v| !parses_as_number(v))
            .collect();
        if !non_numeric.is_empty() {
            findings.push(ValidatorFinding::Reject(format!(
                "proposed value_type=Number but real sampled values include non-numeric \
                 entries: {non_numeric:?}"
            )));
        }
    }

    // Cardinality/density/selectivity: a generous ceiling, deliberately
    // conservative so a real, legitimate high-cardinality Enum (e.g.
    // WANDS's own `color` at 4,686 distinct) is not rejected outright --
    // only downgraded well past it.
    const ENUM_CARDINALITY_CEILING: usize = 6000;
    if proposal.semantic_role == SemanticRole::Enum
        && stats.distinct_values > ENUM_CARDINALITY_CEILING
    {
        findings.push(ValidatorFinding::Downgrade(format!(
            "proposed Enum but real distinct_values={} exceeds the generous {ENUM_CARDINALITY_CEILING} \
             ceiling -- plausible but unusually high for a bounded categorical vocabulary",
            stats.distinct_values
        )));
    }

    // Uniqueness/collision check for Identifier proposals: reuses the
    // real, production IdentifierClassifier-shaped statistics directly
    // (via commerce_core::index::compute_field_stats over automotive's
    // own real Catalog) is not reachable here for WANDS's raw-string
    // source (which has no domain::Catalog to build), so the check is
    // applied structurally: the real uniqueness ratio and sample size
    // must independently clear the same production cutoffs
    // IdentifierClassifier::accepts requires, not merely assumed because
    // the proposal says so.
    if proposal.semantic_role == SemanticRole::Identifier {
        let clears_uniqueness =
            stats.uniqueness_ratio >= commerce_core::index::MIN_UNIQUENESS_RATIO;
        let clears_sample_size = stats.occurrences >= MIN_IDENTIFIER_SAMPLE_SIZE;
        if !clears_uniqueness || !clears_sample_size {
            findings.push(ValidatorFinding::Reject(format!(
                "proposed Identifier but real stats do not independently clear \
                 IdentifierClassifier's own production cutoffs: uniqueness_ratio={:.4} (need >= \
                 {:.2}), occurrences={} (need >= {MIN_IDENTIFIER_SAMPLE_SIZE})",
                stats.uniqueness_ratio,
                commerce_core::index::MIN_UNIQUENESS_RATIO,
                stats.occurrences
            )));
        }
    }

    // Scope consistency: a Relationship proposal must name
    // relationship_semantics; a non-Relationship proposal must not.
    match (proposal.scope, &proposal.relationship_semantics) {
        (Scope::Relationship, None) => {
            findings.push(ValidatorFinding::Reject(
                "scope=Relationship but relationship_semantics is not set".to_string(),
            ));
        }
        (scope, Some(_)) if scope != Scope::Relationship => {
            findings.push(ValidatorFinding::Reject(format!(
                "scope={scope:?} (not Relationship) but relationship_semantics is set"
            )));
        }
        _ => {}
    }

    // Supported serving semantics: every operator must be one this
    // repository's own domain model can actually express today.
    for op in &proposal.supported_operators {
        if !operator_has_real_serving_semantics(*op) {
            findings.push(ValidatorFinding::Downgrade(format!(
                "operator {op:?} has no real serving-side representation in \
                 commerce_core::domain::Constraint/StructuralConstraint today"
            )));
        }
    }

    // Workload evidence: does any proposed alias appear as a real
    // substring of any of WANDS's own 480 real shopper queries?
    let workload_evidence_found = proposal.aliases.iter().any(|alias| {
        let alias_lower = alias.to_lowercase();
        !alias_lower.is_empty()
            && wands_queries
                .iter()
                .any(|q| q.to_lowercase().contains(&alias_lower))
    });
    if !proposal.aliases.is_empty() && !workload_evidence_found {
        findings.push(ValidatorFinding::Downgrade(
            "none of the proposed aliases appears in any of WANDS's own 480 real shopper \
             queries -- plausible but unconfirmed against real workload evidence"
                .to_string(),
        ));
    }

    let memory_estimate_bytes = stats.distinct_values
        * stats
            .sample_values
            .iter()
            .map(|v| v.len())
            .max()
            .unwrap_or(8)
            .max(8);

    let accepted = !findings
        .iter()
        .any(|f| matches!(f, ValidatorFinding::Reject(_)));

    ValidationResult {
        accepted,
        findings,
        memory_estimate_bytes,
        workload_evidence_found,
    }
}

/// Reused by tests/the eval binary so every call site normalizes query
/// text the same way.
pub fn wands_query_texts() -> Vec<String> {
    crate::e2b_workload::load_wands_queries()
        .into_iter()
        .map(|q| q.text)
        .collect()
}

/// Real, structural confirmation that a `PhysicalPrimitive::IdentifierDictionary`
/// proposal for a field of automotive's own real `Catalog` would also be
/// accepted by the real, unmodified, production `IdentifierClassifier` --
/// used only where a real domain `Catalog` (automotive) is available;
/// WANDS's raw feed has no `Catalog` to build one over at all.
pub fn confirms_against_real_identifier_classifier(
    catalog: &commerce_core::domain::Catalog,
    field: &str,
) -> bool {
    let stats = compute_field_stats(catalog);
    stats
        .get(field)
        .map(IdentifierClassifier::accepts)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::e2b_schema::{Operator, PhysicalPrimitive, Significance};
    use crate::e2b_workload::{load_wands_feed, UnifiedFieldStats};

    fn stub_stats(
        key: &str,
        uniqueness_ratio: f64,
        occurrences: usize,
        samples: &[&str],
    ) -> UnifiedFieldStats {
        UnifiedFieldStats {
            key: key.to_string(),
            occurrences,
            distinct_values: samples.len(),
            uniqueness_ratio,
            numeric_parseable_fraction: 0.0,
            mean_value_length: 5.0,
            variant_scoped: None,
            sample_values: samples.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn base_descriptor(key: &str) -> Descriptor {
        Descriptor {
            key: key.to_string(),
            real_key: None,
            semantic_role: SemanticRole::Enum,
            value_type: ValueType::String,
            scope: Scope::Product,
            supported_operators: vec![Operator::Eq],
            aliases: vec![],
            relationship_semantics: None,
            retrieval_significance: Significance::RetrievalSignificant,
            candidate_physical_primitive: PhysicalPrimitive::BitmapEnum,
            confidence: 0.8,
            evidence: "test".to_string(),
            abstain: false,
        }
    }

    #[test]
    fn rejects_a_number_proposal_whose_real_values_are_not_numeric() {
        let mut proposal = base_descriptor("color");
        proposal.value_type = ValueType::Number;
        let stats = stub_stats("color", 0.2, 100, &["blue", "red"]);
        let result = validate(&proposal, &stats, &[]);
        assert!(!result.accepted);
        assert!(result
            .findings
            .iter()
            .any(|f| matches!(f, ValidatorFinding::Reject(_))));
    }

    #[test]
    fn accepts_a_number_proposal_whose_real_values_are_numeric() {
        let mut proposal = base_descriptor("weight");
        proposal.value_type = ValueType::Number;
        proposal.semantic_role = SemanticRole::Numeric;
        let stats = stub_stats("weight", 0.9, 100, &["10.5", "20", "5.25"]);
        let result = validate(&proposal, &stats, &[]);
        assert!(result.accepted);
    }

    #[test]
    fn rejects_an_identifier_proposal_that_does_not_clear_the_real_production_cutoffs() {
        let mut proposal = base_descriptor("title");
        proposal.semantic_role = SemanticRole::Identifier;
        // Real uniqueness ratio 0.9029, well under the 0.95 production cutoff.
        let stats = stub_stats("title", 0.9029, 546, &["a", "b"]);
        let result = validate(&proposal, &stats, &[]);
        assert!(!result.accepted);
    }

    #[test]
    fn rejects_a_relationship_proposal_missing_relationship_semantics() {
        let mut proposal = base_descriptor("compatiblediningchairpartnumber");
        proposal.scope = Scope::Relationship;
        proposal.relationship_semantics = None;
        let stats = stub_stats("compatiblediningchairpartnumber", 1.0, 71, &["a", "b"]);
        let result = validate(&proposal, &stats, &[]);
        assert!(!result.accepted);
    }

    #[test]
    fn rejects_a_non_relationship_proposal_that_sets_relationship_semantics() {
        let mut proposal = base_descriptor("color");
        proposal.relationship_semantics = Some("bogus".to_string());
        let stats = stub_stats("color", 0.2, 100, &["blue"]);
        let result = validate(&proposal, &stats, &[]);
        assert!(!result.accepted);
    }

    #[test]
    fn workload_evidence_is_found_for_a_real_alias_appearing_in_a_real_wands_query() {
        let queries = wands_query_texts();
        assert!(!queries.is_empty());
        let mut proposal = base_descriptor("style");
        proposal.aliases = vec!["chair".to_string()];
        let stats = stub_stats("style", 0.1, 100, &["modern"]);
        let result = validate(&proposal, &stats, &queries);
        assert!(
            result.workload_evidence_found,
            "'chair' must appear in at least one of WANDS's real 480 queries"
        );
    }

    #[test]
    fn workload_evidence_is_absent_for_a_nonsense_alias() {
        let mut proposal = base_descriptor("style");
        proposal.aliases = vec!["zzzznonexistentqueryterm".to_string()];
        let stats = stub_stats("style", 0.1, 100, &["modern"]);
        let result = validate(&proposal, &stats, &["salon chair".to_string()]);
        assert!(!result.workload_evidence_found);
        assert!(result
            .findings
            .iter()
            .any(|f| matches!(f, ValidatorFinding::Downgrade(_))));
    }

    #[test]
    fn real_wands_color_field_validated_against_real_stats_and_workload() {
        let feed = load_wands_feed();
        let stats: UnifiedFieldStats = feed.stats.get("color").unwrap().into();
        let mut proposal = base_descriptor("color");
        proposal.aliases = vec!["blue".to_string()];
        let queries = wands_query_texts();
        let result = validate(&proposal, &stats, &queries);
        assert!(result.accepted);
        assert!(result.memory_estimate_bytes > 0);
    }

    #[test]
    fn confirms_part_number_against_the_real_production_identifier_classifier() {
        let products = issue38_e2e3_eval::automotive::generate_catalog(1500);
        let ingested = issue38_e2e3_eval::ingest::build_catalog(&products);
        assert!(confirms_against_real_identifier_classifier(
            &ingested.catalog,
            "part_number"
        ));
        assert!(!confirms_against_real_identifier_classifier(
            &ingested.catalog,
            "voltage"
        ));
    }
}
