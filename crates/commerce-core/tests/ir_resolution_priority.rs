//! Phase 9 (Issue #34) defect-fix cycle: `compile()`'s resolution-priority
//! defect localized by P9-E03/P9-E04
//! (`docs/experiments/PHASE9_LOG.md`, `PHASE9_DECISION.md`).
//!
//! **Old behavior**: a phrase that resolves to a single-token
//! attribute-level `Candidate` (e.g. a color/material enum value from
//! `cold_start::profile::compile_lexicon`) became an unconditional hard
//! `ResolvedConstraint::Attribute` the moment it was the *longest available
//! match at that scan position* -- with no regard for whether the rest of
//! the query ever resolved a real entity (`ProductType`/`Category`/`Brand`)
//! at all. On real WANDS traffic this produced badly wrong hard filters:
//! e.g. "smart coffee table" never matches the real `product_class`
//! ("Coffee & Cocktail Tables") verbatim, so the scan fell through to the
//! single token "coffee", which the profiler had already seeded as a
//! `color` value from unrelated products -- and compiled a confident
//! `color=coffee` hard filter that excludes nearly every genuinely
//! relevant coffee table. P9-E04 measured this precisely: queries with a
//! real entity constraint recalled 47.6% of Exact-labeled ground truth vs.
//! 7.2% for attribute-only queries, a 6.6x gap explained by exactly this
//! mechanism.
//!
//! **Intended behavior**: a hard attribute constraint that arose purely
//! from the open, collision-prone phrase-lexicon lookup (never the
//! deterministic `size`/`under`/`over` keyword parses, which cannot
//! collide) is only trustworthy as an *exclusionary filter* when at least
//! one real entity constraint (`Brand`/`BrandAny`/`ProductType`/`Category`)
//! is also present elsewhere in the same compiled query, corroborating
//! that the query is really about a specific commerce entity. When no such
//! entity constraint exists anywhere in the query, every lexicon-derived
//! attribute constraint is demoted to a `Preference::Boost` (additive
//! ranking signal, never a filter) and its originating phrase is kept in
//! `residual_lexical` -- the same "additive, not exclusive" contract this
//! compiler already applies to low-confidence `Preference` candidates
//! (see `apply_candidates`'s existing P1-B comment). The phrase is never
//! silently dropped.
//!
//! **Why the old priority was incorrect**: the scan's longest-match-first
//! rule inside one fixed starting position is a reasonable disambiguation
//! rule *between two candidate readings of the same span*, but it was
//! silently promoted into a claim it never earned: "the longest phrase
//! that matched anywhere is exactly as trustworthy as any other". A real,
//! multi-word entity phrase that fails to match verbatim (P9-E03: this is
//! the common case, not the exception, on real shopper vocabulary) leaves
//! nothing behind to compare against -- so a coincidental single-token
//! collision was accepted with the same unconditional confidence as a
//! genuine multi-word entity match, even though shorter spans are
//! categorically more collision-prone against an open, uncurated
//! vocabulary of colors/materials/features.
//!
//! **Why the new rule generalizes**: it does not make "entities always
//! win" -- an attribute constraint that coexists with a real entity
//! constraint is left exactly as before (a hard filter), and a lone
//! attribute constraint is never deleted, only demoted to an additive
//! preference plus residual text, so a query whose *entire* real intent is
//! an attribute value (no entity present because none exists, e.g. a bare
//! color word) still surfaces that value as ranking signal and keeps it
//! lexically searchable -- exactly the "legitimate attribute resolution"
//! case `PHASE9_DECISION.md` named as the risk to guard against. The rule
//! is keyed only on *how* a constraint was resolved (open lexicon lookup
//! vs. a reserved deterministic keyword) and *what else* the query
//! resolved, not on which specific attribute/value pair is involved, so it
//! is not a special case for "coffee"/"chrome"/"pearl" or any other
//! disclosed example.

use commerce_core::domain::{BrandId, CategoryId, Constraint, ProductTypeId};
use commerce_core::ir::SemanticLexicon;
use commerce_core::ir::{compile, Candidate, Preference, ResolvedConstraint, StructuralConstraint};

fn lexicon_with(entries: &[(&str, Candidate)]) -> SemanticLexicon {
    let mut lex = SemanticLexicon::new();
    for (phrase, candidate) in entries {
        lex.insert(phrase, vec![candidate.clone()]);
    }
    lex
}

fn color(attribute: &str, value: &str) -> Candidate {
    Candidate::constraint(
        ResolvedConstraint::Attribute(Constraint::Enum {
            attribute: attribute.to_string(),
            value: value.to_string(),
        }),
        1.0,
    )
}

/// The smallest realistic reproduction of the localized defect: a query
/// whose real intended entity ("coffee table") never appears verbatim in
/// the lexicon, so the scan falls through to a single coincidental
/// attribute-value collision ("coffee" as a color). Mirrors the real
/// P9-E04 example verbatim (`docs/experiments/PHASE9_LOG.md` P9-E04).
#[test]
fn coincidental_attribute_collision_with_no_entity_is_demoted_not_a_hard_filter() {
    let lex = lexicon_with(&[("coffee", color("color", "coffee"))]);

    let query = compile("smart coffee table", &lex);

    assert!(
        query.constraints.is_empty(),
        "a lone coincidental attribute match with no corroborating entity \
         must never become a hard filter: {:?}",
        query.constraints
    );
    assert_eq!(
        query.preferences,
        vec![Preference::Boost {
            attribute: "color".to_string(),
            value: "coffee".to_string(),
            weight: 0.5,
        }],
        "the demoted match must survive as an additive ranking signal, not vanish"
    );
    assert!(
        query.residual_lexical.contains(&"coffee".to_string()),
        "the demoted phrase must stay lexically searchable: {:?}",
        query.residual_lexical
    );
    assert!(query.ambiguous.is_empty());
}

/// The same coincidental word, but now the query *also* resolves a real
/// entity elsewhere -- the attribute constraint must remain a hard filter.
/// This is the direct test that the fix does not simply make "entities
/// always win": entities and attributes legitimately coexist as filters
/// whenever an entity is actually present.
#[test]
fn same_attribute_word_stays_a_hard_filter_when_an_entity_is_also_present() {
    let lex = lexicon_with(&[
        ("coffee", color("color", "coffee")),
        (
            "mugs",
            Candidate::constraint(
                ResolvedConstraint::Structural(StructuralConstraint::ProductType(ProductTypeId(7))),
                1.0,
            ),
        ),
    ]);

    let query = compile("coffee mugs", &lex);

    assert_eq!(
        query.constraints,
        vec![
            ResolvedConstraint::Attribute(Constraint::Enum {
                attribute: "color".to_string(),
                value: "coffee".to_string(),
            }),
            ResolvedConstraint::Structural(StructuralConstraint::ProductType(ProductTypeId(7))),
        ],
        "with a real entity present, the attribute match is corroborated \
         and must stay a hard filter: {:?}",
        query.constraints
    );
    assert!(query.preferences.is_empty());
    assert!(query.residual_lexical.is_empty());
}

/// When the real multi-word entity phrase IS registered verbatim, this
/// compiler's existing longest-match-first scan already prefers it over
/// the shorter coincidental attribute word at the very same scan position
/// -- a regression lock on already-correct behavior the fix must not
/// disturb (the defect only ever manifests when the longer phrase is
/// *absent* from the lexicon, per P9-E03).
#[test]
fn registered_multiword_entity_phrase_already_wins_over_the_shorter_attribute_word() {
    let mut lex = lexicon_with(&[("coffee", color("color", "coffee"))]);
    lex.insert(
        "coffee table",
        vec![Candidate::constraint(
            ResolvedConstraint::Structural(StructuralConstraint::ProductType(ProductTypeId(3))),
            1.0,
        )],
    );

    let query = compile("coffee table", &lex);

    assert_eq!(
        query.constraints,
        vec![ResolvedConstraint::Structural(
            StructuralConstraint::ProductType(ProductTypeId(3))
        )],
        "longest-match-first must still prefer the registered entity phrase: {:?}",
        query.constraints
    );
    assert!(query.preferences.is_empty());
    assert!(query.residual_lexical.is_empty());
}

/// A query whose *entire* real intent is a single attribute value with no
/// entity anywhere in the catalog's vocabulary at all (e.g. a bare color
/// search) is the named risk in `PHASE9_DECISION.md`'s "what should
/// explicitly not be built yet": the fix must not silently drop this
/// signal. It is demoted the same way, which keeps it both rankable and
/// lexically searchable rather than deleting real intent.
#[test]
fn standalone_legitimate_attribute_query_is_demoted_but_never_dropped() {
    let lex = lexicon_with(&[("turquoise", color("color", "turquoise"))]);

    let query = compile("turquoise", &lex);

    assert!(query.constraints.is_empty());
    assert_eq!(
        query.preferences,
        vec![Preference::Boost {
            attribute: "color".to_string(),
            value: "turquoise".to_string(),
            weight: 0.5,
        }]
    );
    assert_eq!(query.residual_lexical, vec!["turquoise".to_string()]);
}

/// A `PriceUnderCents`/`PriceOverCents` structural constraint must not
/// count as "an entity" that corroborates a coincidental attribute match:
/// price bounds come from the reserved `under`/`over` keyword parses, not
/// from resolving any commerce entity, and do nothing to disambiguate
/// whether "coffee" means the color or a missed product-type phrase.
#[test]
fn price_bound_alone_does_not_corroborate_a_coincidental_attribute_match() {
    let lex = lexicon_with(&[("coffee", color("color", "coffee"))]);

    let query = compile("coffee table under $50", &lex);

    assert!(
        !query
            .constraints
            .iter()
            .any(|c| matches!(c, ResolvedConstraint::Attribute(_))),
        "no entity is present, so the attribute match must still be demoted: {:?}",
        query.constraints
    );
    assert_eq!(
        query.constraints,
        vec![ResolvedConstraint::Structural(
            StructuralConstraint::PriceUnderCents(5_000)
        )]
    );
    assert_eq!(
        query.preferences,
        vec![Preference::Boost {
            attribute: "color".to_string(),
            value: "coffee".to_string(),
            weight: 0.5,
        }]
    );
}

/// Boolean attribute candidates (e.g. `compile_lexicon`'s
/// `boolean_attributes`, keyed by the attribute's own name) go through the
/// identical open-lexicon path and must be demoted/stringified the same
/// way when no entity is present.
#[test]
fn boolean_attribute_alone_is_demoted_with_a_stringified_value() {
    let lex = lexicon_with(&[(
        "waterproof",
        Candidate::constraint(
            ResolvedConstraint::Attribute(Constraint::Boolean {
                attribute: "waterproof".to_string(),
                value: true,
            }),
            1.0,
        ),
    )]);

    let query = compile("waterproof", &lex);

    assert!(query.constraints.is_empty());
    assert_eq!(
        query.preferences,
        vec![Preference::Boost {
            attribute: "waterproof".to_string(),
            value: "true".to_string(),
            weight: 0.5,
        }]
    );
    assert_eq!(query.residual_lexical, vec!["waterproof".to_string()]);
}

/// `Constraint::MultiEnumContains` candidates must be demoted the same
/// way as `Enum` when no entity is present -- the fix is keyed on *how*
/// the constraint was resolved (open lexicon lookup) and *what else* the
/// query resolved, not on the specific `Constraint` variant.
#[test]
fn multi_enum_attribute_alone_is_demoted_like_enum() {
    let lex = lexicon_with(&[(
        "waxed",
        Candidate::constraint(
            ResolvedConstraint::Attribute(Constraint::MultiEnumContains {
                attribute: "finish".to_string(),
                value: "waxed".to_string(),
            }),
            1.0,
        ),
    )]);

    let query = compile("waxed", &lex);

    assert!(query.constraints.is_empty());
    assert_eq!(
        query.preferences,
        vec![Preference::Boost {
            attribute: "finish".to_string(),
            value: "waxed".to_string(),
            weight: 0.5,
        }]
    );
}

/// Two independent lone attribute matches (different attribute names, so
/// they never trip the pre-existing same-slot conflict guard) with no
/// entity anywhere: both must be demoted, in scan order, and both phrases
/// must remain searchable.
#[test]
fn multiple_lone_attribute_matches_are_all_demoted_in_scan_order() {
    let lex = lexicon_with(&[
        ("coffee", color("color", "coffee")),
        (
            "oak",
            Candidate::constraint(
                ResolvedConstraint::Attribute(Constraint::Enum {
                    attribute: "material".to_string(),
                    value: "oak".to_string(),
                }),
                1.0,
            ),
        ),
    ]);

    let query = compile("coffee oak", &lex);

    assert!(query.constraints.is_empty());
    assert_eq!(
        query.preferences,
        vec![
            Preference::Boost {
                attribute: "color".to_string(),
                value: "coffee".to_string(),
                weight: 0.5,
            },
            Preference::Boost {
                attribute: "material".to_string(),
                value: "oak".to_string(),
                weight: 0.5,
            },
        ]
    );
    assert_eq!(
        query.residual_lexical,
        vec!["coffee".to_string(), "oak".to_string()]
    );
}

/// A genuinely ambiguous span (more than one candidate reading) must be
/// completely unaffected by the demotion pass, even when no entity is
/// present anywhere else in the query -- ambiguity is preserved
/// explicitly (`CLAUDE.md`), never resolved implicitly by this fix.
#[test]
fn ambiguous_span_is_unaffected_by_the_demotion_pass() {
    let mut lex = lexicon_with(&[("coffee", color("color", "coffee"))]);
    lex.insert(
        "clear",
        vec![
            color("color", "clear"),
            Candidate::preference(
                Preference::Boost {
                    attribute: "features".to_string(),
                    value: "clear".to_string(),
                    weight: 0.5,
                },
                0.5,
            ),
        ],
    );

    let query = compile("coffee clear", &lex);

    assert_eq!(query.ambiguous.len(), 1);
    assert_eq!(query.ambiguous[0].text, "clear");
    // The unrelated, unambiguous "coffee" match is still demoted on its own
    // merits (no entity anywhere in the query, ambiguous or not).
    assert!(query.constraints.is_empty());
    assert_eq!(
        query.preferences,
        vec![Preference::Boost {
            attribute: "color".to_string(),
            value: "coffee".to_string(),
            weight: 0.5,
        }]
    );
}

/// A `Category` entity (not just `Brand`/`ProductType`) must also
/// corroborate a coexisting attribute match -- the entity check is not
/// narrowed to only one structural variant.
#[test]
fn category_entity_also_corroborates_a_coexisting_attribute_match() {
    let lex = lexicon_with(&[
        ("coffee", color("color", "coffee")),
        (
            "furniture",
            Candidate::constraint(
                ResolvedConstraint::Structural(StructuralConstraint::Category(CategoryId(4))),
                1.0,
            ),
        ),
    ]);

    let query = compile("coffee furniture", &lex);

    assert_eq!(
        query.constraints,
        vec![
            ResolvedConstraint::Attribute(Constraint::Enum {
                attribute: "color".to_string(),
                value: "coffee".to_string(),
            }),
            ResolvedConstraint::Structural(StructuralConstraint::Category(CategoryId(4))),
        ]
    );
    assert!(query.preferences.is_empty());
}

/// `BrandAny` (the P1-B alias-group variant) must also corroborate, not
/// just the plain single-`BrandId` `Brand` variant.
#[test]
fn brand_any_entity_also_corroborates_a_coexisting_attribute_match() {
    let lex = lexicon_with(&[
        ("coffee", color("color", "coffee")),
        (
            "acme",
            Candidate::constraint(
                ResolvedConstraint::Structural(StructuralConstraint::BrandAny(vec![
                    BrandId(1),
                    BrandId(2),
                ])),
                1.0,
            ),
        ),
    ]);

    let query = compile("coffee acme", &lex);

    assert!(query
        .constraints
        .iter()
        .any(|c| matches!(c, ResolvedConstraint::Attribute(_))));
    assert!(query.preferences.is_empty());
}
