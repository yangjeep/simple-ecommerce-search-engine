//! Issue #55, checkpoint 19's own named next question
//! (`docs/decisions/ISSUE55_ROUTING_OUTCOME_REPLICATION_DECISION.md`):
//! automotive's `Hybrid`-routed bucket still shows a large native-worse-
//! than-Solr gap (-38.75%) even after fixing `issue35-eval`'s Brand/color
//! Solr `fq` fairness bug. Is this a genuine relevance gap, or yet
//! another undiscovered comparator asymmetry?
//!
//! This is a read-only diagnostic probe, not a new production mechanism:
//! it rebuilds the exact same catalog/lexicon/index/policy
//! `issue35_eval::eval::run_vertical_eval` uses for the automotive
//! vertical (same crate, same public ingestion functions, same
//! `MIN_ENUM_FREQUENCY`/`PlannerPolicy`/K), filters to the `Hybrid`-
//! routed, NDCG-scoreable queries, and prints per-query ranked titles
//! from both engines side by side with the real ESCI judgment labels so
//! a human can read WHY each one moved, rather than guessing from an
//! aggregate number.

use std::collections::BTreeMap;

use commerce_core::cold_start::{compile_lexicon, CatalogProfile};
use commerce_core::domain::{BrandId, Constraint, ProductId};
use commerce_core::index::CatalogIndex;
use commerce_core::ir::{compile, ResolvedConstraint, StructuralConstraint};
use commerce_core::plan::{execute_planned, ExecutionOutcome, LexicalDelegate, PlannerPolicy};
use issue35_eval::{build_catalog, label_gain, load_products, load_queries};
use phase9_eval::bitmap_delegate::{build_index, BitmapTantivyDelegate};
use round1_eval::solr::case_insensitive_field_regex;

const K: usize = 10;
const MIN_ENUM_FREQUENCY: usize = 1;

fn ndcg_at_k_graded(ranked_ids: &[String], gains: &BTreeMap<String, f64>, k: usize) -> Option<f64> {
    let mut ideal: Vec<f64> = gains.values().copied().collect();
    ideal.sort_by(|a, b| b.total_cmp(a));
    let idcg: f64 = ideal
        .iter()
        .take(k)
        .enumerate()
        .map(|(i, g)| g / (i as f64 + 2.0).log2())
        .sum();
    if idcg <= 0.0 {
        return None;
    }
    let dcg: f64 = ranked_ids
        .iter()
        .take(k)
        .enumerate()
        .map(|(i, id)| gains.get(id).copied().unwrap_or(0.0) / (i as f64 + 2.0).log2())
        .sum();
    Some(dcg / idcg)
}

fn solr_search(base_url: &str, q: &str, fq: &[String], rows: usize) -> Option<Vec<String>> {
    let url = format!("{base_url}/select");
    let rows_str = rows.to_string();
    let mut form: Vec<(&str, &str)> = vec![
        ("q", q),
        ("defType", "edismax"),
        ("qf", "title description bullet_point"),
        ("rows", &rows_str),
        ("fl", "id"),
    ];
    for f in fq {
        form.push(("fq", f.as_str()));
    }
    let resp = ureq::post(&url)
        .timeout(std::time::Duration::from_secs(30))
        .send_form(&form)
        .ok()?;
    let body: serde_json::Value = resp.into_json().ok()?;
    let docs = body["response"]["docs"].as_array()?;
    Some(
        docs.iter()
            .filter_map(|d| d["id"].as_str().map(str::to_string))
            .collect(),
    )
}

/// Generalized (originally automotive-only) so the same zero-hit-mechanism
/// check can run against all three ESCI verticals: `cargo run --release
/// --bin i55_e15_automotive_hybrid_gap_probe -- <vertical_label>
/// <products_path> <queries_path> <solr_base_url>`. Defaults to automotive
/// (this probe's original target) when no args are given, so the existing
/// invocation keeps working unchanged.
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (vertical_label, products_path, queries_path, solr_base_url) = if args.len() >= 5 {
        (
            args[1].clone(),
            args[2].clone(),
            args[3].clone(),
            args[4].clone(),
        )
    } else {
        (
            "automotive".to_string(),
            "dataset_cache/esci_automotive/esci_automotive_products.jsonl".to_string(),
            "dataset_cache/esci_automotive/esci_automotive_queries.jsonl".to_string(),
            "http://localhost:8983/solr/esci_automotive_bench".to_string(),
        )
    };
    let products_path = products_path.as_str();
    let queries_path = queries_path.as_str();
    let solr_base_url = solr_base_url.as_str();

    let raw_products = load_products(products_path);
    let raw_queries = load_queries(queries_path);
    let ingested = build_catalog(&raw_products);

    let index = CatalogIndex::build(&ingested.catalog);
    let profile = CatalogProfile::build(&ingested.catalog, &ingested.brands, &[], &[]);
    let lexicon = compile_lexicon(&profile, MIN_ENUM_FREQUENCY);
    let built = build_index(&ingested.catalog).expect("in-memory tantivy index build");
    let delegate = BitmapTantivyDelegate::new(
        &built.index,
        vec![built.title_field, built.description_field],
    )
    .expect("tantivy delegate build");
    let policy = PlannerPolicy {
        selectivity_threshold: 0.05,
        delegate_oversample: 20,
    };

    let title_by_product_id: BTreeMap<ProductId, &str> = ingested
        .catalog
        .products
        .iter()
        .map(|p| (p.id, p.title.as_str()))
        .collect();
    let brand_name_by_id: BTreeMap<BrandId, &str> = ingested
        .brands
        .iter()
        .map(|b| (b.id, b.name.as_str()))
        .collect();
    let title_by_asin: BTreeMap<&str, &str> = raw_products
        .iter()
        .map(|p| (p.product_id.as_str(), p.title.as_str()))
        .collect();

    println!("=== Issue #55 checkpoint 19 follow-up: qualitative probe of {vertical_label}'s Hybrid-routed relevance gap ===\n");

    let mut printed = 0usize;
    let mut native_zero_hit = 0usize;
    let mut native_zero_hit_solr_found_real_relevance = 0usize;
    for q in &raw_queries {
        let compiled = compile(&q.query, &lexicon);

        let brand_ids: Vec<BrandId> = compiled
            .constraints
            .iter()
            .filter_map(|c| match c {
                ResolvedConstraint::Structural(StructuralConstraint::Brand(id)) => Some(*id),
                _ => None,
            })
            .collect();
        let mut solr_fq: Vec<String> = Vec::new();
        for brand_id in &brand_ids {
            if let Some(name) = brand_name_by_id.get(brand_id) {
                solr_fq.push(format!("brand:/{}/", case_insensitive_field_regex(name)));
            }
        }
        for c in &compiled.constraints {
            if let ResolvedConstraint::Attribute(Constraint::Enum { attribute, value }) = c {
                if attribute == "color" {
                    solr_fq.push(format!("color:/{}/", case_insensitive_field_regex(value)));
                }
            }
        }

        let (planned, hits) = execute_planned(
            &compiled,
            &ingested.catalog,
            &index,
            Some(&delegate as &dyn LexicalDelegate),
            K,
            &policy,
            None,
        );
        if planned.outcome != ExecutionOutcome::Hybrid {
            continue;
        }

        let gains: BTreeMap<String, f64> = q
            .judgments
            .iter()
            .filter_map(|j| {
                ingested
                    .product_id_by_asin
                    .get(&j.product_id)
                    .map(|pid| (pid.0.to_string(), label_gain(&j.label)))
            })
            .collect();
        let native_ranked: Vec<String> = hits.iter().map(|h| h.product.0.to_string()).collect();
        let Some(native_ndcg) = ndcg_at_k_graded(&native_ranked, &gains, K) else {
            continue;
        };
        let Some(solr_hit_asins) = solr_search(solr_base_url, &q.query, &solr_fq, K) else {
            continue;
        };
        let solr_ranked: Vec<String> = solr_hit_asins
            .iter()
            .filter_map(|asin| ingested.product_id_by_asin.get(asin))
            .map(|pid| pid.0.to_string())
            .collect();
        let solr_ndcg = ndcg_at_k_graded(&solr_ranked, &gains, K).unwrap_or(0.0);

        printed += 1;
        if hits.is_empty() {
            native_zero_hit += 1;
            if solr_ndcg > 0.0 {
                native_zero_hit_solr_found_real_relevance += 1;
            }
        }
        let asin_label: BTreeMap<&str, &str> = q
            .judgments
            .iter()
            .map(|j| (j.product_id.as_str(), j.label.as_str()))
            .collect();

        println!(
            "--- query={:?} constraints={:?} fq={:?} native_ndcg={native_ndcg:.4} solr_ndcg={solr_ndcg:.4} ---",
            q.query, compiled.constraints, solr_fq
        );
        println!("  native top-{K}:");
        for h in hits.iter().take(K) {
            let title = title_by_product_id.get(&h.product).copied().unwrap_or("?");
            println!("    score={:.3} title={title:?}", h.score);
        }
        println!("  solr top-{K}:");
        for asin in solr_hit_asins.iter().take(K) {
            let title = title_by_asin.get(asin.as_str()).copied().unwrap_or("?");
            let label = asin_label
                .get(asin.as_str())
                .copied()
                .unwrap_or("(unjudged)");
            println!("    asin={asin} label={label} title={title:?}");
        }
        println!();
    }

    println!(
        "=== {printed} Hybrid-routed, NDCG-scoreable {vertical_label} queries printed above ==="
    );
    println!(
        "=== zero-hit mechanism check: {native_zero_hit}/{printed} returned literally zero native \
         hits (empty candidate result, not just low-ranked); of those, {native_zero_hit_solr_found_real_relevance} \
         had Solr find a real judged-relevant (non-Irrelevant) product under the identical Brand/color \
         fq -- a 'native missed a recoverable answer' case, not 'no relevant product exists' ==="
    );
}
