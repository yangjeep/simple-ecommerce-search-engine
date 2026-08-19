use super::lexicon::{Candidate, ResolvedTerm, SemanticLexicon};
use super::structural::{ResolvedConstraint, StructuralConstraint};
use crate::domain::{
    effective_attributes, Catalog, Constraint, NumericOp, Product, ProductId, Variant, VariantId,
};

/// A soft ranking signal compiled from the query but not enforced as a
/// hard filter. Gate 2 only compiles preferences; consuming them for
/// ranking is Gate 3's top-K ranking work.
#[derive(Debug, Clone, PartialEq)]
pub enum Preference {
    Boost {
        attribute: String,
        value: String,
        weight: f64,
    },
    /// Issue #6 P1-B: a lower-confidence structural match (e.g. a brand
    /// string only fuzzy/edit-distance-close to a trusted alias group,
    /// `cold_start::profile::compile_lexicon_with_alias_enforcement`'s
    /// tier 2) — real enough to rank higher, not confident enough to
    /// filter out every candidate that fails it.
    StructuralBoost(StructuralConstraint, f64),
}

/// A query phrase with more than one plausible reading. Per `CLAUDE.md`
/// ("preserve ambiguity explicitly when confidence is insufficient"), the
/// compiler records every candidate instead of silently picking one.
#[derive(Debug, Clone, PartialEq)]
pub struct AmbiguousSpan {
    pub text: String,
    pub candidates: Vec<Candidate>,
}

/// The typed Commerce IR a free-text query compiles into (Gate 2).
#[derive(Debug, Default, Clone, PartialEq)]
pub struct CommerceQuery {
    pub constraints: Vec<ResolvedConstraint>,
    pub preferences: Vec<Preference>,
    pub ambiguous: Vec<AmbiguousSpan>,
    pub residual_lexical: Vec<String>,
}

impl CommerceQuery {
    /// Whether every hard constraint holds for one specific variant's
    /// combined attributes/structural fields. Extracted from `execute` so
    /// `plan::execute_planned` can re-verify a delegate-returned hit against
    /// the exact same constraint semantics `execute`'s linear scan and
    /// `CatalogIndex::execute`'s narrow-then-verify path already use --
    /// there must be exactly one place hard-constraint matching is defined.
    pub fn matches_variant(&self, product: &Product, variant: &Variant) -> bool {
        let attrs = effective_attributes(product, variant);
        self.constraints.iter().all(|c| match c {
            ResolvedConstraint::Attribute(constraint) => constraint.matches(&attrs),
            ResolvedConstraint::Structural(structural) => structural.matches(product, variant),
        })
    }

    /// Evaluate every hard constraint against each variant's combined
    /// attributes/structural fields. Ambiguous spans and residual lexical
    /// terms are not enforced here: resolving them needs a lexical index
    /// (Gate 3) or a control-plane decision (Gate 5), neither of which
    /// exist yet.
    pub fn execute(&self, catalog: &Catalog) -> Vec<(ProductId, VariantId)> {
        let mut matches = Vec::new();
        for product in &catalog.products {
            for variant in &product.variants {
                if self.matches_variant(product, variant) {
                    matches.push((product.id, variant.id));
                }
            }
        }
        matches
    }
}

// "and"/"to"/"of"/"on" added per Round 1 R1-E06 (docs/experiments/ROUND1_LOG.md):
// the top real unresolved terms across a real 22,458-query corpus were
// overwhelmingly these four function words, not missing vocabulary.
// "or" is deliberately NOT included — see R1-E03: dropping it silently
// would hide disjunction queries' mishandling rather than surface it.
const STOPWORDS: &[&str] = &["a", "the", "for", "with", "in", "and", "to", "of", "on"];

// Round 1 R1-E03/P2-E04 (docs/experiments/ROUND1_LOG.md,
// docs/experiments/PHASE2_LOG.md): a phrase immediately following one of
// these must not become a positive constraint/preference (see the
// negation-handling block in `compile` below). Includes the split forms
// `split_whitespace` produces for common contractions ("aren't" stays one
// token; "don't"/"doesn't"/"isn't" likewise).
const NEGATION_WORDS: &[&str] = &[
    "not", "no", "without", "non", "aren't", "isn't", "don't", "doesn't",
];

fn parse_money(token: Option<&str>) -> Option<i64> {
    let token = token?;
    let digits = token.strip_prefix('$').unwrap_or(token);
    let dollars: f64 = digits.parse().ok()?;
    Some((dollars * 100.0).round() as i64)
}

fn apply_candidates(result: &mut CommerceQuery, phrase: &str, candidates: &[Candidate]) {
    if let [only] = candidates {
        match &only.resolved {
            ResolvedTerm::Constraint(c) => result.constraints.push(c.clone()),
            ResolvedTerm::Preference(p) => result.preferences.push(p.clone()),
        }
    } else {
        result.ambiguous.push(AmbiguousSpan {
            text: phrase.to_string(),
            candidates: candidates.to_vec(),
        });
    }
}

/// Compile free text into a [`CommerceQuery`] against a fixed
/// [`SemanticLexicon`]. Deterministic: no model call, no randomness, no
/// silent flattening of an ambiguous phrase into a single guessed
/// constraint.
pub fn compile(text: &str, lexicon: &SemanticLexicon) -> CommerceQuery {
    let tokens: Vec<String> = text.split_whitespace().map(str::to_lowercase).collect();
    let mut result = CommerceQuery::default();
    let max_window = lexicon.max_phrase_words().max(1);
    let mut i = 0;
    while i < tokens.len() {
        if tokens[i] == "size" {
            if let Some(n) = tokens.get(i + 1).and_then(|t| t.parse::<f64>().ok()) {
                result
                    .constraints
                    .push(ResolvedConstraint::Attribute(Constraint::Numeric {
                        attribute: "size".to_string(),
                        op: NumericOp::Eq,
                        value: n,
                    }));
                i += 2;
                continue;
            }
        }
        if tokens[i] == "under" {
            if let Some(cents) = parse_money(tokens.get(i + 1).map(String::as_str)) {
                result.constraints.push(ResolvedConstraint::Structural(
                    StructuralConstraint::PriceUnderCents(cents),
                ));
                i += 2;
                continue;
            }
        }
        if tokens[i] == "over" {
            if let Some(cents) = parse_money(tokens.get(i + 1).map(String::as_str)) {
                result.constraints.push(ResolvedConstraint::Structural(
                    StructuralConstraint::PriceOverCents(cents),
                ));
                i += 2;
                continue;
            }
        }

        // Round 1 R1-E03 (docs/experiments/ROUND1_LOG.md): the compiler
        // used to treat every recognized phrase as an unconditional
        // positive assertion, so "not red" compiled to a REQUIRED red
        // constraint -- the exact opposite of stated intent, the single
        // most severe correctness defect that round found. A negation
        // marker immediately before a recognized phrase must never let
        // that phrase become a hard constraint or preference; the safest
        // correct behavior with no negated-constraint representation in
        // the domain model yet is to surface the phrase as residual (seen,
        // deliberately not resolved) rather than silently drop it or
        // resolve it positively.
        if NEGATION_WORDS.contains(&tokens[i].as_str()) {
            let after = i + 1;
            let mut consumed = 1;
            if after < tokens.len() {
                let window_cap = max_window.min(tokens.len() - after);
                for window in (1..=window_cap).rev() {
                    let phrase = tokens[after..after + window].join(" ");
                    if lexicon.resolve(&phrase).is_some() {
                        result.residual_lexical.push(phrase);
                        consumed = 1 + window;
                        break;
                    }
                }
            }
            i += consumed;
            continue;
        }

        let window_cap = max_window.min(tokens.len() - i);
        let mut matched = false;
        for window in (1..=window_cap).rev() {
            let phrase = tokens[i..i + window].join(" ");
            if let Some(candidates) = lexicon.resolve(&phrase) {
                apply_candidates(&mut result, &phrase, candidates);
                i += window;
                matched = true;
                break;
            }
        }
        if matched {
            continue;
        }

        if STOPWORDS.contains(&tokens[i].as_str()) {
            i += 1;
            continue;
        }

        result.residual_lexical.push(tokens[i].clone());
        i += 1;
    }
    result
}
