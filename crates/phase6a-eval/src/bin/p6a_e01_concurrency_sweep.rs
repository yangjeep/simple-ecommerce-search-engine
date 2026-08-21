//! Phase 6A (Issue #23), concurrency sub-experiment -- mirrors Phase 5's
//! `p5e03_concurrency_sweep` exactly, substituting WANDS' category_leaf
//! (dedicated structural field) and category_depth_1 (generic enum
//! attribute) for ESCI's brand/color, to test whether Phase 5's most
//! dramatic finding (native's single-thread throughput beats Solr's best
//! multi-worker throughput by orders of magnitude) reproduces on an
//! independent dataset or was an ESCI-specific artifact.
//!
//! Same isolation/hardware disclosures as Phase 5: native and Solr are
//! measured in separate sweeps (never concurrently with each other);
//! this container has 4 real CPUs, so levels above 4 oversubscribe
//! rather than characterize additional real parallelism.
//!
//! Usage: cargo run --release -p phase6a-eval --bin p6a_e01_concurrency_sweep
//!        [catalog.jsonl] [solr_base_url]

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use commerce_core::domain::{AttributeValue, CategoryId, Constraint};
use commerce_core::index::CatalogIndex;
use commerce_core::ir::{ResolvedConstraint, StructuralConstraint};
use phase6a_eval::{catalog as catalog_ingest, data};

use rand::seq::SliceRandom;
use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;

const SEED: u64 = 7;
const N_LEAF_QUERIES: usize = 25;
const N_DEPTH1_QUERIES: usize = 25;
const RUN_DURATION: Duration = Duration::from_secs(3);
const CONCURRENCY_LEVELS: &[usize] = &[1, 2, 4, 8];

#[derive(Clone)]
enum Query {
    Leaf(CategoryId, String),
    Depth1(String),
}

fn percentile_ms(sorted_ns: &[u128], p: f64) -> f64 {
    if sorted_ns.is_empty() {
        return 0.0;
    }
    let idx = ((sorted_ns.len() as f64 - 1.0) * p).round() as usize;
    sorted_ns[idx] as f64 / 1_000_000.0
}

fn solr_num_found(base_url: &str, fq: &str) -> u64 {
    let resp: serde_json::Value = ureq::get(&format!("{base_url}/select"))
        .query("q", "*:*")
        .query("rows", "0")
        .query("fq", fq)
        .call()
        .unwrap_or_else(|e| panic!("Solr request failed: {e}"))
        .into_json()
        .unwrap_or_else(|e| panic!("Solr response was not valid JSON: {e}"));
    resp["response"]["numFound"].as_u64().unwrap()
}

fn escape_solr_phrase(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

fn fq_exact(field: &str, value: &str) -> String {
    format!("{field}:\"{}\"", escape_solr_phrase(value))
}

fn run_concurrent<F>(worker_count: usize, queries: &[Query], run_one: F) -> (u64, Vec<u128>)
where
    F: Fn(&Query) -> u64 + Send + Sync + 'static,
{
    let run_one = Arc::new(run_one);
    let total = Arc::new(AtomicU64::new(0));
    let all_latencies_ns = Arc::new(Mutex::new(Vec::new()));
    let queries: Arc<Vec<Query>> = Arc::new(queries.to_vec());

    let mut handles = Vec::new();
    for worker_id in 0..worker_count {
        let run_one = Arc::clone(&run_one);
        let total = Arc::clone(&total);
        let all_latencies_ns = Arc::clone(&all_latencies_ns);
        let queries = Arc::clone(&queries);
        handles.push(std::thread::spawn(move || {
            let mut rng = ChaCha8Rng::seed_from_u64(SEED.wrapping_add(worker_id as u64));
            let mut order: Vec<usize> = (0..queries.len()).collect();
            order.shuffle(&mut rng);
            let mut local_latencies = Vec::new();
            let deadline = Instant::now() + RUN_DURATION;
            let mut i = 0usize;
            while Instant::now() < deadline {
                let q = &queries[order[i % order.len()]];
                let start = Instant::now();
                std::hint::black_box(run_one(q));
                local_latencies.push(start.elapsed().as_nanos());
                total.fetch_add(1, Ordering::Relaxed);
                i += 1;
            }
            all_latencies_ns.lock().unwrap().extend(local_latencies);
        }));
    }
    for h in handles {
        h.join().unwrap();
    }
    let total = total.load(Ordering::Relaxed);
    let mut latencies = Arc::try_unwrap(all_latencies_ns)
        .unwrap()
        .into_inner()
        .unwrap();
    latencies.sort_unstable();
    (total, latencies)
}

fn main() {
    let mut args = std::env::args().skip(1);
    let catalog_path = args
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("dataset_cache/wands/catalog.jsonl"));
    let solr_base_url = args
        .next()
        .unwrap_or_else(|| "http://localhost:8983/solr/wands_bench".to_string());

    println!("loading + ingesting real WANDS catalog...");
    let products = data::load_catalog(&catalog_path);
    let ingested = catalog_ingest::build_catalog(&products);
    println!("{} real products ingested", ingested.catalog.products.len());

    println!("building commerce_core structural index...");
    let index = Arc::new(CatalogIndex::build(&ingested.catalog));
    let catalog = Arc::new(ingested.catalog);

    println!("checking Solr ({solr_base_url})...");
    let ping: serde_json::Value = ureq::get(&format!("{solr_base_url}/select"))
        .query("q", "*:*")
        .query("rows", "0")
        .call()
        .unwrap()
        .into_json()
        .unwrap();
    let numfound = ping["response"]["numFound"].as_u64().unwrap();
    assert_eq!(
        numfound as usize,
        catalog.products.len(),
        "Solr and native must index the identical real catalog for this comparison to be meaningful"
    );
    println!("  Solr reachable: numFound={numfound}");

    println!("\ncomputing real category-leaf/depth-1 group distributions...");
    let mut leaf_counts: HashMap<CategoryId, (usize, String)> = HashMap::new();
    let mut depth1_counts: HashMap<String, usize> = HashMap::new();
    for product in &catalog.products {
        if product.category != CategoryId(0) {
            let entry = leaf_counts
                .entry(product.category)
                .or_insert((0, String::new()));
            entry.0 += 1;
        }
        if let Some(AttributeValue::Enum(v)) = product.attributes.get("category_depth_1") {
            *depth1_counts.entry(v.clone()).or_insert(0) += 1;
        }
    }
    let category_name_by_id: HashMap<CategoryId, String> = ingested
        .categories
        .iter()
        .map(|c| (c.id, c.name.clone()))
        .collect();
    for (id, entry) in leaf_counts.iter_mut() {
        if let Some(name) = category_name_by_id.get(id) {
            entry.1 = name.clone();
        }
    }

    let mut rng = ChaCha8Rng::seed_from_u64(SEED);
    let mut leaf_ids: Vec<CategoryId> = leaf_counts.keys().copied().collect();
    leaf_ids.sort();
    leaf_ids.shuffle(&mut rng);
    let mut depth1_values: Vec<String> = depth1_counts.keys().cloned().collect();
    depth1_values.sort();
    depth1_values.shuffle(&mut rng);

    let mut queries: Vec<Query> = Vec::new();
    for &id in leaf_ids.iter().take(N_LEAF_QUERIES) {
        queries.push(Query::Leaf(id, leaf_counts[&id].1.clone()));
    }
    for v in depth1_values.iter().take(N_DEPTH1_QUERIES) {
        queries.push(Query::Depth1(v.clone()));
    }
    println!(
        "  real mixed workload: {} category_leaf queries + {} category_depth_1 queries = {} distinct real filter requests",
        N_LEAF_QUERIES.min(leaf_ids.len()),
        N_DEPTH1_QUERIES.min(depth1_values.len()),
        queries.len()
    );

    println!(
        "\n=== P6A concurrency sweep result (4 real CPUs available -- levels >4 oversubscribe) ==="
    );
    println!(
        "{:<8} {:<8} {:>12} {:>14} {:>10} {:>10}",
        "system", "workers", "total_reqs", "throughput_rps", "p50_ms", "p99_ms"
    );

    let mut csv = String::from("system,workers,total_requests,throughput_rps,p50_ms,p99_ms\n");

    for &workers in CONCURRENCY_LEVELS {
        let index = Arc::clone(&index);
        let queries_clone = queries.clone();
        let (total, latencies) = run_concurrent(workers, &queries_clone, move |q| {
            let constraint = match q {
                Query::Leaf(id, _) => {
                    vec![ResolvedConstraint::Structural(
                        StructuralConstraint::Category(*id),
                    )]
                }
                Query::Depth1(v) => vec![ResolvedConstraint::Attribute(Constraint::Enum {
                    attribute: "category_depth_1".to_string(),
                    value: v.clone(),
                })],
            };
            index.indexed_candidates(&constraint).len()
        });
        let throughput = total as f64 / RUN_DURATION.as_secs_f64();
        let p50 = percentile_ms(&latencies, 0.5);
        let p99 = percentile_ms(&latencies, 0.99);
        println!(
            "{:<8} {:<8} {:>12} {:>14.1} {:>10.4} {:>10.4}",
            "native", workers, total, throughput, p50, p99
        );
        csv.push_str(&format!(
            "native,{workers},{total},{throughput},{p50},{p99}\n"
        ));
    }

    for &workers in CONCURRENCY_LEVELS {
        let solr_base_url = solr_base_url.clone();
        let queries_clone = queries.clone();
        let (total, latencies) = run_concurrent(workers, &queries_clone, move |q| {
            let fq = match q {
                Query::Leaf(_, name) => fq_exact("category_leaf", name),
                Query::Depth1(v) => fq_exact("category_depth_1", v),
            };
            solr_num_found(&solr_base_url, &fq)
        });
        let throughput = total as f64 / RUN_DURATION.as_secs_f64();
        let p50 = percentile_ms(&latencies, 0.5);
        let p99 = percentile_ms(&latencies, 0.99);
        println!(
            "{:<8} {:<8} {:>12} {:>14.1} {:>10.4} {:>10.4}",
            "solr", workers, total, throughput, p50, p99
        );
        csv.push_str(&format!(
            "solr,{workers},{total},{throughput},{p50},{p99}\n"
        ));
    }

    std::fs::create_dir_all("dataset_cache/p6a_e01_concurrency_artifacts").ok();
    std::fs::write(
        "dataset_cache/p6a_e01_concurrency_artifacts/results.csv",
        csv,
    )
    .ok();
    println!("\nartifacts written to dataset_cache/p6a_e01_concurrency_artifacts");
}
