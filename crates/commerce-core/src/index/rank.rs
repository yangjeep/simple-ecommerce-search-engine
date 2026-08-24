use std::collections::HashSet;

use roaring::RoaringBitmap;

use crate::domain::{
    effective_attributes, AttributeMap, AttributeValue, Catalog, Product, ProductId, Variant,
    VariantId,
};
use crate::ir::{CommerceQuery, Preference};

use super::CatalogIndex;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RankedHit {
    pub product: ProductId,
    pub variant: VariantId,
    pub score: f64,
}

fn score_preferences(
    preferences: &[Preference],
    product: &Product,
    variant: &Variant,
    attrs: &AttributeMap,
) -> f64 {
    preferences
        .iter()
        .map(|pref| match pref {
            Preference::Boost {
                attribute,
                value,
                weight,
            } => {
                let hit = match attrs.get(attribute) {
                    Some(AttributeValue::Enum(v)) => v == value,
                    Some(AttributeValue::MultiEnum(vs)) => vs.iter().any(|v| v == value),
                    Some(AttributeValue::Text(t)) => t.contains(value.as_str()),
                    _ => false,
                };
                if hit {
                    *weight
                } else {
                    0.0
                }
            }
            Preference::StructuralBoost(constraint, weight) => {
                if constraint.matches(product, variant) {
                    *weight
                } else {
                    0.0
                }
            }
        })
        .sum()
}

/// Issue #34 Phase 9 (disclosed defect #1, `PHASE2_DECISION.md`): with no
/// curated `Preference` to score (still every real query today --
/// `compile_lexicon`/I7-E04 never emits one), this used to return `0.0`
/// unconditionally, so FastPath's "top-K" was really an arbitrary
/// `(product_id, variant_id)`-ordered subset, not a ranked one. This is the
/// default signal that replaces that: how many of the query's own
/// unresolved tokens (`residual_lexical`) literally appear in the
/// candidate's title or any `Text` attribute (e.g. a WANDS-style
/// `description`). Deliberately intrinsic to `Product` alone (no
/// `effective_attributes` merge) so it stays as cheap as the no-op it
/// replaces -- see `execute_ranked`'s own P1-D comment below for why that
/// merge is avoided whenever possible.
fn score_text_relevance(residual_lexical: &[String], product: &Product) -> f64 {
    if residual_lexical.is_empty() {
        return 0.0;
    }
    let title_lower = product.title.to_lowercase();
    let title_tokens: HashSet<&str> = title_lower.split_whitespace().collect();
    let text_attrs_lower: Vec<String> = product
        .attributes
        .values()
        .filter_map(|v| match v {
            AttributeValue::Text(t) => Some(t.to_lowercase()),
            _ => None,
        })
        .collect();
    let text_attr_tokens: HashSet<&str> = text_attrs_lower
        .iter()
        .flat_map(|t| t.split_whitespace())
        .collect();

    residual_lexical
        .iter()
        .map(|token| {
            let token = token.to_lowercase();
            let mut hit = 0.0;
            if title_tokens.contains(token.as_str()) {
                hit += 2.0;
            }
            if text_attr_tokens.contains(token.as_str()) {
                hit += 1.0;
            }
            hit
        })
        .sum()
}

pub(super) fn execute_ranked(
    index: &CatalogIndex,
    query: &CommerceQuery,
    catalog: &Catalog,
    k: usize,
) -> Vec<RankedHit> {
    // Issue #6 P1-D (`docs/experiments/PHASE2_LOG.md` P2-E13): a real-data
    // benchmark measured this function costing ~1078ms for a single
    // FastPath query against the real 1.2M-product catalog, entirely from
    // computing `effective_attributes` (a per-candidate HashMap merge/
    // clone) for every one of ~1.2M candidates -- even though `compile_lexicon`
    // (this project's own shipping baseline lexicon, I7-E04) never emits a
    // real `Preference`, so `query.preferences` is empty on essentially
    // every real query, and the merged attrs were computed only to feed a
    // `score_preferences` call that would have returned `0.0` regardless,
    // without ever reading them. Still skip the merge in that case -- the
    // Issue #34 default signal above reads only `Product` fields already at
    // hand via `lookup_variant`, not the merged attrs -- so this keeps the
    // P1-D cost fix intact while no longer leaving preference-less queries
    // with zero ranking signal at all.
    let mut scored: Vec<RankedHit> = index
        .execute(query, catalog)
        .into_iter()
        .map(|(product, variant)| {
            let (p, v) = index
                .lookup_variant(catalog, variant)
                .expect("execute() only returns ids that exist in this catalog");
            let score = if query.preferences.is_empty() {
                score_text_relevance(&query.residual_lexical, p)
            } else {
                let attrs = effective_attributes(p, v);
                score_preferences(&query.preferences, p, v, &attrs)
            };
            RankedHit {
                product,
                variant,
                score,
            }
        })
        .collect();

    scored.sort_by(|a, b| {
        b.score
            .total_cmp(&a.score)
            .then(a.product.0.cmp(&b.product.0))
            .then(a.variant.0.cmp(&b.variant.0))
    });
    scored.truncate(k);
    scored
}

/// Issue #14 P3-E03: rank a query's structural candidates further
/// narrowed by an externally-supplied ordinal bitmap -- e.g. Round 1's
/// `lexical_and_candidates` AND-narrowing over unresolved residual-text
/// tokens, the mechanism `admission::admit_lexically_narrowed` uses to
/// safely admit a query with non-empty `residual_lexical` when every
/// residual token is verifiable via the native token-postings index (no
/// delegate call). Every hard constraint in `query.constraints` is still
/// re-verified exactly against each surviving candidate via
/// `CommerceQuery::matches_variant` -- correctness never depends on
/// trusting the caller's narrowing bitmap, matching `plan::execute_planned`'s
/// own "commerce_core re-verifies every hard constraint against every
/// returned hit itself" contract. Still no ranking signal here (unlike
/// `execute_ranked`, which gained a default `residual_lexical`-vs-title/text
/// signal in Issue #34 Phase 9 -- not extended to this narrower, separately
/// admission-gated path): ties break on ascending `(product_id, variant_id)`,
/// which is why this mechanism's own safety depends on the *caller* keeping
/// the narrowed candidate set small, not on anything this function does.
pub(super) fn execute_ranked_narrowed_by(
    index: &CatalogIndex,
    query: &CommerceQuery,
    narrow_by: &RoaringBitmap,
    catalog: &Catalog,
    k: usize,
) -> Vec<RankedHit> {
    let candidates = index.indexed_candidates(&query.constraints) & narrow_by;
    let mut scored: Vec<RankedHit> = candidates
        .iter()
        .filter_map(|ord| {
            let variant_id = index.variant_id_at(ord)?;
            let (product, variant) = index.lookup_variant(catalog, variant_id)?;
            query
                .matches_variant(product, variant)
                .then_some(RankedHit {
                    product: product.id,
                    variant: variant_id,
                    score: 0.0,
                })
        })
        .collect();
    scored.sort_by(|a, b| {
        a.product
            .0
            .cmp(&b.product.0)
            .then(a.variant.0.cmp(&b.variant.0))
    });
    scored.truncate(k);
    scored
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{attributes, BrandId, CategoryId, ProductTypeId};
    use crate::fixtures::variant_safety_catalog;
    use crate::ir::compile;
    use crate::ir::SemanticLexicon;

    fn product_with(title: &str, text_attrs: Vec<(&'static str, AttributeValue)>) -> Product {
        Product {
            id: ProductId(1),
            product_type: ProductTypeId(1),
            brand: BrandId(1),
            category: CategoryId(1),
            title: title.to_string(),
            attributes: attributes(text_attrs),
            variants: vec![],
        }
    }

    #[test]
    fn empty_residual_scores_zero_regardless_of_title() {
        let p = product_with("Nike Air Zoom Runner", vec![]);
        assert_eq!(score_text_relevance(&[], &p), 0.0);
    }

    #[test]
    fn title_token_hit_outweighs_text_attribute_hit() {
        let p = product_with(
            "Nike Air Zoom Runner",
            vec![(
                "description",
                AttributeValue::Text("a lightweight running shoe".to_string()),
            )],
        );
        let title_hit = score_text_relevance(&["zoom".to_string()], &p);
        let attr_hit = score_text_relevance(&["lightweight".to_string()], &p);
        assert_eq!(title_hit, 2.0);
        assert_eq!(attr_hit, 1.0);
        assert!(title_hit > attr_hit);
    }

    #[test]
    fn unmatched_token_contributes_nothing() {
        let p = product_with("Nike Air Zoom Runner", vec![]);
        assert_eq!(score_text_relevance(&["kayak".to_string()], &p), 0.0);
    }

    #[test]
    fn matching_is_case_insensitive() {
        let p = product_with("Nike Air Zoom Runner", vec![]);
        assert_eq!(score_text_relevance(&["ZOOM".to_string()], &p), 2.0);
    }

    #[test]
    fn execute_ranked_uses_the_default_signal_when_preferences_are_empty() {
        let catalog = variant_safety_catalog();
        let index = CatalogIndex::build(&catalog);
        // Empty lexicon: nothing resolves to a constraint or preference, so
        // every token of "zoom" lands in residual_lexical and preferences
        // stays empty -- exactly the real-world shape this fix targets.
        let query = compile("zoom", &SemanticLexicon::new());
        assert!(query.preferences.is_empty());
        assert_eq!(query.residual_lexical, vec!["zoom".to_string()]);

        let ranked = execute_ranked(&index, &query, &catalog, 10);
        assert_eq!(
            ranked.len(),
            2,
            "both variants of the one product: {ranked:?}"
        );
        assert!(
            ranked.iter().all(|hit| hit.score == 2.0),
            "\"zoom\" is a title token, so both of this product's variants should score 2.0: {ranked:?}"
        );
    }
}
