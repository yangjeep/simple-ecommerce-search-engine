use crate::ir::{compile, measure_coverage, CoverageReport, SemanticLexicon};

/// Per-query and aggregate replay evidence comparing a baseline lexicon
/// against a candidate lexicon over the same query set. This is the only
/// input `try_promote` is allowed to decide from — never the candidate's
/// aggregate coverage number alone, since an aggregate gain could hide an
/// individual regression (CLAUDE.md: "no model-generated semantic route
/// promoted without deterministic replay/evaluation evidence").
#[derive(Debug, Clone, PartialEq)]
pub struct ReplayResult {
    pub baseline: CoverageReport,
    pub candidate: CoverageReport,
    /// Queries that were fully resolved against the baseline but are not
    /// fully resolved against the candidate. Any non-empty regression list
    /// fails the promotion gate, regardless of aggregate coverage.
    pub regressions: Vec<String>,
    /// Queries that were not fully resolved against the baseline but are
    /// fully resolved against the candidate.
    pub newly_resolved: Vec<String>,
}

impl ReplayResult {
    pub fn passes_promotion_gate(&self) -> bool {
        self.regressions.is_empty() && self.candidate.fully_resolved > self.baseline.fully_resolved
    }
}

/// Compile every query in `queries` against both lexicons and compare,
/// query by query, whether "fully resolved" (no ambiguity, no residual
/// terms) status changed.
pub fn replay(
    queries: &[&str],
    baseline: &SemanticLexicon,
    candidate: &SemanticLexicon,
) -> ReplayResult {
    let mut regressions = Vec::new();
    let mut newly_resolved = Vec::new();
    for &query in queries {
        let before = compile(query, baseline);
        let after = compile(query, candidate);
        let before_ok = before.ambiguous.is_empty() && before.residual_lexical.is_empty();
        let after_ok = after.ambiguous.is_empty() && after.residual_lexical.is_empty();
        if before_ok && !after_ok {
            regressions.push(query.to_string());
        }
        if !before_ok && after_ok {
            newly_resolved.push(query.to_string());
        }
    }
    ReplayResult {
        baseline: measure_coverage(queries, baseline),
        candidate: measure_coverage(queries, candidate),
        regressions,
        newly_resolved,
    }
}
