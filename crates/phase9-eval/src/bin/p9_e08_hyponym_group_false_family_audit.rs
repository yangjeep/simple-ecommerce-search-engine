//! Issue #55 correctness audit, prompted by the `ProductTypeAny` whole-word
//! hyponym-expansion mechanism added to
//! `commerce_core::cold_start::profile::product_type_hyponym_groups`
//! (`docs/experiments/ISSUE55_PRODUCT_TYPE_HYPONYM_PROTOCOL.md`).
//!
//! That mechanism is a *production* commerce-core change subject to
//! CLAUDE.md's non-negotiable Product/Variant correctness rule: a broader
//! product-type name (e.g. "recliners") is expanded to admit any other
//! real catalog product-type name whose whitespace-split word set is a
//! strict superset of the broader name's words (e.g. "gray recliners").
//! The property tests in `profile.rs` prove this is *sound* against
//! synthetic/randomized vocabularies (every produced pair really is a
//! whole-word superset). They cannot prove it never produces a
//! semantically wrong-family pair on the *real* WANDS vocabulary --
//! whole-word containment is a syntactic proxy for "is a kind of", and a
//! real catalog's free-text category/product-type strings could in
//! principle satisfy the syntactic test without the semantic relationship
//! holding (e.g. a coincidental multi-word category path that happens to
//! contain all the words of some unrelated shorter type).
//!
//! This binary is not a benchmark (no manifest entry, no perf gate): it
//! dumps every hyponym pair the real WANDS catalog vocabulary actually
//! produces, sorted so the broadest (shortest, most syntactically
//! permissive) groups are printed first, for **direct human audit** of
//! whether any pair pulls in a genuinely wrong-family product.

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

    println!(
        "=== P9-E08: ProductTypeAny hyponym-group false-family audit (real WANDS vocabulary) ==="
    );
    println!(
        "distinct product-type names: {}, names participating in >=1 hyponym group as the broader term: {}",
        names_by_id.len(),
        groups.len()
    );

    let mut rows: Vec<(&str, usize, Vec<&str>)> = groups
        .iter()
        .map(|(broader_id, narrower_ids)| {
            let broader_name = names_by_id.get(broader_id).copied().unwrap_or("?");
            let mut narrower_names: Vec<&str> = narrower_ids
                .iter()
                .map(|id| names_by_id.get(id).copied().unwrap_or("?"))
                .collect();
            narrower_names.sort_unstable();
            (
                broader_name,
                broader_name.split_whitespace().count(),
                narrower_names,
            )
        })
        .collect();
    // Shortest (most syntactically permissive, highest false-positive
    // risk) broader terms first; ties broken by group size descending,
    // then name for determinism.
    rows.sort_by(|a, b| {
        a.1.cmp(&b.1)
            .then(b.2.len().cmp(&a.2.len()))
            .then(a.0.cmp(b.0))
    });

    println!(
        "\n=== all {} groups, ordered by ascending broader-term word count (highest false-positive risk first) ===",
        rows.len()
    );
    for (broader_name, word_count, narrower_names) in &rows {
        println!(
            "[{word_count}-word] {broader_name:?} -> {} narrower name(s): {narrower_names:?}",
            narrower_names.len()
        );
    }

    let single_word_groups = rows.iter().filter(|(_, wc, _)| *wc == 1).count();
    println!(
        "\nsingle-word broader terms (widest possible expansion, manual review priority): {single_word_groups}/{}",
        rows.len()
    );
}
