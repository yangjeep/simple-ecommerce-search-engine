//! Issue #57 frozen full-matrix benchmark, ESCI vertical cell: native vs
//! Solr vs Elasticsearch vs OpenSearch vs Havenask, per
//! `docs/experiments/FULL_MATRIX_PROTOCOL.md`.
//!
//! Reuses Issue #35's real-catalog ingestion (`issue35_eval`) and this
//! crate's shared engine-transport/timing/reporting helpers
//! (`issue57_eval`). Exercises **Q2** (Brand filter -- ESCI's headline
//! capability per Issue #35, and the class its own comparator-hardening
//! audit found real casing collisions on) and **Q11** (lexical
//! title/description/bullet_point search, not correctness-gated, same
//! disclosed exception as the WANDS cell).
//!
//! Usage: cargo run --release -p issue57-eval --bin esci_full_matrix -- <vertical>
//!   e.g. cargo run --release -p issue57-eval --bin esci_full_matrix -- electronics

use std::collections::HashMap;

use commerce_core::domain::BrandId;
use commerce_core::index::CatalogIndex;
use commerce_core::ir::{ResolvedConstraint, StructuralConstraint};
use issue35_eval::{build_catalog, load_products};
use issue57_eval::{
    es_count, es_text_count, escape_sql_literal, havenask_count, havenask_text_count, report,
    solr_count, solr_text_count, stats_ms, time_reps, Row,
};

fn main() {
    let vertical = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "electronics".to_string());
    let catalog_path = format!("dataset_cache/esci_{vertical}/esci_{vertical}_products.jsonl");

    println!("loading + ingesting real ESCI {vertical} slice...");
    let products = load_products(&catalog_path);
    let ingested = build_catalog(&products);
    let n_products = ingested.catalog.products.len();
    println!("{n_products} real products ingested");

    println!("building commerce_core structural index...");
    let index = CatalogIndex::build(&ingested.catalog);

    let solr_url = format!("http://localhost:8983/solr/esci_{vertical}_bench");
    let es_url = "http://127.0.0.1:9200";
    let os_url = "http://127.0.0.1:9201";
    let havenask_url =
        std::env::var("HAVENASK_URL").unwrap_or_else(|_| "http://172.17.0.2:45800".to_string());
    let es_index = format!("esci_{vertical}_bench");
    let havenask_table = format!("esci_{vertical}");

    let brand_name_by_id: HashMap<BrandId, String> = ingested
        .brands
        .iter()
        .map(|b| (b.id, b.name.clone()))
        .collect();

    // Pick the brands with the most real products (a meaningful,
    // non-trivial candidate set, not an arbitrary single item) up to 3,
    // by scanning the real ingested catalog rather than guessing names.
    let mut brand_counts: HashMap<BrandId, usize> = HashMap::new();
    for p in &ingested.catalog.products {
        if p.brand != BrandId(0) {
            *brand_counts.entry(p.brand).or_insert(0) += 1;
        }
    }
    let mut ranked: Vec<(BrandId, usize)> = brand_counts.into_iter().collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0 .0.cmp(&b.0 .0)));
    // Real ESCI data has genuine brand-string casing collisions (e.g.
    // "FilterBuy" vs "Filterbuy" -- confirmed live, same real company,
    // two distinct seller-entered casings). Native's `Brand` structural
    // constraint is case-sensitive-identity as `issue35_eval::build_catalog`
    // currently interns it (unchanged from Issue #35's own ingestion),
    // while every comparator translation (Solr's existing
    // `case_insensitive_field_regex`, and this revision's ES/Havenask
    // lowercase-both-sides equivalent) is deliberately case-insensitive
    // to match real-world brand identity despite messy marketplace data
    // -- see this dataset's own Q2b row below for a disclosed, NOT-gated
    // demonstration of exactly this gap. To keep the *gated* Q2 rows a
    // clean apples-to-apples comparison, casing-collision brands are
    // skipped here rather than silently producing a confusing mismatch.
    let mut variants_by_lower: HashMap<String, Vec<(BrandId, String)>> = HashMap::new();
    for (id, _count) in &ranked {
        let name = brand_name_by_id[id].clone();
        variants_by_lower
            .entry(name.to_lowercase())
            .or_default()
            .push((*id, name));
    }
    let mut collision_pair: Option<(BrandId, String, BrandId, String)> = None;
    for variants in variants_by_lower.values() {
        if variants.len() >= 2 && collision_pair.is_none() {
            collision_pair = Some((
                variants[0].0,
                variants[0].1.clone(),
                variants[1].0,
                variants[1].1.clone(),
            ));
        }
    }
    let target_brands: Vec<(BrandId, String)> = ranked
        .iter()
        .filter(|(id, _)| variants_by_lower[&brand_name_by_id[id].to_lowercase()].len() == 1)
        .take(3)
        .map(|(id, _)| (*id, brand_name_by_id[id].clone()))
        .collect();

    let mut rows: Vec<Row> = Vec::new();
    let mut mismatches: Vec<String> = Vec::new();

    // ---- Q2: Brand filter (ESCI's headline capability) ----
    for (brand_id, brand_name) in &target_brands {
        let constraint = vec![ResolvedConstraint::Structural(StructuralConstraint::Brand(
            *brand_id,
        ))];
        let (native_ns, native_count) =
            time_reps(|| index.indexed_candidates(&constraint).len() as u64);

        let fq_solr = vec![format!(
            "brand:/{}/",
            comparator_eval::case_insensitive_field_regex(brand_name)
        )];
        let (solr_ns, solr_count_v) =
            time_reps(|| solr_count(&solr_url, &fq_solr).expect("solr count"));

        let filter_es = vec![serde_json::json!({"term": {"brand": brand_name.to_lowercase()}})];
        let (es_ns, es_c) =
            time_reps(|| es_count(es_url, &es_index, &filter_es).expect("es count"));
        let (os_ns, os_c) =
            time_reps(|| es_count(os_url, &es_index, &filter_es).expect("os count"));

        let hv_where = format!(
            " where brand = '{}'",
            escape_sql_literal(&brand_name.to_lowercase())
        );
        let (hv_ns, hv_c) = time_reps(|| {
            havenask_count(&havenask_url, &havenask_table, &hv_where).expect("havenask count")
        });

        let counts = vec![
            ("solr".to_string(), solr_count_v),
            ("elasticsearch".to_string(), es_c),
            ("opensearch".to_string(), os_c),
            ("havenask".to_string(), hv_c),
        ];
        let counts_match = counts.iter().all(|(_, c)| *c == native_count);
        if !counts_match {
            mismatches.push(format!(
                "Q2 brand={brand_name}: native={native_count} {counts:?} (a Havenask mismatch on this vertical may be the single disclosed ingestion-failure row -- see FULL_MATRIX_PROTOCOL.md's ESCI addendum)"
            ));
        }
        let (n_mean, n_p50, n_p99) = stats_ms(native_ns);
        let (s_mean, s_p50, s_p99) = stats_ms(solr_ns);
        let (e_mean, e_p50, e_p99) = stats_ms(es_ns);
        let (o_mean, o_p50, o_p99) = stats_ms(os_ns);
        let (h_mean, h_p50, h_p99) = stats_ms(hv_ns);
        rows.push(Row {
            class: "Q2_brand_filter".to_string(),
            key: brand_name.clone(),
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
        });
    }

    // ---- Q2b: disclosed, NOT-gated demonstration of a real ESCI brand
    // casing collision (see the comment above target_brands' selection).
    // native's two colliding BrandIds are queried separately; every
    // comparator engine is case-insensitive and returns the SAME
    // (merged) count for both queries -- expected, not a defect.
    if let Some((id_a, name_a, id_b, name_b)) = collision_pair {
        for (label, brand_id, brand_name) in [
            ("variant_a", id_a, name_a.clone()),
            ("variant_b", id_b, name_b.clone()),
        ] {
            let constraint = vec![ResolvedConstraint::Structural(StructuralConstraint::Brand(
                brand_id,
            ))];
            let native_count = index.indexed_candidates(&constraint).len() as u64;
            let hv_where = format!(
                " where brand = '{}'",
                escape_sql_literal(&brand_name.to_lowercase())
            );
            let hv_c =
                havenask_count(&havenask_url, &havenask_table, &hv_where).expect("havenask count");
            rows.push(Row {
                class: "Q2b_brand_casing_collision_NOT_gated".to_string(),
                key: format!("{label}={brand_name} (collides with {name_a}/{name_b})"),
                native_count,
                counts: vec![("havenask_case_insensitive_merged".to_string(), hv_c)],
                counts_match: true,
                timings_ms: vec![],
            });
        }
    }

    // ---- Q11: lexical title/description/bullet_point search (NOT
    // correctness-gated -- different analyzers/tokenizers are expected
    // to produce different candidate sets, per Issue #57 §7) ----
    let lexical_terms: &[&str] = match vertical.as_str() {
        "electronics" => &["monitor", "cable"],
        "automotive" => &["tire", "oil"],
        "beauty" => &["shampoo", "cream"],
        _ => &["the"],
    };
    for &term in lexical_terms {
        let text_constraint = vec![ResolvedConstraint::Attribute(
            commerce_core::domain::Constraint::Text {
                attribute: "title".to_string(),
                contains: term.to_string(),
            },
        )];
        let (native_ns, native_count) = time_reps(|| {
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
        });

        let (solr_ns, solr_count_v) = time_reps(|| {
            solr_text_count(&solr_url, term, "title description bullet_point").expect("solr text")
        });
        let (es_ns, es_c) = time_reps(|| {
            es_text_count(
                es_url,
                &es_index,
                term,
                &["title", "description", "bullet_point"],
            )
            .expect("es text")
        });
        let (os_ns, os_c) = time_reps(|| {
            es_text_count(
                os_url,
                &es_index,
                term,
                &["title", "description", "bullet_point"],
            )
            .expect("os text")
        });
        let (hv_ns, hv_c) = time_reps(|| {
            havenask_text_count(&havenask_url, &havenask_table, "default", term)
                .expect("havenask text")
        });

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
            class: "Q11_lexical_search_NOT_correctness_gated".to_string(),
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
        });
    }

    report(&format!("esci_{vertical}"), &rows, &mismatches);
}
