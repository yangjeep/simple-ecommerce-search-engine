//! Phase 6B (Issue #21's Phase 6, continuing directly from Issue #23 /
//! Phase 6A's WANDS work -- see `PHASE6A_DECISION.md`) P6B-E00: a
//! **controlled-stress scale ladder** built by replicating the real
//! WANDS catalog (`scripts/datasets/replicate_wands_scale.py`), used to
//! extend Phase 6A's facet-scan crossover characterization past WANDS'
//! real ~43K-product ceiling and to test whether the crossover point is
//! governed by candidate-set SIZE (would shift with replication) or by
//! per-candidate attribute-map COMPLEXITY (would stay fixed under
//! replication, since replication holds attribute complexity constant
//! and facet cardinality constant while scaling only per-bucket
//! candidate-set size).
//!
//! **This is deliberately NOT a claim about organic catalog growth.**
//! A real 860K-product WANDS-like catalog would almost certainly have
//! more distinct categories/colors/styles than the real 42,994-product
//! one, not just 20x-deeper buckets of the same ones. Replication holds
//! facet cardinality fixed on purpose, to isolate one variable
//! (candidate-set size) from another (attribute/facet complexity) that
//! Phase 6A's ESCI-vs-WANDS comparison varied together. See
//! `docs/experiments/PHASE6B_LOG.md` and `scripts/datasets/
//! replicate_wands_scale.py`'s own doc comment for the full disclosure.
//!
//! Reuses Phase 6A's `catalog`/`data` modules, `_by_scan` facet methods,
//! and REPS/WARMUP/timing methodology unchanged -- no new commerce_core
//! query semantics are exercised here beyond what P6A-E00 already used,
//! plus one addition: a real numeric-range filter on WANDS' genuine
//! `average_rating` field (Issue #21's "numeric/price ranges" PLP
//! operator class, which P6A-E00 could not attempt at all since WANDS
//! has no price field -- `average_rating` is a real, already-ingested
//! numeric attribute, not a fabricated substitute for price).
//!
//! Usage: cargo run --release -p phase6a-eval --bin p6b_e00_scale_ladder
//!        <catalog.jsonl> <solr_base_url> <tier_label> <csv_out_path>
//!
//! Run once per tier (matching the one-binary-run-per-dataset-variant
//! pattern `p6a_e00` already established); each run APPENDS to
//! <csv_out_path>, writing the header only if the file does not exist
//! yet, so the same command can be repeated across tiers into one
//! combined ladder CSV.

use std::collections::BTreeMap;
use std::io::Write as _;
use std::path::PathBuf;
use std::time::Instant;

use commerce_core::domain::{Constraint, NumericOp};
use commerce_core::index::CatalogIndex;
use commerce_core::ir::ResolvedConstraint;
use phase6a_eval::{catalog as catalog_ingest, data};

const REPS: usize = 30;
const WARMUP: usize = 5;

fn percentile_ms(sorted_ns: &[u128], p: f64) -> f64 {
    if sorted_ns.is_empty() {
        return 0.0;
    }
    let idx = ((sorted_ns.len() as f64 - 1.0) * p).round() as usize;
    sorted_ns[idx] as f64 / 1_000_000.0
}

fn stats_ms(mut samples_ns: Vec<u128>) -> (f64, f64, f64) {
    samples_ns.sort_unstable();
    let mean = samples_ns.iter().sum::<u128>() as f64 / samples_ns.len() as f64 / 1_000_000.0;
    (
        mean,
        percentile_ms(&samples_ns, 0.5),
        percentile_ms(&samples_ns, 0.99),
    )
}

fn time_reps<T, F: FnMut() -> T>(mut f: F) -> (Vec<u128>, T) {
    for _ in 0..WARMUP {
        f();
    }
    let mut samples = Vec::with_capacity(REPS);
    let mut last = None;
    for _ in 0..REPS {
        let start = Instant::now();
        let result = f();
        samples.push(start.elapsed().as_nanos());
        last = Some(result);
    }
    (samples, last.unwrap())
}

fn current_rss_kb() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    status
        .lines()
        .find_map(|line| line.strip_prefix("VmRSS:"))
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(|kb| kb.parse().ok())
}

fn solr_get(base_url: &str, path: &str, params: &[(&str, &str)]) -> serde_json::Value {
    let mut req = ureq::get(&format!("{base_url}{path}"));
    for &(k, v) in params {
        req = req.query(k, v);
    }
    req.call()
        .unwrap_or_else(|e| panic!("Solr request failed: {e}"))
        .into_json()
        .unwrap_or_else(|e| panic!("Solr response was not valid JSON: {e}"))
}

fn solr_num_found(base_url: &str, fq: &[String]) -> u64 {
    let mut params: Vec<(&str, &str)> = vec![("q", "*:*"), ("rows", "0")];
    for f in fq {
        params.push(("fq", f));
    }
    let resp = solr_get(base_url, "/select", &params);
    resp["response"]["numFound"].as_u64().unwrap()
}

fn solr_facet(
    base_url: &str,
    fq: &[String],
    facet_field: &str,
    limit: u64,
) -> BTreeMap<String, u64> {
    let facet_spec =
        format!(r#"{{"vals":{{"type":"terms","field":"{facet_field}","limit":{limit}}}}}"#);
    let mut params: Vec<(&str, &str)> = vec![("q", "*:*"), ("rows", "0")];
    for f in fq {
        params.push(("fq", f));
    }
    params.push(("json.facet", &facet_spec));
    let resp = solr_get(base_url, "/select", &params);
    let mut out = BTreeMap::new();
    if let Some(buckets) = resp["facets"]["vals"]["buckets"].as_array() {
        for b in buckets {
            let val = b["val"].as_str().unwrap().to_string();
            let count = b["count"].as_u64().unwrap();
            out.insert(val, count);
        }
    }
    out
}

fn top_n(map: BTreeMap<String, u64>, n: usize) -> BTreeMap<String, u64> {
    let mut entries: Vec<(String, u64)> = map.into_iter().collect();
    entries.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    entries.truncate(n);
    entries.into_iter().collect()
}

fn escape_solr_phrase(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

fn fq_exact(field: &str, value: &str) -> String {
    format!("{field}:\"{}\"", escape_solr_phrase(value))
}

/// Real depth-1 checkpoints, identical set to P6A-E00's crossover sweep,
/// so the tiers form one continuous ladder against that already-archived
/// real (1x) row rather than a differently-chosen sample.
const DEPTH1_CHECKPOINTS: [&str; 7] = [
    "Rugs",
    "Storage & Organization",
    "Lighting",
    "Outdoor",
    "Décor & Pillows",
    "Home Improvement",
    "Furniture",
];

struct Row {
    tier: String,
    class: String,
    key: String,
    native_count: u64,
    solr_count: u64,
    counts_match: bool,
    native_mean_ms: f64,
    native_p50_ms: f64,
    native_p99_ms: f64,
    solr_mean_ms: f64,
    solr_p50_ms: f64,
    solr_p99_ms: f64,
    speedup_mean: f64,
}

#[allow(clippy::too_many_arguments)]
fn make_row(
    tier: &str,
    class: &str,
    key: &str,
    native_count: u64,
    solr_count: u64,
    native_ns: Vec<u128>,
    solr_ns: Vec<u128>,
    mismatches: &mut Vec<String>,
) -> Row {
    let counts_match = native_count == solr_count;
    if !counts_match {
        mismatches.push(format!(
            "COUNT MISMATCH tier={tier} {class} key={key}: native={native_count} solr={solr_count}"
        ));
    }
    let (native_mean_ms, native_p50_ms, native_p99_ms) = stats_ms(native_ns);
    let (solr_mean_ms, solr_p50_ms, solr_p99_ms) = stats_ms(solr_ns);
    let speedup_mean = if native_mean_ms > 0.0 {
        solr_mean_ms / native_mean_ms
    } else {
        f64::INFINITY
    };
    Row {
        tier: tier.to_string(),
        class: class.to_string(),
        key: key.to_string(),
        native_count,
        solr_count,
        counts_match,
        native_mean_ms,
        native_p50_ms,
        native_p99_ms,
        solr_mean_ms,
        solr_p50_ms,
        solr_p99_ms,
        speedup_mean,
    }
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
    let tier: String = args.next().unwrap_or_else(|| "1x".to_string());
    let csv_out = args.next().map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from("docs/research/artifacts/p6b_e00_scale_ladder_run1/results.csv")
    });

    println!("=== P6B-E00 scale-ladder tier {tier} ===");
    println!("loading catalog from {}...", catalog_path.display());
    let rss_before_load_kb = current_rss_kb();
    let products = data::load_catalog(&catalog_path);
    let n_products = products.len();
    println!("{n_products} products loaded");

    let ingested = catalog_ingest::build_catalog(&products);

    println!("building commerce_core structural index...");
    let build_start = Instant::now();
    let index = CatalogIndex::build(&ingested.catalog);
    let build_elapsed_ms = build_start.elapsed().as_secs_f64() * 1000.0;
    let rss_after_build_kb = current_rss_kb();
    let index_bytes = index.approximate_size_bytes();
    println!(
        "index build: {build_elapsed_ms:.1} ms, approx_size={} bytes ({:.1} bytes/product), rss {:?} -> {:?} kb",
        index_bytes,
        index_bytes as f64 / n_products as f64,
        rss_before_load_kb,
        rss_after_build_kb
    );

    println!("checking Solr ({solr_base_url})...");
    let solr_ping = solr_num_found(&solr_base_url, &[]);
    assert_eq!(
        solr_ping as usize, n_products,
        "Solr and native must index the identical controlled-stress catalog for this tier's comparison to be meaningful"
    );
    println!("  Solr reachable: numFound={solr_ping}");

    let mut rows: Vec<Row> = Vec::new();
    let mut mismatches: Vec<String> = Vec::new();

    println!("\nrunning depth-1 color-facet crossover sweep at tier {tier}...");
    for name in DEPTH1_CHECKPOINTS {
        let fq = fq_exact("category_depth_1", name);
        let constraint = vec![ResolvedConstraint::Attribute(Constraint::Enum {
            attribute: "category_depth_1".to_string(),
            value: name.to_string(),
        })];
        let candidates = index.indexed_candidates(&constraint);
        let n = candidates.len();
        let solr_n = solr_num_found(&solr_base_url, std::slice::from_ref(&fq));

        let (native_ns, native_facets_full) =
            time_reps(|| index.facet_counts_by_scan(&candidates, &ingested.catalog, "color"));
        let (solr_ns, solr_facets) =
            time_reps(|| solr_facet(&solr_base_url, std::slice::from_ref(&fq), "color", 50));
        let native_facets = top_n(native_facets_full, 50);
        if native_facets != solr_facets {
            mismatches.push(format!(
                "FACET MISMATCH tier={tier} color_under_depth1={name}: native={native_facets:?} solr={solr_facets:?}"
            ));
        }
        println!("  {name}: n={n} (solr n={solr_n})");
        rows.push(make_row(
            &tier,
            "color_facet_under_depth1_crossover_sweep",
            name,
            n as u64,
            solr_n,
            native_ns,
            solr_ns,
            &mut mismatches,
        ));
    }

    // Real numeric-range filter (Issue #21's "numeric/price ranges" PLP
    // operator class) on WANDS' genuine `average_rating` field -- never
    // attempted in P6A-E00 since that binary only covered what its own
    // doc comment lists, and price-range was explicitly out of scope
    // there for lack of a price field. `average_rating` is real,
    // already-ingested WANDS data (see `catalog.rs`), not a substitute.
    println!("\nrunning average_rating numeric-range filter at tier {tier}...");
    for threshold in [4.0_f64, 4.5] {
        let fq = format!("average_rating:[{threshold} TO *]");
        let constraint = vec![ResolvedConstraint::Attribute(Constraint::Numeric {
            attribute: "average_rating".to_string(),
            op: NumericOp::Gte,
            value: threshold,
        })];
        let (native_ns, native_count) = time_reps(|| index.indexed_candidates(&constraint).len());
        let (solr_ns, solr_count) =
            time_reps(|| solr_num_found(&solr_base_url, std::slice::from_ref(&fq)));
        rows.push(make_row(
            &tier,
            "numeric_range_average_rating_gte",
            &format!("{threshold}"),
            native_count,
            solr_count,
            native_ns,
            solr_ns,
            &mut mismatches,
        ));
    }

    // Combined: a real depth-1 subtree AND a numeric-range rating floor
    // in the same request -- Issue #21's "mixed PLP requests" class.
    {
        let name = "Furniture";
        let threshold = 4.0_f64;
        let fq = vec![
            fq_exact("category_depth_1", name),
            format!("average_rating:[{threshold} TO *]"),
        ];
        let constraint = vec![
            ResolvedConstraint::Attribute(Constraint::Enum {
                attribute: "category_depth_1".to_string(),
                value: name.to_string(),
            }),
            ResolvedConstraint::Attribute(Constraint::Numeric {
                attribute: "average_rating".to_string(),
                op: NumericOp::Gte,
                value: threshold,
            }),
        ];
        let (native_ns, native_count) = time_reps(|| index.indexed_candidates(&constraint).len());
        let (solr_ns, solr_count) = time_reps(|| solr_num_found(&solr_base_url, &fq));
        rows.push(make_row(
            &tier,
            "mixed_depth1_and_rating_filter",
            &format!("{name}+rating>={threshold}"),
            native_count,
            solr_count,
            native_ns,
            solr_ns,
            &mut mismatches,
        ));
    }

    println!("\n=== P6B-E00 tier {tier} result ===");
    println!(
        "{:<10} {:<42} {:<26} {:>10} {:>10} {:>7} {:>10} {:>10} {:>10}",
        "tier", "class", "key", "n_count", "s_count", "match", "n_mean_ms", "s_mean_ms", "speedup"
    );
    for r in &rows {
        println!(
            "{:<10} {:<42} {:<26} {:>10} {:>10} {:>7} {:>10.4} {:>10.4} {:>10.2}",
            r.tier,
            r.class,
            r.key.chars().take(26).collect::<String>(),
            r.native_count,
            r.solr_count,
            r.counts_match,
            r.native_mean_ms,
            r.solr_mean_ms,
            r.speedup_mean
        );
    }

    let all_match = rows.iter().all(|r| r.counts_match) && mismatches.is_empty();
    println!(
        "\ncorrectness: {}/{} rows had matching native/Solr counts{}",
        rows.iter().filter(|r| r.counts_match).count(),
        rows.len(),
        if all_match {
            " (ALL MATCH)"
        } else {
            " -- SEE MISMATCHES BELOW"
        }
    );
    for m in &mismatches {
        println!("  {m}");
    }

    // Append to the shared ladder CSV -- one line per tier's per-row
    // result, plus the tier-level build/RSS/size metrics repeated on
    // every row so the CSV stays flat and simple to plot from.
    if let Some(parent) = csv_out.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let write_header = !csv_out.exists();
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&csv_out)
        .expect("open csv_out");
    if write_header {
        writeln!(
            f,
            "tier,n_products,build_ms,index_bytes,index_bytes_per_product,rss_after_build_kb,class,key,native_count,solr_count,counts_match,native_mean_ms,native_p50_ms,native_p99_ms,solr_mean_ms,solr_p50_ms,solr_p99_ms,speedup_mean"
        )
        .unwrap();
    }
    for r in &rows {
        writeln!(
            f,
            "{},{},{:.2},{},{:.2},{},{},{},{},{},{},{},{:.4},{:.4},{:.4},{:.4},{:.4},{:.2}",
            r.tier,
            n_products,
            build_elapsed_ms,
            index_bytes,
            index_bytes as f64 / n_products as f64,
            rss_after_build_kb.unwrap_or(0),
            r.class,
            r.key.replace(',', ";"),
            r.native_count,
            r.solr_count,
            r.counts_match,
            r.native_mean_ms,
            r.native_p50_ms,
            r.native_p99_ms,
            r.solr_mean_ms,
            r.solr_p50_ms,
            r.solr_p99_ms,
            r.speedup_mean
        )
        .unwrap();
    }
    println!("\nappended {} rows to {}", rows.len(), csv_out.display());
}
