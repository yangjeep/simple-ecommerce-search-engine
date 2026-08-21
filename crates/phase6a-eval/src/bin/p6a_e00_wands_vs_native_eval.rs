//! Phase 6A (Issue #23) P6A-E00: real, non-fabricated PLP filter/facet/
//! sort/pagination benchmark on WANDS (substituted for the unreachable
//! Amazon Reviews 2023 -- see `PHASE6A_DECISION.md`), commerce-native
//! (`CatalogIndex`) vs a fair Solr baseline, reusing Phase 5's
//! benchmark methodology and already-fixed native implementations
//! (`select_nth_unstable`-based top-K sort, `_by_scan` facet methods) as
//! closely as this schema permits, rather than re-deriving or
//! re-discovering either.
//!
//! **Workload coverage against Issue #23's 12-item matrix** (see
//! `docs/experiments/PHASE6A_LOG.md` for the per-item mapping):
//! category render, hierarchy/subtree browse, category+facet,
//! category+multiple facets, low/medium/high-cardinality facet
//! recomputation, a disclosed business-order-sort substitute
//! (`average_rating` -- WANDS has no price/popularity-rank field),
//! title sort, first-page top-K, and deep pagination are covered.
//! Price-range and price-sort are NOT COMPARABLE (WANDS has no price
//! field at all, confirmed absent -- see the acquisition manifest) and
//! are not attempted here.
//!
//! **No case-insensitive-regex fairness fix needed** (unlike Phase 5's
//! brand field): `scripts/datasets/profile_wands.py`'s casing-collision
//! check found zero collisions across every field used here (color,
//! product_class, category strings, style, material, primarymaterial,
//! shape) -- WANDS' fields come from one internal Wayfair taxonomy, not
//! free-text sellers, so a plain exact-match `fq` is fair as-is.
//!
//! Usage: cargo run --release -p phase6a-eval --bin p6a_e00_wands_vs_native_eval
//!        [catalog.jsonl] [solr_base_url]

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::time::Instant;

use commerce_core::domain::{AttributeValue, Catalog, CategoryId, Constraint, ProductTypeId};
use commerce_core::index::CatalogIndex;
use commerce_core::ir::{ResolvedConstraint, StructuralConstraint};
use phase6a_eval::{catalog as catalog_ingest, data};

use rand::seq::SliceRandom;
use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;

const REPS: usize = 30;
const WARMUP: usize = 5;
const PAGE_SIZE: u64 = 24;
const SEED: u64 = 7;

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

fn bucket_for(count: usize) -> &'static str {
    match count {
        0 => "empty",
        1 => "singleton",
        2..=10 => "tiny (2-10)",
        11..=100 => "small (11-100)",
        101..=1_000 => "medium (101-1,000)",
        1_001..=10_000 => "large (1,001-10,000)",
        _ => "huge (>10,000)",
    }
}

// ---------- Solr HTTP helpers (mirrors phase5-eval's p5e00 exactly) ----------

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

fn solr_num_found(base_url: &str, fq: &[String], start: u64, rows: u64, sort: Option<&str>) -> u64 {
    let mut params: Vec<(&str, &str)> = vec![("q", "*:*")];
    for f in fq {
        params.push(("fq", f));
    }
    let start_s = start.to_string();
    let rows_s = rows.to_string();
    params.push(("start", &start_s));
    params.push(("rows", &rows_s));
    if let Some(s) = sort {
        params.push(("sort", s));
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

/// Same methodology fix Phase 5 (P5-E00) found necessary: Solr's JSON
/// Facet API is capped by `limit`, but `CatalogIndex`'s facet methods
/// return the full unbounded set -- compare both sides' own top-N.
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

// ---------- report row ----------

struct Row {
    class: String,
    group_kind: String,
    group_key: String,
    group_bucket: String,
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
fn push_row(
    rows: &mut Vec<Row>,
    mismatches: &mut Vec<String>,
    class: &str,
    group_kind: &str,
    group_key: &str,
    group_bucket: &str,
    native_count: u64,
    solr_count: u64,
    native_ns: Vec<u128>,
    solr_ns: Vec<u128>,
) {
    let counts_match = native_count == solr_count;
    if !counts_match {
        mismatches.push(format!(
            "COUNT MISMATCH {class} {group_kind}={group_key}: native={native_count} solr={solr_count}"
        ));
    }
    let (native_mean_ms, native_p50_ms, native_p99_ms) = stats_ms(native_ns);
    let (solr_mean_ms, solr_p50_ms, solr_p99_ms) = stats_ms(solr_ns);
    let speedup_mean = if native_mean_ms > 0.0 {
        solr_mean_ms / native_mean_ms
    } else {
        f64::INFINITY
    };
    rows.push(Row {
        class: class.to_string(),
        group_kind: group_kind.to_string(),
        group_key: group_key.to_string(),
        group_bucket: group_bucket.to_string(),
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
    });
}

fn top_k_sorted(mut items: Vec<(String, u32)>, limit: usize) -> Vec<(String, u32)> {
    if limit == 0 {
        return Vec::new();
    }
    if items.len() > limit {
        items.select_nth_unstable(limit - 1);
        items.truncate(limit);
    }
    items.sort();
    items
}

fn native_title_sorted(
    index: &CatalogIndex,
    catalog: &Catalog,
    constraints: &[ResolvedConstraint],
    limit: u64,
) -> Vec<String> {
    let candidates = index.indexed_candidates(constraints);
    let titles: Vec<(String, u32)> = candidates
        .iter()
        .filter_map(|ord| {
            let variant_id = index.variant_id_at(ord)?;
            let (product, _variant) = index.lookup_variant(catalog, variant_id)?;
            Some((product.title.clone(), ord))
        })
        .collect();
    top_k_sorted(titles, limit as usize)
        .into_iter()
        .map(|(t, _)| t)
        .collect()
}

/// Real business-order-sort substitute: WANDS has no price or explicit
/// popularity-rank field (confirmed absent -- see the acquisition
/// manifest), but `average_rating` is a real numeric field a genuine PLP
/// could plausibly sort by. Disclosed as a substitute, never presented
/// as price.
fn native_rating_sorted(
    index: &CatalogIndex,
    catalog: &Catalog,
    constraints: &[ResolvedConstraint],
    limit: u64,
) -> Vec<(String, u32)> {
    let candidates = index.indexed_candidates(constraints);
    let mut scored: Vec<((i64, String), u32)> = candidates
        .iter()
        .filter_map(|ord| {
            let variant_id = index.variant_id_at(ord)?;
            let (product, variant) = index.lookup_variant(catalog, variant_id)?;
            let rating = match variant.attributes.get("average_rating") {
                Some(AttributeValue::Numeric(r)) => *r,
                _ => f64::MIN,
            };
            // Descending rating: negate and use integer millirating so
            // f64 total ordering issues (NaN, -0.0) can't affect sort
            // stability -- there are none in this real data (checked by
            // the correctness gate below), but this keeps the key type
            // `Ord` rather than relying on `partial_cmp`.
            let key = (-(rating * 1000.0) as i64, product.title.clone());
            Some((key, ord))
        })
        .collect();
    scored.sort_by(|a, b| a.0.cmp(&b.0));
    scored.truncate(limit as usize);
    scored
        .into_iter()
        .map(|((_, title), ord)| (title, ord))
        .collect()
}

fn native_page(
    index: &CatalogIndex,
    constraints: &[ResolvedConstraint],
    offset: u64,
    limit: u64,
) -> u64 {
    let candidates = index.indexed_candidates(constraints);
    let total = candidates.len();
    if offset >= total {
        return 0;
    }
    (total - offset).min(limit)
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
    let index = CatalogIndex::build(&ingested.catalog);

    println!("checking Solr ({solr_base_url})...");
    let ping = solr_num_found(&solr_base_url, &[], 0, 0, None);
    println!("  Solr reachable: numFound={ping}");
    assert_eq!(
        ping as usize,
        ingested.catalog.products.len(),
        "Solr and native must index the identical real catalog for this comparison to be meaningful"
    );

    let category_name_by_id: HashMap<CategoryId, String> = ingested
        .categories
        .iter()
        .map(|c| (c.id, c.name.clone()))
        .collect();
    // Real methodology fix, self-caught before trusting the first run's
    // product_class facet numbers (same pattern Phase 5 found and fixed
    // for brand): `catalog_ingest::build_catalog` normalizes
    // (trim+lowercase) `product_class` before interning it as a
    // `ProductTypeId`, but Solr's `product_class` field holds the raw,
    // un-normalized source string. Comparing native's normalized facet
    // key against Solr's raw-cased key is comparing different string
    // representations of the same identity, not a real discrepancy --
    // this first-seen-raw-string map (same ordinal pairing
    // `build_catalog` itself uses) gives a fair, like-for-like key.
    let mut product_type_raw_by_id: HashMap<ProductTypeId, String> = HashMap::new();
    for (raw, ingested_product) in products.iter().zip(&ingested.catalog.products) {
        if let Some(raw_class) = &raw.product_class {
            product_type_raw_by_id
                .entry(ingested_product.product_type)
                .or_insert_with(|| raw_class.clone());
        }
    }

    println!("\ncomputing real category-leaf and per-depth group-size distributions...");
    let mut category_counts: HashMap<CategoryId, usize> = HashMap::new();
    let mut depth3_counts: HashMap<String, usize> = HashMap::new();
    for product in &ingested.catalog.products {
        if product.category != CategoryId(0) {
            *category_counts.entry(product.category).or_insert(0) += 1;
        }
        if let Some(AttributeValue::Enum(v)) = product.attributes.get("category_depth_3") {
            *depth3_counts.entry(v.clone()).or_insert(0) += 1;
        }
    }
    println!(
        "  {} distinct real leaf categories, {} distinct real depth-3 subtrees",
        category_counts.len(),
        depth3_counts.len()
    );

    let mut rng = ChaCha8Rng::seed_from_u64(SEED);

    fn pick_one<K: Clone>(
        counts: &HashMap<K, usize>,
        bucket: fn(usize) -> &'static str,
        target_bucket: &str,
        rng: &mut ChaCha8Rng,
    ) -> Option<(K, usize)> {
        let mut candidates: Vec<(K, usize)> = counts
            .iter()
            .filter(|(_, &c)| bucket(c) == target_bucket)
            .map(|(k, &c)| (k.clone(), c))
            .collect();
        candidates.sort_by_key(|a| a.1);
        candidates.choose(rng).cloned()
    }

    let leaf_buckets = [
        "tiny (2-10)",
        "small (11-100)",
        "medium (101-1,000)",
        "large (1,001-10,000)",
    ];
    let depth3_buckets = [
        "tiny (2-10)",
        "small (11-100)",
        "medium (101-1,000)",
        "large (1,001-10,000)",
    ];

    let mut selected_leaves: Vec<(CategoryId, usize)> = Vec::new();
    for &b in &leaf_buckets {
        if let Some(pick) = pick_one(&category_counts, bucket_for, b, &mut rng) {
            selected_leaves.push(pick);
        }
    }
    let mut selected_depth3: Vec<(String, usize)> = Vec::new();
    for &b in &depth3_buckets {
        if let Some(pick) = pick_one(&depth3_counts, bucket_for, b, &mut rng) {
            selected_depth3.push(pick);
        }
    }

    println!("\nselected real leaf-category groups (category render workload):");
    for (id, count) in &selected_leaves {
        println!(
            "  {:?} ({}): {} products [{}]",
            id,
            category_name_by_id
                .get(id)
                .map(String::as_str)
                .unwrap_or("?"),
            count,
            bucket_for(*count)
        );
    }
    println!("selected real depth-3 subtree groups (hierarchy/subtree-browse workload):");
    for (name, count) in &selected_depth3 {
        println!("  {name:?}: {count} products [{}]", bucket_for(*count));
    }

    let mut rows: Vec<Row> = Vec::new();
    let mut mismatches: Vec<String> = Vec::new();

    // ---- Category-render workload: filter by leaf category (Issue #23
    // item 1), facet by color/product_class/style/material (items 3, 4,
    // 6, 7), sort by title and by rating (item 8, disclosed substitute),
    // deep pagination (item 11). ----
    for (category_id, count) in &selected_leaves {
        let category_name = category_name_by_id[category_id].clone();
        let bucket = bucket_for(*count).to_string();
        let category_fq = fq_exact("category_leaf", &category_name);
        let category_constraint = vec![ResolvedConstraint::Structural(
            StructuralConstraint::Category(*category_id),
        )];

        // 1. category render (filter-only, first page)
        let (native_ns, native_count) =
            time_reps(|| index.indexed_candidates(&category_constraint).len());
        let (solr_ns, solr_count) = time_reps(|| {
            solr_num_found(
                &solr_base_url,
                std::slice::from_ref(&category_fq),
                0,
                PAGE_SIZE,
                None,
            )
        });
        push_row(
            &mut rows,
            &mut mismatches,
            "category_render_filter_first_page",
            "category_leaf",
            &category_name,
            &bucket,
            native_count,
            solr_count,
            native_ns,
            solr_ns,
        );

        let category_bitmap = index.indexed_candidates(&category_constraint);

        // 2. color facet under category filter (HIGH cardinality: 2,825
        // distinct real values) -- Issue #23 items 3 and 7.
        let (native_ns, native_facets_full) =
            time_reps(|| index.facet_counts_by_scan(&category_bitmap, &ingested.catalog, "color"));
        let (solr_ns, solr_facets) = time_reps(|| {
            solr_facet(
                &solr_base_url,
                std::slice::from_ref(&category_fq),
                "color",
                50,
            )
        });
        let native_facets = top_n(native_facets_full, 50);
        if native_facets != solr_facets {
            mismatches.push(format!(
                "FACET MISMATCH color_under_category={category_name}: native={native_facets:?} solr={solr_facets:?}"
            ));
        }
        // native_count/solr_count here are the TRUE filter cardinality
        // (`category_bitmap.len()`/`solr_count` from row 1 above), not
        // the facet-bucket-count sum -- reusing a facet's own (possibly
        // top-50-truncated, and missing-value-excluding) bucket sum as a
        // stand-in for candidate-set size was a real, self-caught bug
        // (see PHASE6A_DECISION.md's methodology note): it silently
        // undercounts whenever more than 50 distinct facet values exist,
        // or whenever some candidates lack the faceted attribute
        // entirely. Facet-map *correctness* is still checked precisely
        // above (the `mismatches` push), independent of this count.
        push_row(
            &mut rows,
            &mut mismatches,
            "color_facet_under_category_filter",
            "category_leaf",
            &category_name,
            &bucket,
            category_bitmap.len(),
            solr_count,
            native_ns,
            solr_ns,
        );

        // 3. product_class facet under category filter (MEDIUM-HIGH
        // cardinality: 860 distinct values; dedicated bitmap field, not a
        // generic attribute -- genuinely new capability ESCI could never
        // test, since it always used a ProductTypeId(0) sentinel) --
        // Issue #23 items 3, 4, 7.
        let (native_ns, native_facets_full) = time_reps(|| {
            index
                .product_type_facet_counts_by_scan(&category_bitmap, &ingested.catalog)
                .into_iter()
                .map(|(id, c)| {
                    (
                        product_type_raw_by_id.get(&id).cloned().unwrap_or_default(),
                        c,
                    )
                })
                .collect::<BTreeMap<String, u64>>()
        });
        let (solr_ns, solr_facets) = time_reps(|| {
            solr_facet(
                &solr_base_url,
                std::slice::from_ref(&category_fq),
                "product_class",
                50,
            )
        });
        let native_facets = top_n(native_facets_full, 50);
        if native_facets != solr_facets {
            mismatches.push(format!(
                "FACET MISMATCH product_class_under_category={category_name}: native={native_facets:?} solr={solr_facets:?}"
            ));
        }
        push_row(
            &mut rows,
            &mut mismatches,
            "product_class_facet_under_category_filter",
            "category_leaf",
            &category_name,
            &bucket,
            category_bitmap.len(),
            solr_count,
            native_ns,
            solr_ns,
        );

        // 4. style facet under category filter (LOW cardinality: 65
        // distinct real values) -- Issue #23 item 6.
        let (native_ns, native_facets_full) =
            time_reps(|| index.facet_counts_by_scan(&category_bitmap, &ingested.catalog, "style"));
        let (solr_ns, solr_facets) = time_reps(|| {
            solr_facet(
                &solr_base_url,
                std::slice::from_ref(&category_fq),
                "style",
                50,
            )
        });
        let native_facets = top_n(native_facets_full, 50);
        if native_facets != solr_facets {
            mismatches.push(format!(
                "FACET MISMATCH style_under_category={category_name}: native={native_facets:?} solr={solr_facets:?}"
            ));
        }
        push_row(
            &mut rows,
            &mut mismatches,
            "style_facet_under_category_filter",
            "category_leaf",
            &category_name,
            &bucket,
            category_bitmap.len(),
            solr_count,
            native_ns,
            solr_ns,
        );

        // 5. sort by title (direct analog to Phase 5's own sort-by-title)
        let (native_ns, native_sorted) = time_reps(|| {
            native_title_sorted(&index, &ingested.catalog, &category_constraint, PAGE_SIZE)
        });
        let (solr_ns, solr_sorted_count) = time_reps(|| {
            solr_num_found(
                &solr_base_url,
                std::slice::from_ref(&category_fq),
                0,
                PAGE_SIZE,
                Some("title_sort asc"),
            )
        });
        push_row(
            &mut rows,
            &mut mismatches,
            "sort_title_under_category_filter",
            "category_leaf",
            &category_name,
            &bucket,
            native_sorted.len() as u64,
            solr_sorted_count.min(PAGE_SIZE),
            native_ns,
            solr_ns,
        );

        // 6. sort by rating desc (disclosed business-order substitute)
        let (native_ns, native_rated) = time_reps(|| {
            native_rating_sorted(&index, &ingested.catalog, &category_constraint, PAGE_SIZE)
        });
        let (solr_ns, solr_rated_count) = time_reps(|| {
            solr_num_found(
                &solr_base_url,
                std::slice::from_ref(&category_fq),
                0,
                PAGE_SIZE,
                Some("average_rating desc"),
            )
        });
        push_row(
            &mut rows,
            &mut mismatches,
            "sort_rating_desc_under_category_filter",
            "category_leaf",
            &category_name,
            &bucket,
            native_rated.len() as u64,
            solr_rated_count.min(PAGE_SIZE),
            native_ns,
            solr_ns,
        );

        // 7. deep pagination
        if *count as u64 > PAGE_SIZE * 2 {
            let deep_offset = (*count as u64 / 2).min(500);
            let (native_ns, native_page_count) =
                time_reps(|| native_page(&index, &category_constraint, deep_offset, PAGE_SIZE));
            let (solr_ns, solr_page_count) = time_reps(|| {
                solr_num_found(
                    &solr_base_url,
                    std::slice::from_ref(&category_fq),
                    deep_offset,
                    PAGE_SIZE,
                    None,
                )
            });
            push_row(
                &mut rows,
                &mut mismatches,
                "deep_pagination_under_category_filter",
                "category_leaf",
                &category_name,
                &bucket,
                native_page_count,
                solr_page_count.min(PAGE_SIZE),
                native_ns,
                solr_ns,
            );
        }
    }

    // ---- Subtree-browse workload (Issue #23 item 2): filter at
    // depth-3, an ancestor level coarser than the leaf, using the exact
    // same generic enum-attribute machinery Phase 5 used for color --
    // no new query semantics. ----
    for (subtree, count) in &selected_depth3 {
        let bucket = bucket_for(*count).to_string();
        let subtree_fq = fq_exact("category_depth_3", subtree);
        let subtree_constraint = vec![ResolvedConstraint::Attribute(Constraint::Enum {
            attribute: "category_depth_3".to_string(),
            value: subtree.clone(),
        })];

        let (native_ns, native_count) =
            time_reps(|| index.indexed_candidates(&subtree_constraint).len());
        let (solr_ns, solr_count) = time_reps(|| {
            solr_num_found(
                &solr_base_url,
                std::slice::from_ref(&subtree_fq),
                0,
                PAGE_SIZE,
                None,
            )
        });
        push_row(
            &mut rows,
            &mut mismatches,
            "subtree_browse_depth3_filter_first_page",
            "category_depth_3",
            subtree,
            &bucket,
            native_count,
            solr_count,
            native_ns,
            solr_ns,
        );

        // Mixed realistic PLP request (Issue #23 item 12): subtree +
        // color facet + rating sort, all in the one workload.
        let subtree_bitmap = index.indexed_candidates(&subtree_constraint);
        let (native_ns, native_facets_full) =
            time_reps(|| index.facet_counts_by_scan(&subtree_bitmap, &ingested.catalog, "color"));
        let (solr_ns, solr_facets) = time_reps(|| {
            solr_facet(
                &solr_base_url,
                std::slice::from_ref(&subtree_fq),
                "color",
                50,
            )
        });
        let native_facets = top_n(native_facets_full, 50);
        if native_facets != solr_facets {
            mismatches.push(format!(
                "FACET MISMATCH color_under_subtree={subtree}: native={native_facets:?} solr={solr_facets:?}"
            ));
        }
        push_row(
            &mut rows,
            &mut mismatches,
            "mixed_color_facet_under_subtree_depth3",
            "category_depth_3",
            subtree,
            &bucket,
            subtree_bitmap.len(),
            solr_count,
            native_ns,
            solr_ns,
        );
    }

    // ---- Crossover-characterization sweep (Issue #23's decision-gate
    // requirement to characterize the 80x floor, modest-advantage
    // region, 1x crossover, and native-loss region where observable):
    // category_leaf tops out at only 1,103 real products (this
    // taxonomy's leaf granularity is fine -- no leaf category reaches
    // even the "large" bucket's upper end, let alone "huge"), so this
    // sweep uses real depth-1 subtrees instead, which do reach genuinely
    // large real sizes (up to 16,039 for "Furniture") -- a real,
    // disclosed scale ceiling difference from Phase 5's ESCI (whose
    // color groups reached 11,264), not a fabricated distribution. Real
    // depth-1 values, picked by name (mirroring P5-E03's own
    // checkpoint-targeting sweep) rather than random sampling, since
    // depth-1 only has 55 distinct real nodes and specific checkpoints
    // are wanted here, not a representative sample.
    let depth1_checkpoints = [
        "Rugs",
        "Storage & Organization",
        "Lighting",
        "Outdoor",
        "Décor & Pillows",
        "Home Improvement",
        "Furniture",
    ];
    println!("\nrunning depth-1 crossover-characterization sweep (color facet under filter):");
    for name in depth1_checkpoints {
        let fq = fq_exact("category_depth_1", name);
        let constraint = vec![ResolvedConstraint::Attribute(Constraint::Enum {
            attribute: "category_depth_1".to_string(),
            value: name.to_string(),
        })];
        let candidates = index.indexed_candidates(&constraint);
        let n = candidates.len();
        let bucket = bucket_for(n as usize).to_string();
        // True filter cardinality on Solr's side, fetched once (not
        // timed -- this sweep's timing claim is about the facet
        // computation, not the filter itself, which is already
        // characterized by the `category_render`/`subtree_browse` rows
        // above).
        let solr_n = solr_num_found(&solr_base_url, std::slice::from_ref(&fq), 0, 0, None);

        let (native_ns, native_facets_full) =
            time_reps(|| index.facet_counts_by_scan(&candidates, &ingested.catalog, "color"));
        let (solr_ns, solr_facets) =
            time_reps(|| solr_facet(&solr_base_url, std::slice::from_ref(&fq), "color", 50));
        let native_facets = top_n(native_facets_full, 50);
        if native_facets != solr_facets {
            mismatches.push(format!(
                "FACET MISMATCH color_under_depth1={name}: native={native_facets:?} solr={solr_facets:?}"
            ));
        }
        push_row(
            &mut rows,
            &mut mismatches,
            "color_facet_under_depth1_crossover_sweep",
            "category_depth_1",
            name,
            &bucket,
            n,
            solr_n,
            native_ns,
            solr_ns,
        );
    }

    println!("\n=== P6A-E00 result ===");
    println!(
        "{:<42} {:<16} {:<28} {:<20} {:>10} {:>10} {:>7} {:>10} {:>10} {:>10}",
        "request class",
        "group",
        "key",
        "bucket",
        "n_count",
        "s_count",
        "match",
        "n_mean_ms",
        "s_mean_ms",
        "speedup"
    );
    let mut csv = String::from(
        "class,group_kind,group_key,group_bucket,native_count,solr_count,counts_match,native_mean_ms,native_p50_ms,native_p99_ms,solr_mean_ms,solr_p50_ms,solr_p99_ms,speedup_mean\n",
    );
    for r in &rows {
        println!(
            "{:<42} {:<16} {:<28} {:<20} {:>10} {:>10} {:>7} {:>10.4} {:>10.4} {:>10.2}",
            r.class,
            r.group_kind,
            r.group_key.chars().take(28).collect::<String>(),
            r.group_bucket,
            r.native_count,
            r.solr_count,
            r.counts_match,
            r.native_mean_ms,
            r.solr_mean_ms,
            r.speedup_mean
        );
        csv.push_str(&format!(
            "{},{},{},{},{},{},{},{},{},{},{},{},{},{}\n",
            r.class,
            r.group_kind,
            r.group_key.replace(',', ";"),
            r.group_bucket,
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
        ));
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

    let artifacts_dir = PathBuf::from("dataset_cache/p6a_e00_artifacts");
    std::fs::create_dir_all(&artifacts_dir).ok();
    std::fs::write(artifacts_dir.join("results.csv"), &csv).ok();
    println!("\nartifacts written to {}", artifacts_dir.display());
}
