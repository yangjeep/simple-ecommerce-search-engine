//! Shared self-consistency checks for generated ground truth, used by
//! this crate's own tests before any experiment binary trusts a
//! judgment set.

use std::collections::{BTreeMap, BTreeSet};

use crate::{RelevanceLabel, SynthProduct, SynthQuery};

/// Every judged variant id must actually exist in the catalog the
/// judgments were generated against, and every query must judge at least
/// one variant (a query with zero judgments is a generator bug, not a
/// legitimate "nothing is relevant" case -- every template here always
/// targets a real, present product type).
pub fn assert_self_consistent(
    products: &[SynthProduct],
    judgments: &BTreeMap<String, BTreeMap<String, RelevanceLabel>>,
) {
    let known_variant_ids: BTreeSet<&str> = products
        .iter()
        .flat_map(|p| p.variants.iter().map(|v| v.external_id.as_str()))
        .collect();
    for (query_id, per_query) in judgments {
        assert!(
            !per_query.is_empty(),
            "query {query_id:?} has zero judged variants -- generator bug"
        );
        for variant_id in per_query.keys() {
            assert!(
                known_variant_ids.contains(variant_id.as_str()),
                "query {query_id:?} judges unknown variant {variant_id:?}"
            );
        }
    }
}

/// A stronger check than [`assert_self_consistent`]'s "non-empty
/// judgments" for templates whose whole point is a specific attribute
/// value's `Exact` match (e.g. "black sofas" claiming a real black sofa
/// exists, not merely that *some* sofa exists as a weaker `Partial`
/// fallback). An adversarial review found this exact gap: a query
/// generator that iterates a *fixed candidate list* (e.g. a type's first
/// two colors) rather than deriving the value from an actually-generated
/// product can silently emit a query whose claimed `Exact` case was never
/// generated at all -- `assert_self_consistent` alone cannot catch this,
/// since a same-type `Partial` fallback keeps the per-query judgment set
/// non-empty regardless. Only call this for templates whose judge
/// closure is expected to produce `Exact` for every product it targets
/// (this crate derives every such template's query value directly from a
/// real generated product specifically so this always holds) -- a
/// template that is legitimately `Partial`-only by design (e.g.
/// `cross_category_bare_attribute`) must not be checked this way.
pub fn assert_every_query_of_template_has_an_exact_match(
    queries: &[SynthQuery],
    judgments: &BTreeMap<String, BTreeMap<String, RelevanceLabel>>,
    template: &str,
) {
    for q in queries.iter().filter(|q| q.template == template) {
        let per_query = judgments.get(&q.id).unwrap_or_else(|| {
            panic!(
                "query {:?} (template {template:?}) has no judgment entry at all",
                q.id
            )
        });
        let has_exact = per_query
            .values()
            .any(|label| matches!(label, RelevanceLabel::Exact));
        assert!(
            has_exact,
            "query {:?} (template {template:?}, text {:?}) has no Exact-labeled judgment -- \
             its claimed exact-match case was never actually generated",
            q.id, q.text
        );
    }
}
