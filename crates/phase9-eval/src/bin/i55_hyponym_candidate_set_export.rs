//! Issue #55 Priority 2 (semantic promotion architecture): exports the
//! CURRENT, live `product_type_hyponym_groups` candidate set (the exact
//! same production mechanism `p9_e08_hyponym_group_false_family_audit`
//! audits) as JSON, so a promotion-evidence experiment can consume the
//! real, full candidate set programmatically instead of a hand-picked
//! subset or a fragile parse of `p9_e08`'s human-readable text output.
//!
//! Deliberately a separate binary, not a `--json` flag on `p9_e08`
//! itself: `p9_e08`'s existing human-readable output is relied on,
//! byte-for-byte, by prior checkpoints' own disclosed re-audits
//! (`ISSUE55_HYPONYM_LEAF_ONLY_DECISION.md`), so it is left untouched.

use std::collections::BTreeMap;

use commerce_core::cold_start::{product_type_hyponym_groups, CatalogProfile};
use commerce_core::domain::ProductTypeId;

fn main() {
    let raw_products =
        phase6a_eval::data::load_catalog(std::path::Path::new("dataset_cache/wands/catalog.jsonl"));
    let ingested = phase6a_eval::catalog::build_catalog(&raw_products);
    let profile = CatalogProfile::build(
        &ingested.catalog,
        &[],
        &ingested.product_types,
        &ingested.categories,
    );

    let names_by_id: BTreeMap<ProductTypeId, &str> = profile
        .product_type_names_with_ids()
        .map(|(name, id)| (id, name))
        .collect();
    let names_with_ids: BTreeMap<String, ProductTypeId> = profile
        .product_type_names_with_ids()
        .map(|(name, id)| (name.to_string(), id))
        .collect();

    let groups = product_type_hyponym_groups(&names_with_ids);

    let mut out: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for (broader_id, narrower_ids) in &groups {
        let broader_name = names_by_id.get(broader_id).copied().unwrap_or("?");
        let mut narrower_names: Vec<&str> = narrower_ids
            .iter()
            .map(|id| names_by_id.get(id).copied().unwrap_or("?"))
            .collect();
        narrower_names.sort_unstable();
        out.insert(broader_name, narrower_names);
    }

    println!("{}", serde_json::to_string_pretty(&out).expect("serialize"));
}
