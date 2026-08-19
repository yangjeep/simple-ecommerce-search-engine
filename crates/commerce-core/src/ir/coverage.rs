use super::lexicon::SemanticLexicon;
use super::query::compile;

/// Gate 4's explicit metric: "what fraction of a representative ecommerce
/// query set resolves without model inference." A query is *fully
/// resolved* when it compiles with no ambiguous spans and no residual
/// lexical terms — everything the shopper typed was either mapped to a
/// typed constraint/preference or intentionally dropped as a stopword.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoverageReport {
    pub total_queries: usize,
    pub fully_resolved: usize,
    pub had_ambiguity: usize,
    pub had_residual: usize,
}

impl CoverageReport {
    pub fn fraction_fully_resolved(&self) -> f64 {
        if self.total_queries == 0 {
            return 0.0;
        }
        self.fully_resolved as f64 / self.total_queries as f64
    }
}

pub fn measure_coverage(queries: &[&str], lexicon: &SemanticLexicon) -> CoverageReport {
    let mut report = CoverageReport {
        total_queries: queries.len(),
        fully_resolved: 0,
        had_ambiguity: 0,
        had_residual: 0,
    };
    for query in queries {
        let compiled = compile(query, lexicon);
        let ambiguous = !compiled.ambiguous.is_empty();
        let residual = !compiled.residual_lexical.is_empty();
        if ambiguous {
            report.had_ambiguity += 1;
        }
        if residual {
            report.had_residual += 1;
        }
        if !ambiguous && !residual {
            report.fully_resolved += 1;
        }
    }
    report
}
