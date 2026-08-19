use crate::domain::{
    effective_attributes, AttributeMap, AttributeValue, Catalog, ProductId, VariantId,
};
use crate::ir::{CommerceQuery, Preference};

use super::CatalogIndex;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RankedHit {
    pub product: ProductId,
    pub variant: VariantId,
    pub score: f64,
}

fn score_preferences(preferences: &[Preference], attrs: &AttributeMap) -> f64 {
    preferences
        .iter()
        .map(
            |Preference::Boost {
                 attribute,
                 value,
                 weight,
             }| {
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
            },
        )
        .sum()
}

pub(super) fn execute_ranked(
    index: &CatalogIndex,
    query: &CommerceQuery,
    catalog: &Catalog,
    k: usize,
) -> Vec<RankedHit> {
    let mut scored: Vec<RankedHit> = index
        .execute(query, catalog)
        .into_iter()
        .map(|(product, variant)| {
            let (p, v) = index
                .lookup_variant(catalog, variant)
                .expect("execute() only returns ids that exist in this catalog");
            let attrs = effective_attributes(p, v);
            RankedHit {
                product,
                variant,
                score: score_preferences(&query.preferences, &attrs),
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
