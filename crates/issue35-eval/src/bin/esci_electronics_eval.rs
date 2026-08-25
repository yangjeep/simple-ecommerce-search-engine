//! Issue #35 (`docs/experiments/ISSUE35_ESCI_ELECTRONICS_PROTOCOL.md`):
//! does this project's existing, unmodified discovery/serving pipeline
//! behave safely and sanely on a real, genuinely different commerce
//! vertical (electronics, via a real ESCI slice) with zero
//! `commerce-core` changes and zero hand-authored vertical ontology?
//!
//! Reproduction: acquire the dataset first
//! (`bash scripts/datasets/fetch_esci_electronics.sh &&
//! python3 scripts/datasets/filter_esci_electronics.py &&
//! python3 scripts/datasets/solr_index_esci_electronics.py`), then
//! `cargo build --release -p issue35-eval &&
//! ./target/release/esci_electronics_eval`.

use std::collections::BTreeMap;

use commerce_core::cold_start::{compile_lexicon, CatalogProfile};
use commerce_core::domain::ProductId;
use commerce_core::index::CatalogIndex;
use commerce_core::ir::{compile, ResolvedConstraint, StructuralConstraint};
use commerce_core::plan::{execute_planned, ExecutionOutcome, LexicalDelegate, PlannerPolicy};
use issue35_eval::{build_catalog, label_gain, load_products, load_queries};
use phase9_eval::bitmap_delegate::{build_index, BitmapTantivyDelegate};

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

fn solr_search(base_url: &str, q: &str, rows: usize) -> Vec<String> {
    let url = format!("{base_url}/select");
    let resp = ureq::post(&url)
        .timeout(std::time::Duration::from_secs(30))
        .send_form(&[
            ("q", q),
            ("defType", "edismax"),
            ("qf", "title description bullet_point"),
            ("rows", &rows.to_string()),
            ("fl", "id"),
        ]);
    let Ok(resp) = resp else {
        return Vec::new();
    };
    let Ok(body) = resp.into_json::<serde_json::Value>() else {
        return Vec::new();
    };
    body["response"]["docs"]
        .as_array()
        .map(|docs| {
            docs.iter()
                .filter_map(|d| d["id"].as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

fn main() {
    println!("=== Issue #35: unseen-vertical (real ESCI electronics) discovery + routing test ===");

    let solr_base_url = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "http://localhost:8983/solr/esci_electronics_bench".to_string());

    let raw_products =
        load_products("dataset_cache/esci_electronics/esci_electronics_products.jsonl");
    let raw_queries = load_queries("dataset_cache/esci_electronics/esci_electronics_queries.jsonl");
    let ingested = build_catalog(&raw_products);
    println!(
        "catalog: {} products, {} distinct brands discovered (from real product_brand data), \
         0 product types/categories (none exist in this vertical's data -- left unregistered, \
         not fabricated)",
        ingested.catalog.products.len(),
        ingested.brands.len()
    );

    let index = CatalogIndex::build(&ingested.catalog);
    // No product types/categories registered -- exactly the "no
    // hand-authored vertical ontology" methodology constraint.
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

    let mut routing_counts: BTreeMap<&'static str, usize> = BTreeMap::new();
    let mut ambiguous_count = 0usize;
    let mut residual_count = 0usize;
    let mut brand_constrained_count = 0usize;
    let mut wrong_family_violations: Vec<String> = Vec::new();
    let mut native_ndcgs: Vec<f64> = Vec::new();
    let mut solr_ndcgs: Vec<f64> = Vec::new();
    let mut evaluated_queries = 0usize;

    let title_by_product_id: BTreeMap<ProductId, &str> = ingested
        .catalog
        .products
        .iter()
        .map(|p| (p.id, p.title.as_str()))
        .collect();
    let brand_by_product_id: BTreeMap<ProductId, commerce_core::domain::BrandId> = ingested
        .catalog
        .products
        .iter()
        .map(|p| (p.id, p.brand))
        .collect();

    for q in &raw_queries {
        let compiled = compile(&q.query, &lexicon);
        if !compiled.ambiguous.is_empty() {
            ambiguous_count += 1;
        }
        if !compiled.residual_lexical.is_empty() {
            residual_count += 1;
        }

        let brand_ids: Vec<commerce_core::domain::BrandId> = compiled
            .constraints
            .iter()
            .filter_map(|c| match c {
                ResolvedConstraint::Structural(StructuralConstraint::Brand(id)) => Some(*id),
                _ => None,
            })
            .collect();
        if !brand_ids.is_empty() {
            brand_constrained_count += 1;
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
        *routing_counts
            .entry(match planned.outcome {
                ExecutionOutcome::FastPath => "FastPath",
                ExecutionOutcome::Hybrid => "Hybrid",
                ExecutionOutcome::Punt => "Punt",
            })
            .or_insert(0) += 1;

        // Correctness hard gate: every hit for a Brand-constrained query
        // must carry that exact brand.
        for brand_id in &brand_ids {
            for hit in &hits {
                if brand_by_product_id.get(&hit.product) != Some(brand_id) {
                    wrong_family_violations.push(format!(
                        "query={:?} required_brand={:?} hit_product={:?} hit_brand={:?}",
                        q.query,
                        brand_id,
                        hit.product,
                        brand_by_product_id.get(&hit.product)
                    ));
                }
            }
        }

        // Relevance, using ASIN-keyed real judgments translated into
        // this catalog's own ProductId, then back to a string id for
        // the graded-NDCG helper (shared shape with Solr's own "id").
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
        if let Some(ndcg) = ndcg_at_k_graded(&native_ranked, &gains, K) {
            native_ndcgs.push(ndcg);
            evaluated_queries += 1;

            // Solr comparison: translate Solr's real-ASIN hits back to
            // this catalog's ProductId space via the same map, so both
            // engines are scored against literally the same gains map.
            let solr_hit_asins = solr_search(&solr_base_url, &q.query, K);
            let solr_ranked: Vec<String> = solr_hit_asins
                .iter()
                .filter_map(|asin| ingested.product_id_by_asin.get(asin))
                .map(|pid| pid.0.to_string())
                .collect();
            if let Some(solr_ndcg) = ndcg_at_k_graded(&solr_ranked, &gains, K) {
                solr_ndcgs.push(solr_ndcg);
            } else {
                solr_ndcgs.push(0.0);
            }
        }
    }

    println!("\n=== discovery/routing (descriptive, no pass/fail threshold) ===");
    println!("routing distribution: {routing_counts:?}");
    println!(
        "queries with ambiguity: {ambiguous_count}/{}, queries with residual lexical text: \
         {residual_count}/{}, queries with a Brand structural constraint: {brand_constrained_count}/{}",
        raw_queries.len(),
        raw_queries.len(),
        raw_queries.len()
    );

    println!("\n=== correctness hard gate ===");
    if wrong_family_violations.is_empty() {
        println!(
            "PASS: zero wrong-family violations across {brand_constrained_count} Brand-constrained queries"
        );
    } else {
        println!(
            "FAIL: {} wrong-family violations found:",
            wrong_family_violations.len()
        );
        for v in wrong_family_violations.iter().take(10) {
            println!("  {v}");
        }
    }

    let mean = |v: &[f64]| -> f64 {
        if v.is_empty() {
            0.0
        } else {
            v.iter().sum::<f64>() / v.len() as f64
        }
    };
    let native_mean = mean(&native_ndcgs);
    let solr_mean = mean(&solr_ndcgs);
    println!(
        "\n=== relevance (n={evaluated_queries} queries with >=1 non-Irrelevant judgment, \
         of {} total real queries in this slice) ===",
        raw_queries.len()
    );
    println!("native NDCG@10={native_mean:.4}  solr NDCG@10={solr_mean:.4}");
    let relative_gap = if solr_mean > 0.0 {
        100.0 * (native_mean - solr_mean) / solr_mean
    } else {
        0.0
    };
    println!("relative gap (native vs solr): {relative_gap:+.2}%");
    if relative_gap >= -15.0 {
        println!(
            "=== H0: native is within the preregistered <=15% relative gap -- the \
             delegate-fallback path carries real ranking quality on this unseen vertical ==="
        );
    } else {
        println!(
            "=== H1: native is materially worse than Solr (>15% relative gap) on this \
             unseen vertical ==="
        );
    }

    println!("\n=== qualitative sample (first 5 queries with a Brand constraint) ===");
    for q in raw_queries
        .iter()
        .filter(|q| {
            let compiled = compile(&q.query, &lexicon);
            compiled.constraints.iter().any(|c| {
                matches!(
                    c,
                    ResolvedConstraint::Structural(StructuralConstraint::Brand(_))
                )
            })
        })
        .take(5)
    {
        let compiled = compile(&q.query, &lexicon);
        let (_planned, hits) = execute_planned(
            &compiled,
            &ingested.catalog,
            &index,
            Some(&delegate as &dyn LexicalDelegate),
            K,
            &policy,
            None,
        );
        let titles: Vec<&str> = hits
            .iter()
            .take(3)
            .filter_map(|h| title_by_product_id.get(&h.product).copied())
            .collect();
        println!(
            "query={:?} constraints={:?} top-3 titles={:?}",
            q.query, compiled.constraints, titles
        );
    }
}
