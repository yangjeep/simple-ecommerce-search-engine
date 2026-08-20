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
    // every real query, and the merged attrs are computed only to feed a
    // `score_preferences` call that would have returned `0.0` regardless,
    // without ever reading them. Skip the merge entirely in that case --
    // behavior is identical (score is always `0.0` either way), just
    // without the wasted allocation.
    let mut scored: Vec<RankedHit> = index
        .execute(query, catalog)
        .into_iter()
        .map(|(product, variant)| {
            let score = if query.preferences.is_empty() {
                0.0
            } else {
                let (p, v) = index
                    .lookup_variant(catalog, variant)
                    .expect("execute() only returns ids that exist in this catalog");
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
