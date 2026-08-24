//! Issue #45 E2c's own preregistered GO-gate criterion 6 ("serving
//! overhead remains within the existing commerce-native fast-path
//! budget") -- `docs/experiments/ISSUE45_PROTOCOL.md` section 10/11.
//! Closely modeled on `e2b_serving_overhead_eval.rs` (same measurement
//! discipline: `bench_harness::round_robin_schedule`, `REPS_PER_QUERY=30`,
//! batched `black_box`'d calls, a pre-declared timer floor, both
//! `indexed_candidates` and `execute_ranked` measured) -- compares the
//! oracle-compiled bundle against Treatment C's own compiled bundle,
//! rather than E2b's LLM+validator bundle, since E2c's own contribution
//! is making `candidate_physical_primitive` selection real (via R1)
//! rather than advisory. Reuses `e2c_compile::build_wands_catalog`
//! (which additionally ingests `Identifier`-role descriptors as `Text`,
//! unlike `e2b_ingest::build_catalog` -- see that module's own doc
//! comment).
//!
//! Reproduction: `cargo build --release -p issue42-eval &&
//! ./target/release/e2c_serving_overhead_eval [output_summary_json_path]`

use std::collections::BTreeMap;
use std::env;
use std::fs;

use bench_harness::{round_robin_schedule, Distribution};
use commerce_core::domain::Constraint;
use commerce_core::index::CatalogIndex;
use commerce_core::ir::ResolvedConstraint;

use issue42_eval::e2b_key_mapping::{anonymized_mapping, noisy_mapping};
use issue42_eval::e2b_pipeline::{self, oracle_wands_accepted, CANONICAL_CONFIGS, CONFIGS};
use issue42_eval::e2b_schema::SemanticRole;
use issue42_eval::e2b_validator::wands_query_texts;
use issue42_eval::e2b_workload::{automotive_unified_stats, load_wands_feed, UnifiedFieldStats};
use issue42_eval::e2c_canonicalizer::canonicalize;
use issue42_eval::e2c_compile::{build_wands_catalog, CompiledWandsCatalog};
use issue42_eval::e2c_metrics::group_by_real_key;
use issue42_eval::e2c_schema::CanonicalDescriptor;

const BASELINE_SHA: &str = "d965b7444e1ae563707af987da1a55b98d939135";
const REPS_PER_QUERY: usize = 30;
const IN_PROCESS_BATCH: usize = 200;
const WARMUP_PER_QUERY: usize = 2;
const MAX_VALUES_PER_ENUM_FIELD: usize = 3;
const MAX_AND_PAIRS: usize = 20;
const TIMER_FLOOR_MS: f64 = 0.001;
const HAS_REAL_VARIANT_GROUPING: bool = false;

#[derive(Debug, Clone)]
enum Query {
    EnumEq(String, String),
    EnumAnd(String, String, String, String),
}

fn constraints_for(query: &Query) -> Vec<ResolvedConstraint> {
    match query {
        Query::EnumEq(attr, val) => vec![ResolvedConstraint::Attribute(Constraint::Enum {
            attribute: attr.clone(),
            value: val.clone(),
        })],
        Query::EnumAnd(a1, v1, a2, v2) => vec![
            ResolvedConstraint::Attribute(Constraint::Enum {
                attribute: a1.clone(),
                value: v1.clone(),
            }),
            ResolvedConstraint::Attribute(Constraint::Enum {
                attribute: a2.clone(),
                value: v2.clone(),
            }),
        ],
    }
}

fn label(query: &Query) -> String {
    match query {
        Query::EnumEq(a, v) => format!("Enum({a}={v:?})"),
        Query::EnumAnd(a1, v1, a2, v2) => format!("Enum({a1}={v1:?}) AND Enum({a2}={v2:?})"),
    }
}

/// Treatment C's own promoted, WANDS-scoped canonical descriptors --
/// computed the same way `e2c_canonicalization_eval`'s own headline
/// numbers are computed (CANONICAL_CONFIGS only), so this binary's
/// bundle is exactly what that binary's own accuracy numbers describe,
/// never a second, independently-written computation.
fn canonicalized_wands_accepted() -> Vec<CanonicalDescriptor> {
    let per_config_runs = e2b_pipeline::load_all_runs(CONFIGS);
    let anon = anonymized_mapping();
    let noisy = noisy_mapping();
    let wands_feed = load_wands_feed();
    let wands_unified: BTreeMap<String, UnifiedFieldStats> = wands_feed
        .stats
        .iter()
        .map(|(k, s)| (k.clone(), UnifiedFieldStats::from(s)))
        .collect();
    let mut all_unified = wands_unified.clone();
    all_unified.extend(automotive_unified_stats(1500));
    let wands_queries_text = wands_query_texts();

    let mut out = Vec::new();
    for config in CANONICAL_CONFIGS {
        let Some(runs) = per_config_runs.get(*config) else {
            continue;
        };
        let by_key = group_by_real_key(config, runs, &anon, &noisy);
        for (real_key, runs_for_key) in &by_key {
            // Only WANDS's 36-key sample is ingested into a real Catalog
            // here (automotive uses a different generator/catalog
            // entirely -- matching e2b_pipeline::oracle_wands_accepted's
            // own WANDS-only scope).
            let Some(stats) = wands_unified.get(real_key) else {
                continue;
            };
            let outcome = canonicalize(
                runs_for_key,
                real_key,
                stats,
                &wands_queries_text,
                HAS_REAL_VARIANT_GROUPING,
                false,
            );
            if let Some(d) = outcome.promoted() {
                if matches!(
                    d.semantic_role,
                    SemanticRole::Enum
                        | SemanticRole::Boolean
                        | SemanticRole::Numeric
                        | SemanticRole::Identifier
                ) {
                    out.push(d.clone());
                }
            }
        }
    }
    let _ = all_unified;
    out
}

struct Bundle {
    label: &'static str,
    compiled: CompiledWandsCatalog,
    index: CatalogIndex,
    build_ms: f64,
}

fn build_bundle(label: &'static str, compiled: CompiledWandsCatalog) -> Bundle {
    let build_start = std::time::Instant::now();
    let index = CatalogIndex::build(&compiled.catalog);
    let build_ms = build_start.elapsed().as_secs_f64() * 1000.0;
    Bundle {
        label,
        compiled,
        index,
        build_ms,
    }
}

fn enum_values(catalog: &commerce_core::domain::Catalog, field: &str) -> Vec<String> {
    let mut out = std::collections::BTreeSet::new();
    for product in &catalog.products {
        for variant in &product.variants {
            let attrs = commerce_core::domain::effective_attributes(product, variant);
            if let Some(commerce_core::domain::AttributeValue::Enum(v)) = attrs.get(field) {
                out.insert(v.clone());
            }
        }
    }
    out.into_iter().collect()
}

fn common_enum_fields_and_values(
    oracle_bundle: &Bundle,
    canonical_bundle: &Bundle,
    oracle_accepted: &[issue42_eval::e2b_schema::Descriptor],
    canonical_accepted: &[CanonicalDescriptor],
) -> BTreeMap<String, Vec<String>> {
    let oracle_enum_fields: std::collections::BTreeSet<&str> = oracle_accepted
        .iter()
        .filter(|d| d.semantic_role == SemanticRole::Enum)
        .map(|d| d.key.as_str())
        .collect();
    let canonical_enum_fields: std::collections::BTreeSet<&str> = canonical_accepted
        .iter()
        .filter(|d| d.semantic_role == SemanticRole::Enum)
        .map(|d| d.real_key.as_str())
        .collect();

    let mut out = BTreeMap::new();
    for field in oracle_enum_fields.intersection(&canonical_enum_fields) {
        let oracle_values: std::collections::BTreeSet<String> =
            enum_values(&oracle_bundle.compiled.catalog, field)
                .into_iter()
                .collect();
        let canonical_values: std::collections::BTreeSet<String> =
            enum_values(&canonical_bundle.compiled.catalog, field)
                .into_iter()
                .collect();
        let common: Vec<String> = oracle_values
            .intersection(&canonical_values)
            .take(MAX_VALUES_PER_ENUM_FIELD)
            .cloned()
            .collect();
        if !common.is_empty() {
            out.insert(field.to_string(), common);
        }
    }
    out
}

fn build_workload(common: &BTreeMap<String, Vec<String>>) -> Vec<Query> {
    let mut queries = Vec::new();
    for (field, values) in common {
        for value in values {
            queries.push(Query::EnumEq(field.clone(), value.clone()));
        }
    }
    let fields: Vec<&String> = common.keys().collect();
    let mut and_count = 0;
    'outer: for i in 0..fields.len() {
        for j in (i + 1)..fields.len() {
            if and_count >= MAX_AND_PAIRS {
                break 'outer;
            }
            let (f1, f2) = (fields[i], fields[j]);
            let v1 = &common[f1][0];
            let v2 = &common[f2][0];
            queries.push(Query::EnumAnd(
                f1.clone(),
                v1.clone(),
                f2.clone(),
                v2.clone(),
            ));
            and_count += 1;
        }
    }
    queries
}

fn tail_gate(oracle_p: f64, canonical_p: f64) -> (f64, bool, &'static str) {
    let below_floor = oracle_p < TIMER_FLOOR_MS || canonical_p < TIMER_FLOOR_MS;
    let overhead = if oracle_p > 0.0 {
        (canonical_p - oracle_p) / oracle_p * 100.0
    } else {
        f64::INFINITY
    };
    let gate = if below_floor {
        "INCONCLUSIVE"
    } else if overhead.abs() <= 5.0 {
        "PASS"
    } else {
        "FAIL"
    };
    (overhead, below_floor, gate)
}

fn main() {
    println!("=== Issue #45 E2c serving-overhead measurement (GO-gate criterion 6) ===");
    println!("baseline_sha: {BASELINE_SHA}");

    let oracle_accepted = oracle_wands_accepted();
    let canonical_accepted = canonicalized_wands_accepted();
    if canonical_accepted.is_empty() {
        println!("\nNo canonical descriptors promoted -- cannot measure serving overhead.");
        return;
    }
    println!(
        "oracle: {} WANDS structural descriptors accepted",
        oracle_accepted.len()
    );
    println!(
        "Treatment C (canonicalizer): {} WANDS structural descriptors promoted",
        canonical_accepted.len()
    );

    println!("\nbuilding oracle-compiled bundle...");
    let oracle_compiled = issue42_eval::e2b_ingest::build_catalog(&oracle_accepted);
    let oracle_bundle = build_bundle(
        "oracle",
        CompiledWandsCatalog {
            catalog: oracle_compiled.catalog,
        },
    );
    println!(
        "  {} products, build={:.2}ms, index size={} bytes",
        oracle_bundle.compiled.catalog.products.len(),
        oracle_bundle.build_ms,
        oracle_bundle.index.approximate_size_bytes()
    );

    println!("building Treatment C-compiled bundle...");
    let canonical_bundle = build_bundle("canonicalizer", build_wands_catalog(&canonical_accepted));
    println!(
        "  {} products, build={:.2}ms, index size={} bytes",
        canonical_bundle.compiled.catalog.products.len(),
        canonical_bundle.build_ms,
        canonical_bundle.index.approximate_size_bytes()
    );

    assert_eq!(
        oracle_bundle.compiled.catalog.products.len(),
        canonical_bundle.compiled.catalog.products.len(),
        "both bundles must be built from the identical real WANDS catalog"
    );

    let common = common_enum_fields_and_values(
        &oracle_bundle,
        &canonical_bundle,
        &oracle_accepted,
        &canonical_accepted,
    );
    println!(
        "\n{} fields common to both bundles' own accepted Enum set, with real shared values",
        common.len()
    );
    let workload = build_workload(&common);
    if workload.is_empty() {
        println!("\nNo common queryable field/value -- serving-overhead gate NOT ESTABLISHED.");
        write_summary(&env::args().nth(1), &oracle_bundle, &canonical_bundle, None);
        return;
    }
    println!("workload: {} queries", workload.len());

    let mut candidate_set_mismatches = 0usize;
    for q in &workload {
        let cs = constraints_for(q);
        let o = oracle_bundle.index.indexed_candidates(&cs).len();
        let c = canonical_bundle.index.indexed_candidates(&cs).len();
        if o != c {
            candidate_set_mismatches += 1;
            println!(
                "  WARNING: candidate-set mismatch for {}: oracle={o} canonical={c}",
                label(q)
            );
        }
    }
    println!(
        "candidate-set cross-check: {}/{} match",
        workload.len() - candidate_set_mismatches,
        workload.len()
    );

    for q in &workload {
        let cs = constraints_for(q);
        let _ = oracle_bundle.index.indexed_candidates(&cs);
        let _ = canonical_bundle.index.indexed_candidates(&cs);
    }

    const METHOD_COUNT: usize = 2;
    let mut latencies_ms: [Vec<f64>; METHOD_COUNT] = Default::default();
    for q in &workload {
        let cs = constraints_for(q);
        let schedule = round_robin_schedule(METHOD_COUNT, REPS_PER_QUERY, 246_813_579);
        for _ in 0..WARMUP_PER_QUERY {
            let _ = oracle_bundle.index.indexed_candidates(&cs);
            let _ = canonical_bundle.index.indexed_candidates(&cs);
        }
        for &method in &schedule {
            let bundle = if method == 0 {
                &oracle_bundle
            } else {
                &canonical_bundle
            };
            let start = std::time::Instant::now();
            for _ in 0..IN_PROCESS_BATCH {
                std::hint::black_box(bundle.index.indexed_candidates(&cs));
            }
            let elapsed = start.elapsed().as_secs_f64() * 1000.0 / IN_PROCESS_BATCH as f64;
            latencies_ms[method].push(elapsed);
        }
    }
    let oracle_dist = Distribution::compute(&latencies_ms[0]);
    let canonical_dist = Distribution::compute(&latencies_ms[1]);
    println!("\n=== indexed_candidates (ms) ===");
    oracle_dist.print("oracle", "ms");
    canonical_dist.print("canonicalizer", "ms");
    let (p50_overhead, p50_below_floor, p50_gate) = tail_gate(oracle_dist.p50, canonical_dist.p50);
    println!("P50 overhead: {p50_overhead:+.2}% below_floor={p50_below_floor} => {p50_gate}");

    const RANKED_K: usize = 10;
    let mut ranked_latencies_ms: [Vec<f64>; 2] = Default::default();
    for q in &workload {
        let cs = constraints_for(q);
        let query = commerce_core::ir::CommerceQuery {
            constraints: cs,
            preferences: vec![],
            ambiguous: vec![],
            residual_lexical: vec![],
        };
        let schedule = round_robin_schedule(2, REPS_PER_QUERY, 975_318_642);
        for _ in 0..WARMUP_PER_QUERY {
            let _ = oracle_bundle.index.execute_ranked(
                &query,
                &oracle_bundle.compiled.catalog,
                RANKED_K,
            );
            let _ = canonical_bundle.index.execute_ranked(
                &query,
                &canonical_bundle.compiled.catalog,
                RANKED_K,
            );
        }
        for &method in &schedule {
            let (bundle, idx) = if method == 0 {
                (&oracle_bundle, 0)
            } else {
                (&canonical_bundle, 1)
            };
            let start = std::time::Instant::now();
            for _ in 0..IN_PROCESS_BATCH {
                let hits = std::hint::black_box(bundle.index.execute_ranked(
                    &query,
                    &bundle.compiled.catalog,
                    RANKED_K,
                ));
                std::hint::black_box(hits.len());
            }
            let elapsed = start.elapsed().as_secs_f64() * 1000.0 / IN_PROCESS_BATCH as f64;
            ranked_latencies_ms[idx].push(elapsed);
        }
    }
    let oracle_ranked = Distribution::compute(&ranked_latencies_ms[0]);
    let canonical_ranked = Distribution::compute(&ranked_latencies_ms[1]);
    println!("\n=== execute_ranked top-{RANKED_K} (ms) ===");
    oracle_ranked.print("oracle", "ms");
    canonical_ranked.print("canonicalizer", "ms");
    let (p50r, p50r_floor, p50r_gate) = tail_gate(oracle_ranked.p50, canonical_ranked.p50);
    let (p95r, p95r_floor, p95r_gate) = tail_gate(oracle_ranked.p95, canonical_ranked.p95);
    let (p99r, p99r_floor, p99r_gate) = tail_gate(oracle_ranked.p99, canonical_ranked.p99);
    println!("execute_ranked P50: {p50r:+.2}% floor={p50r_floor} => {p50r_gate}");
    println!("execute_ranked P95: {p95r:+.2}% floor={p95r_floor} => {p95r_gate}");
    println!("execute_ranked P99: {p99r:+.2}% floor={p99r_floor} => {p99r_gate}");

    let all_gates = [p50_gate, p50r_gate, p95r_gate, p99r_gate];
    let combined = if all_gates.contains(&"FAIL") {
        "FAIL"
    } else if all_gates.contains(&"PASS") {
        "PASS"
    } else {
        "INCONCLUSIVE"
    };
    println!("\n=> E2c serving-overhead gate (criterion 6): {combined}");

    write_summary(
        &env::args().nth(1),
        &oracle_bundle,
        &canonical_bundle,
        Some(serde_json::json!({
            "indexed_candidates": {
                "n_queries": workload.len(),
                "candidate_set_mismatches": candidate_set_mismatches,
                "oracle_p50_ms": oracle_dist.p50, "canonical_p50_ms": canonical_dist.p50,
                "overhead_pct_p50": p50_overhead, "below_timer_floor": p50_below_floor, "gate": p50_gate,
            },
            "execute_ranked_top10": {
                "oracle_p50_ms": oracle_ranked.p50, "canonical_p50_ms": canonical_ranked.p50,
                "oracle_p95_ms": oracle_ranked.p95, "canonical_p95_ms": canonical_ranked.p95,
                "oracle_p99_ms": oracle_ranked.p99, "canonical_p99_ms": canonical_ranked.p99,
                "overhead_pct_p50": p50r, "gate_p50": p50r_gate,
                "overhead_pct_p95": p95r, "gate_p95": p95r_gate,
                "overhead_pct_p99": p99r, "gate_p99": p99r_gate,
            },
            "go_gate_criterion_6": combined,
        })),
    );
}

fn write_summary(
    out_path: &Option<String>,
    oracle_bundle: &Bundle,
    canonical_bundle: &Bundle,
    result: Option<serde_json::Value>,
) {
    let summary = serde_json::json!({
        "experiment_id": "I45-E2c-serving-overhead",
        "baseline_sha": BASELINE_SHA,
        "bundles": {
            "oracle": {
                "label": oracle_bundle.label,
                "n_products": oracle_bundle.compiled.catalog.products.len(),
                "build_ms": oracle_bundle.build_ms,
                "index_size_bytes": oracle_bundle.index.approximate_size_bytes(),
            },
            "canonicalizer": {
                "label": canonical_bundle.label,
                "n_products": canonical_bundle.compiled.catalog.products.len(),
                "build_ms": canonical_bundle.build_ms,
                "index_size_bytes": canonical_bundle.index.approximate_size_bytes(),
            },
        },
        "result": result,
    });
    println!("\n{}", serde_json::to_string_pretty(&summary).unwrap());
    if let Some(path) = out_path {
        fs::write(path, serde_json::to_string_pretty(&summary).unwrap())
            .expect("write summary json");
        println!("summary written to {path}");
    }
}
