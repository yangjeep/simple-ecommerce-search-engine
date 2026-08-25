//! Issue #55 follow-up: while investigating the empty-residual regression
//! (checkpoints 6/7), the two real WANDS queries responsible
//! ("driftwood mirror", "marble") turned out to compile with empty
//! `residual_lexical` *and* empty `preferences` -- but non-empty
//! `ambiguous`: every token resolves to 2-3 genuinely ambiguous attribute
//! readings (e.g. "marble" could be `color=marble`, `material=marble`,
//! or `primarymaterial=marble`). `ir::query::compile` correctly preserves
//! this ambiguity (`crates/commerce-core/src/ir/query.rs:256-261`), but
//! `plan()` (`crates/commerce-core/src/plan/mod.rs:166-199`) never reads
//! `query.ambiguous` -- its FastPath/Hybrid/Punt decision is a function
//! of `residual_lexical`/`constraints` only. A query whose ambiguity was
//! carefully preserved by the compiler is routed identically to a query
//! with genuinely zero signal.
//!
//! This binary classifies every real WANDS query by whether it matches
//! that pattern (`ambiguous` non-empty AND `residual_lexical` empty AND
//! `preferences` empty), then reports native vs. Solr NDCG@10 restricted
//! to that subpopulation -- see
//! `docs/experiments/ISSUE55_AMBIGUOUS_ROUTING_PROTOCOL.md` for the
//! preregistered gates.
use std::collections::{BTreeMap, HashMap};

use commerce_core::cold_start::{compile_lexicon, CatalogProfile};
use commerce_core::domain::{CategoryId, Constraint, ProductId, ProductTypeId};
use commerce_core::index::CatalogIndex;
use commerce_core::ir::{compile, ResolvedConstraint, StructuralConstraint};
use commerce_core::plan::{execute_planned, ExecutionOutcome, PlannerPolicy};
use phase9_eval::bitmap_delegate::{build_index, BitmapTantivyDelegate, BuiltIndex};
use phase9_eval::wands_relevance::{ndcg_recall_mrr, WandsLabel};
use round1_eval::solr::{case_insensitive_field_regex, solr_search};

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

/// Identical to `p9_e02_wands_physical_advantage`'s own
/// `wands_solr_query_for` -- kept as its own copy (matching this
/// project's own eval-binary convention of light duplication over a
/// shared crate for one-off diagnostics) so this experiment's Solr query
/// construction is provably the same one every other Issue #55/#34
/// checkpoint's numbers were measured with.
fn wands_solr_query_for(
    query_text: &str,
    residual_lexical: &[String],
    constraints: &[ResolvedConstraint],
    category_name_by_id: &HashMap<CategoryId, String>,
    product_type_name_by_id: &HashMap<ProductTypeId, String>,
) -> (String, Vec<String>) {
    let mut fq = Vec::new();
    for c in constraints {
        match c {
            ResolvedConstraint::Structural(StructuralConstraint::Category(id)) => {
                if let Some(name) = category_name_by_id.get(id) {
                    fq.push(format!(
                        "category_leaf:/{}/",
                        case_insensitive_field_regex(name)
                    ));
                }
            }
            ResolvedConstraint::Structural(StructuralConstraint::ProductType(id)) => {
                if let Some(name) = product_type_name_by_id.get(id) {
                    fq.push(format!(
                        "product_class:/{}/",
                        case_insensitive_field_regex(name)
                    ));
                }
            }
            ResolvedConstraint::Attribute(Constraint::Enum { attribute, value }) => {
                fq.push(format!(
                    "{attribute}:/{}/",
                    case_insensitive_field_regex(value)
                ));
            }
            _ => {}
        }
    }
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
    (q, fq)
}

fn mean(v: &[f64]) -> f64 {
    if v.is_empty() {
        0.0
    } else {
        v.iter().sum::<f64>() / v.len() as f64
    }
}

fn main() {
    println!("=== P9-E07: ambiguous-but-residual-empty routing diagnostic ===");

    let solr_base_url = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "http://localhost:8983/solr/wands_bench".to_string());

    println!("loading WANDS catalog...");
    let products = phase6a_eval::data::load_catalog(std::path::Path::new(CATALOG_PATH));
    let ingested = phase6a_eval::catalog::build_catalog(&products);
    println!("catalog: {} products", ingested.catalog.products.len());

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
        &[],
        &ingested.product_types,
        &ingested.categories,
    );
    let lexicon = compile_lexicon(&profile, MIN_ENUM_FREQUENCY);

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

    let mut matched_queries: Vec<String> = Vec::new();
    let mut matched_ambiguous_texts: Vec<Vec<String>> = Vec::new();
    let mut matched_candidate_counts: Vec<u64> = Vec::new();
    let mut matched_routing: BTreeMap<&'static str, usize> = BTreeMap::new();
    let mut native_ndcg = Vec::new();
    let mut solr_ndcg = Vec::new();
    let mut skipped_no_judgments = 0usize;

    // Control group (added after adversarial review found the initial
    // n=4 comparison confounds two already-known full-catalog-arbitrary-
    // order queries with two partially-constrained ones, and never
    // isolated "ambiguous discarded" from the pre-existing, already-
    // documented "FastPath zero-signal returns arbitrary order" defect):
    // every real query that is ALSO fully zero-signal for FastPath
    // (residual_lexical/preferences empty) but WITHOUT any ambiguity
    // (`ambiguous` empty too) -- e.g. "dinosaur", "sofa with ottoman".
    // If this control group's own native-vs-Solr gap is comparably bad,
    // the effect is the generic zero-signal-FastPath pathology, not
    // something specifically attributable to discarded `ambiguous`.
    let mut control_queries: Vec<String> = Vec::new();
    let mut control_candidate_counts: Vec<u64> = Vec::new();
    let mut control_native_ndcg = Vec::new();
    let mut control_solr_ndcg = Vec::new();

    // Exploratory only (not preregistered, not a KEEP/REJECT input): the
    // broader population of every real query whose `ambiguous` is
    // non-empty, regardless of whether `residual_lexical`/`preferences`
    // are also empty -- checked cheaply, in the same run, purely to see
    // whether the strict pattern's effect size generalizes to a larger
    // sample before deciding whether a follow-up experiment is warranted.
    let mut broad_native_ndcg = Vec::new();
    let mut broad_solr_ndcg = Vec::new();
    let mut broad_n = 0usize;

    // Also track the same metrics for every OTHER real query (not
    // matching the pattern) as a same-run, same-methodology comparison
    // baseline -- so the matched population's numbers are read against
    // this run's own rest-of-corpus average, not a number from a
    // different checkpoint's run.
    let mut rest_native_ndcg = Vec::new();
    let mut rest_solr_ndcg = Vec::new();

    for q in &queries {
        let Some(query_judged) = judged.get(&q.query_id) else {
            skipped_no_judgments += 1;
            continue;
        };
        let compiled = compile(&q.text, &lexicon);
        let zero_signal_for_fastpath =
            compiled.residual_lexical.is_empty() && compiled.preferences.is_empty();
        let is_pattern_match = !compiled.ambiguous.is_empty() && zero_signal_for_fastpath;
        let is_control = compiled.ambiguous.is_empty()
            && compiled.constraints.is_empty()
            && zero_signal_for_fastpath;
        let candidate_count = index.indexed_candidates(&compiled.constraints).len();

        let (planned, native_hits) = execute_planned(
            &compiled,
            &ingested.catalog,
            &index,
            Some(&delegate),
            K,
            &policy,
            None,
        );
        let native_ids: Vec<String> = native_hits
            .iter()
            .filter_map(|h| wands_id_by_product_id.get(&h.product).cloned())
            .collect();
        let (n_ndcg, _, _) = ndcg_recall_mrr(&native_ids, query_judged, K);

        let (solr_q, solr_fq) = wands_solr_query_for(
            &q.text,
            &compiled.residual_lexical,
            &compiled.constraints,
            &category_name_by_id,
            &product_type_name_by_id,
        );
        let solr_result = solr_search(&solr_base_url, &solr_q, &solr_fq, K);
        let solr_ids: Vec<String> = solr_result.map(|r| r.ids).unwrap_or_default();
        let (s_ndcg, _, _) = ndcg_recall_mrr(&solr_ids, query_judged, K);

        if is_pattern_match {
            matched_queries.push(q.text.clone());
            matched_ambiguous_texts
                .push(compiled.ambiguous.iter().map(|a| a.text.clone()).collect());
            matched_candidate_counts.push(candidate_count);
            let outcome_label = match planned.outcome {
                ExecutionOutcome::FastPath => "FastPath",
                ExecutionOutcome::Hybrid => "Hybrid",
                ExecutionOutcome::Punt => "Punt",
            };
            *matched_routing.entry(outcome_label).or_insert(0) += 1;
            native_ndcg.push(n_ndcg);
            solr_ndcg.push(s_ndcg);
        } else {
            rest_native_ndcg.push(n_ndcg);
            rest_solr_ndcg.push(s_ndcg);
        }

        if is_control {
            control_queries.push(q.text.clone());
            control_candidate_counts.push(candidate_count);
            control_native_ndcg.push(n_ndcg);
            control_solr_ndcg.push(s_ndcg);
        }

        if !compiled.ambiguous.is_empty() {
            broad_n += 1;
            broad_native_ndcg.push(n_ndcg);
            broad_solr_ndcg.push(s_ndcg);
        }
    }

    println!(
        "evaluated {} queries ({skipped_no_judgments} skipped, no judgments found)",
        native_ndcg.len() + rest_native_ndcg.len()
    );
    println!();
    println!("=== pattern population: ambiguous non-empty AND residual_lexical empty AND preferences empty ===");
    println!("matched queries: n={}", matched_queries.len());
    for i in 0..matched_queries.len() {
        println!(
            "  {:?} -- ambiguous tokens: {:?} candidates={} native_ndcg={:.4} solr_ndcg={:.4}",
            matched_queries[i],
            matched_ambiguous_texts[i],
            matched_candidate_counts[i],
            native_ndcg[i],
            solr_ndcg[i]
        );
    }
    println!("routing distribution within matched population: {matched_routing:?}");
    println!();
    println!(
        "=== control group: zero-signal-for-FastPath but NOT ambiguous (residual_lexical/preferences empty, ambiguous empty) ==="
    );
    println!("control queries: n={}", control_queries.len());
    for i in 0..control_queries.len() {
        println!(
            "  {:?} candidates={} native_ndcg={:.4} solr_ndcg={:.4}",
            control_queries[i],
            control_candidate_counts[i],
            control_native_ndcg[i],
            control_solr_ndcg[i]
        );
    }
    let control_nat = mean(&control_native_ndcg);
    let control_solr = mean(&control_solr_ndcg);
    let control_gap = if control_solr > 0.0 {
        (control_nat - control_solr) / control_solr * 100.0
    } else {
        0.0
    };
    println!(
        "control population: native NDCG@10={control_nat:.4}  solr NDCG@10={control_solr:.4}  relative gap={control_gap:+.2}%"
    );
    println!();
    println!(
        "matched population: native NDCG@10={:.4}  solr NDCG@10={:.4}",
        mean(&native_ndcg),
        mean(&solr_ndcg)
    );
    println!(
        "rest of corpus (same run, same methodology): native NDCG@10={:.4}  solr NDCG@10={:.4}",
        mean(&rest_native_ndcg),
        mean(&rest_solr_ndcg)
    );

    let nat = mean(&native_ndcg);
    let solr = mean(&solr_ndcg);
    let gap = if solr > 0.0 {
        (nat - solr) / solr * 100.0
    } else {
        0.0
    };
    println!();
    println!("relative NDCG gap (native vs solr, matched population): {gap:+.2}%");
    if matched_queries.len() < 5 {
        println!(
            "=== FALSIFIED/low-priority (preregistered gate): population too small to generalize from (n={}) ===",
            matched_queries.len()
        );
    } else if gap <= -10.0 {
        println!("=== CONFIRMED: real, material relevance defect on this pattern ===");
    } else {
        println!(
            "=== FALSIFIED: pattern is real but native is not materially worse than Solr here ==="
        );
    }

    println!();
    println!("=== exploratory only, not preregistered (broader population: ambiguous non-empty, any residual/preferences state) ===");
    let broad_nat = mean(&broad_native_ndcg);
    let broad_solr = mean(&broad_solr_ndcg);
    let broad_gap = if broad_solr > 0.0 {
        (broad_nat - broad_solr) / broad_solr * 100.0
    } else {
        0.0
    };
    println!(
        "n={broad_n}: native NDCG@10={broad_nat:.4}  solr NDCG@10={broad_solr:.4}  relative gap={broad_gap:+.2}%"
    );
}
