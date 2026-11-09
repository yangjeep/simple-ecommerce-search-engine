//! Public QP-Faith probe through the real compiler and hard-constraint executor.
//! Measures candidate survival, NOT ranked recall or production traffic prevalence.
use std::collections::BTreeSet;
use std::path::Path;

use commerce_core::ir::{compile, ResolvedConstraint, StructuralConstraint};
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
                StructuralConstraint::ProductType(_)
                    | StructuralConstraint::ProductTypeAny(_)
            )
        )
    })
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 4 {
        return Err("usage: qp_faith_native <wands-catalog.jsonl> <probe.jsonl> <output.json>".into());
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
    for p in probes {
        let relevant: BTreeSet<String> = p.relevant_pids.iter().map(u64::to_string).collect();
        if relevant.len() != p.relevant_pids.len() || !relevant.is_subset(&universe) {
            return Err(format!("{}: duplicate or missing public relevant product IDs", p.query).into());
        }
        let compiled = compile(&p.query, &data.lexicon);
        let candidates: BTreeSet<String> = compiled
            .execute(&data.catalog)
            .iter()
            .map(|(pid, _)| data.source_id_by_product[pid].clone())
            .collect();
        let retained = relevant.intersection(&candidates).count();
        let retention = retained as f64 / relevant.len() as f64;
        let category = category_emitted(&compiled.constraints);
        if p.family == "A_entity" {
            entity_fires += usize::from(category);
            entity_retention += retention;
        }
        rows.push(json!({
            "query": p.query, "family": p.family, "known_slots": p.known_slots,
            "product_type_hard_filter": category,
            "hard_constraints_debug": format!("{:?}", compiled.constraints),
            "preferences_debug": format!("{:?}", compiled.preferences),
            "ambiguity_debug": format!("{:?}", compiled.ambiguous),
            "residual_lexical": compiled.residual_lexical,
            "candidate_products": candidates.len(),
            "relevant_products": relevant.len(), "retained_relevant_products": retained,
            "candidate_relevant_retention": retention,
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
}
