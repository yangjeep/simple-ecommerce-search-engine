//! Issue #6's multi-round research campaign asks for a *stable* query-class
//! taxonomy, reported per-class and as a traffic-weighted aggregate, so a
//! 5x win on one narrow class can't get silently generalized into "the
//! engine is 5x faster." This is a materially different taxonomy from
//! `classify::QueryClass` (R1-E02's resolution-state taxonomy: did the
//! compiler resolve this query at all, and how): this one classifies by
//! *structural predicate shape*, matching Issue #6's own execution-mode
//! dimensions (structural-only / narrow-then-rank / lexical-first) so a
//! permutation-matrix experiment can report "which shapes does method X
//! help" rather than one aggregate number.
//!
//! Both taxonomies coexist deliberately -- `classify::classify` answers
//! "did semantic interpretation succeed," this answers "what does the
//! resulting *plan* look like, and is it interesting to route
//! differently." A caller building a permutation-matrix report typically
//! wants both.

use std::collections::HashSet;

use commerce_core::ir::{CommerceQuery, ResolvedConstraint, StructuralConstraint};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum QueryClass9 {
    /// Exactly one structural *entity* constraint (Brand/BrandAny/
    /// ProductType/Category), nothing else -- the cleanest, most precise
    /// single-entity resolution. A literal pasted product-id-shaped query
    /// is treated as a degenerate case of this class, not a separate one
    /// (Issue #6's list names 9 classes, not 10).
    StructuralExactEntity,
    /// Two or more structural *entity* constraints combined (e.g. brand +
    /// product_type), no numeric range, no attribute-level (variant-
    /// scoped) constraint, no residual text.
    SelectiveMultiAttribute,
    /// At least one variant-level attribute constraint (`Constraint::Enum`/
    /// `MultiEnumContains`/`Boolean`/`Numeric`/`Text` -- in this project's
    /// domain model these match against `effective_attributes`, the
    /// product+variant merge, so e.g. `color` is a per-variant
    /// distinguishing signal even though it is stored generically), no
    /// numeric price range, no residual text.
    VariantScoped,
    /// A numeric price-range constraint (`PriceUnderCents`/
    /// `PriceOverCents`) combined with the query being otherwise fully
    /// resolved (no residual text) -- range constraints are checked
    /// before the entity/attribute buckets since they're the more
    /// specific, information-bearing shape Issue #6 names explicitly.
    RangeStructural,
    /// At least one structural constraint of any kind plus leftover free
    /// text (`residual_lexical`), not occasion-flavored.
    StructuralLexicalResidual,
    /// Same shape as `StructuralLexicalResidual`, but the residual text is
    /// occasion/semantic-flavored (gift, birthday, wedding, ...) -- this
    /// project has no vector backend yet, so occasion-word detection
    /// (the same heuristic `classify::classify`'s `SemanticOccasion`
    /// class already uses) is the deliberate, documented proxy for "would
    /// plausibly benefit from a semantic/vector residual leg rather than
    /// pure lexical," not a claim that vector retrieval has been tested.
    StructuralSemanticResidual,
    /// No structural constraint at all, but real free text to search on.
    LexicalFirst,
    /// The compiler recorded genuine ambiguity (`compiled.ambiguous`), or
    /// the query resolved to nothing usable at all (no constraints, no
    /// preferences, no residual text) -- a true punt with no signal to
    /// route on either way.
    AmbiguousPunt,
    /// A long, mostly-unresolved query: at least 4 raw tokens, and at
    /// least 75% of them ended up as unresolved residual text. Real
    /// ecommerce query logs have a long tail of rambling, misspelled, or
    /// unusually specific queries distinct from a normal 2-3-word
    /// "lexical-first" query -- checked before the residual buckets above
    /// so this more specific shape takes precedence.
    LongTailNoisy,
}

impl QueryClass9 {
    pub fn label(self) -> &'static str {
        match self {
            QueryClass9::StructuralExactEntity => "structural_exact_entity",
            QueryClass9::SelectiveMultiAttribute => "selective_multi_attribute_structural",
            QueryClass9::VariantScoped => "variant_scoped_structural",
            QueryClass9::RangeStructural => "range_plus_structural",
            QueryClass9::StructuralLexicalResidual => "structural_plus_lexical_residual",
            QueryClass9::StructuralSemanticResidual => "structural_plus_semantic_residual",
            QueryClass9::LexicalFirst => "lexical_first",
            QueryClass9::AmbiguousPunt => "ambiguous_punt",
            QueryClass9::LongTailNoisy => "long_tail_noisy",
        }
    }

    pub fn all() -> [QueryClass9; 9] {
        [
            QueryClass9::StructuralExactEntity,
            QueryClass9::SelectiveMultiAttribute,
            QueryClass9::VariantScoped,
            QueryClass9::RangeStructural,
            QueryClass9::StructuralLexicalResidual,
            QueryClass9::StructuralSemanticResidual,
            QueryClass9::LexicalFirst,
            QueryClass9::AmbiguousPunt,
            QueryClass9::LongTailNoisy,
        ]
    }
}

// Duplicated from `classify.rs` deliberately (not imported): the two
// taxonomies are allowed to diverge independently, and this is a small
// enough list that sharing it would create a coupling neither taxonomy's
// own evolution should be constrained by.
const OCCASION_WORDS: &[&str] = &[
    "gift",
    "gifts",
    "birthday",
    "wedding",
    "anniversary",
    "christmas",
    "valentine",
    "valentines",
    "mother's",
    "mothers",
    "father's",
    "fathers",
    "graduation",
    "baby",
    "shower",
    "halloween",
    "thanksgiving",
    "easter",
    "housewarming",
    "retirement",
];

fn is_occasion_query(query: &str) -> bool {
    let lower = query.to_lowercase();
    OCCASION_WORDS
        .iter()
        .any(|w| lower.split_whitespace().any(|t| t == *w))
}

fn is_structural_entity(c: &ResolvedConstraint) -> bool {
    matches!(
        c,
        ResolvedConstraint::Structural(
            StructuralConstraint::Brand(_)
                | StructuralConstraint::BrandAny(_)
                | StructuralConstraint::ProductType(_)
                | StructuralConstraint::Category(_)
        )
    )
}

fn is_range(c: &ResolvedConstraint) -> bool {
    matches!(
        c,
        ResolvedConstraint::Structural(
            StructuralConstraint::PriceUnderCents(_) | StructuralConstraint::PriceOverCents(_)
        )
    )
}

fn is_attribute(c: &ResolvedConstraint) -> bool {
    matches!(c, ResolvedConstraint::Attribute(_))
}

/// Classify one compiled query by structural predicate shape. Mirrors
/// `classify::classify`'s `(query, compiled)` signature deliberately, so
/// an eval binary can call both side by side against the same compiled
/// query without restructuring its loop.
pub fn classify9(query: &str, compiled: &CommerceQuery) -> QueryClass9 {
    if !compiled.ambiguous.is_empty()
        || (compiled.constraints.is_empty()
            && compiled.preferences.is_empty()
            && compiled.residual_lexical.is_empty())
    {
        return QueryClass9::AmbiguousPunt;
    }

    let token_count = query.split_whitespace().count().max(1);
    let has_residual = !compiled.residual_lexical.is_empty();
    let residual_fraction = compiled.residual_lexical.len() as f64 / token_count as f64;

    if has_residual && token_count >= 4 && residual_fraction >= 0.75 {
        return QueryClass9::LongTailNoisy;
    }

    let entity_count = compiled
        .constraints
        .iter()
        .filter(|c| is_structural_entity(c))
        .count();
    let range_count = compiled.constraints.iter().filter(|c| is_range(c)).count();
    let attribute_count = compiled
        .constraints
        .iter()
        .filter(|c| is_attribute(c))
        .count();
    let total_structural = entity_count + range_count + attribute_count;

    if total_structural >= 1 && has_residual {
        return if is_occasion_query(query) {
            QueryClass9::StructuralSemanticResidual
        } else {
            QueryClass9::StructuralLexicalResidual
        };
    }

    if total_structural >= 1 && !has_residual {
        if range_count >= 1 {
            return QueryClass9::RangeStructural;
        }
        if attribute_count >= 1 {
            return QueryClass9::VariantScoped;
        }
        if entity_count >= 2 {
            return QueryClass9::SelectiveMultiAttribute;
        }
        return QueryClass9::StructuralExactEntity;
    }

    if total_structural == 0 && has_residual {
        return QueryClass9::LexicalFirst;
    }

    // Fallback: no constraint of any kind, no residual text, but a
    // Preference did resolve (e.g. P1-B tier 2's fuzzy brand match on a
    // query consisting solely of that one term) -- real signal exists,
    // but nothing hard to route a structural/range/attribute plan on and
    // no free text left to search either. Documented, not silently
    // swallowed: see this module's own doc comment.
    QueryClass9::AmbiguousPunt
}

/// Traffic-weighted counts across a batch, for the "report each class
/// separately as well as traffic-weighted aggregate results" requirement.
#[derive(Debug, Default, Clone)]
pub struct QueryClass9Counts {
    pub counts: std::collections::BTreeMap<QueryClass9, usize>,
    pub total: usize,
}

impl QueryClass9Counts {
    pub fn record(&mut self, class: QueryClass9) {
        *self.counts.entry(class).or_insert(0) += 1;
        self.total += 1;
    }

    pub fn fraction(&self, class: QueryClass9) -> f64 {
        if self.total == 0 {
            return 0.0;
        }
        *self.counts.get(&class).unwrap_or(&0) as f64 / self.total as f64
    }

    pub fn print(&self) {
        println!("=== 9-class query taxonomy (Issue #6 research campaign) ===");
        println!("total queries: {}", self.total);
        for class in QueryClass9::all() {
            let n = *self.counts.get(&class).unwrap_or(&0);
            println!(
                "  {:<34} {:>7}  ({:.1}%)",
                class.label(),
                n,
                self.fraction(class) * 100.0
            );
        }
        println!();
    }
}

/// Not itself used by `classify9` (kept the function pure/deterministic
/// over its two arguments), but every eval binary building a real
/// `known_product_ids` set for `classify::classify` already has one --
/// exposed so a caller can additionally note the (rare) literal-ASIN
/// case if it wants a finer report, without this module depending on it.
pub fn is_literal_id_lookup(query: &str, known_product_ids: &HashSet<&str>) -> bool {
    known_product_ids.contains(query.trim())
}

#[cfg(test)]
mod tests {
    use super::*;
    use commerce_core::domain::{BrandId, CategoryId, Constraint, NumericOp, ProductTypeId};
    use commerce_core::ir::{AmbiguousSpan, Preference};

    fn q(
        constraints: Vec<ResolvedConstraint>,
        preferences: Vec<Preference>,
        residual: Vec<&str>,
    ) -> CommerceQuery {
        CommerceQuery {
            constraints,
            preferences,
            ambiguous: vec![],
            residual_lexical: residual.into_iter().map(String::from).collect(),
        }
    }

    #[test]
    fn single_brand_constraint_is_structural_exact_entity() {
        let query = q(
            vec![ResolvedConstraint::Structural(StructuralConstraint::Brand(
                BrandId(1),
            ))],
            vec![],
            vec![],
        );
        assert_eq!(
            classify9("nike", &query),
            QueryClass9::StructuralExactEntity
        );
    }

    #[test]
    fn brand_plus_product_type_is_selective_multi_attribute() {
        let query = q(
            vec![
                ResolvedConstraint::Structural(StructuralConstraint::Brand(BrandId(1))),
                ResolvedConstraint::Structural(StructuralConstraint::ProductType(ProductTypeId(2))),
            ],
            vec![],
            vec![],
        );
        assert_eq!(
            classify9("nike sneaker", &query),
            QueryClass9::SelectiveMultiAttribute
        );
    }

    #[test]
    fn color_attribute_alone_is_variant_scoped() {
        let query = q(
            vec![ResolvedConstraint::Attribute(Constraint::Enum {
                attribute: "color".to_string(),
                value: "white".to_string(),
            })],
            vec![],
            vec![],
        );
        assert_eq!(classify9("white", &query), QueryClass9::VariantScoped);
    }

    #[test]
    fn brand_plus_price_range_is_range_structural_not_multi_attribute() {
        let query = q(
            vec![
                ResolvedConstraint::Structural(StructuralConstraint::Brand(BrandId(1))),
                ResolvedConstraint::Structural(StructuralConstraint::PriceUnderCents(5000)),
            ],
            vec![],
            vec![],
        );
        assert_eq!(
            classify9("nike under $50", &query),
            QueryClass9::RangeStructural
        );
    }

    #[test]
    fn structural_plus_residual_without_occasion_word() {
        let query = q(
            vec![ResolvedConstraint::Structural(StructuralConstraint::Brand(
                BrandId(1),
            ))],
            vec![],
            vec!["comfortable", "running"],
        );
        assert_eq!(
            classify9("nike comfortable running", &query),
            QueryClass9::StructuralLexicalResidual
        );
    }

    #[test]
    fn structural_plus_residual_with_occasion_word_is_semantic_residual() {
        let query = q(
            vec![ResolvedConstraint::Structural(StructuralConstraint::Brand(
                BrandId(1),
            ))],
            vec![],
            vec!["gift"],
        );
        assert_eq!(
            classify9("nike gift", &query),
            QueryClass9::StructuralSemanticResidual
        );
    }

    #[test]
    fn no_structural_but_real_residual_is_lexical_first() {
        let query = q(vec![], vec![], vec!["comfortable", "shoes"]);
        assert_eq!(
            classify9("comfortable shoes", &query),
            QueryClass9::LexicalFirst
        );
    }

    #[test]
    fn ambiguous_span_always_wins_regardless_of_other_signal() {
        let mut query = q(
            vec![ResolvedConstraint::Structural(StructuralConstraint::Brand(
                BrandId(1),
            ))],
            vec![],
            vec![],
        );
        query.ambiguous.push(AmbiguousSpan {
            text: "polo".to_string(),
            candidates: vec![],
        });
        assert_eq!(classify9("polo shirt", &query), QueryClass9::AmbiguousPunt);
    }

    #[test]
    fn totally_empty_query_is_ambiguous_punt() {
        let query = q(vec![], vec![], vec![]);
        assert_eq!(classify9("", &query), QueryClass9::AmbiguousPunt);
    }

    #[test]
    fn long_mostly_unresolved_query_is_long_tail_noisy() {
        let query = q(
            vec![],
            vec![],
            vec!["xyzzy", "flibbertigibbet", "unusual", "rare", "term"],
        );
        assert_eq!(
            classify9("xyzzy flibbertigibbet unusual rare term", &query),
            QueryClass9::LongTailNoisy
        );
    }

    #[test]
    fn short_residual_query_is_lexical_first_not_long_tail() {
        // Only 2 tokens: below LongTailNoisy's token_count>=4 gate even
        // though residual_fraction is 100%.
        let query = q(vec![], vec![], vec!["red", "shoes"]);
        assert_eq!(classify9("red shoes", &query), QueryClass9::LexicalFirst);
    }

    #[test]
    fn preference_only_resolution_falls_back_to_ambiguous_punt() {
        let query = q(
            vec![],
            vec![Preference::StructuralBoost(
                StructuralConstraint::Brand(BrandId(1)),
                0.5,
            )],
            vec![],
        );
        assert_eq!(classify9("nikee", &query), QueryClass9::AmbiguousPunt);
    }

    #[test]
    fn numeric_attribute_constraint_counts_as_variant_scoped_not_multi_attribute() {
        let query = q(
            vec![ResolvedConstraint::Attribute(Constraint::Numeric {
                attribute: "size".to_string(),
                op: NumericOp::Eq,
                value: 9.0,
            })],
            vec![],
            vec![],
        );
        assert_eq!(classify9("size 9", &query), QueryClass9::VariantScoped);
    }

    #[test]
    fn category_alone_is_structural_exact_entity() {
        let query = q(
            vec![ResolvedConstraint::Structural(
                StructuralConstraint::Category(CategoryId(3)),
            )],
            vec![],
            vec![],
        );
        assert_eq!(
            classify9("shoes", &query),
            QueryClass9::StructuralExactEntity
        );
    }

    #[test]
    fn all_returns_exactly_nine_distinct_classes() {
        let all = QueryClass9::all();
        let unique: HashSet<QueryClass9> = all.iter().copied().collect();
        assert_eq!(unique.len(), 9);
    }
}
