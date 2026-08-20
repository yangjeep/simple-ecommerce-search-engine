//! Issue #9: the canonicalization-strategy abstraction. `commerce_core`
//! defines the mechanism and the shared evidence/classification vocabulary
//! only, exactly like `control_plane::provider::ModelProvider` --
//! offline/index-time only, never called from `ir::compile` or
//! `CatalogIndex::execute` (the hot query path), and never hard-wiring a
//! specific vendor/model. A real model-backed implementation belongs in an
//! experiment-harness crate (`phase2-eval`), the same place
//! `plan::LexicalDelegate`'s first real implementation (`TantivyDelegate`)
//! lives.
//!
//! This module does not (yet) change `compile_lexicon`'s production
//! behavior -- `compile_lexicon` still uses the existing binary
//! `min_enum_frequency` gate. This is deliberately an *evaluation*
//! surface: `phase2-eval`'s Issue #9 experiment compares strategies here
//! before any of them are wired into the production cold-start pipeline,
//! per this project's "architecture changes must be evidence-backed, not
//! opinion-backed" discipline.

/// The five-class taxonomy Issue #9 and
/// `docs/research/brand-adjudication-rubric.md` define for classifying one
/// raw-catalog-derived vocabulary value (e.g. a brand string) during
/// cold-start canonicalization.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VocabularyClass {
    /// Clearly names a real-world brand/manufacturer entity, or is an
    /// unambiguous spelling/formatting/casing variant of one identifiable
    /// from the evidence.
    CanonicalKnownEntityOrAlias,
    /// Reads as a plausible, distinct brand/manufacturer name, even if
    /// small/regional/unfamiliar -- not a slogan, not a mis-filed value.
    LegitimateNewEntity,
    /// Real text, but not a brand entity (a tagline, a product-title
    /// fragment, a generic descriptive phrase) -- should stay free text,
    /// never become a trusted hard-filter lexicon entry.
    LexicalOnlyNotStructural,
    /// The given evidence genuinely does not support a confident decision.
    AmbiguousInsufficientEvidence,
    /// No brand-identifying signal at all: gibberish, wrong-field content,
    /// near-empty, or purely numeric/coded.
    JunkMalformedWrongField,
}

impl VocabularyClass {
    /// Whether this classification means "safe to compile into a trusted
    /// hard-filter structural lexicon entry" -- the actual production
    /// question `compile_lexicon`'s binary gate answers today.
    pub fn trusted_as_structural(self) -> bool {
        matches!(
            self,
            VocabularyClass::CanonicalKnownEntityOrAlias | VocabularyClass::LegitimateNewEntity
        )
    }
}

/// The compressed catalog evidence a canonicalizer is allowed to use for
/// one candidate value -- deliberately bounded (a handful of
/// representative products, not the full candidate set), matching Gate 6's
/// "semantic problem compression" principle: a real production
/// canonicalizer (deterministic or model-assisted) must scale with
/// distinct *values*, not with SKU count.
#[derive(Debug, Clone)]
pub struct CanonicalizationEvidence<'a> {
    /// The raw candidate value, in its original casing (not lowercased --
    /// callers that need the lowercase key already have it separately).
    pub value: &'a str,
    /// How many products/attribute-occurrences this exact value was seen
    /// under, catalog-wide (the existing `enum_occurrence`/
    /// `brand_occurrence` signal `compile_lexicon` already computes).
    pub occurrence_count: usize,
    /// Titles of a handful of representative products carrying this
    /// value, for shape/consistency signals (e.g. does the value appear as
    /// a prefix of its own products' titles, the way a genuine brand
    /// field usually does on this catalog).
    pub representative_titles: &'a [String],
}

/// The mechanism every canonicalization strategy must implement.
/// `commerce_core` has no idea what implements this (a frequency
/// threshold, a deterministic heuristic, a real model) and no dependency
/// on anything beyond what's in this crate already -- mirrors
/// `control_plane::provider::ModelProvider` exactly.
pub trait VocabularyCanonicalizer {
    fn classify(&self, evidence: &CanonicalizationEvidence) -> VocabularyClass;
}

/// Wraps the existing, shipping `min_enum_frequency` mechanism
/// (`compile_lexicon`) as a `VocabularyCanonicalizer`, so it can be
/// compared against other strategies on equal footing in an experiment
/// harness. Produces only the two classes the existing binary gate can
/// actually distinguish: below `min_enum_frequency` maps to
/// `JunkMalformedWrongField` (the existing gate's implicit claim: an
/// excluded value is being treated as unfit for structural use, whatever
/// the *real* reason), at or above it maps to `LegitimateNewEntity` (the
/// existing gate's implicit claim: a frequent-enough value is fit for
/// structural use). This is deliberately the crude baseline Issue #9 asks
/// every other strategy to beat -- not a strawman weakened further for
/// this comparison, the literal shipping mechanism.
#[derive(Debug, Clone, Copy)]
pub struct FrequencyOnlyCanonicalizer {
    pub min_frequency: usize,
}

impl VocabularyCanonicalizer for FrequencyOnlyCanonicalizer {
    fn classify(&self, evidence: &CanonicalizationEvidence) -> VocabularyClass {
        if evidence.occurrence_count >= self.min_frequency {
            VocabularyClass::LegitimateNewEntity
        } else {
            VocabularyClass::JunkMalformedWrongField
        }
    }
}

/// Generic marketing/descriptive words observed in real excluded brand
/// values on the real ESCI catalog (`docs/research/brand-adjudication-rubric.md`'s
/// corpus, e.g. "funny musician gifts co", "this is a sharp not a hashtag
/// tee") -- a value containing one of these is very unlikely to be a
/// genuine brand/manufacturer name. Deliberately small and literal (no
/// stemming/fuzzy matching): a heuristic baseline should be auditable at a
/// glance, not itself a hidden model.
const MARKETING_WORDS: &[&str] = &[
    "gift",
    "gifts",
    "tee",
    "tees",
    "shirt",
    "shirts",
    "funny",
    "birthday",
    "anniversary",
    "co",
    "shop",
    "store",
    "apparel",
    "design",
    "designs",
    "custom",
    "print",
    "style",
    "wear",
    "lover",
    "lovers",
    "mom",
    "dad",
    "mother",
    "father",
    "hoodie",
    "sweatshirt",
    "novelty",
    "gag",
    "quote",
    "quotes",
    "saying",
    "sayings",
    "cute",
    "vintage",
    "retro",
    "graphic",
];

fn word_count(value: &str) -> usize {
    value.split_whitespace().count()
}

fn contains_marketing_word(value_lower: &str) -> bool {
    let tokens: Vec<&str> = value_lower
        .split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty())
        .collect();
    MARKETING_WORDS.iter().any(|w| tokens.contains(w))
}

/// Fraction of characters that are ASCII letters or whitespace -- a crude,
/// cheap proxy for "looks like a name" vs. "looks like a code/gibberish
/// string." Empty values score 0.0 (never mistaken for a clean shape).
fn clean_shape_ratio(value: &str) -> f64 {
    if value.is_empty() {
        return 0.0;
    }
    let clean = value
        .chars()
        .filter(|c| c.is_ascii_alphabetic() || c.is_whitespace())
        .count();
    clean as f64 / value.chars().count() as f64
}

/// Does any representative product's title start with `value` (case-
/// insensitive, matching how a genuine brand field's value commonly
/// appears verbatim at the start of that product's own title on this
/// catalog -- e.g. real product "Aero Pure AP80RVLW Super Quiet 80 CFM..."
/// with brand "Aero Pure"). A value that never appears in any of its own
/// products' titles is a real, cheap signal that it may be a mis-filed or
/// unrelated string rather than a genuine brand label.
fn title_prefix_match(value_lower: &str, representative_titles: &[String]) -> bool {
    representative_titles
        .iter()
        .any(|t| t.to_lowercase().starts_with(value_lower))
}

/// A deterministic, non-model canonicalization heuristic combining
/// frequency with cheap, auditable shape/consistency signals: word count,
/// a marketing-word blocklist, character-shape cleanliness, and title-
/// prefix self-consistency. This is Issue #9's required "competent cheap
/// baseline" -- built to be genuinely reasonable, not a strawman, so any
/// measured model-assisted improvement is real and not an artifact of
/// comparing against a weak baseline.
#[derive(Debug, Clone, Copy)]
pub struct HeuristicCanonicalizer {
    pub min_frequency_for_trust: usize,
}

impl VocabularyCanonicalizer for HeuristicCanonicalizer {
    fn classify(&self, evidence: &CanonicalizationEvidence) -> VocabularyClass {
        let value_lower = evidence.value.to_lowercase();
        let words = word_count(evidence.value);
        let marketing_hit = contains_marketing_word(&value_lower);
        let shape = clean_shape_ratio(evidence.value);
        let title_match = title_prefix_match(&value_lower, evidence.representative_titles);

        if shape < 0.6 {
            // Heavy punctuation/digits/symbols: not name-shaped at all.
            return VocabularyClass::JunkMalformedWrongField;
        }
        if marketing_hit && !title_match {
            // Reads as marketing/descriptive text, and isn't even
            // self-consistent with its own products' titles.
            return VocabularyClass::LexicalOnlyNotStructural;
        }
        if words > 4 && !title_match {
            // Long, title-fragment-shaped, and not self-consistent.
            return VocabularyClass::LexicalOnlyNotStructural;
        }

        let mut score: i32 = 0;
        if title_match {
            score += 3;
        }
        if (1..=3).contains(&words) {
            score += 1;
        }
        if words >= 5 {
            score -= 2;
        }
        if evidence.occurrence_count >= self.min_frequency_for_trust {
            score += 2;
        } else if evidence.occurrence_count >= self.min_frequency_for_trust / 4 {
            score += 1;
        }
        if marketing_hit {
            score -= 3;
        }

        match score {
            s if s >= 3 => VocabularyClass::LegitimateNewEntity,
            s if s <= -3 => VocabularyClass::JunkMalformedWrongField,
            _ => VocabularyClass::AmbiguousInsufficientEvidence,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn evidence<'a>(
        value: &'a str,
        occurrence_count: usize,
        titles: &'a [String],
    ) -> CanonicalizationEvidence<'a> {
        CanonicalizationEvidence {
            value,
            occurrence_count,
            representative_titles: titles,
        }
    }

    #[test]
    fn frequency_only_reproduces_the_existing_binary_gate() {
        let c = FrequencyOnlyCanonicalizer { min_frequency: 25 };
        let titles = vec![];
        assert_eq!(
            c.classify(&evidence("Anything", 24, &titles)),
            VocabularyClass::JunkMalformedWrongField
        );
        assert_eq!(
            c.classify(&evidence("Anything", 25, &titles)),
            VocabularyClass::LegitimateNewEntity
        );
    }

    #[test]
    fn heuristic_trusts_a_title_consistent_low_frequency_brand_shaped_value() {
        let c = HeuristicCanonicalizer {
            min_frequency_for_trust: 25,
        };
        let titles = vec!["Aero Pure AP80RVLW Super Quiet 80 CFM Fan".to_string()];
        let e = evidence("Aero Pure", 3, &titles);
        assert_eq!(
            c.classify(&e),
            VocabularyClass::LegitimateNewEntity,
            "a short, title-prefix-consistent value should be trusted even at low frequency"
        );
    }

    #[test]
    fn heuristic_rejects_a_marketing_phrase_even_at_moderate_frequency() {
        let c = HeuristicCanonicalizer {
            min_frequency_for_trust: 25,
        };
        let titles = vec!["Funny Musician Gift Tee for Band Members".to_string()];
        let e = evidence("Funny Musician Gifts Co", 8, &titles);
        assert_eq!(
            c.classify(&e),
            VocabularyClass::LexicalOnlyNotStructural,
            "a marketing-shaped phrase with no title self-consistency should not be trusted structurally"
        );
    }

    #[test]
    fn heuristic_flags_junk_shaped_values() {
        let c = HeuristicCanonicalizer {
            min_frequency_for_trust: 25,
        };
        let titles = vec![];
        let e = evidence("#4 !! 2000 //", 1, &titles);
        assert_eq!(c.classify(&e), VocabularyClass::JunkMalformedWrongField);
    }

    #[test]
    fn heuristic_marks_genuinely_thin_evidence_as_ambiguous_not_a_forced_guess() {
        let c = HeuristicCanonicalizer {
            min_frequency_for_trust: 25,
        };
        let titles = vec![];
        // A plausible-shaped but unverified two-word value with no
        // title-consistency evidence and low frequency: not junk-shaped,
        // not marketing-shaped, but not confidently trustworthy either.
        let e = evidence("Kestrel Bindery", 1, &titles);
        assert_eq!(
            c.classify(&e),
            VocabularyClass::AmbiguousInsufficientEvidence
        );
    }

    #[test]
    fn trusted_as_structural_matches_the_two_positive_classes_only() {
        assert!(VocabularyClass::CanonicalKnownEntityOrAlias.trusted_as_structural());
        assert!(VocabularyClass::LegitimateNewEntity.trusted_as_structural());
        assert!(!VocabularyClass::LexicalOnlyNotStructural.trusted_as_structural());
        assert!(!VocabularyClass::AmbiguousInsufficientEvidence.trusted_as_structural());
        assert!(!VocabularyClass::JunkMalformedWrongField.trusted_as_structural());
    }
}
