//! Issue #17 Phase 5, Stage B (P5-E03), concurrency sub-experiment.
//!
//! P5-E00/E01/E02 measured single-threaded latency only. A real deployment
//! serves concurrent query traffic, so this binary asks a distinct
//! question: does native's aggregate *throughput* scale with concurrency
//! the way a real production workload needs it to, and how does that
//! compare to Solr's own concurrent-request handling?
//!
//! `commerce_core::index::CatalogIndex` has no interior mutability (every
//! field is a plain `HashMap`/`Vec`/`RoaringBitmap`) -- it is naturally
//! `Sync`, so this shares one `Arc<CatalogIndex>` + `Arc<Catalog>` across
//! OS threads for concurrent reads with zero synchronization overhead
//! beyond what `Arc`'s reference count already costs. Solr is exercised
//! with the same OS-thread model (blocking `ureq` calls, no async
//! runtime), keeping the concurrency mechanism comparable on both sides.
//!
//! **Workload**: the simplest, most fundamental real operation --
//! filter-only ("how many products match this real brand/color") -- over
//! a broad, real, seeded sample of 25 real brand values + 25 real color
//! values (not fabricated), cycled round-robin by each worker thread. This
//! isolates concurrency scaling from the facet-scan crossover (P5-E03's
//! other sub-experiment) and the sort-inefficiency finding (P5-E00),
//! rather than conflating three mechanisms into one number.
//!
//! **Isolation**: native and Solr are measured in separate sweeps (never
//! concurrently with each other), so neither system's threads compete for
//! this machine's CPUs with the other's -- consistent with every other
//! P5-Enn measurement in this campaign.
//!
//! **Disclosed hardware constraint**: this container has 4 CPUs. Levels
//! at or below 4 characterize genuine parallel scaling; levels above 4
//! oversubscribe the machine and characterize contention/scheduling
//! behavior under load, not additional real parallelism headroom -- both
//! are reported, neither is hidden.
//!
//! Usage: cargo run --release -p phase5-eval --bin p5e03_concurrency_sweep
//!        [catalog.jsonl] [solr_base_url]

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use commerce_core::domain::{AttributeValue, BrandId, Constraint};
use commerce_core::index::CatalogIndex;
use commerce_core::ir::{ResolvedConstraint, StructuralConstraint};
use round1_eval::catalog as catalog_ingest;
use round1_eval::data;

use rand::seq::SliceRandom;
use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;

const SEED: u64 = 7;
const N_BRAND_QUERIES: usize = 25;
const N_COLOR_QUERIES: usize = 25;
const RUN_DURATION: Duration = Duration::from_secs(3);
const CONCURRENCY_LEVELS: &[usize] = &[1, 2, 4, 8];

#[derive(Clone)]
enum Query {
    Brand(BrandId, String),
    Color(String),
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

/// One shared timing run: `worker_count` OS threads each hammer a
/// round-robin, per-thread-shuffled sequence of `queries` for
/// `RUN_DURATION`, via `run_one` (native's `indexed_candidates` or Solr's
/// `fq`). Returns (total requests completed, all per-request latencies
/// merged across every thread).
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
        .unwrap_or_else(|| PathBuf::from("dataset_cache/export/catalog.jsonl"));
    let solr_base_url = args
        .next()
        .unwrap_or_else(|| "http://localhost:8983/solr/commerce_bench".to_string());

    println!("loading + ingesting real catalog...");
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

    println!("\ncomputing real brand/color group-size distributions...");
    let mut brand_counts: HashMap<BrandId, (usize, String)> = HashMap::new();
    let mut color_counts: HashMap<String, usize> = HashMap::new();
    let mut brand_raw_by_id: HashMap<BrandId, String> = HashMap::new();
    for (raw, product) in products.iter().zip(&catalog.products) {
        if let Some(raw_brand) = &raw.brand {
            brand_raw_by_id
                .entry(product.brand)
                .or_insert_with(|| raw_brand.clone());
        }
        if product.brand != BrandId(0) {
            let entry = brand_counts
                .entry(product.brand)
                .or_insert((0, String::new()));
            entry.0 += 1;
            if let Some(name) = brand_raw_by_id.get(&product.brand) {
                entry.1 = name.clone();
            }
        }
        if let Some(AttributeValue::Enum(color)) = product.variants[0].attributes.get("color") {
            *color_counts.entry(color.clone()).or_insert(0) += 1;
        }
    }

    let mut rng = ChaCha8Rng::seed_from_u64(SEED);
    let mut brand_ids: Vec<BrandId> = brand_counts.keys().copied().collect();
    brand_ids.sort();
    brand_ids.shuffle(&mut rng);
    let mut color_values: Vec<String> = color_counts.keys().cloned().collect();
    color_values.sort();
    color_values.shuffle(&mut rng);

    let mut queries: Vec<Query> = Vec::new();
    for &id in brand_ids.iter().take(N_BRAND_QUERIES) {
        queries.push(Query::Brand(id, brand_counts[&id].1.clone()));
    }
    for color in color_values.iter().take(N_COLOR_QUERIES) {
        queries.push(Query::Color(color.clone()));
    }
    println!(
        "  real mixed workload: {} brand queries + {} color queries = {} distinct real filter requests",
        N_BRAND_QUERIES.min(brand_ids.len()),
        N_COLOR_QUERIES.min(color_values.len()),
        queries.len()
    );

    println!(
        "\n=== P5-E03 concurrency sweep result (4 real CPUs available -- levels >4 oversubscribe) ==="
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
                Query::Brand(id, _) => {
                    vec![ResolvedConstraint::Structural(StructuralConstraint::Brand(
                        *id,
                    ))]
                }
                Query::Color(c) => vec![ResolvedConstraint::Attribute(Constraint::Enum {
                    attribute: "color".to_string(),
                    value: c.clone(),
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
                Query::Brand(_, name) => fq_exact("brand", name),
                Query::Color(c) => fq_exact("color", c),
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

    std::fs::create_dir_all("dataset_cache/p5e03_concurrency_artifacts").ok();
    std::fs::write("dataset_cache/p5e03_concurrency_artifacts/results.csv", csv).ok();
    println!("\nartifacts written to dataset_cache/p5e03_concurrency_artifacts");
}
