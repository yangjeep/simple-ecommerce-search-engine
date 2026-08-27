//! Issue #34 Phase 9, P9-E02: re-run Phase 2's P1-D/P1-E-style physical-
//! advantage-by-query-class + traffic-weighted-economics measurement on
//! the real WANDS catalog (genuine `Category`/`ProductType` structural
//! entities, not just ESCI's single Brand entity), with both disclosed
//! defects fixed: FastPath's default ranking signal (P9-E00,
//! `commerce_core::index::rank::execute_ranked`) and Hybrid's bitmap-based
//! delegate restriction (P9-E01, `phase9_eval::bitmap_delegate`), against a
//! fresh, same-run, same-environment Solr baseline.
//!
//! **Hypothesis**: with both defects fixed, and on a catalog with genuine
//! multi-entity structural data, `commerce_core`'s structural/hybrid
//! execution shows a materially different (i.e. not uniformly
//! STOP-leaning, per Phase 2's ESCI-catalog result) relevance and/or
//! latency picture for at least the structural-dominant query classes
//! (`structural_exact_entity`, `selective_multi_attribute_structural`,
//! `variant_scoped_structural`) than Phase 2 found on ESCI.
//!
//! **Decision criteria, stated before implementation**:
//! - **KEEP/PROCEED signal**: for the traffic-weighted majority of real
//!   WANDS query classes, native NDCG@10 is within 10% relative of Solr's
//!   (not the -31.5%-class gap P2-E17 found for `structural_exact_entity`
//!   pre-fix) AND native mean latency is materially lower (>=2x, this
//!   project's standing bar).
//! - **REVISE signal**: a real split -- some classes clear the bar, others
//!   do not -- reported class-by-class, not averaged into one headline.
//! - **STOP/negative signal**: if native relevance still trails materially
//!   even with both defects fixed, that is preserved as a genuine, strong
//!   negative result (this project does not force a win) -- it would mean
//!   Phase 2's STOP is robust across catalogs, not an ESCI-specific
//!   artifact of the two defects alone.
//!
//! No aggregate number is reported without its traffic-weighted,
//! per-class breakdown alongside it (this project's standing "traffic-
//! weighted economics" discipline) -- a headline that hides a losing
//! class is exactly what this measurement exists to prevent.
//!
//! **A correction to a prior assumption in this session, disclosed rather
//! than smoothed over**: WANDS has no real price data at all (confirmed:
//! `dataset_cache/wands/product.csv` has no price column;
//! `phase6a_eval::catalog::build_catalog` uses a `Price::usd(0)` sentinel
//! for every product). `range_plus_structural` is therefore not expected
//! to populate meaningfully here either -- for a real reason (no data),
//! not an implementation gap. This experiment does not fabricate price
//! data to manufacture that class.

use std::collections::{BTreeMap, HashMap};

use commerce_core::cold_start::{compile_lexicon, CatalogProfile};
use commerce_core::domain::{BrandId, CategoryId, ProductId, ProductTypeId};
use commerce_core::index::CatalogIndex;
use commerce_core::ir::{compile, ResolvedConstraint};
use commerce_core::plan::{execute_planned, PlannerPolicy};
use comparator_eval::solr::solr_search;
use comparator_eval::translate::{translate_all, SolrFieldMap, StructuralNames};
use phase9_eval::bitmap_delegate::{build_index, BitmapTantivyDelegate, BuiltIndex};
use phase9_eval::wands_relevance::{ndcg_recall_mrr, WandsLabel};
use round1_eval::query_taxonomy::{classify9, QueryClass9};

const CATALOG_PATH: &str = "dataset_cache/wands/catalog.jsonl";
const QUERY_PATH: &str = "dataset_cache/wands/query.csv";
const LABEL_PATH: &str = "dataset_cache/wands/label.csv";
const K: usize = 10;
const MIN_ENUM_FREQUENCY: usize = 1;

struct WandsQuery {
    query_id: String,
    text: String,
}

fn load_queries(path: &str) -> Vec<WandsQuery> {
    let content = std::fs::read_to_string(path).expect("read query.csv");
    content
        .lines()
        .skip(1)
        .filter_map(|line| {
            let mut parts = line.splitn(3, '\t');
            let query_id = parts.next()?.to_string();
            let text = parts.next()?.to_string();
            Some(WandsQuery { query_id, text })
        })
        .collect()
}

/// `query_id -> (wands product_id -> label)`.
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

/// `StructuralNames` for the WANDS catalog: no `Brand` data exists at
/// all (WANDS has no brand field), so `brand_name` always returns
/// `None` -- combined with `wands_field_map`'s `brand: None` below, any
/// `Brand`/`BrandAny` constraint (which a generic compiler should never
/// actually produce against this catalog) translates to
/// `Translation::NotApplicable`, not a fabricated filter.
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

/// Issue #55 A3: `fq` is now built by `comparator_eval::translate::translate_all`,
/// the single, exhaustively-matched (no wildcard arm) constraint
/// translator shared across every comparator binary in this workspace --
/// replacing this function's own local match, whose missing
/// `ProductTypeAny` arm (`docs/decisions/ISSUE55_PAIRED_COMPARATOR_DECISION.md`)
/// is exactly the defect class the shared translator exists to make
/// impossible to reintroduce (a new `ResolvedConstraint`/`StructuralConstraint`
/// variant now fails to *compile* in the shared translator rather than
/// silently falling through a local `_ => {}`). `wands_field_map` declares
/// which Solr fields this core actually has (`category_leaf`,
/// `product_class`; no `brand`/price field), matching this function's
/// previous behavior for every constraint kind WANDS data can produce.
/// The free-text `q` construction (`title`/`description`, no `all_text`
/// copy field on this core) is unchanged.
fn wands_field_map() -> SolrFieldMap {
    SolrFieldMap {
        brand: None,
        product_type: Some("product_class"),
        category: Some("category_leaf"),
        price_cents: None,
    }
}

fn wands_solr_query_for(
    query_text: &str,
    residual_lexical: &[String],
    constraints: &[ResolvedConstraint],
    category_name_by_id: &HashMap<CategoryId, String>,
    product_type_name_by_id: &HashMap<ProductTypeId, String>,
) -> (String, Vec<String>, Vec<String>) {
    let names = WandsNames {
        category_name_by_id,
        product_type_name_by_id,
    };
    let (fq, translation_failures) = translate_all(constraints, &wands_field_map(), &names);
    let text = if residual_lexical.is_empty() {
        query_text.to_string()
    } else {
        residual_lexical.join(" ")
    };
    let q = if text.trim().is_empty() {
        "*:*".to_string()
    } else {
        format!("{{!edismax qf=\"title description\"}}{}", text)
    };
    (q, fq, translation_failures)
}

struct ResultPoint {
    native_ndcg: f64,
    native_recall: f64,
    native_mrr: f64,
    native_latency_ms: f64,
    solr_ndcg: f64,
    solr_recall: f64,
    solr_mrr: f64,
    solr_latency_ms: f64,
}

#[derive(Default)]
struct ClassResult {
    n: usize,
    native_ndcg: Vec<f64>,
    native_recall: Vec<f64>,
    native_mrr: Vec<f64>,
    native_latency_ms: Vec<f64>,
    solr_ndcg: Vec<f64>,
    solr_recall: Vec<f64>,
    solr_mrr: Vec<f64>,
    solr_latency_ms: Vec<f64>,
}

fn push_result<K: Ord>(map: &mut BTreeMap<K, ClassResult>, key: K, p: &ResultPoint) {
    let entry = map.entry(key).or_default();
    entry.n += 1;
    entry.native_ndcg.push(p.native_ndcg);
    entry.native_recall.push(p.native_recall);
    entry.native_mrr.push(p.native_mrr);
    entry.native_latency_ms.push(p.native_latency_ms);
    entry.solr_ndcg.push(p.solr_ndcg);
    entry.solr_recall.push(p.solr_recall);
    entry.solr_mrr.push(p.solr_mrr);
    entry.solr_latency_ms.push(p.solr_latency_ms);
}

fn mean(v: &[f64]) -> f64 {
    if v.is_empty() {
        0.0
    } else {
        v.iter().sum::<f64>() / v.len() as f64
    }
}

fn main() {
    println!(
        "=== P9-E02: WANDS physical-advantage-by-query-class + traffic-weighted economics ==="
    );

    let solr_base_url = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "http://localhost:8983/solr/wands_bench".to_string());

    println!("loading WANDS catalog...");
    let products = phase6a_eval::data::load_catalog(std::path::Path::new(CATALOG_PATH));
    let ingested = phase6a_eval::catalog::build_catalog(&products);
    println!(
        "catalog: {} products, {} categories, {} product types",
        ingested.catalog.products.len(),
        ingested.categories.len(),
        ingested.product_types.len()
    );

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
    let mut wands_id_by_product_id: HashMap<ProductId, String> = HashMap::new();
    for (wands_id, pid) in &ingested.wands_id_to_product_id {
        wands_id_by_product_id.insert(*pid, wands_id.clone());
    }

    println!("building native index + lexicon...");
    let index = CatalogIndex::build(&ingested.catalog);
    let profile = CatalogProfile::build(
        &ingested.catalog,
        &[], // WANDS has no real brand data
        &ingested.product_types,
        &ingested.categories,
    );
    let lexicon = compile_lexicon(&profile, MIN_ENUM_FREQUENCY);
    println!(
        "profile: {} distinct structural/attribute values (min_enum_frequency={MIN_ENUM_FREQUENCY})",
        profile.distinct_value_count()
    );

    println!("building bitmap-delegate Tantivy index...");
    let BuiltIndex {
        index: tantivy_index,
        title_field,
        description_field,
        ordinal_field: _,
    } = build_index(&ingested.catalog).expect("build tantivy index");
    let delegate = BitmapTantivyDelegate::new(&tantivy_index, vec![title_field, description_field])
        .expect("build bitmap delegate");

    let policy = PlannerPolicy {
        selectivity_threshold: 0.05,
        delegate_oversample: 20,
    };

    println!("loading queries + labels...");
    let queries = load_queries(QUERY_PATH);
    let judged = load_labels(LABEL_PATH);
    println!(
        "{} queries, {} judged query groups",
        queries.len(),
        judged.len()
    );

    // Warmup pass (this project's own P2-E16 precedent: an unwarmed Solr
    // latency measurement was previously found broken by exactly this
    // omission) -- runs every query against both engines once, discarding
    // results, before the single measured pass below. Warms Solr's JVM
    // JIT/query cache and the OS page cache for the Lucene index files;
    // the in-process native structures are already fully resident memory
    // regardless, so this only meaningfully changes Solr's numbers.
    println!(
        "warmup pass ({} queries, both engines, discarded)...",
        queries.len()
    );
    for q in &queries {
        let compiled = compile(&q.text, &lexicon);
        let _ = execute_planned(
            &compiled,
            &ingested.catalog,
            &index,
            Some(&delegate),
            K,
            &policy,
            None,
        );
        let (solr_q, solr_fq, _translation_failures) = wands_solr_query_for(
            &q.text,
            &compiled.residual_lexical,
            &compiled.constraints,
            &category_name_by_id,
            &product_type_name_by_id,
        );
        let _ = solr_search(
            &solr_base_url,
            "title description",
            &solr_q,
            &solr_fq,
            K,
            std::time::Duration::from_secs(30),
        );
    }

    let mut results: BTreeMap<QueryClass9, ClassResult> = BTreeMap::new();
    let mut by_routing: BTreeMap<&'static str, ClassResult> = BTreeMap::new();
    let mut evaluated = 0usize;
    let mut skipped_no_judgments = 0usize;
    let mut outcome_counts: BTreeMap<&'static str, usize> = BTreeMap::new();
    let mut zero_constraint_examples: Vec<String> = Vec::new();
    let mut variant_scoped_examples: Vec<(String, Vec<String>, Vec<String>)> = Vec::new();
    // Issue #55 A3 (`docs/decisions/ISSUE55_COMPARATOR_CENTRALIZATION_DECISION.md`):
    // a Solr transport/query/parse failure, or a constraint this run
    // could not symmetrically translate to `fq`, must never be scored as
    // native-favoring NDCG=0.0 (the confirmed bug this file previously
    // had) -- every such query is excluded from every aggregate below and
    // recorded here instead; a non-empty list aborts the run before any
    // number is printed, matching `issue35_eval::eval::run_vertical_eval`'s
    // existing discipline.
    let mut solr_failures: Vec<String> = Vec::new();

    for q in &queries {
        let Some(query_judged) = judged.get(&q.query_id) else {
            skipped_no_judgments += 1;
            continue;
        };
        let compiled = compile(&q.text, &lexicon);
        let class = classify9(&q.text, &compiled);
        if compiled.constraints.is_empty() && zero_constraint_examples.len() < 5 {
            zero_constraint_examples.push(q.text.clone());
        }

        let t0 = std::time::Instant::now();
        let (planned, native_hits) = execute_planned(
            &compiled,
            &ingested.catalog,
            &index,
            Some(&delegate),
            K,
            &policy,
            None,
        );
        let native_latency_ms = t0.elapsed().as_secs_f64() * 1000.0;
        let outcome_label = match planned.outcome {
            commerce_core::plan::ExecutionOutcome::FastPath => "FastPath",
            commerce_core::plan::ExecutionOutcome::Hybrid => "Hybrid",
            commerce_core::plan::ExecutionOutcome::Punt => "Punt",
        };
        *outcome_counts.entry(outcome_label).or_insert(0) += 1;
        let native_ids: Vec<String> = native_hits
            .iter()
            .filter_map(|h| wands_id_by_product_id.get(&h.product).cloned())
            .collect();
        let (native_ndcg, native_recall, native_mrr) =
            ndcg_recall_mrr(&native_ids, query_judged, K);

        let (solr_q, solr_fq, translation_failures) = wands_solr_query_for(
            &q.text,
            &compiled.residual_lexical,
            &compiled.constraints,
            &category_name_by_id,
            &product_type_name_by_id,
        );
        if !translation_failures.is_empty() {
            solr_failures.push(format!(
                "query={:?} fq_translation_failed={translation_failures:?}",
                q.text
            ));
            continue;
        }
        let t1 = std::time::Instant::now();
        let solr_result = solr_search(
            &solr_base_url,
            "title description",
            &solr_q,
            &solr_fq,
            K,
            std::time::Duration::from_secs(30),
        );
        let solr_latency_ms = t1.elapsed().as_secs_f64() * 1000.0;
        // A real, legitimate zero-result Solr answer is scored 0.0 here --
        // that is a true relevance measurement. A transport/query/parse
        // failure is a different case entirely and is excluded above (via
        // `continue`) rather than reaching this match at all.
        let (solr_ids, solr_ndcg, solr_recall, solr_mrr) = match &solr_result {
            comparator_eval::outcome::EngineLookup::Success(ids) => {
                let (ndcg, recall, mrr) = ndcg_recall_mrr(ids, query_judged, K);
                (ids.clone(), ndcg, recall, mrr)
            }
            other => {
                solr_failures.push(format!(
                    "query={:?} solr_lookup_failure={:?}",
                    q.text,
                    other.failure_description()
                ));
                continue;
            }
        };

        if class == QueryClass9::VariantScoped && variant_scoped_examples.len() < 3 {
            variant_scoped_examples.push((q.text.clone(), native_ids.clone(), solr_ids.clone()));
        }

        let point = ResultPoint {
            native_ndcg,
            native_recall,
            native_mrr,
            native_latency_ms,
            solr_ndcg,
            solr_recall,
            solr_mrr,
            solr_latency_ms,
        };
        push_result(&mut results, class, &point);
        let routing_key = if matches!(outcome_label, "FastPath" | "Hybrid") {
            "structural_routed (FastPath+Hybrid)"
        } else {
            "punt_routed (delegate-only, no structural constraint)"
        };
        push_result(&mut by_routing, routing_key, &point);
        // Issue #55 whole-workload diagnostic: `execute_ranked` (the code
        // path both Issue #55 fixes touch) is only ever called from the
        // FastPath branch of execute_planned, never from Hybrid (Hybrid
        // uses bitmap narrowing + delegate + verify_and_truncate instead).
        // Split structural_routed further so a FastPath-only vs
        // Hybrid-only comparison can show whether the isolated ranking-only
        // gain is being diluted by Hybrid traffic that never touches it.
        if matches!(outcome_label, "FastPath" | "Hybrid") {
            push_result(&mut by_routing, outcome_label, &point);
        }
        evaluated += 1;
    }

    if !solr_failures.is_empty() {
        eprintln!(
            "\n=== SOLR COMPARATOR FAILURE: {} of {} attempted queries got no legitimate, \
             symmetric Solr answer -- excluded from every aggregate above, NOT scored as Solr \
             NDCG=0.0 (docs/decisions/ISSUE55_COMPARATOR_CENTRALIZATION_DECISION.md) ===",
            solr_failures.len(),
            evaluated + solr_failures.len()
        );
        for failure in &solr_failures {
            eprintln!("  {failure}");
        }
        eprintln!(
            "This Solr core is same-host and locally controlled, so any transport/query/parse \
             failure indicates a real infrastructure or comparator-construction defect, not \
             expected flakiness. Fix it and rerun -- the numbers below are NOT a certified \
             comparison."
        );
        std::process::exit(1);
    }

    println!("evaluated {evaluated} queries ({skipped_no_judgments} skipped, no judgments found)");
    println!("routing distribution: {outcome_counts:?}");
    println!(
        "sample fully-unresolved queries (zero constraints, {} of {evaluated} total): {zero_constraint_examples:?}",
        zero_constraint_examples.len()
    );
    println!();
    println!("=== variant_scoped_structural sample queries (its real, disclosed native loss vs Solr) ===");
    for (text, native_ids, solr_ids) in &variant_scoped_examples {
        let query_id = queries
            .iter()
            .find(|q| &q.text == text)
            .map(|q| q.query_id.clone())
            .unwrap_or_default();
        let query_judged = judged.get(&query_id);
        let label_of = |id: &str| -> String {
            query_judged
                .and_then(|j| j.get(id))
                .map(|l| format!("{l:?}"))
                .unwrap_or_else(|| "unjudged".to_string())
        };
        println!("query: {text:?}");
        println!(
            "  native top-3: {:?}",
            native_ids
                .iter()
                .take(3)
                .map(|id| format!("{id}({})", label_of(id)))
                .collect::<Vec<_>>()
        );
        println!(
            "  solr   top-3: {:?}",
            solr_ids
                .iter()
                .take(3)
                .map(|id| format!("{id}({})", label_of(id)))
                .collect::<Vec<_>>()
        );
    }
    println!();
    println!(
        "{:<38} {:>5} {:>8} {:>8} {:>7} {:>8} {:>7} {:>8} {:>8}",
        "class",
        "n",
        "nat_ndcg",
        "solr_ndcg",
        "nat_rec",
        "solr_rec",
        "nat_mrr",
        "nat_ms",
        "solr_ms"
    );

    let mut total_native_ndcg = 0.0;
    let mut total_solr_ndcg = 0.0;
    let mut total_native_ms = 0.0;
    let mut total_solr_ms = 0.0;

    for (class, r) in &results {
        let weight = r.n as f64 / evaluated as f64;
        let native_ndcg_mean = mean(&r.native_ndcg);
        let solr_ndcg_mean = mean(&r.solr_ndcg);
        let native_ms_mean = mean(&r.native_latency_ms);
        let solr_ms_mean = mean(&r.solr_latency_ms);
        total_native_ndcg += weight * native_ndcg_mean;
        total_solr_ndcg += weight * solr_ndcg_mean;
        total_native_ms += weight * native_ms_mean;
        total_solr_ms += weight * solr_ms_mean;
        println!(
            "{:<38} {:>5} {:>8.4} {:>8.4} {:>7.4} {:>8.4} {:>7.4} {:>8.4} {:>8.4}",
            class.label(),
            r.n,
            native_ndcg_mean,
            solr_ndcg_mean,
            mean(&r.native_recall),
            mean(&r.solr_recall),
            mean(&r.native_mrr),
            native_ms_mean,
            solr_ms_mean,
        );
    }

    println!();
    println!(
        "=== traffic-weighted totals (weighted by real class frequency in this query mix) ==="
    );
    println!("native NDCG@10: {total_native_ndcg:.4}  |  Solr NDCG@10: {total_solr_ndcg:.4}");
    let relative_ndcg_gap = if total_solr_ndcg > 0.0 {
        (total_native_ndcg - total_solr_ndcg) / total_solr_ndcg * 100.0
    } else {
        0.0
    };
    println!("relative NDCG@10 gap (native vs solr): {relative_ndcg_gap:+.2}%");
    println!(
        "native mean latency: {total_native_ms:.4}ms  |  Solr mean latency: {total_solr_ms:.4}ms"
    );
    let latency_ratio = if total_native_ms > 0.0 {
        total_solr_ms / total_native_ms
    } else {
        0.0
    };
    println!("mean latency ratio (solr / native): {latency_ratio:.2}x");

    // Critical scoping split: `Punt`-routed queries (no structural
    // constraint at all) never touch commerce_core's structural index --
    // the delegate is called with restrict_to=None, so "native" there is
    // really just embedded Tantivy's own plain-text relevance versus
    // remote Solr's edismax relevance on the same title/description text.
    // That is a real, useful data point (an engine-choice question), but
    // it is NOT a test of the commerce-native structural-retrieval thesis
    // Issue #34 asks about -- only the structural_routed (FastPath+Hybrid)
    // slice is. Printed separately so the headline above cannot be
    // mistaken for "commerce-native structural retrieval beats Solr" when
    // the traffic-weighted majority of it (Punt) is a different claim.
    println!();
    println!("=== routing-split breakdown (the scoping caveat above, made explicit) ===");
    println!(
        "{:<55} {:>5} {:>9} {:>9} {:>8} {:>8}",
        "routing", "n", "nat_ndcg", "solr_ndcg", "nat_ms", "solr_ms"
    );
    for (key, r) in &by_routing {
        println!(
            "{:<55} {:>5} {:>9.4} {:>9.4} {:>8.4} {:>8.4}",
            key,
            r.n,
            mean(&r.native_ndcg),
            mean(&r.solr_ndcg),
            mean(&r.native_latency_ms),
            mean(&r.solr_latency_ms),
        );
    }

    println!();
    println!("=== overall traffic-weighted verdict (context only -- see the structural-routed-only verdict below for the actual Issue #34 decision) ===");
    if relative_ndcg_gap >= -10.0 && latency_ratio >= 2.0 {
        println!(
            "relevance within 10% of Solr AND >=2x latency advantage, traffic-weighted overall."
        );
    } else if relative_ndcg_gap >= -10.0 || latency_ratio >= 2.0 {
        println!(
            "one axis (relevance or latency) clears the bar, not both, traffic-weighted overall."
        );
    } else {
        println!("neither bar cleared, traffic-weighted overall.");
    }

    // The decision-relevant comparison: Issue #34/Phase 9 asks whether
    // commerce-native STRUCTURAL execution (FastPath/Hybrid) is real --
    // the Punt-routed majority of traffic above is a different question
    // (embedded Tantivy vs remote Solr on plain text) and must not decide
    // this verdict, even though it dominates the traffic-weighted total.
    println!();
    println!("=== VERDICT (structural_routed traffic only -- FastPath+Hybrid, the actual Issue #34 question) ===");
    if let Some(structural) = by_routing.get("structural_routed (FastPath+Hybrid)") {
        let nat = mean(&structural.native_ndcg);
        let solr = mean(&structural.solr_ndcg);
        let nat_ms = mean(&structural.native_latency_ms);
        let solr_ms = mean(&structural.solr_latency_ms);
        let gap = if solr > 0.0 {
            (nat - solr) / solr * 100.0
        } else {
            0.0
        };
        let ratio = if nat_ms > 0.0 { solr_ms / nat_ms } else { 0.0 };
        println!(
            "structural_routed: n={}, native NDCG@10={nat:.4}, solr NDCG@10={solr:.4}, relative gap={gap:+.2}%, latency ratio={ratio:.2}x",
            structural.n
        );
        if gap >= -10.0 && ratio >= 2.0 {
            println!("=== KEEP/PROCEED-leaning: relevance within 10% of Solr AND >=2x latency advantage, on structural-routed traffic specifically ===");
        } else if gap >= -10.0 || ratio >= 2.0 {
            println!("=== REVISE: one axis clears the bar, not both, on structural-routed traffic specifically ===");
        } else {
            println!("=== STOP-leaning (negative result preserved): structural-routed relevance still trails Solr materially even with both disclosed defects fixed -- Phase 2's STOP replicates on WANDS's genuine multi-entity structural data, not just ESCI's single-entity (Brand) catalog ===");
        }
    }
}
