//! Issue #55: does every group `product_type_hyponym_groups` outputs
//! actually compile into a reachable `ProductTypeAny` hard constraint via
//! its own literal broader-term text, or do some (particularly the
//! synthesized ancestor-breadcrumb path names) resolve to a soft
//! `Preference` instead and are therefore not a real query-time risk?
//! `p9_e08_hyponym_group_false_family_audit`'s own sort order ("ascending
//! broader-term word count, highest false-positive risk first") already
//! implies short broader terms matter most; this makes that assumption
//! mechanically checked rather than just eyeballed, for every one of the
//! 149 live groups, not a hand-picked sample.

use std::collections::BTreeMap;

use commerce_core::cold_start::{compile_lexicon, product_type_hyponym_groups, CatalogProfile};
use commerce_core::domain::ProductTypeId;
use commerce_core::ir::{compile, ResolvedConstraint, StructuralConstraint};

const MIN_ENUM_FREQUENCY: usize = 1;

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
    let lexicon = compile_lexicon(&profile, MIN_ENUM_FREQUENCY);

    let names_by_id: BTreeMap<ProductTypeId, &str> = profile
        .product_type_names_with_ids()
        .map(|(name, id)| (id, name))
        .collect();
    let names_with_ids: BTreeMap<String, ProductTypeId> = profile
        .product_type_names_with_ids()
        .map(|(name, id)| (name.to_string(), id))
        .collect();

    let groups = product_type_hyponym_groups(&names_with_ids);

    let mut rows: Vec<(&str, Vec<&str>)> = groups
        .iter()
        .map(|(broader_id, narrower_ids)| {
            let broader_name = names_by_id.get(broader_id).copied().unwrap_or("?");
            let mut narrower_names: Vec<&str> = narrower_ids
                .iter()
                .map(|id| names_by_id.get(id).copied().unwrap_or("?"))
                .collect();
            narrower_names.sort_unstable();
            (broader_name, narrower_names)
        })
        .collect();
    rows.sort_by_key(|(name, _)| (name.split_whitespace().count(), *name));

    let mut reachable = 0usize;
    let mut unreachable = 0usize;
    println!(
        "=== reachability of every live hyponym group via its own literal broader-term text ===\n"
    );
    for (broader_name, narrower_names) in &rows {
        let compiled = compile(broader_name, &lexicon);
        let hard_match = compiled.constraints.iter().find_map(|c| match c {
            ResolvedConstraint::Structural(StructuralConstraint::ProductTypeAny(ids)) => {
                Some(ids.clone())
            }
            _ => None,
        });
        match hard_match {
            Some(ids) => {
                reachable += 1;
                let mut names: Vec<&str> = ids
                    .iter()
                    .filter_map(|id| names_by_id.get(id).copied())
                    .collect();
                names.sort_unstable();
                println!(
                    "REACHABLE  [{}-word] {broader_name:?} -> queried verbatim admits {} types: {:?}",
                    broader_name.split_whitespace().count(),
                    names.len(),
                    names
                );
            }
            None => {
                unreachable += 1;
                println!(
                    "unreachable [{}-word] {broader_name:?} -> {} narrower name(s) exist in the raw group but querying this exact text does NOT produce a ProductTypeAny (residual_lexical={:?}, preferences={:?})",
                    broader_name.split_whitespace().count(),
                    narrower_names.len(),
                    compiled.residual_lexical,
                    compiled.preferences
                );
            }
        }
    }
    println!("\n=== {reachable} of {} groups are reachable via their own literal broader-term text; {unreachable} are not ===", rows.len());
}
