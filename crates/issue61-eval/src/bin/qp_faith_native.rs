//! Public QP-Faith probe through the real compiler and hard-constraint executor.
//! Measures candidate survival, NOT ranked recall or production traffic prevalence.
use std::collections::{BTreeSet, HashMap};
use std::path::Path;

use commerce_core::domain::{Catalog, Constraint, ProductId};
use commerce_core::ir::{compile, CommerceQuery, ResolvedConstraint, StructuralConstraint};
use comparator_eval::translate::StructuralNames;
use issue61_eval::{load_dataset, sha256_hex, Dataset};
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Deserialize)]
struct Probe {
    query: String,
    family: String,
    known_slots: Value,
    relevant_pids: Vec<u64>,
}

fn category_emitted(constraints: &[ResolvedConstraint]) -> bool {
    constraints.iter().any(|c| {
        matches!(
            c,
            ResolvedConstraint::Structural(
                StructuralConstraint::ProductType(_) | StructuralConstraint::ProductTypeAny(_)
            )
        )
    })
}

fn candidate_source_ids(
    compiled: &CommerceQuery,
    catalog: &Catalog,
    source_ids: &HashMap<ProductId, String>,
) -> BTreeSet<String> {
    compiled
        .execute(catalog)
        .iter()
        .map(|(pid, _)| source_ids[pid].clone())
        .collect()
}

// Exact normalized values only. No semantic synonym matching or oracle-driven rewrites.
fn known_slot_matches(
    slot: &str,
    value: &str,
    constraints: &[ResolvedConstraint],
    names: &impl StructuralNames,
) -> bool {
    let equal = |actual: &str| actual.trim().eq_ignore_ascii_case(value.trim());
    constraints.iter().any(|c| match c {
        ResolvedConstraint::Structural(StructuralConstraint::ProductType(id))
            if slot == "product_type" =>
        {
            names.product_type_name(*id).is_some_and(equal)
        }
        ResolvedConstraint::Attribute(Constraint::Enum { attribute, value }) => {
            (attribute == slot || (slot == "material" && attribute == "primarymaterial"))
                && equal(value)
        }
        _ => false,
    })
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 4 {
        return Err(
            "usage: qp_faith_native <wands-catalog.jsonl> <probe.jsonl> <output.json>".into(),
        );
    }
    let catalog_bytes = std::fs::read(&args[1])?;
    let probe_bytes = std::fs::read(&args[2])?;
    let probes: Vec<Probe> = std::str::from_utf8(&probe_bytes)?
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(serde_json::from_str)
        .collect::<Result<_, _>>()?;
    let mut queries = BTreeSet::new();
    for p in &probes {
        if !queries.insert(&p.query)
            || !matches!(p.family.as_str(), "A_entity" | "B_attr")
            || p.relevant_pids.is_empty()
            || (p.family == "A_entity" && p.known_slots != json!({}))
        {
            return Err("invalid, empty or duplicate public probe row".into());
        }
    }
    if probes.len() != 73 || probes.iter().filter(|p| p.family == "A_entity").count() != 55 {
        return Err("expected the frozen 73-row / 55-entity public probe".into());
    }
    let data = load_dataset(Path::new(&args[1]), Dataset::Wands)?;
    let universe: BTreeSet<String> = data.source_id_by_product.values().cloned().collect();
    if universe.len() != data.catalog.products.len() || universe.len() != 42_994 {
        return Err("expected unique source IDs for the full 42,994-product WANDS catalog".into());
    }
    let mut rows = Vec::new();
    let mut entity_fires = 0;
    let mut entity_retention = 0.0;
    let mut attributed_all_slots = 0;
    let mut attributed_any_hard = 0;
    for p in probes {
        let relevant: BTreeSet<String> = p.relevant_pids.iter().map(u64::to_string).collect();
        if relevant.len() != p.relevant_pids.len() || !relevant.is_subset(&universe) {
            return Err(format!(
                "{}: duplicate or missing public relevant product IDs",
                p.query
            )
            .into());
        }
        let compiled = compile(&p.query, &data.lexicon);
        let candidates = candidate_source_ids(&compiled, &data.catalog, &data.source_id_by_product);
        let retained = relevant.intersection(&candidates).count();
        let retention = retained as f64 / relevant.len() as f64;
        let category = category_emitted(&compiled.constraints);
        let known = p
            .known_slots
            .as_object()
            .ok_or("known_slots must be an object")?;
        let matched = known
            .iter()
            .filter(|(slot, value)| {
                value
                    .as_str()
                    .is_some_and(|v| known_slot_matches(slot, v, &compiled.constraints, &data))
            })
            .count();
        if p.family == "B_attr" {
            attributed_all_slots += usize::from(!known.is_empty() && matched == known.len());
            attributed_any_hard += usize::from(!compiled.constraints.is_empty());
        }
        if p.family == "A_entity" {
            entity_fires += usize::from(category);
            entity_retention += retention;
        }
        rows.push(json!({
            "query": p.query, "family": p.family, "known_slots": p.known_slots,
            "product_type_hard_filter": category,
            "known_slot_count": known.len(), "exact_known_slots_matched": matched,
            "hard_constraints_debug": format!("{:?}", compiled.constraints),
            "preferences_debug": format!("{:?}", compiled.preferences),
            "ambiguity_debug": format!("{:?}", compiled.ambiguous),
            "residual_lexical": compiled.residual_lexical,
            "candidate_products": candidates.len(),
            "relevant_products": relevant.len(), "retained_relevant_products": retained,
            "candidate_relevant_retention": retention,
            "retained_relevant_pids": if p.family == "B_attr" {
                Some(relevant.intersection(&candidates).cloned().collect::<Vec<_>>())
            } else {
                None
            },
            "hard_enum_constraints": compiled.constraints.iter().filter_map(|c| match c {
                ResolvedConstraint::Attribute(Constraint::Enum { attribute, value }) => {
                    Some(json!({"attribute": attribute, "value": value}))
                }
                _ => None,
            }).collect::<Vec<_>>(),
        }));
    }
    let summary = json!({
        "engine_commit": std::env::var("GITHUB_SHA").unwrap_or_else(|_| "unrecorded".into()),
        "arm": "existing_issue61_wands_catalog_profile_compile_lexicon_min_frequency_1",
        "catalog_sha256": sha256_hex(&catalog_bytes),
        "probe_sha256": sha256_hex(&probe_bytes),
        "catalog_products": universe.len(), "query_count": rows.len(), "entity_queries": 55,
        "entity_product_type_hard_filter_count": entity_fires,
        "entity_product_type_hard_filter_rate": entity_fires as f64 / 55.0,
        "entity_mean_candidate_relevant_retention": entity_retention / 55.0,
        "attributed_queries": 18,
        "attributed_all_known_slots_matched": attributed_all_slots,
        "attributed_any_hard_constraint": attributed_any_hard,
        "ranking_executed": false,
        "limitations": [
            "Existing statistics-derived lexicon, not an LLM-installed merchant-context arm.",
            "Candidate retention is not Recall@K; there is no ranker in this experiment.",
            "Procedural relevance sets are not WANDS human judgments.",
            "No ESCI result or production prevalence inference."
        ],
    });
    std::fs::write(
        &args[3],
        serde_json::to_string_pretty(&json!({"summary": summary, "rows": rows}))? + "\n",
    )?;
    println!("QP_FAITH_NATIVE_SUMMARY={summary}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use commerce_core::domain::{BrandId, ProductTypeId};

    #[test]
    fn only_product_type_hard_constraints_count() {
        assert!(!category_emitted(&[]));
        assert!(!category_emitted(&[ResolvedConstraint::Structural(
            StructuralConstraint::Brand(BrandId(1))
        )]));
        assert!(category_emitted(&[ResolvedConstraint::Structural(
            StructuralConstraint::ProductType(ProductTypeId(1))
        )]));
        assert!(category_emitted(&[ResolvedConstraint::Structural(
            StructuralConstraint::ProductTypeAny(vec![ProductTypeId(1), ProductTypeId(2)])
        )]));
    }
    #[test]
    fn compiler_executor_and_external_id_join() {
        use commerce_core::fixtures::{shoe_lexicon, variant_safety_catalog};
        let mut catalog = variant_safety_catalog();
        let mut other = catalog.products[0].clone();
        other.id = ProductId(99);
        other.brand = BrandId(2);
        catalog.products.push(other);
        let ids = HashMap::from([
            (ProductId(1), "external-A".to_owned()),
            (ProductId(99), "external-Z".to_owned()),
        ]);
        let lexicon = shoe_lexicon();
        let query = compile("Nike", &lexicon);
        let got = candidate_source_ids(&query, &catalog, &ids);
        assert_eq!(got, BTreeSet::from(["external-A".to_owned()]));
        let contradictory = compile("black Nike size 9", &lexicon);
        assert!(candidate_source_ids(&contradictory, &catalog, &ids).is_empty());
    }
}
