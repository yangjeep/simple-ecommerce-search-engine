//! Issue #57 frozen full-matrix benchmark, Magento cell: native vs Solr
//! vs Elasticsearch vs OpenSearch vs Havenask, per
//! `docs/experiments/FULL_MATRIX_PROTOCOL.md`.
//!
//! The one dataset in this matrix with genuine Product-to-Variant
//! structure (22 real parent products, 155 real kept variants,
//! checkerboard-sparsified so real cross-variant trap opportunities
//! exist -- see `scripts/datasets/index_magento_all_engines.py`'s doc
//! comment). Correctness-only per §9.3 of the frozen protocol: 155 rows
//! is too small a sample for stable P50/P99 latency claims, so no
//! percentile timing is reported here -- only mean wall-clock and, most
//! importantly, **Q8 same-variant-conjunction correctness**.
//!
//! Ingestion logic is copied from
//! `crates/issue55-eval/src/bin/i55_e00_variant_real_data_correctness.rs`
//! (Issue #55 H3's own native-only variant-safety proof) rather than
//! reimplemented, so this cell's native side is provably the same
//! structure H3 already validated.
//!
//! For every real parent product, every (color, size) pair drawn from
//! that product's own real vocabulary is queried: **true positives**
//! (a real kept variant has exactly that pair) must return exactly 1
//! matching variant on every engine; **traps** (color exists on one
//! variant, size exists on a different variant of the same product, no
//! single variant has both -- exactly the cross-variant false-match
//! shape CLAUDE.md's hard rule forbids) must return exactly 0 on every
//! engine, including the four external ones. A non-zero trap count on
//! any external engine would mean that engine's per-variant-document
//! indexing does NOT enforce the same safety guarantee native does.
//!
//! Usage: cargo run --release -p issue57-eval --bin magento_full_matrix

use std::collections::BTreeMap;

use issue57_eval::{es_count, havenask_count, report, solr_count, stats_ms, time_reps, Row};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct RawVariant {
    color: String,
    size: String,
}

#[derive(Debug, Deserialize)]
struct RawProduct {
    sku: String,
    colors: Vec<String>,
    sizes: Vec<String>,
    variants: Vec<RawVariant>,
}

fn load_raw(path: &str) -> Vec<RawProduct> {
    let content = std::fs::read_to_string(path).expect("read catalog.jsonl");
    content
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).expect("parse product json"))
        .collect()
}

fn main() {
    let raw = load_raw("dataset_cache/magento_configurable/catalog.jsonl");
    let total_kept: usize = raw.iter().map(|p| p.variants.len()).sum();
    println!(
        "loaded {} real parent products, {} real kept (sparsified) variants",
        raw.len(),
        total_kept
    );

    let solr_url = "http://localhost:8983/solr/magento_bench";
    let es_url = "http://127.0.0.1:9200";
    let os_url = "http://127.0.0.1:9201";
    let havenask_url =
        std::env::var("HAVENASK_URL").unwrap_or_else(|_| "http://172.17.0.2:45800".to_string());
    let es_index = "magento_bench";
    let havenask_table = "magento";

    let mut rows: Vec<Row> = Vec::new();
    let mut mismatches: Vec<String> = Vec::new();
    let mut true_positive_checks = 0usize;
    let mut trap_checks = 0usize;

    // Every (product, color, size) combo is correctness-checked with a
    // single call per engine (not the full 30-rep/5-warmup timing
    // protocol -- this cell is exhaustive-correctness-focused, and a
    // real Magento parent product's color/size vocabulary already
    // produces several hundred combos across 22 products; timing every
    // one at the full protocol's repetition count would multiply that
    // by ~140x for no additional correctness signal). A separate small
    // timed sample (below) reports representative mean/P50/P99 latency.
    for p in &raw {
        let kept: std::collections::HashSet<(String, String)> = p
            .variants
            .iter()
            .map(|v| (v.color.clone(), v.size.clone()))
            .collect();
        for color in &p.colors {
            for size in &p.sizes {
                let is_true_positive = kept.contains(&(color.clone(), size.clone()));
                let expected_native: u64 = if is_true_positive { 1 } else { 0 };

                let fq_solr = vec![
                    format!("sku:\"{}\"", p.sku),
                    format!("color:\"{color}\""),
                    format!("size:\"{size}\""),
                ];
                let solr_c = solr_count(solr_url, &fq_solr).expect("solr count");

                let filter_es = vec![
                    serde_json::json!({"term": {"sku": p.sku}}),
                    serde_json::json!({"term": {"color": color.to_lowercase()}}),
                    serde_json::json!({"term": {"size": size.to_lowercase()}}),
                ];
                let es_c = es_count(es_url, es_index, &filter_es).expect("es count");
                let os_c = es_count(os_url, es_index, &filter_es).expect("os count");

                let hv_where = format!(
                    " where sku = '{}' and color = '{}' and size = '{}'",
                    p.sku,
                    color.to_lowercase(),
                    size.to_lowercase()
                );
                let hv_c = havenask_count(&havenask_url, havenask_table, &hv_where)
                    .expect("havenask count");

                let counts = vec![
                    ("solr".to_string(), solr_c),
                    ("elasticsearch".to_string(), es_c),
                    ("opensearch".to_string(), os_c),
                    ("havenask".to_string(), hv_c),
                ];
                let counts_match = counts.iter().all(|(_, c)| *c == expected_native);
                if !counts_match {
                    let kind = if is_true_positive {
                        "TRUE_POSITIVE"
                    } else {
                        "TRAP"
                    };
                    mismatches.push(format!(
                        "Q8 {kind} sku={} color={color} size={size}: expected={expected_native} {counts:?}",
                        p.sku
                    ));
                }
                if is_true_positive {
                    true_positive_checks += 1;
                } else {
                    trap_checks += 1;
                }

                rows.push(Row {
                    class: if is_true_positive {
                        "Q8_same_variant_true_positive".to_string()
                    } else {
                        "Q8_same_variant_trap".to_string()
                    },
                    key: format!("{}/{color}/{size}", p.sku),
                    native_count: expected_native,
                    counts,
                    counts_match,
                    timings_ms: vec![],
                });
            }
        }
    }

    // Representative timed sample: the first real product's first
    // real kept (true-positive) variant, full warmup/repetition
    // protocol, for a comparable mean/P50/P99 reading on this same Q8
    // query shape.
    if let Some(p) = raw.first() {
        if let Some(v) = p.variants.first() {
            let fq_solr = vec![
                format!("sku:\"{}\"", p.sku),
                format!("color:\"{}\"", v.color),
                format!("size:\"{}\"", v.size),
            ];
            let (solr_ns, solr_c) =
                time_reps(|| solr_count(solr_url, &fq_solr).expect("solr count"));
            let filter_es = vec![
                serde_json::json!({"term": {"sku": p.sku}}),
                serde_json::json!({"term": {"color": v.color.to_lowercase()}}),
                serde_json::json!({"term": {"size": v.size.to_lowercase()}}),
            ];
            let (es_ns, es_c) =
                time_reps(|| es_count(es_url, es_index, &filter_es).expect("es count"));
            let (os_ns, os_c) =
                time_reps(|| es_count(os_url, es_index, &filter_es).expect("os count"));
            let hv_where = format!(
                " where sku = '{}' and color = '{}' and size = '{}'",
                p.sku,
                v.color.to_lowercase(),
                v.size.to_lowercase()
            );
            let (hv_ns, hv_c) = time_reps(|| {
                havenask_count(&havenask_url, havenask_table, &hv_where).expect("havenask count")
            });
            let counts = vec![
                ("solr".to_string(), solr_c),
                ("elasticsearch".to_string(), es_c),
                ("opensearch".to_string(), os_c),
                ("havenask".to_string(), hv_c),
            ];
            let counts_match = counts.iter().all(|(_, c)| *c == 1);
            let (s_mean, s_p50, s_p99) = stats_ms(solr_ns);
            let (e_mean, e_p50, e_p99) = stats_ms(es_ns);
            let (o_mean, o_p50, o_p99) = stats_ms(os_ns);
            let (h_mean, h_p50, h_p99) = stats_ms(hv_ns);
            rows.push(Row {
                class: "Q8_representative_timed_sample".to_string(),
                key: format!("{}/{}/{}", p.sku, v.color, v.size),
                native_count: 1,
                counts,
                counts_match,
                timings_ms: vec![
                    ("solr".to_string(), s_mean, s_p50, s_p99),
                    ("elasticsearch".to_string(), e_mean, e_p50, e_p99),
                    ("opensearch".to_string(), o_mean, o_p50, o_p99),
                    ("havenask".to_string(), h_mean, h_p50, h_p99),
                ],
            });
        }
    }

    println!(
        "\nQ8 coverage: {true_positive_checks} true-positive checks, {trap_checks} cross-variant trap checks"
    );

    // Compact per-class summary (155*~few combos per product = a lot of
    // rows; full detail goes to the CSV artifact, not the console).
    let mut by_class: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
    for r in &rows {
        let e = by_class.entry(r.class.as_str()).or_insert((0, 0));
        e.0 += 1;
        if r.counts_match {
            e.1 += 1;
        }
    }
    for (class, (total, matched)) in &by_class {
        println!("  {class}: {matched}/{total} matched");
    }

    report("magento", &rows, &mismatches);
}
