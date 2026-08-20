//! Issue #6 P1-B: brand-string *identity* normalization for the
//! alias-normalized enforcement experiment
//! (`cold_start::profile::compile_lexicon_with_alias_enforcement`). This is
//! deliberately **not** a canonicalization/trust-classification strategy —
//! Issue #9 already answered "which strings are trustworthy enough to
//! trust at all" three independent ways
//! (`docs/experiments/PHASE2_LOG.md` P2-E07-E10, decision: CANONICALIZATION
//! FRONTIER IS FUNDAMENTAL). These functions only decide whether two
//! strings that already passed that trust gate denote the same real-world
//! brand, given how each happens to be written.

const CORPORATE_SUFFIXES: &[&str] = &[
    "inc",
    "incorporated",
    "llc",
    "ltd",
    "limited",
    "co",
    "company",
    "corp",
    "corporation",
    "group",
    "intl",
    "international",
];

/// Deterministic identity key for a lowercased brand string: punctuation
/// becomes whitespace, then any *trailing* corporate/legal-suffix tokens
/// are stripped repeatedly — "nike, inc.", "nike inc", and "nike" all key
/// to "nike". Deliberately conservative: only trailing tokens are ever
/// stripped, never a token appearing mid-name, to limit false-merge risk
/// (a brand whose own name legitimately contains e.g. "group" as a
/// non-suffix word is left untouched). Falls back to the trimmed original
/// if stripping would remove every token, so a brand legitimately named
/// just "Company" or "Group" never collapses to an empty key.
///
/// Known, accepted limitation: a real brand literally named e.g. "The
/// Group" would still collapse to "the" (its only non-suffix token) —
/// rare in practice for real ecommerce brand fields, not specifically
/// checked against real data here, recorded rather than hidden.
pub fn alias_key(name_lower: &str) -> String {
    let cleaned: String = name_lower
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect();
    let mut tokens: Vec<&str> = cleaned.split_whitespace().collect();
    while tokens.len() > 1 {
        let last = *tokens.last().expect("len > 1 checked above");
        if CORPORATE_SUFFIXES.contains(&last) {
            tokens.pop();
        } else {
            break;
        }
    }
    if tokens.is_empty() {
        return name_lower.trim().to_string();
    }
    tokens.join(" ")
}

/// Standard Levenshtein edit distance (insert/delete/substitute, unit
/// cost), computed over `char`s rather than bytes so it stays correct for
/// non-ASCII input. O(len(a) * len(b)) time via the usual two-row rolling
/// DP — more than fast enough for the short brand-name strings this
/// module compares against a caller-bounded candidate set (never every
/// raw catalog string — see `compile_lexicon_with_alias_enforcement`'s
/// own doc comment for why bounding the candidate set matters).
pub fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut curr = vec![0usize; b.len() + 1];
    for i in 1..=a.len() {
        curr[0] = i;
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            curr[j] = (prev[j] + 1).min(curr[j - 1] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alias_key_merges_common_corporate_suffix_variants() {
        assert_eq!(alias_key("nike"), "nike");
        assert_eq!(alias_key("nike inc"), "nike");
        assert_eq!(alias_key("nike, inc."), "nike");
        assert_eq!(alias_key("nike inc."), "nike");
        assert_eq!(alias_key("nike llc"), "nike");
        assert_eq!(alias_key("nike corporation"), "nike");
    }

    #[test]
    fn alias_key_does_not_merge_a_different_real_brand() {
        assert_ne!(alias_key("nike"), alias_key("nikon"));
        assert_ne!(alias_key("nike"), alias_key("nike air"));
    }

    #[test]
    fn alias_key_only_strips_trailing_suffix_tokens_not_mid_name_words() {
        // "Group" is part of the actual brand name here, not a legal
        // suffix -- it is not trailing, so it must survive intact.
        assert_eq!(alias_key("group fitness gear"), "group fitness gear");
    }

    #[test]
    fn alias_key_never_collapses_to_empty_for_a_name_that_is_only_a_suffix_word() {
        assert_eq!(alias_key("company"), "company");
        assert_eq!(alias_key("group"), "group");
    }

    #[test]
    fn edit_distance_matches_known_values() {
        assert_eq!(edit_distance("nike", "nike"), 0);
        assert_eq!(edit_distance("nike", "nikee"), 1);
        assert_eq!(edit_distance("adidas", "adiddas"), 1);
        assert_eq!(edit_distance("nike", "nikon"), 2);
        assert_eq!(edit_distance("", "abc"), 3);
    }
}
