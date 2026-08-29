//! Issue #57 Revision 2 gap closure: NDCG@10/Recall@10/MRR@10 for
//! native vs. Solr vs. Elasticsearch vs. OpenSearch vs. Havenask on each
//! real ESCI vertical slice's judged queries -- Revision 1 measured
//! **zero** relevance-quality metrics for any external engine on ESCI
//! (Issue #35's own prior NDCG evidence is Solr-only). Reuses
//! `issue35_eval`'s ingestion/label-gain (already established, Solr-only,
//! in `issue35_eval::eval::run_vertical_eval`) and
//! `comparator_eval::translate{,_es,_havenask}`'s exhaustively-matched
//! translators -- extended to ranked full-text retrieval across all four
//! external engines, not reimplemented.
//!
//! See `wands_relevance.rs`'s doc comment for the shared fairness/
//! disclosure discipline (same-constraints-every-engine, Havenask's
//! ranked-order capability gap).
//!
//! Usage: cargo run --release -p issue57-eval --bin esci_relevance -- <vertical>
//!   e.g. cargo run --release -p issue57-eval --bin esci_relevance -- electronics

use std::collections::BTreeMap;

use commerce_core::cold_start::{compile_lexicon, CatalogProfile};
use commerce_core::domain::{BrandId, CategoryId, ProductTypeId};
use commerce_core::index::CatalogIndex;
use commerce_core::ir::compile;
use commerce_core::plan::{execute_planned, PlannerPolicy};
use comparator_eval::translate::{translate_all, SolrFieldMap, StructuralNames};
use comparator_eval::translate_es::translate_all_es;
use comparator_eval::translate_havenask::translate_all_havenask;
use issue35_eval::{build_catalog, label_gain, load_products, load_queries};
use issue57_eval::{
    es_search_ids, havenask_search_ids, ndcg_recall_mrr, report_relevance, solr_search_ids,
    RelevanceRow,
};
use phase9_eval::bitmap_delegate::{build_index, BitmapTantivyDelegate};

const K: usize = 10;
const MIN_ENUM_FREQUENCY: usize = 1;

/// ESCI has no real category/product_type data (see `issue35_eval`'s own
/// doc comment) -- both name lookups are always `None`, matching that
/// crate's "left unregistered, not fabricated" convention.
struct EsciNames;

impl StructuralNames for EsciNames {
    fn brand_name(&self, _id: BrandId) -> Option<&str> {
        None
    }
    fn product_type_name(&self, _id: ProductTypeId) -> Option<&str> {
        None
    }
    fn category_name(&self, _id: CategoryId) -> Option<&str> {
        None
    }
}

fn esci_field_map() -> SolrFieldMap {
    SolrFieldMap {
        brand: Some("brand"),
        product_type: None,
        category: None,
        price_cents: None,
    }
}

fn mean(v: &[f64]) -> f64 {
    if v.is_empty() {
        0.0
    } else {
        v.iter().sum::<f64>() / v.len() as f64
    }
}

fn main() {
    let vertical = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "electronics".to_string());
    println!(
        "=== Issue #57 ESCI {vertical} relevance: NDCG@10/Recall@10/MRR@10, all 5 systems ==="
    );

    let catalog_path = format!("dataset_cache/esci_{vertical}/esci_{vertical}_products.jsonl");
    let queries_path = format!("dataset_cache/esci_{vertical}/esci_{vertical}_queries.jsonl");
    let raw_products = load_products(&catalog_path);
    let raw_queries = load_queries(&queries_path);
    let ingested = build_catalog(&raw_products);
    println!("{} real products ingested", ingested.catalog.products.len());

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
    let names = EsciNames;
    let fields = esci_field_map();

    let solr_url = format!("http://localhost:8983/solr/esci_{vertical}_bench");
    let es_url = "http://127.0.0.1:9200";
    let os_url = "http://127.0.0.1:9201";
    let havenask_url =
        std::env::var("HAVENASK_URL").unwrap_or_else(|_| "http://172.17.0.2:45800".to_string());
    let es_index = format!("esci_{vertical}_bench");
    let havenask_table = format!("esci_{vertical}");

    let mut native_scores = (Vec::new(), Vec::new(), Vec::new());
    let mut solr_scores = (Vec::new(), Vec::new(), Vec::new());
    let mut es_scores = (Vec::new(), Vec::new(), Vec::new());
    let mut os_scores = (Vec::new(), Vec::new(), Vec::new());
    let mut hv_scores = (Vec::new(), Vec::new(), Vec::new());
    let mut evaluated = 0usize;
    let mut translation_failures: Vec<String> = Vec::new();

    for q in &raw_queries {
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
        if gains.values().all(|&g| g <= 0.0) {
            continue;
        }

        let compiled = compile(&q.query, &lexicon);
        let (planned, hits) = execute_planned(
            &compiled,
            &ingested.catalog,
            &index,
            Some(&delegate as &dyn commerce_core::plan::LexicalDelegate),
            K,
            &policy,
            None,
        );
        let _ = planned;
        let native_ranked: Vec<String> = hits.iter().map(|h| h.product.0.to_string()).collect();
        let text = if compiled.residual_lexical.is_empty() {
            q.query.clone()
        } else {
            compiled.residual_lexical.join(" ")
        };

        let (solr_fq, solr_fail) = translate_all(&compiled.constraints, &fields, &names);
        let (es_filters_raw, es_fail) = translate_all_es(&compiled.constraints, &fields, &names);
        let (hv_where, hv_fail) = translate_all_havenask(&compiled.constraints, &fields, &names);
        for f in solr_fail.into_iter().chain(es_fail).chain(hv_fail) {
            translation_failures.push(format!("query={:?}: {f}", q.query));
        }
        let es_filters: Vec<serde_json::Value> = es_filters_raw
            .iter()
            .map(|s| serde_json::from_str(s).expect("es filter clause is valid JSON"))
            .collect();

        let solr_ranked = solr_search_ids(
            &solr_url,
            &text,
            &solr_fq,
            "title description bullet_point",
            K,
        )
        .expect("solr search");
        let es_ranked = es_search_ids(
            es_url,
            &es_index,
            &text,
            &["title", "description", "bullet_point"],
            &es_filters,
            K,
        )
        .expect("es search");
        let os_ranked = es_search_ids(
            os_url,
            &es_index,
            &text,
            &["title", "description", "bullet_point"],
            &es_filters,
            K,
        )
        .expect("os search");
        let hv_ranked = havenask_search_ids(
            &havenask_url,
            &havenask_table,
            "default",
            "id",
            &text,
            &hv_where,
            K,
        )
        .expect("havenask search");

        let n = ndcg_recall_mrr(&native_ranked, &gains, K);
        let s = ndcg_recall_mrr(&solr_ranked, &gains, K);
        let e = ndcg_recall_mrr(&es_ranked, &gains, K);
        let o = ndcg_recall_mrr(&os_ranked, &gains, K);
        let h = ndcg_recall_mrr(&hv_ranked, &gains, K);
        native_scores.0.push(n.0);
        native_scores.1.push(n.1);
        native_scores.2.push(n.2);
        solr_scores.0.push(s.0);
        solr_scores.1.push(s.1);
        solr_scores.2.push(s.2);
        es_scores.0.push(e.0);
        es_scores.1.push(e.1);
        es_scores.2.push(e.2);
        os_scores.0.push(o.0);
        os_scores.1.push(o.1);
        os_scores.2.push(o.2);
        hv_scores.0.push(h.0);
        hv_scores.1.push(h.1);
        hv_scores.2.push(h.2);
        evaluated += 1;
    }

    if !translation_failures.is_empty() {
        eprintln!(
            "\n=== {} structural-constraint translation failures (excluded from no engine, \
             disclosed) ===",
            translation_failures.len()
        );
        for f in translation_failures.iter().take(10) {
            eprintln!("  {f}");
        }
    }

    println!(
        "\nevaluated {evaluated}/{} queries (>=1 non-Irrelevant judgment)",
        raw_queries.len()
    );
    let dataset = format!("esci_{vertical}");
    let rows = vec![
        RelevanceRow {
            engine: "native".to_string(),
            n_queries: evaluated,
            ndcg_at_10: mean(&native_scores.0),
            recall_at_10: mean(&native_scores.1),
            mrr_at_10: mean(&native_scores.2),
        },
        RelevanceRow {
            engine: "solr".to_string(),
            n_queries: evaluated,
            ndcg_at_10: mean(&solr_scores.0),
            recall_at_10: mean(&solr_scores.1),
            mrr_at_10: mean(&solr_scores.2),
        },
        RelevanceRow {
            engine: "elasticsearch".to_string(),
            n_queries: evaluated,
            ndcg_at_10: mean(&es_scores.0),
            recall_at_10: mean(&es_scores.1),
            mrr_at_10: mean(&es_scores.2),
        },
        RelevanceRow {
            engine: "opensearch".to_string(),
            n_queries: evaluated,
            ndcg_at_10: mean(&os_scores.0),
            recall_at_10: mean(&os_scores.1),
            mrr_at_10: mean(&os_scores.2),
        },
        RelevanceRow {
            engine: "havenask_UNRANKED_capability_gap".to_string(),
            n_queries: evaluated,
            ndcg_at_10: mean(&hv_scores.0),
            recall_at_10: mean(&hv_scores.1),
            mrr_at_10: mean(&hv_scores.2),
        },
    ];
    report_relevance(&dataset, &rows);
    println!(
        "\nNOTE: the havenask row uses Havenask's own default MATCHINDEX result order, not a \
         verified relevance-ranked order (disclosed capability gap, see \
         issue57_eval::havenask_search_ids) -- do not read it as a head-to-head relevance \
         comparison against the other four engines."
    );
}
