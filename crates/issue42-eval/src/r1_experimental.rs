//! R1 experimental treatments (`docs/experiments/ISSUE42_PROTOCOL.md`'s R1
//! section): typed ambiguity and corroborated resolution.
//!
//! Treatment A is the real, unmodified `commerce_core::ir::query::compile`,
//! reused verbatim -- "current behavior" cannot itself be a source of
//! divergence. Treatments B/C/D reuse compile()'s own real output for
//! everything about a query *except* how a numeric-shaped token that ALSO
//! resolves via the lexicon to a distinct typed interpretation is handled
//! (the exact apparel Enum `size="34"` vs. automotive Numeric `size=34.0`
//! collision `mixed_merchant.rs`'s own doc comment names, and E3/issue #40
//! measured a concrete instance of). None of these reimplement compile()'s
//! full tokenizer -- only the one keyword pattern that can collide
//! ([`find_size_numeric_token`]) needs re-detecting; everything else about
//! the query (entity/phrase resolution, negation, price keywords) is taken
//! from the real `compile()`'s own output.
//!
//! Nothing here edits `commerce_core::ir::query::compile` or
//! `commerce_core::plan::execute_planned` -- both are called as-is.
//! "Resolves both typed interpretations simultaneously as separate hard
//! constraints ORed at the top level" (Treatment B) is realized by calling
//! the real `execute_planned` once per interpretation and union-ing the
//! verified hit sets, rather than inventing a new OR-aware executor: this
//! reuses real production matching semantics for each interpretation
//! individually instead of duplicating it.

use std::collections::BTreeMap;

use commerce_core::domain::{
    effective_attributes, AttributeValue, Catalog, Constraint, NumericOp, ProductId, ProductTypeId,
    VariantId,
};
use commerce_core::index::CatalogIndex;
use commerce_core::ir::{
    compile, CommerceQuery, Preference, ResolvedConstraint, ResolvedTerm, SemanticLexicon,
    StructuralConstraint,
};
use commerce_core::plan::{
    execute_planned, LexicalDelegate, PlannedHit, PlannedQuery, PlannerPolicy,
};

/// Mirrors -- does not reuse -- exactly the one keyword pattern
/// `commerce_core::ir::query::compile`'s numeric branch matches: a `size`
/// token immediately followed by a token that parses as `f64`. Returns the
/// raw next-token text (for an exact-string lexicon lookup, since
/// reformatting the parsed `f64` back to a string would be lossy for
/// values like `"034"`) and its parsed value.
pub fn find_size_numeric_token(text: &str) -> Option<(String, f64)> {
    let tokens: Vec<&str> = text.split_whitespace().collect();
    for i in 0..tokens.len() {
        if tokens[i].eq_ignore_ascii_case("size") {
            if let Some(raw) = tokens.get(i + 1) {
                if let Ok(n) = raw.parse::<f64>() {
                    return Some(((*raw).to_string(), n));
                }
            }
        }
    }
    None
}

/// Every distinct typed reading `raw_token` has in `lexicon`, as
/// `Constraint`s. Preference-only candidates are not a typed ambiguity and
/// are excluded.
pub fn lexicon_alternatives(raw_token: &str, lexicon: &SemanticLexicon) -> Vec<Constraint> {
    lexicon
        .resolve(raw_token)
        .map(|candidates| {
            candidates
                .iter()
                .filter_map(|c| match &c.resolved {
                    ResolvedTerm::Constraint(ResolvedConstraint::Attribute(inner)) => {
                        Some(inner.clone())
                    }
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default()
}

fn is_size_eq_numeric(c: &ResolvedConstraint, value: f64) -> bool {
    matches!(
        c,
        ResolvedConstraint::Attribute(Constraint::Numeric { attribute, op: NumericOp::Eq, value: v })
            if attribute == "size" && (*v - value).abs() < 1e-9
    )
}

/// Mirrors `commerce_core::ir::query::compile`'s own private
/// `attribute_constraint_to_preference` -- reimplemented locally (small
/// and self-contained) since that function is not part of the crate's
/// public API and this module must not edit production code to expose it.
fn constraint_to_preference(c: &Constraint) -> Preference {
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
            ..
        } => (attribute.clone(), contains.clone()),
    };
    Preference::Boost {
        attribute,
        value,
        weight: 0.5,
    }
}

fn corroborating_product_type(compiled: &CommerceQuery) -> Option<ProductTypeId> {
    compiled.constraints.iter().find_map(|c| match c {
        ResolvedConstraint::Structural(StructuralConstraint::ProductType(id)) => Some(*id),
        _ => None,
    })
}

/// Does any product of `product_type` in `catalog` carry `constraint`'s
/// attribute as exactly the `AttributeValue` *kind* `constraint` targets
/// (Enum vs. MultiEnum vs. Boolean vs. Numeric vs. Text)? Checked directly
/// against the real materialized catalog, not any generator's own
/// bookkeeping -- Treatment D's corroboration decision must not trust a
/// label any more than the independent oracle does.
fn constraint_kind_registered_on_product_type(
    catalog: &Catalog,
    product_type: ProductTypeId,
    constraint: &Constraint,
) -> bool {
    let attribute = match constraint {
        Constraint::Enum { attribute, .. }
        | Constraint::MultiEnumContains { attribute, .. }
        | Constraint::Boolean { attribute, .. }
        | Constraint::Numeric { attribute, .. }
        | Constraint::Text { attribute, .. } => attribute.as_str(),
    };
    catalog
        .products
        .iter()
        .filter(|p| p.product_type == product_type)
        .any(|p| {
            p.variants.iter().any(|v| {
                let merged = effective_attributes(p, v);
                matches!(
                    (merged.get(attribute), constraint),
                    (Some(AttributeValue::Enum(_)), Constraint::Enum { .. })
                        | (
                            Some(AttributeValue::MultiEnum(_)),
                            Constraint::MultiEnumContains { .. }
                        )
                        | (Some(AttributeValue::Boolean(_)), Constraint::Boolean { .. })
                        | (Some(AttributeValue::Numeric(_)), Constraint::Numeric { .. })
                        | (Some(AttributeValue::Text(_)), Constraint::Text { .. })
                )
            })
        })
}

/// One treatment's resolution of a query: normally a single
/// [`CommerceQuery`], but two for Treatment B when a genuine alternative
/// interpretation exists -- each is a fully-typed sub-query executed
/// independently; their union is the disjunction.
#[derive(Debug, Clone)]
pub struct Resolution {
    pub queries: Vec<CommerceQuery>,
}

/// Treatment A: the real, unmodified `compile()`, reused verbatim.
pub fn resolve_a(text: &str, lexicon: &SemanticLexicon) -> Resolution {
    Resolution {
        queries: vec![compile(text, lexicon)],
    }
}

/// Treatment B: if the numeric "size N" branch fired AND the same literal
/// token also resolves via the lexicon to a distinct typed interpretation,
/// keep BOTH as separate hard-constraint sub-queries (a disjunction).
/// Otherwise identical to A.
pub fn resolve_b(text: &str, lexicon: &SemanticLexicon) -> Resolution {
    let base = compile(text, lexicon);
    let Some((raw, n)) = find_size_numeric_token(text) else {
        return Resolution {
            queries: vec![base],
        };
    };
    let alternatives = lexicon_alternatives(&raw, lexicon);
    if alternatives.is_empty() {
        return Resolution {
            queries: vec![base],
        };
    }
    let mut queries = vec![base.clone()];
    for alt in alternatives {
        let mut q = base.clone();
        if let Some(pos) = q.constraints.iter().position(|c| is_size_eq_numeric(c, n)) {
            q.constraints[pos] = ResolvedConstraint::Attribute(alt);
        }
        queries.push(q);
    }
    Resolution { queries }
}

/// Treatment C: reuses B's multi-interpretation discovery, but demotes
/// every interpretation to a `Preference::Boost` instead of keeping any as
/// a hard constraint -- generalizing the existing P9-E05 demotion rule
/// (today applied only to lexicon-derived attribute matches) to
/// numeric-keyword-derived ones too.
pub fn resolve_c(text: &str, lexicon: &SemanticLexicon) -> Resolution {
    let mut base = compile(text, lexicon);
    let Some((raw, n)) = find_size_numeric_token(text) else {
        return Resolution {
            queries: vec![base],
        };
    };
    let alternatives = lexicon_alternatives(&raw, lexicon);
    if alternatives.is_empty() {
        return Resolution {
            queries: vec![base],
        };
    }
    if let Some(pos) = base
        .constraints
        .iter()
        .position(|c| is_size_eq_numeric(c, n))
    {
        let numeric = Constraint::Numeric {
            attribute: "size".to_string(),
            op: NumericOp::Eq,
            value: n,
        };
        base.constraints.remove(pos);
        base.preferences.push(constraint_to_preference(&numeric));
    }
    for alt in &alternatives {
        base.preferences.push(constraint_to_preference(alt));
    }
    if !base.residual_lexical.contains(&raw) {
        base.residual_lexical.push(raw);
    }
    Resolution {
        queries: vec![base],
    }
}

/// Diagnostic only, NOT a fifth preregistered treatment: identical to
/// [`resolve_c`] except it does not push the demoted token into
/// `residual_lexical`. Added after an adversarial review of R1's results
/// found that `resolve_c`'s residual-push, combined with
/// `plan::execute_planned`'s Hybrid/Punt outcomes never falling back to
/// a structural-only result when the lexical delegate returns nothing
/// (the same strict-veto mechanism R2 exists to address), silently
/// discards a corroborating `ProductType` entity constraint's own
/// narrowing power on a query C would otherwise resolve to `FastPath`.
/// Comparing this isolated variant's corroborated-row NDCG against
/// `resolve_c`'s own tells us how much of C's measured NDCG gap vs. D is
/// attributable to *that* confound rather than to C's own defining
/// property (never selecting one interpretation using corroborating
/// context) -- see `docs/experiments/ISSUE42_LOG.md`'s R1 section for
/// the reasoning and the isolated measurement itself. This function must
/// never be used to compute R1's own preregistered GO-gate numbers for
/// Treatment C -- only as a side, disclosed diagnostic.
pub fn resolve_c_isolated_no_residual_push(text: &str, lexicon: &SemanticLexicon) -> Resolution {
    let mut base = compile(text, lexicon);
    let Some((_raw, n)) = find_size_numeric_token(text) else {
        return Resolution {
            queries: vec![base],
        };
    };
    let alternatives = lexicon_alternatives(&_raw, lexicon);
    if alternatives.is_empty() {
        return Resolution {
            queries: vec![base],
        };
    }
    if let Some(pos) = base
        .constraints
        .iter()
        .position(|c| is_size_eq_numeric(c, n))
    {
        let numeric = Constraint::Numeric {
            attribute: "size".to_string(),
            op: NumericOp::Eq,
            value: n,
        };
        base.constraints.remove(pos);
        base.preferences.push(constraint_to_preference(&numeric));
    }
    for alt in &alternatives {
        base.preferences.push(constraint_to_preference(alt));
    }
    Resolution {
        queries: vec![base],
    }
}

/// Treatment D: like C, but when a corroborating `ProductType` entity
/// constraint is present elsewhere in the same query, uses the real
/// materialized catalog to select exactly one typed interpretation as the
/// hard constraint -- whichever one (numeric or an alternative) is
/// actually registered on that entity's product type. Falls back to C's
/// demotion behavior when there is no corroborating `ProductType`
/// constraint, or when corroboration does not disambiguate cleanly
/// (neither or both interpretations registered) -- a treatment must not
/// fabricate a unique meaning it cannot actually justify from the catalog.
pub fn resolve_d(text: &str, lexicon: &SemanticLexicon, catalog: &Catalog) -> Resolution {
    let base = compile(text, lexicon);
    let Some((raw, n)) = find_size_numeric_token(text) else {
        return Resolution {
            queries: vec![base],
        };
    };
    let alternatives = lexicon_alternatives(&raw, lexicon);
    if alternatives.is_empty() {
        return Resolution {
            queries: vec![base],
        };
    }
    let Some(product_type) = corroborating_product_type(&base) else {
        return resolve_c(text, lexicon);
    };
    let numeric = Constraint::Numeric {
        attribute: "size".to_string(),
        op: NumericOp::Eq,
        value: n,
    };
    let numeric_registered =
        constraint_kind_registered_on_product_type(catalog, product_type, &numeric);
    let matching_alts: Vec<&Constraint> = alternatives
        .iter()
        .filter(|alt| constraint_kind_registered_on_product_type(catalog, product_type, alt))
        .collect();

    match (numeric_registered, matching_alts.as_slice()) {
        (true, []) => Resolution {
            queries: vec![base],
        },
        (false, [only_alt]) => {
            let mut q = base.clone();
            if let Some(pos) = q.constraints.iter().position(|c| is_size_eq_numeric(c, n)) {
                q.constraints[pos] = ResolvedConstraint::Attribute((*only_alt).clone());
            }
            Resolution { queries: vec![q] }
        }
        // Neither registered, both registered, or more than one
        // alternative registered: corroboration does not cleanly select a
        // single interpretation -- do not guess, demote instead.
        _ => resolve_c(text, lexicon),
    }
}

/// Runs every `CommerceQuery` in `resolution` through the real
/// `execute_planned`, then unions the verified hit sets (deduped by
/// `(ProductId, VariantId)`, keeping the higher score on a tie) -- the
/// disjunction Treatment B's two sub-queries realize.
pub fn run_treatment(
    resolution: &Resolution,
    catalog: &Catalog,
    index: &CatalogIndex,
    delegate: Option<&dyn LexicalDelegate>,
    k: usize,
    policy: &PlannerPolicy,
) -> (Vec<PlannedQuery>, Vec<PlannedHit>) {
    let mut outcomes = Vec::with_capacity(resolution.queries.len());
    let mut by_key: BTreeMap<(ProductId, VariantId), PlannedHit> = BTreeMap::new();
    for query in &resolution.queries {
        let (planned, hits) = execute_planned(query, catalog, index, delegate, k, policy);
        outcomes.push(planned);
        for hit in hits {
            by_key
                .entry((hit.product, hit.variant))
                .and_modify(|existing| {
                    if hit.score > existing.score {
                        *existing = hit;
                    }
                })
                .or_insert(hit);
        }
    }
    (outcomes, by_key.into_values().collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use commerce_core::cold_start::{compile_lexicon, CatalogProfile};
    use issue38_e2e3_eval::{ingest, mixed_merchant};

    fn lexicon_and_catalog() -> (
        SemanticLexicon,
        Catalog,
        Vec<commerce_core::domain::ProductType>,
    ) {
        let products = mixed_merchant::generate_mixed_catalog(80, 80, 80);
        let ingested = ingest::build_catalog(&products);
        let profile = CatalogProfile::build(
            &ingested.catalog,
            &ingested.brands,
            &ingested.product_types,
            &ingested.categories,
        );
        let lexicon = compile_lexicon(&profile, 1);
        (lexicon, ingested.catalog, ingested.product_types)
    }

    #[test]
    fn find_size_numeric_token_matches_only_the_real_keyword_shape() {
        assert_eq!(
            find_size_numeric_token("size 34 jeans"),
            Some(("34".to_string(), 34.0))
        );
        assert_eq!(find_size_numeric_token("size purple"), None);
        assert_eq!(find_size_numeric_token("under $34"), None);
        assert_eq!(find_size_numeric_token("no size token here"), None);
    }

    #[test]
    fn treatment_a_is_exactly_the_real_compile_output() {
        let (lexicon, _, _) = lexicon_and_catalog();
        let direct = compile("size 34 jeans", &lexicon);
        let a = resolve_a("size 34 jeans", &lexicon);
        assert_eq!(a.queries, vec![direct]);
    }

    #[test]
    fn treatment_b_keeps_both_interpretations_as_separate_hard_constraint_queries() {
        let (lexicon, _, _) = lexicon_and_catalog();
        // "size 34" alone (no entity word) is exactly R1 workload row 1:
        // ambiguous, corroboration absent.
        let b = resolve_b("size 34", &lexicon);
        assert_eq!(
            b.queries.len(),
            2,
            "expected the real numeric interpretation plus exactly one apparel Enum alternative for this catalog's real value"
        );
        let has_numeric = b
            .queries
            .iter()
            .any(|q| q.constraints.iter().any(|c| is_size_eq_numeric(c, 34.0)));
        let has_enum = b.queries.iter().any(|q| {
            q.constraints.iter().any(|c| {
                matches!(c, ResolvedConstraint::Attribute(Constraint::Enum { attribute, value })
                    if attribute == "size" && value == "34")
            })
        });
        assert!(
            has_numeric,
            "one sub-query must keep the Numeric interpretation"
        );
        assert!(has_enum, "one sub-query must keep the Enum interpretation");
    }

    #[test]
    fn treatment_c_demotes_both_interpretations_to_preferences_never_a_hard_constraint() {
        let (lexicon, _, _) = lexicon_and_catalog();
        let c = resolve_c("size 34", &lexicon);
        assert_eq!(c.queries.len(), 1);
        let q = &c.queries[0];
        assert!(
            !q.constraints.iter().any(|constr| is_size_eq_numeric(constr, 34.0)
                || matches!(constr, ResolvedConstraint::Attribute(Constraint::Enum { attribute, .. }) if attribute == "size")),
            "no size-typed hard constraint may survive Treatment C for an uncorroborated ambiguous query: {:?}",
            q.constraints
        );
        assert!(
            q.preferences.len() >= 2,
            "both the numeric and the enum interpretation must appear as preferences: {:?}",
            q.preferences
        );
    }

    #[test]
    fn isolated_c_diagnostic_differs_from_c_only_in_residual_lexical() {
        let (lexicon, _, _) = lexicon_and_catalog();
        let c = resolve_c("size 34 jeans", &lexicon);
        let isolated = resolve_c_isolated_no_residual_push("size 34 jeans", &lexicon);
        assert_eq!(c.queries.len(), 1);
        assert_eq!(isolated.queries.len(), 1);
        assert_eq!(
            c.queries[0].constraints, isolated.queries[0].constraints,
            "the isolated diagnostic must demote the same constraints C does"
        );
        assert_eq!(
            c.queries[0].preferences, isolated.queries[0].preferences,
            "the isolated diagnostic must produce the same preferences C does"
        );
        assert!(
            !c.queries[0].residual_lexical.is_empty(),
            "resolve_c is expected to push the demoted token into residual_lexical"
        );
        assert!(
            isolated.queries[0].residual_lexical.is_empty(),
            "the isolated diagnostic must NOT push the demoted token into residual_lexical -- \
             that is precisely the one behavior it isolates away"
        );
    }

    #[test]
    fn treatment_d_selects_the_enum_interpretation_when_jeans_corroborates() {
        let (lexicon, catalog, _) = lexicon_and_catalog();
        let d = resolve_d("size 34 jeans", &lexicon, &catalog);
        assert_eq!(d.queries.len(), 1);
        let q = &d.queries[0];
        let has_enum_hard_constraint = q.constraints.iter().any(|c| {
            matches!(c, ResolvedConstraint::Attribute(Constraint::Enum { attribute, value })
                if attribute == "size" && value == "34")
        });
        assert!(
            has_enum_hard_constraint,
            "jeans corroborates the Enum interpretation, so it alone should be the hard constraint: {:?}",
            q.constraints
        );
        assert!(
            !q.constraints.iter().any(|c| is_size_eq_numeric(c, 34.0)),
            "the Numeric interpretation must not also survive as a hard constraint: {:?}",
            q.constraints
        );
    }

    #[test]
    fn treatment_d_falls_back_to_demotion_with_no_corroborating_entity() {
        let (lexicon, catalog, _) = lexicon_and_catalog();
        let d = resolve_d("size 34", &lexicon, &catalog);
        let c = resolve_c("size 34", &lexicon);
        assert_eq!(
            d.queries, c.queries,
            "with no corroborating ProductType entity, D must behave exactly like C"
        );
    }

    #[test]
    fn all_treatments_agree_when_no_ambiguity_exists_at_all() {
        let (lexicon, _, _) = lexicon_and_catalog();
        for text in ["under $34", "over $34", "part number IA-1234-BP"] {
            let a = resolve_a(text, &lexicon);
            let b = resolve_b(text, &lexicon);
            let c = resolve_c(text, &lexicon);
            assert_eq!(
                a.queries, b.queries,
                "B must equal A for {text:?} (no size-numeric token)"
            );
            assert_eq!(
                a.queries, c.queries,
                "C must equal A for {text:?} (no size-numeric token)"
            );
        }
    }
}
