//! E2c (`docs/experiments/ISSUE45_PROTOCOL.md`): the canonical descriptor
//! shape R1-R11 produce, plus the raw-proposal type they consume.
//!
//! `CandidateDescriptor` is deliberately **not** a new struct -- the
//! protocol's own section 2 documents why `e2b_schema::Descriptor`
//! already matches Issue #45's required schema field for field, and
//! reusing it verbatim (rather than a parallel type) is what lets E2c
//! read the 20 already-frozen `dataset_cache/export/e2b_llm_proposals_*.json`
//! artifacts with zero reformatting.

use crate::e2b_schema::{
    Operator, PhysicalPrimitive, Scope, SemanticRole, Significance, ValueType,
};

/// A raw LLM proposal -- see `docs/experiments/ISSUE45_PROTOCOL.md`
/// section 2. Alias, not a new type.
pub type CandidateDescriptor = crate::e2b_schema::Descriptor;

pub const CANONICAL_SCHEMA_VERSION: u32 = 1;

/// One contributing raw proposal's own role/primitive/confidence,
/// preserved even after canonicalization resolves a single final answer
/// -- Issue #45's own deliverable list requires "raw proposal
/// artifact... provenance back to... proposal runs" never discarded.
#[derive(Debug, Clone, PartialEq)]
pub struct RunProvenance {
    pub run_index: u32,
    pub semantic_role: SemanticRole,
    pub candidate_physical_primitive: PhysicalPrimitive,
    pub confidence: f64,
    pub abstained: bool,
}

/// The deterministic canonicalizer's output for one (configuration, real
/// key) once evidence has resolved it to a single, safe, compilable
/// answer.
#[derive(Debug, Clone, PartialEq)]
pub struct CanonicalDescriptor {
    pub schema_version: u32,
    pub real_key: String,
    pub semantic_role: SemanticRole,
    pub value_type: ValueType,
    pub scope: Scope,
    pub supported_operators: Vec<Operator>,
    pub aliases: Vec<String>,
    pub retrieval_significance: Significance,
    pub canonical_physical_primitive: PhysicalPrimitive,
    pub confidence: f64,
    pub provenance: Vec<RunProvenance>,
    /// Which rule(s) fired and why -- a human-auditable decision trail,
    /// not consumed by any downstream metric, only ever printed/preserved.
    pub decision_reasons: Vec<String>,
}

/// `ValidatedDescriptor | Abstain` from Issue #45's own architecture
/// diagram.
#[derive(Debug, Clone, PartialEq)]
pub enum CanonicalOutcome {
    Promoted(CanonicalDescriptor),
    Abstain {
        real_key: String,
        reason: String,
        contributing_runs: Vec<u32>,
    },
}

impl CanonicalOutcome {
    pub fn real_key(&self) -> &str {
        match self {
            CanonicalOutcome::Promoted(d) => &d.real_key,
            CanonicalOutcome::Abstain { real_key, .. } => real_key,
        }
    }

    pub fn is_promoted(&self) -> bool {
        matches!(self, CanonicalOutcome::Promoted(_))
    }

    pub fn promoted(&self) -> Option<&CanonicalDescriptor> {
        match self {
            CanonicalOutcome::Promoted(d) => Some(d),
            CanonicalOutcome::Abstain { .. } => None,
        }
    }
}
