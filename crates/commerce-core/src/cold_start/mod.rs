//! Gate 6: cold-start semantic fuzzing. Given only a catalog fixture (no
//! per-SKU model calls, no hand-authored vocabulary), profile the
//! catalog's own distinct semantic values, compile them into a lexicon,
//! generate shopper-like queries from that same profile, and identify
//! which of those queries the derived lexicon still fails to resolve
//! fully — a "coverage hole" the catalog data alone could not close.

pub mod alias;
mod canonicalize;
mod generate;
mod profile;

pub use canonicalize::{
    CanonicalizationEvidence, FrequencyOnlyCanonicalizer, HeuristicCanonicalizer,
    VocabularyCanonicalizer, VocabularyClass,
};
pub use generate::generate_shopper_queries;
pub use profile::{
    compile_lexicon, compile_lexicon_with_alias_enforcement,
    compile_lexicon_with_brand_canonicalizer, CatalogProfile,
};

use crate::ir::{compile, SemanticLexicon};

/// Compile every query against `lexicon` and return the ones that are
/// *not* fully resolved (ambiguous or residual) — Gate 6's "identify
/// semantic coverage holes" deliverable.
pub fn coverage_holes<'q>(queries: &[&'q str], lexicon: &SemanticLexicon) -> Vec<&'q str> {
    queries
        .iter()
        .copied()
        .filter(|query| {
            let compiled = compile(query, lexicon);
            !compiled.ambiguous.is_empty() || !compiled.residual_lexical.is_empty()
        })
        .collect()
}
