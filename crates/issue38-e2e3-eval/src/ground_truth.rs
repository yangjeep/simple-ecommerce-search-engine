//! Shared self-consistency checks for generated ground truth, used by
//! this crate's own tests before any experiment binary trusts a
//! judgment set.

use std::collections::{BTreeMap, BTreeSet};

use crate::{RelevanceLabel, SynthProduct};

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
