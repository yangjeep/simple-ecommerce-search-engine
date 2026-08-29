//! Issue #57 frozen full-matrix benchmark, WANDS cell: native vs Solr vs
//! Elasticsearch vs OpenSearch vs Havenask, per
//! `docs/experiments/FULL_MATRIX_PROTOCOL.md`.
//!
//! Reuses Phase 6A's real-catalog ingestion (`phase6a_eval::{data,
//! catalog}`) and this crate's shared engine-transport/timing/reporting
//! helpers (`issue57_eval`, built while developing this cell -- see that
//! module's doc comment for the two real bugs found and fixed here: the
//! ES/OpenSearch 10,000-hit `track_total_hits` cap, and Havenask's
//! default-`''`-for-unset-STRING-column plus undefined facet tie-break).
//!
//! Four query classes are exercised per the frozen protocol's Q1-Q17
//! taxonomy: **Q9** (category filter), **Q5** (numeric range on
//! `average_rating`), **Q10** (color facet under a category filter),
//! **Q11** (lexical title/description search). Every cell's result is
//! correctness-gated against native before its timing is trusted -- a
//! mismatch aborts the run rather than silently publishing a number, per
//! CLAUDE.md's "never improve benchmark numbers by ... dropping failed
//! cases." Q11 is the one disclosed exception: lexical search is not
//! expected to produce identical candidate sets across engines with
//! different analyzers/tokenizers (Issue #57 §7), so it is reported but
//! not gated.
//!
//! Usage: cargo run --release -p issue57-eval --bin wands_full_matrix

use std::path::PathBuf;

use commerce_core::domain::CategoryId;
use commerce_core::index::CatalogIndex;
use commerce_core::ir::{ResolvedConstraint, StructuralConstraint};
use issue57_eval::{
    cell_seed, es_count, es_facet, es_text_count, escape_solr_phrase, escape_sql_literal,
    havenask_count, havenask_facet, havenask_text_count, report, run_shuffled, solr_count,
    solr_facet, solr_text_count, stats_ms, Row,
};
use phase6a_eval::{catalog as catalog_ingest, data};

fn main() {
    let catalog_path = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("dataset_cache/wands/catalog.jsonl"));

    println!("loading + ingesting real WANDS catalog...");
    let products = data::load_catalog(&catalog_path);
    let ingested = catalog_ingest::build_catalog(&products);
    let n_products = ingested.catalog.products.len();
    println!("{n_products} real products ingested");

    println!("building commerce_core structural index...");
    let index = CatalogIndex::build(&ingested.catalog);

    let solr_url = "http://localhost:8983/solr/wands_bench";
    let es_url = "http://127.0.0.1:9200";
    let os_url = "http://127.0.0.1:9201";
    // The Havenask container runs on the default Docker bridge network
    // without an explicit -p port mapping (see FULL_MATRIX_PROTOCOL.md
    // §3.1) -- reachable from the host directly via its bridge IP.
    let havenask_url =
        std::env::var("HAVENASK_URL").unwrap_or_else(|_| "http://172.17.0.2:45800".to_string());
    let es_index = "wands_bench";
    let havenask_table = "wands";

    let category_name_by_id: std::collections::HashMap<CategoryId, String> = ingested
        .categories
        .iter()
        .map(|c| (c.id, c.name.clone()))
        .collect();

    let mut rows: Vec<Row> = Vec::new();
    let mut mismatches: Vec<String> = Vec::new();

    // ---- Q9: category filter (real leaf category) ----
    let target_categories = [
        "Furniture / Bedroom Furniture / Beds & Headboards / Beds / Twin Beds",
        "Furniture / Living Room Furniture / Sofas",
        "Furniture / Kitchen & Dining Furniture / Kitchen & Dining Chairs",
    ];
    for &cat_name in &target_categories {
        let Some((&cat_id, _)) = category_name_by_id
            .iter()
            .find(|(_, name)| name.as_str() == cat_name)
        else {
            println!("SKIP (category not found in this real catalog): {cat_name}");
            continue;
        };
        let constraint = vec![ResolvedConstraint::Structural(
            StructuralConstraint::Category(cat_id),
        )];
        let fq_solr = vec![format!(
            "category_leaf:\"{}\"",
            escape_solr_phrase(cat_name)
        )];
        let filter_es =
            vec![serde_json::json!({"term": {"category_leaf": cat_name.to_lowercase()}})];
        let hv_where = format!(
            " where category_leaf = '{}'",
            escape_sql_literal(&cat_name.to_lowercase())
        );

        let seed = cell_seed(&["wands", "Q9_category_filter", cat_name]);
        let engines: Vec<(&str, Box<dyn FnMut() -> u64>)> = vec![
            ("native", Box::new(|| index.indexed_candidates(&constraint).len() as u64)),
            ("solr", Box::new(|| solr_count(solr_url, &fq_solr).expect("solr count"))),
            ("elasticsearch", Box::new(|| es_count(es_url, es_index, &filter_es).expect("es count"))),
            ("opensearch", Box::new(|| es_count(os_url, es_index, &filter_es).expect("os count"))),
            ("havenask", Box::new(|| {
                havenask_count(&havenask_url, havenask_table, &hv_where).expect("havenask count")
            })),
        ];
        let (mut results, engine_order) = run_shuffled(seed, engines);
        let (native_ns, native_count) = results.remove("native").unwrap();
        let (solr_ns, solr_count_v) = results.remove("solr").unwrap();
        let (es_ns, es_c) = results.remove("elasticsearch").unwrap();
        let (os_ns, os_c) = results.remove("opensearch").unwrap();
        let (hv_ns, hv_c) = results.remove("havenask").unwrap();

        let counts = vec![
            ("solr".to_string(), solr_count_v),
            ("elasticsearch".to_string(), es_c),
            ("opensearch".to_string(), os_c),
            ("havenask".to_string(), hv_c),
        ];
        let counts_match = counts.iter().all(|(_, c)| *c == native_count);
        if !counts_match {
            mismatches.push(format!(
                "Q9 category={cat_name}: native={native_count} {counts:?}"
            ));
        }
        let (n_mean, n_p50, n_p99) = stats_ms(native_ns);
        let (s_mean, s_p50, s_p99) = stats_ms(solr_ns);
        let (e_mean, e_p50, e_p99) = stats_ms(es_ns);
        let (o_mean, o_p50, o_p99) = stats_ms(os_ns);
        let (h_mean, h_p50, h_p99) = stats_ms(hv_ns);
        rows.push(Row {
            class: "Q9_category_filter".to_string(),
            key: cat_name.to_string(),
            native_count,
            counts,
            counts_match,
            timings_ms: vec![
                ("native".to_string(), n_mean, n_p50, n_p99),
                ("solr".to_string(), s_mean, s_p50, s_p99),
                ("elasticsearch".to_string(), e_mean, e_p50, e_p99),
                ("opensearch".to_string(), o_mean, o_p50, o_p99),
                ("havenask".to_string(), h_mean, h_p50, h_p99),
            ],
            engine_order,
        });
    }

    // ---- Q5: numeric range on average_rating ----
    for &threshold in &[4.5, 3.0] {
        let constraint = vec![ResolvedConstraint::Attribute(
            commerce_core::domain::Constraint::Numeric {
                attribute: "average_rating".to_string(),
                op: commerce_core::domain::NumericOp::Gte,
                value: threshold,
            },
        )];
        let fq_solr = vec![format!("average_rating:[{threshold} TO *]")];
        let filter_es = vec![serde_json::json!({"range": {"average_rating": {"gte": threshold}}})];
        let hv_where = format!(" where average_rating >= {threshold}");

        let seed = cell_seed(&["wands", "Q5_numeric_range", &threshold.to_string()]);
        let engines: Vec<(&str, Box<dyn FnMut() -> u64>)> = vec![
            ("native", Box::new(|| index.indexed_candidates(&constraint).len() as u64)),
            ("solr", Box::new(|| solr_count(solr_url, &fq_solr).expect("solr count"))),
            ("elasticsearch", Box::new(|| es_count(es_url, es_index, &filter_es).expect("es count"))),
            ("opensearch", Box::new(|| es_count(os_url, es_index, &filter_es).expect("os count"))),
            ("havenask", Box::new(|| {
                havenask_count(&havenask_url, havenask_table, &hv_where).expect("havenask count")
            })),
        ];
        let (mut results, engine_order) = run_shuffled(seed, engines);
        let (native_ns, native_count) = results.remove("native").unwrap();
        let (solr_ns, solr_count_v) = results.remove("solr").unwrap();
        let (es_ns, es_c) = results.remove("elasticsearch").unwrap();
        let (os_ns, os_c) = results.remove("opensearch").unwrap();
        let (hv_ns, hv_c) = results.remove("havenask").unwrap();

        let counts = vec![
            ("solr".to_string(), solr_count_v),
            ("elasticsearch".to_string(), es_c),
            ("opensearch".to_string(), os_c),
            ("havenask".to_string(), hv_c),
        ];
        let counts_match = counts.iter().all(|(_, c)| *c == native_count);
        if !counts_match {
            mismatches.push(format!(
                "Q5 average_rating>={threshold}: native={native_count} {counts:?}"
            ));
        }
        let (n_mean, n_p50, n_p99) = stats_ms(native_ns);
        let (s_mean, s_p50, s_p99) = stats_ms(solr_ns);
        let (e_mean, e_p50, e_p99) = stats_ms(es_ns);
        let (o_mean, o_p50, o_p99) = stats_ms(os_ns);
        let (h_mean, h_p50, h_p99) = stats_ms(hv_ns);
        rows.push(Row {
            class: "Q5_numeric_range".to_string(),
            key: format!("average_rating>={threshold}"),
            native_count,
            counts,
            counts_match,
            timings_ms: vec![
                ("native".to_string(), n_mean, n_p50, n_p99),
                ("solr".to_string(), s_mean, s_p50, s_p99),
                ("elasticsearch".to_string(), e_mean, e_p50, e_p99),
                ("opensearch".to_string(), o_mean, o_p50, o_p99),
                ("havenask".to_string(), h_mean, h_p50, h_p99),
            ],
            engine_order,
        });
    }

    // ---- Q10: color facet under category filter ----
    for &cat_name in &target_categories[..1] {
        let Some((&cat_id, _)) = category_name_by_id
            .iter()
            .find(|(_, name)| name.as_str() == cat_name)
        else {
            continue;
        };
        let constraint = vec![ResolvedConstraint::Structural(
            StructuralConstraint::Category(cat_id),
        )];
        let candidates = index.indexed_candidates(&constraint);
        let fq_solr = vec![format!(
            "category_leaf:\"{}\"",
            escape_solr_phrase(cat_name)
        )];
        let filter_es =
            vec![serde_json::json!({"term": {"category_leaf": cat_name.to_lowercase()}})];
        let hv_where = format!(
            " where category_leaf = '{}'",
            escape_sql_literal(&cat_name.to_lowercase())
        );

        let seed = cell_seed(&["wands", "Q10_color_facet_under_category", cat_name]);
        let engines: Vec<(&str, Box<dyn FnMut() -> std::collections::BTreeMap<String, u64>>)> = vec![
            ("native", Box::new(|| {
                let mut top: Vec<(String, u64)> = index
                    .facet_counts_by_scan(&candidates, &ingested.catalog, "color")
                    .into_iter()
                    .collect();
                top.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
                top.truncate(20);
                top.into_iter().collect()
            })),
            ("solr", Box::new(|| solr_facet(solr_url, &fq_solr, "color", 20).expect("solr facet"))),
            ("elasticsearch", Box::new(|| {
                es_facet(es_url, es_index, &filter_es, "color", 20).expect("es facet")
            })),
            ("opensearch", Box::new(|| {
                es_facet(os_url, es_index, &filter_es, "color", 20).expect("os facet")
            })),
            ("havenask", Box::new(|| {
                havenask_facet(&havenask_url, havenask_table, &hv_where, "color", 20)
                    .expect("havenask facet")
            })),
        ];
        let (mut results, engine_order) = run_shuffled(seed, engines);
        let (native_ns, native_facets) = results.remove("native").unwrap();
        let (solr_ns, solr_facets) = results.remove("solr").unwrap();
        let (es_ns, es_facets) = results.remove("elasticsearch").unwrap();
        let (os_ns, os_facets) = results.remove("opensearch").unwrap();
        let (hv_ns, hv_facets) = results.remove("havenask").unwrap();

        // Native's facet map keys are raw (non-lowercased); the engine
        // maps are lowercased -- WANDS' own color field has no casing
        // collisions (confirmed by profile_wands.py), so lowercasing
        // native's keys for this comparison is an identity mapping in
        // practice, not a lossy one.
        let native_lower: std::collections::BTreeMap<String, u64> = native_facets
            .iter()
            .map(|(k, v)| (k.to_lowercase(), *v))
            .collect();
        let solr_lower: std::collections::BTreeMap<String, u64> = solr_facets
            .iter()
            .map(|(k, v)| (k.to_lowercase(), *v))
            .collect();
        let facets_match = native_lower == solr_lower
            && native_lower == es_facets
            && native_lower == os_facets
            && native_lower == hv_facets;
        if !facets_match {
            mismatches.push(format!(
                "Q10 color_facet under {cat_name}: native={native_lower:?} solr={solr_lower:?} es={es_facets:?} os={os_facets:?} havenask={hv_facets:?}"
            ));
        }
        let (n_mean, n_p50, n_p99) = stats_ms(native_ns);
        let (s_mean, s_p50, s_p99) = stats_ms(solr_ns);
        let (e_mean, e_p50, e_p99) = stats_ms(es_ns);
        let (o_mean, o_p50, o_p99) = stats_ms(os_ns);
        let (h_mean, h_p50, h_p99) = stats_ms(hv_ns);
        rows.push(Row {
            class: "Q10_color_facet_under_category".to_string(),
            key: cat_name.to_string(),
            native_count: candidates.len() as u64,
            counts: vec![],
            counts_match: facets_match,
            timings_ms: vec![
                ("native".to_string(), n_mean, n_p50, n_p99),
                ("solr".to_string(), s_mean, s_p50, s_p99),
                ("elasticsearch".to_string(), e_mean, e_p50, e_p99),
                ("opensearch".to_string(), o_mean, o_p50, o_p99),
                ("havenask".to_string(), h_mean, h_p50, h_p99),
            ],
            engine_order,
        });
    }

    // ---- Q11: lexical title/description search (NOT correctness-gated
    // -- see this binary's doc comment) ----
    for &term in &["wood", "storage"] {
        let text_constraint = vec![ResolvedConstraint::Attribute(
            commerce_core::domain::Constraint::Text {
                attribute: "title".to_string(),
                contains: term.to_string(),
            },
        )];
        let seed = cell_seed(&["wands", "Q11_lexical_title_search", term]);
        let engines: Vec<(&str, Box<dyn FnMut() -> u64>)> = vec![
            ("native", Box::new(|| {
                index
                    .indexed_candidates(&text_constraint)
                    .iter()
                    .filter(|&ord| {
                        index
                            .variant_id_at(ord)
                            .and_then(|vid| index.lookup_variant(&ingested.catalog, vid))
                            .map(|(p, _)| p.title.to_lowercase().contains(term))
                            .unwrap_or(false)
                    })
                    .count() as u64
            })),
            ("solr", Box::new(|| {
                solr_text_count(solr_url, term, "title description").expect("solr text")
            })),
            ("elasticsearch", Box::new(|| {
                es_text_count(es_url, es_index, term, &["title", "description"]).expect("es text")
            })),
            ("opensearch", Box::new(|| {
                es_text_count(os_url, es_index, term, &["title", "description"]).expect("os text")
            })),
            ("havenask", Box::new(|| {
                havenask_text_count(&havenask_url, havenask_table, "default", term)
                    .expect("havenask text")
            })),
        ];
        let (mut results, engine_order) = run_shuffled(seed, engines);
        let (native_ns, native_count) = results.remove("native").unwrap();
        let (solr_ns, solr_count_v) = results.remove("solr").unwrap();
        let (es_ns, es_c) = results.remove("elasticsearch").unwrap();
        let (os_ns, os_c) = results.remove("opensearch").unwrap();
        let (hv_ns, hv_c) = results.remove("havenask").unwrap();

        let counts = vec![
            ("solr".to_string(), solr_count_v),
            ("elasticsearch".to_string(), es_c),
            ("opensearch".to_string(), os_c),
            ("havenask".to_string(), hv_c),
        ];
        let (n_mean, n_p50, n_p99) = stats_ms(native_ns);
        let (s_mean, s_p50, s_p99) = stats_ms(solr_ns);
        let (e_mean, e_p50, e_p99) = stats_ms(es_ns);
        let (o_mean, o_p50, o_p99) = stats_ms(os_ns);
        let (h_mean, h_p50, h_p99) = stats_ms(hv_ns);
        rows.push(Row {
            class: "Q11_lexical_title_search_NOT_correctness_gated".to_string(),
            key: term.to_string(),
            native_count,
            counts,
            counts_match: true,
            timings_ms: vec![
                ("native".to_string(), n_mean, n_p50, n_p99),
                ("solr".to_string(), s_mean, s_p50, s_p99),
                ("elasticsearch".to_string(), e_mean, e_p50, e_p99),
                ("opensearch".to_string(), o_mean, o_p50, o_p99),
                ("havenask".to_string(), h_mean, h_p50, h_p99),
            ],
            engine_order,
        });
    }

    report("wands", &rows, &mismatches);
}
