//! Issue #57 Revision 2 gap closure: NDCG@10/Recall@10/MRR@10 for
//! native vs. Solr vs. Elasticsearch vs. OpenSearch vs. Havenask on
//! WANDS's real 480 queries / 233,448 judgments -- Revision 1 measured
//! **zero** relevance-quality metrics for any external engine
//! (adversarial review, Lens 3). Reuses `phase9_eval::wands_relevance`'s
//! WANDS-label scoring (already established, single-engine, for Solr
//! only in `p9_e02_wands_physical_advantage.rs`) and
//! `comparator_eval::translate{,_es,_havenask}`'s exhaustively-matched
//! structural-constraint translators (already established for
//! count/facet queries) -- extended here to ranked full-text retrieval,
//! not reimplemented.
//!
//! Every engine answers the SAME query text plus the SAME structural
//! constraints native's own `compile()` resolves for that query (Issue
//! #57 fairness contract: no engine gets an easier/broader question).
//! Havenask's ranked-order output is a disclosed capability gap (see
//! `issue57_eval::havenask_search_ids`'s doc comment) -- its row is
//! reported but explicitly labeled non-comparable, not silently included
//! in a head-to-head relevance ranking claim.
//!
//! Usage: cargo run --release -p issue57-eval --bin wands_relevance

use std::collections::{BTreeMap, HashMap};

use commerce_core::cold_start::{compile_lexicon, CatalogProfile};
use commerce_core::domain::{BrandId, CategoryId, ProductTypeId};
use commerce_core::index::CatalogIndex;
use commerce_core::ir::compile;
use commerce_core::plan::{execute_planned, PlannerPolicy};
use comparator_eval::translate::{translate_all, SolrFieldMap, StructuralNames};
use comparator_eval::translate_es::translate_all_es;
use comparator_eval::translate_havenask::translate_all_havenask;
use issue57_eval::{es_search_ids, havenask_search_ids, ndcg_recall_mrr, report_relevance, solr_search_ids, RelevanceRow};
use phase6a_eval::{catalog as catalog_ingest, data};
use phase9_eval::bitmap_delegate::{build_index, BitmapTantivyDelegate};
use phase9_eval::wands_relevance::WandsLabel;

const K: usize = 10;
const MIN_ENUM_FREQUENCY: usize = 1;

struct WandsQuery {
    text: String,
}

fn load_queries(path: &str) -> Vec<WandsQuery> {
    let content = std::fs::read_to_string(path).expect("read query.csv");
    content
        .lines()
        .skip(1)
        .filter_map(|line| {
            let mut parts = line.splitn(3, '\t');
            let _id = parts.next()?;
            let text = parts.next()?.to_string();
            Some(WandsQuery { text })
        })
        .collect()
}

/// `query_id -> (wands product_id -> label)`, keyed positionally by
/// query.csv's row order (query_id is a stable small integer string in
/// this dataset, `"0"..="479"`), matching `p9_e02`'s own loader.
fn load_labels(path: &str) -> BTreeMap<String, BTreeMap<String, WandsLabel>> {
    let content = std::fs::read_to_string(path).expect("read label.csv");
    let mut judged: BTreeMap<String, BTreeMap<String, WandsLabel>> = BTreeMap::new();
    for line in content.lines().skip(1) {
        let mut parts = line.splitn(4, '\t');
        let Some(_id) = parts.next() else { continue };
        let Some(query_id) = parts.next() else {
            continue;
        };
        let Some(product_id) = parts.next() else {
            continue;
        };
        let Some(raw_label) = parts.next() else {
            continue;
        };
        let Some(label) = WandsLabel::parse(raw_label) else {
            continue;
        };
        judged
            .entry(query_id.to_string())
            .or_default()
            .insert(product_id.to_string(), label);
    }
    judged
}

struct WandsNames<'a> {
    category_name_by_id: &'a HashMap<CategoryId, String>,
    product_type_name_by_id: &'a HashMap<ProductTypeId, String>,
}

impl StructuralNames for WandsNames<'_> {
    fn brand_name(&self, _id: BrandId) -> Option<&str> {
        None
    }
    fn product_type_name(&self, id: ProductTypeId) -> Option<&str> {
        self.product_type_name_by_id.get(&id).map(String::as_str)
    }
    fn category_name(&self, id: CategoryId) -> Option<&str> {
        self.category_name_by_id.get(&id).map(String::as_str)
    }
}

fn wands_field_map() -> SolrFieldMap {
    SolrFieldMap {
        brand: None,
        product_type: Some("product_class"),
        category: Some("category_leaf"),
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
    println!("=== Issue #57 WANDS relevance: NDCG@10/Recall@10/MRR@10, all 5 systems ===");

    let products = data::load_catalog(&std::path::PathBuf::from(
        "dataset_cache/wands/catalog.jsonl",
    ));
    let ingested = catalog_ingest::build_catalog(&products);
    println!("{} real products ingested", ingested.catalog.products.len());

    let index = CatalogIndex::build(&ingested.catalog);
    let profile = CatalogProfile::build(
        &ingested.catalog,
        &[],
        &ingested.product_types,
        &ingested.categories,
    );
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

    let category_name_by_id: HashMap<CategoryId, String> = ingested
        .categories
        .iter()
        .map(|c| (c.id, c.name.clone()))
        .collect();
    let product_type_name_by_id: HashMap<ProductTypeId, String> = ingested
        .product_types
        .iter()
        .map(|pt| (pt.id, pt.name.clone()))
        .collect();
    let names = WandsNames {
        category_name_by_id: &category_name_by_id,
        product_type_name_by_id: &product_type_name_by_id,
    };
    let fields = wands_field_map();

    let queries = load_queries("dataset_cache/wands/query.csv");
    let labels = load_labels("dataset_cache/wands/label.csv");

    let solr_url = "http://localhost:8983/solr/wands_bench";
    let es_url = "http://127.0.0.1:9200";
    let os_url = "http://127.0.0.1:9201";
    let havenask_url =
        std::env::var("HAVENASK_URL").unwrap_or_else(|_| "http://172.17.0.2:45800".to_string());
    let es_index = "wands_bench";
    let havenask_table = "wands";

    let mut native_scores = (Vec::new(), Vec::new(), Vec::new());
    let mut solr_scores = (Vec::new(), Vec::new(), Vec::new());
    let mut es_scores = (Vec::new(), Vec::new(), Vec::new());
    let mut os_scores = (Vec::new(), Vec::new(), Vec::new());
    let mut hv_scores = (Vec::new(), Vec::new(), Vec::new());
    let mut evaluated = 0usize;
    let mut translation_failures: Vec<String> = Vec::new();

    for (i, q) in queries.iter().enumerate() {
        let query_id = i.to_string();
        let Some(judged) = labels.get(&query_id) else {
            continue;
        };
        let gains: BTreeMap<String, f64> = judged
            .iter()
            .filter_map(|(wands_pid, label)| {
                ingested
                    .wands_id_to_product_id
                    .get(wands_pid)
                    .map(|pid| {
                        (
                            pid.0.to_string(),
                            match label {
                                WandsLabel::Exact => 2.0,
                                WandsLabel::Partial => 1.0,
                                WandsLabel::Irrelevant => 0.0,
                            },
                        )
                    })
            })
            .collect();
        if gains.values().all(|&g| g <= 0.0) {
            continue;
        }

        let compiled = compile(&q.text, &lexicon);
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
            q.text.clone()
        } else {
            compiled.residual_lexical.join(" ")
        };

        let (solr_fq, solr_fail) = translate_all(&compiled.constraints, &fields, &names);
        let (es_filters_raw, es_fail) = translate_all_es(&compiled.constraints, &fields, &names);
        let (hv_where, hv_fail) = translate_all_havenask(&compiled.constraints, &fields, &names);
        for f in solr_fail.into_iter().chain(es_fail).chain(hv_fail) {
            translation_failures.push(format!("query={:?}: {f}", q.text));
        }
        let es_filters: Vec<serde_json::Value> = es_filters_raw
            .iter()
            .map(|s| serde_json::from_str(s).expect("es filter clause is valid JSON"))
            .collect();

        let solr_ranked = solr_search_ids(solr_url, &text, &solr_fq, "title description", K)
            .expect("solr search");
        let es_ranked = es_search_ids(
            es_url,
            es_index,
            &text,
            &["title", "description"],
            &es_filters,
            K,
        )
        .expect("es search");
        let os_ranked = es_search_ids(
            os_url,
            es_index,
            &text,
            &["title", "description"],
            &es_filters,
            K,
        )
        .expect("os search");
        let hv_ranked = havenask_search_ids(
            &havenask_url,
            havenask_table,
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

    println!("\nevaluated {evaluated}/{} queries (>=1 non-Irrelevant judgment)", queries.len());
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
    report_relevance("wands", &rows);
    println!(
        "\nNOTE: the havenask row uses Havenask's own default MATCHINDEX result order, not a \
         verified relevance-ranked order (disclosed capability gap, see \
         issue57_eval::havenask_search_ids) -- do not read it as a head-to-head relevance \
         comparison against the other four engines."
    );
}
