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

/// A field that holds exactly one value per product/variant: two
/// *different* hard constraints on the same slot can never be jointly
/// satisfied by any real product (a product has exactly one brand, one
/// product type, one category; `AttributeValue::Enum` is single-valued per
/// attribute name), unlike e.g. two constraints on different attributes,
/// or `Constraint::MultiEnumContains` (deliberately multi-valued -- a
/// variant can carry several tags/features at once), which legitimately
/// combine.
#[derive(Debug, Clone, PartialEq, Eq)]
enum SingleValuedSlot {
    Brand,
    ProductType,
    Category,
    /// A variant-level `Constraint::Enum`, keyed by attribute name (e.g.
    /// "color") -- P2-E15 (`docs/experiments/PHASE2_LOG.md`): a real query,
    /// "skeleton toy", independently resolved two phrases to two different
    /// `color` values and hard-ANDed them, the same guaranteed-empty shape
    /// P2-E14 fixed for structural entities.
    Attribute(String),
}

fn single_valued_slot(c: &ResolvedConstraint) -> Option<SingleValuedSlot> {
    match c {
        ResolvedConstraint::Structural(StructuralConstraint::Brand(_))
        | ResolvedConstraint::Structural(StructuralConstraint::BrandAny(_)) => {
            Some(SingleValuedSlot::Brand)
        }
        ResolvedConstraint::Structural(StructuralConstraint::ProductType(_))
        | ResolvedConstraint::Structural(StructuralConstraint::ProductTypeAny(_)) => {
            Some(SingleValuedSlot::ProductType)
        }
        ResolvedConstraint::Structural(StructuralConstraint::Category(_)) => {
            Some(SingleValuedSlot::Category)
        }
        ResolvedConstraint::Attribute(Constraint::Enum { attribute, .. }) => {
            Some(SingleValuedSlot::Attribute(attribute.clone()))
        }
        _ => None,
    }
}

/// Phase 9 (Issue #34) defect fix, `docs/experiments/PHASE9_LOG.md` /
/// `PHASE9_DECISION.md`: converts a lexicon-derived hard `Constraint`
/// (never reached for the `size`/`under`/`over` keyword parses, which
/// push directly into `CommerceQuery::constraints` and never call
/// `apply_candidates` at all) into the additive ranking signal it becomes
/// once demoted by `compile`'s end-of-scan reconciliation pass, for
/// exactly the `Constraint` variants `cold_start::profile::compile_lexicon`
/// ever proposes via the open phrase-lexicon lookup (`Enum`,
/// `MultiEnumContains`, `Boolean`). `Numeric`/`Text` are handled
/// defensively (stringifying the value) rather than panicking, since
/// nothing in the domain model forbids a hand-authored test lexicon from
/// proposing them this way even though production code never does.
fn attribute_constraint_to_preference(c: &Constraint) -> Preference {
    let (attribute, value) = match c {
        Constraint::Enum { attribute, value }
        | Constraint::MultiEnumContains { attribute, value } => (attribute.clone(), value.clone()),
        Constraint::Boolean { attribute, value } => (attribute.clone(), value.to_string()),
        Constraint::Numeric {
            attribute, value, ..
        } => (attribute.clone(), value.to_string()),
        Constraint::Text {
            attribute,
            contains,
        } => (attribute.clone(), contains.clone()),
    };
    // Weight 0.5 matches this compiler's existing convention for a soft,
    // non-decisive signal (`fixtures::shoe_lexicon`'s hand-curated
    // "cushioned"/"breathable" `Preference::Boost`s use the same value) --
    // this constraint was resolved with full (1.0) confidence in its own
    // *value*, but demoted specifically because nothing in the query
    // corroborates that value as the shopper's actual filtering intent.
    Preference::Boost {
        attribute,
        value,
        weight: 0.5,
    }
}

/// Whether `c` identifies a real commerce entity (as opposed to a price
/// bound, which comes from a separate deterministic keyword parse and
/// establishes nothing about whether an attribute-word collision is real).
fn is_entity_constraint(c: &ResolvedConstraint) -> bool {
    matches!(
        c,
        ResolvedConstraint::Structural(
            StructuralConstraint::Brand(_)
                | StructuralConstraint::BrandAny(_)
                | StructuralConstraint::ProductType(_)
                | StructuralConstraint::ProductTypeAny(_)
                | StructuralConstraint::Category(_)
        )
    )
}

fn apply_candidates(
    result: &mut CommerceQuery,
    phrase: &str,
    candidates: &[Candidate],
    lexicon_attribute_matches: &mut Vec<(Constraint, String)>,
) {
    if let [only] = candidates {
        match &only.resolved {
            ResolvedTerm::Constraint(c) => {
                // Issue #6 P1-D / P2-E14 / P2-E15 (`docs/experiments/PHASE2_LOG.md`):
                // a real P1-D sweep found real query classes -- two independent
                // phrases in one query each unambiguously resolving to a
                // *different* value on the same single-valued field -- at
                // majority-to-total zero-result, because both became hard
                // constraints ANDed together (e.g. "harry potter lego"
                // compiling to `Brand(Harry Potter) AND Brand(Lego)`, or
                // "skeleton toy" compiling to `color=Skeleton AND
                // color=Toy`), which no product/variant can ever satisfy.
                // The first phrase to claim a single-valued slot (matching
                // this compiler's existing leftmost/longest-match-first
                // bias) keeps its hard constraint; a later phrase that would
                // conflict with an already-filled slot falls back to
                // residual free text instead of a second, mutually-
                // exclusive hard constraint -- this project's own
                // established structural-narrows/lexical-ranks-residual
                // contract, not a new mechanism.
                let conflicts = single_valued_slot(c).is_some_and(|slot| {
                    result.constraints.iter().any(|existing| {
                        single_valued_slot(existing) == Some(slot.clone()) && existing != c
                    })
                });
                if conflicts {
                    result.residual_lexical.push(phrase.to_string());
                } else {
                    // Phase 9 (Issue #34): remember every lexicon-derived
                    // *attribute* constraint (never a real entity match --
                    // those are trustworthy the moment they resolve) so
                    // `compile`'s end-of-scan pass can demote it if no
                    // entity ever corroborates it anywhere in the query.
                    if let ResolvedConstraint::Attribute(inner) = c {
                        lexicon_attribute_matches.push((inner.clone(), phrase.to_string()));
                    }
                    result.constraints.push(c.clone());
                }
            }
            // Issue #6 P1-B (`docs/experiments/PHASE2_LOG.md`): a real-data
            // replay found that resolving a phrase to *only* a Preference
            // consumed it exactly like a hard Constraint would -- removing
            // it from `residual_lexical` and therefore from what a lexical
            // delegate ever sees -- even though a Preference explicitly
            // does not enforce anything. For a low-confidence signal (this
            // phase's fuzzy brand match; before this phase, no candidate
            // pool ever produced a real Preference at all, so this path
            // was untested) that trade is strictly bad: real retrieval
            // signal is given up for a ranking-only boost that only
            // applies within whatever candidate set results from
            // everything *else* in the query. A Preference is additive,
            // not exclusive: keep the phrase searchable.
            ResolvedTerm::Preference(p) => {
                result.preferences.push(p.clone());
                result.residual_lexical.push(phrase.to_string());
            }
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
    // Phase 9 (Issue #34) defect fix: every hard `Constraint` resolved via
    // the open phrase-lexicon lookup below (as opposed to the `size`/
    // `under`/`over` keyword parses a few lines down, which push directly
    // into `result.constraints` and never touch this list), tracked so the
    // end-of-scan reconciliation pass can demote it if the query never
    // resolves a real entity anywhere.
    let mut lexicon_attribute_matches: Vec<(Constraint, String)> = Vec::new();
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
                apply_candidates(
                    &mut result,
                    &phrase,
                    candidates,
                    &mut lexicon_attribute_matches,
                );
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

    // Phase 9 (Issue #34) defect fix, applied once at the end of the scan
    // rather than per-token: a lexicon-derived attribute constraint is
    // trustworthy as a hard filter only when corroborated by a real entity
    // constraint somewhere in the *whole* query. Without one, every such
    // constraint is demoted to an additive `Preference::Boost` and its
    // phrase kept searchable, instead of silently excluding nearly every
    // relevant product on a confident but coincidental word match (see
    // this module's `attribute_constraint_to_preference`/
    // `is_entity_constraint` doc comments, and
    // `docs/experiments/PHASE9_LOG.md` P9-E03/P9-E04 for the real-data
    // evidence this rule was derived from).
    let has_entity_constraint = result.constraints.iter().any(is_entity_constraint);
    if !has_entity_constraint {
        for (inner, phrase) in lexicon_attribute_matches {
            let target = ResolvedConstraint::Attribute(inner.clone());
            if let Some(pos) = result.constraints.iter().position(|c| *c == target) {
                result.constraints.remove(pos);
                result
                    .preferences
                    .push(attribute_constraint_to_preference(&inner));
                if !result.residual_lexical.contains(&phrase) {
                    result.residual_lexical.push(phrase);
                }
            }
        }
    }

    result
}
