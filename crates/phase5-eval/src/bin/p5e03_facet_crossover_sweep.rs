//! Issue #17 Phase 5, Stage B (P5-E03), facet-cardinality sub-experiment.
//!
//! P5-E00 found that `CatalogIndex::brand_facet_counts_by_scan` (an
//! `O(|candidates|)` alternative to the existing `O(global vocabulary)`
//! `brand_facet_counts`) resolves native faceting's measured 20-80x
//! *loss* against a fairly-tuned Solr baseline into a 4-7800x *win* for
//! every real color-group size this catalog produces except the largest
//! sampled (11,264 candidates), where it crossed over to a 0.74x native
//! *loss* -- because scan cost is linear in `|candidates|` while Solr's
//! docValues-backed facet cost is close to flat. That crossover was
//! observed at exactly one point, not characterized.
//!
//! This binary characterizes it precisely: it selects real color groups
//! (never fabricated sizes) whose actual sizes are closest to a spread of
//! target checkpoints spanning the suspected crossover region, times the
//! same `brand_facet_under_color_filter` request P5-E00 measured at each,
//! and reports where the native-scan/Solr time ratio actually crosses 1.0.
//!
//! Usage: cargo run --release -p phase5-eval --bin p5e03_facet_crossover_sweep
//!        [catalog.jsonl] [solr_base_url]

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::time::Instant;

use commerce_core::domain::{AttributeValue, BrandId, Constraint};
use commerce_core::index::CatalogIndex;
use commerce_core::ir::ResolvedConstraint;
use round1_eval::catalog as catalog_ingest;
use round1_eval::data;

const REPS: usize = 30;
const WARMUP: usize = 5;

/// Real checkpoint targets spanning the P5-E00-observed crossover region
/// (a native-scan win at 2,112 candidates, a native-scan loss at 11,264) --
/// plus a few points further out on each side for context. Each is matched
/// to the *closest real color-group size that actually exists* in this
/// catalog, not fabricated.
const TARGET_CHECKPOINTS: &[usize] = &[
    500, 1_500, 3_000, 4_500, 6_000, 7_500, 9_000, 10_500, 12_000, 15_000, 20_000, 30_000,
];

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

fn escape_solr_phrase(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

fn fq_exact(field: &str, value: &str) -> String {
    format!("{field}:\"{}\"", escape_solr_phrase(value))
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

/// Same top-N truncation fix P5-E00 required: native's facet map is
/// unbounded, Solr's JSON Facet API is capped by `limit` -- comparing an
/// unbounded sum against a capped one is a methodology bug, not a real
/// discrepancy.
fn top_n(
    map: BTreeMap<BrandId, u64>,
    brand_raw_by_id: &HashMap<BrandId, String>,
    n: usize,
) -> BTreeMap<String, u64> {
    let mut entries: Vec<(String, u64)> = map
        .into_iter()
        .map(|(id, c)| (brand_raw_by_id.get(&id).cloned().unwrap_or_default(), c))
        .collect();
    entries.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    entries.truncate(n);
    entries.into_iter().collect()
}

/// Diagnostic for a facet-count mismatch: rather than dumping both full
/// (up to 50-entry) maps every time, reports only the actual symmetric
/// difference -- keys present on only one side, and keys present on both
/// with different counts -- plus flags any native-only/Solr-only pair
/// that differs only by casing (P5-E00's already-documented
/// brand-casing-consolidation artifact: native facets by `BrandId`, which
/// interns case-insensitively, so `"STAR WARS"`/`"Star Wars"` collapse to
/// one native bucket but stay separate Solr buckets). Anything left over
/// after that explanation is a real, unexplained discrepancy worth
/// investigating further, not silently absorbed into the same footnote.
fn print_facet_diff(color: &str, native: &BTreeMap<String, u64>, solr: &BTreeMap<String, u64>) {
    let native_only: Vec<&String> = native.keys().filter(|k| !solr.contains_key(*k)).collect();
    let solr_only: Vec<&String> = solr.keys().filter(|k| !native.contains_key(*k)).collect();
    let differing: Vec<(&String, u64, u64)> = native
        .iter()
        .filter_map(|(k, &nc)| solr.get(k).filter(|&&sc| sc != nc).map(|&sc| (k, nc, sc)))
        .collect();

    let mut unexplained_native_only = Vec::new();
    for n in &native_only {
        let has_casing_twin = solr_only
            .iter()
            .any(|s| s.eq_ignore_ascii_case(n) && *s != *n);
        if !has_casing_twin {
            unexplained_native_only.push(*n);
        }
    }
    let mut unexplained_solr_only = Vec::new();
    for s in &solr_only {
        let has_casing_twin = native_only
            .iter()
            .any(|n| n.eq_ignore_ascii_case(s) && *n != *s);
        if !has_casing_twin {
            unexplained_solr_only.push(*s);
        }
    }

    println!(
        "  MISMATCH DIFF color={color:?}: native_only={native_only:?} solr_only={solr_only:?} differing_counts={differing:?}"
    );
    println!(
        "    after excluding casing-twin pairs: unexplained_native_only={unexplained_native_only:?} unexplained_solr_only={unexplained_solr_only:?}"
    );
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
    let index = CatalogIndex::build(&ingested.catalog);

    println!("checking Solr ({solr_base_url})...");
    let ping = solr_num_found(&solr_base_url, &[]);
    assert_eq!(
        ping as usize,
        ingested.catalog.products.len(),
        "Solr and native must index the identical real catalog for this comparison to be meaningful"
    );
    println!("  Solr reachable: numFound={ping}");

    let mut brand_raw_by_id: HashMap<BrandId, String> = HashMap::new();
    for (raw, ingested_product) in products.iter().zip(&ingested.catalog.products) {
        if let Some(raw_brand) = &raw.brand {
            brand_raw_by_id
                .entry(ingested_product.brand)
                .or_insert_with(|| raw_brand.clone());
        }
    }

    println!("\ncomputing real color group-size distribution...");
    let mut color_counts: HashMap<String, usize> = HashMap::new();
    for product in &ingested.catalog.products {
        if let Some(AttributeValue::Enum(color)) = product.variants[0].attributes.get("color") {
            *color_counts.entry(color.clone()).or_insert(0) += 1;
        }
    }
    println!("  {} distinct real color values", color_counts.len());

    // For each target checkpoint, pick the real color value whose actual
    // count is closest to it -- never a fabricated size -- excluding
    // values already picked for an earlier checkpoint so the sweep covers
    // distinct real groups.
    let mut used: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut selected: Vec<(String, usize)> = Vec::new();
    for &target in TARGET_CHECKPOINTS {
        if let Some((color, count)) = color_counts
            .iter()
            .filter(|(c, _)| !used.contains(*c))
            .min_by_key(|(_, &count)| (count as i64 - target as i64).abs())
        {
            used.insert(color.clone());
            selected.push((color.clone(), *count));
        }
    }
    selected.sort_by_key(|(_, count)| *count);

    println!("\nselected real color groups (closest real match to each target checkpoint):");
    for (color, count) in &selected {
        println!("  {color:?}: {count} products");
    }

    println!("\n=== P5-E03 facet-crossover sweep result ===");
    println!(
        "{:<28} {:>10} {:>12} {:>12} {:>7}  {:>10} {:>10} {:>10}",
        "color", "candidates", "n_scan_ms", "solr_ms", "match", "n_count", "s_count", "ratio(s/n)"
    );

    let mut csv = String::from(
        "color,candidates,native_scan_mean_ms,solr_mean_ms,counts_match,native_count,solr_count,ratio_solr_over_native\n",
    );
    let mut crossover_reported = false;
    let mut prev_ratio: Option<f64> = None;
    let mut prev_candidates = 0usize;

    for (color, candidates) in &selected {
        let color_fq = fq_exact("color", color);
        let color_constraint = vec![ResolvedConstraint::Attribute(Constraint::Enum {
            attribute: "color".to_string(),
            value: color.clone(),
        })];
        let color_bitmap = index.indexed_candidates(&color_constraint);

        let (native_ns, native_facets_full) =
            time_reps(|| index.brand_facet_counts_by_scan(&color_bitmap, &ingested.catalog));
        let native_facets = top_n(native_facets_full, &brand_raw_by_id, 50);

        let (solr_ns, solr_facets) =
            time_reps(|| solr_facet(&solr_base_url, std::slice::from_ref(&color_fq), "brand", 50));

        let native_count: u64 = native_facets.values().sum();
        let solr_count: u64 = solr_facets.values().sum();
        let counts_match = native_count == solr_count;

        let (native_mean_ms, _, _) = stats_ms(native_ns);
        let (solr_mean_ms, _, _) = stats_ms(solr_ns);
        let ratio = if native_mean_ms > 0.0 {
            solr_mean_ms / native_mean_ms
        } else {
            f64::INFINITY
        };

        println!(
            "{:<28} {:>10} {:>12.4} {:>12.4} {:>7}  {:>10} {:>10} {:>10.2}",
            color,
            candidates,
            native_mean_ms,
            solr_mean_ms,
            counts_match,
            native_count,
            solr_count,
            ratio
        );
        csv.push_str(&format!(
            "{color},{candidates},{native_mean_ms},{solr_mean_ms},{counts_match},{native_count},{solr_count},{ratio}\n"
        ));
        if !counts_match {
            print_facet_diff(color, &native_facets, &solr_facets);
        }

        if !crossover_reported {
            if let Some(prev) = prev_ratio {
                if prev >= 1.0 && ratio < 1.0 {
                    println!(
                        "  >>> crossover: native-scan wins at {prev_candidates} candidates (ratio {prev:.2}), loses at {candidates} candidates (ratio {ratio:.2})"
                    );
                    crossover_reported = true;
                }
            }
        }
        prev_ratio = Some(ratio);
        prev_candidates = *candidates;
    }

    if !crossover_reported {
        println!(
            "\n  no crossover observed within the sampled range ({} to {} candidates) -- either always ahead, always behind, or the crossover lies outside this sweep's checkpoints",
            selected.first().map(|(_, c)| *c).unwrap_or(0),
            selected.last().map(|(_, c)| *c).unwrap_or(0)
        );
    }

    std::fs::create_dir_all("dataset_cache/p5e03_artifacts").ok();
    std::fs::write("dataset_cache/p5e03_artifacts/results.csv", csv).ok();
    println!("\nartifacts written to dataset_cache/p5e03_artifacts");
}
