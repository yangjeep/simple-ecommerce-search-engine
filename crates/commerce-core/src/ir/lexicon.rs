use std::collections::BTreeMap;

use super::query::Preference;
use super::structural::ResolvedConstraint;

/// One candidate reading of a query phrase, with a confidence in `[0, 1]`.
/// Confidence is recorded for downstream consumers but is deliberately
/// *not* used by [`super::query::compile`] to auto-pick a winner when a
/// phrase has more than one candidate: see that function's doc comment.
#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    pub resolved: ResolvedTerm,
    pub confidence: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ResolvedTerm {
    Constraint(ResolvedConstraint),
    Preference(Preference),
}

impl Candidate {
    pub fn constraint(resolved: ResolvedConstraint, confidence: f64) -> Self {
        Candidate {
            resolved: ResolvedTerm::Constraint(resolved),
            confidence,
        }
    }

    pub fn preference(preference: Preference, confidence: f64) -> Self {
        Candidate {
            resolved: ResolvedTerm::Preference(preference),
            confidence,
        }
    }
}

/// A deterministic, hand-authored token/phrase -> candidate-resolution
/// table. This is a Gate 2 compiler prototype, not the versioned, promoted
/// semantic FIB Gate 4 requires: it carries no version and no promotion
/// workflow, and is rebuilt from source on every process start.
#[derive(Debug, Default)]
pub struct SemanticLexicon {
    entries: BTreeMap<String, Vec<Candidate>>,
}

impl SemanticLexicon {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, phrase: &str, candidates: Vec<Candidate>) {
        self.entries.insert(phrase.to_lowercase(), candidates);
    }

    pub fn resolve(&self, phrase: &str) -> Option<&[Candidate]> {
        self.entries.get(&phrase.to_lowercase()).map(Vec::as_slice)
    }

    pub fn max_phrase_words(&self) -> usize {
        self.entries
            .keys()
            .map(|k| k.split_whitespace().count())
            .max()
            .unwrap_or(1)
    }
}
