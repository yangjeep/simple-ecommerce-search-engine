use super::lexicon::SemanticLexicon;

/// A versioned, compiled semantic context — Gate 4's "FIB" prototype: a
/// lexicon plus the version/provenance metadata a Gate 5 promotion
/// workflow will need to compare context versions against replay evidence.
/// Building a new context is the only way to change resolution behavior;
/// there is no in-place mutation API, matching Gate 4's "versioned
/// compiled semantic context" requirement.
#[derive(Debug)]
pub struct SemanticContext {
    pub version: u32,
    pub source: &'static str,
    lexicon: SemanticLexicon,
}

impl SemanticContext {
    pub fn new(version: u32, source: &'static str, lexicon: SemanticLexicon) -> Self {
        SemanticContext {
            version,
            source,
            lexicon,
        }
    }

    pub fn lexicon(&self) -> &SemanticLexicon {
        &self.lexicon
    }
}
