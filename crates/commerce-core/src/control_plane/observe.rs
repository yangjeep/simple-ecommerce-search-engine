use std::collections::BTreeMap;

use crate::ir::{compile, SemanticLexicon};

/// One unresolved term seen while compiling a query batch against a
/// [`SemanticLexicon`], with how many queries it appeared in. Only
/// `residual_lexical` terms are observed here (no lexicon entry at all);
/// ambiguous spans are a different repair — narrowing an existing entry,
/// not adding one — and are out of scope for this gate's promotion flow
/// (see `docs/adr/0005-control-plane-prototype.md`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Observation {
    pub term: String,
    pub frequency: usize,
}

/// Compile every query in `queries` against `lexicon` and count how often
/// each residual (unmapped) term appears, most-frequent first with an
/// alphabetical tie-break so the result is deterministic regardless of
/// query order.
pub fn observe_residual_terms(queries: &[&str], lexicon: &SemanticLexicon) -> Vec<Observation> {
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for query in queries {
        let compiled = compile(query, lexicon);
        for term in compiled.residual_lexical {
            *counts.entry(term).or_insert(0) += 1;
        }
    }
    let mut observations: Vec<Observation> = counts
        .into_iter()
        .map(|(term, frequency)| Observation { term, frequency })
        .collect();
    observations.sort_by(|a, b| b.frequency.cmp(&a.frequency).then(a.term.cmp(&b.term)));
    observations
}
