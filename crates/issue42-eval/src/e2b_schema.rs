//! E2b (`docs/experiments/ISSUE42_PROTOCOL.md`'s E2b amendment 1):
//! versioned descriptor schema every baseline (statistics-only, LLM
//! proposal, LLM+validator, oracle) produces instances of, so all four
//! are comparable on the same shape. `serde`-derived so a real LLM
//! pass's frozen JSON output (`dataset_cache/export/e2b_llm_proposals_*.json`)
//! deserializes directly into this type -- no bespoke parsing of free-text
//! model output.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticRole {
    Identifier,
    Enum,
    Numeric,
    Boolean,
    FreeText,
    Relationship,
    Ignore,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValueType {
    String,
    Number,
    Boolean,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    Product,
    Variant,
    Relationship,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Operator {
    Eq,
    Range,
    Contains,
    ExactLookup,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Significance {
    RetrievalSignificant,
    RankingOnly,
    Ignore,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PhysicalPrimitive {
    BitmapEnum,
    NumericRange,
    IdentifierDictionary,
    LexicalPostings,
    None,
}

/// One field's proposed (or oracle-authored) descriptor. `key` is
/// whatever name the pass actually saw -- the real key for the
/// unperturbed configuration, `feature_NNN` for the anonymized one, or a
/// noisy alias for the noisy-names one; `real_key` is filled in by the
/// harness after the fact (never given to a proposing pass) so results
/// can be joined back to the oracle regardless of which name variant was
/// shown.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Descriptor {
    pub key: String,
    #[serde(default)]
    pub real_key: Option<String>,
    pub semantic_role: SemanticRole,
    pub value_type: ValueType,
    pub scope: Scope,
    pub supported_operators: Vec<Operator>,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub relationship_semantics: Option<String>,
    pub retrieval_significance: Significance,
    pub candidate_physical_primitive: PhysicalPrimitive,
    pub confidence: f64,
    pub evidence: String,
    #[serde(default)]
    pub abstain: bool,
}

/// One perturbation configuration's frozen LLM-pass output: which
/// configuration produced it, which of the 2 independent runs this is,
/// and the raw proposals -- exactly the shape written to
/// `dataset_cache/export/e2b_llm_proposals_*.json` and read back by the
/// evaluation binary. Never constructed by any test/production code path
/// via a live model call (CLAUDE.md: "No test may require a real model
/// API key") -- only ever deserialized from a file already on disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmPassOutput {
    pub configuration: String,
    pub run_index: u32,
    pub descriptors: Vec<Descriptor>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descriptor_round_trips_through_json() {
        let d = Descriptor {
            key: "color".to_string(),
            real_key: None,
            semantic_role: SemanticRole::Enum,
            value_type: ValueType::String,
            scope: Scope::Product,
            supported_operators: vec![Operator::Eq],
            aliases: vec!["colour".to_string()],
            relationship_semantics: None,
            retrieval_significance: Significance::RetrievalSignificant,
            candidate_physical_primitive: PhysicalPrimitive::BitmapEnum,
            confidence: 0.9,
            evidence: "low cardinality string values, real shopper term".to_string(),
            abstain: false,
        };
        let json = serde_json::to_string(&d).unwrap();
        let back: Descriptor = serde_json::from_str(&json).unwrap();
        assert_eq!(d, back);
    }

    #[test]
    fn descriptor_deserializes_without_optional_fields_present() {
        let json = r#"{
            "key": "x",
            "semantic_role": "ignore",
            "value_type": "string",
            "scope": "product",
            "supported_operators": [],
            "retrieval_significance": "ignore",
            "candidate_physical_primitive": "none",
            "confidence": 0.0,
            "evidence": "no evidence"
        }"#;
        let d: Descriptor = serde_json::from_str(json).unwrap();
        assert_eq!(d.real_key, None);
        assert_eq!(d.aliases, Vec::<String>::new());
        assert_eq!(d.relationship_semantics, None);
        assert!(!d.abstain);
    }
}
