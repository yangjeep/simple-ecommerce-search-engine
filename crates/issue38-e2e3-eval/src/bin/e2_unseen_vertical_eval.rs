//! Issue #38 E2: full-pipeline evaluation of a genuinely unseen commerce
//! vertical (synthetic automotive parts, `issue38_e2e3_eval::automotive`)
//! against the real, production `commerce_core` pipeline (`ir::compile`
//! -> `plan::plan` -> `plan::execute_planned`, the identical path E1
//! validated the physical layer of), not a reimplementation of it.
//!
//! **Question**: does the architecture established against WANDS (home
//! furnishings) generalize to a catalog with a materially different
//! attribute schema (`thread_size`, `voltage`, `micron_rating`, ...) and,
//! for the first time, a genuinely new structural relationship pattern
//! (`compatible_fitment`, a many-to-many `MultiEnum` compatibility set,
//! queried via the pre-existing `Constraint::MultiEnumContains` --
//! deliberately not a new mechanism, per `lib.rs`'s doc comment)?
//!
//! **Measured, per Issue #38's own ask**: coverage (fraction of queries
//! returning >=1 hit), correctness (NDCG@10/Recall@10/MRR against
//! ground truth derived directly from the generator's own parameters),
//! candidate-set size, latency, fallback rate (`Punt` share), and failure
//! taxonomy (`issue38_e2e3_eval::failure_taxonomy`, the same
//! classification E3's binary reuses so both reports use one vocabulary).
//!
//! **Decision framing**: this is a generalization/coverage measurement,
//! not a falsification gate the way E1 was -- there is no single pass/fail
//! threshold declared up front, since Issue #38 asks E2 to characterize
//! *how* the architecture behaves on unseen structure, not to clear a bar.
//! A materially low fitment-query NDCG or a high Punt share on
//! fitment-bearing queries would itself be the finding.
//!
//! No `commerce_core` production code is touched or reimplemented here:
//! `execute_planned` is called directly, not re-derived branch-by-branch.

use std::collections::{BTreeMap, HashMap};

use bench_harness::{Distribution, RunManifest};
use commerce_core::cold_start::{compile_lexicon, CatalogProfile};
use commerce_core::domain::VariantId;
use commerce_core::index::CatalogIndex;
use commerce_core::ir::compile as compile_query;
use commerce_core::plan::{
    execute_planned, plan, ExecutionOutcome, LexicalDelegate, PlannerPolicy,
};
use issue38_e2e3_eval::{automotive, failure_taxonomy, ground_truth, ingest};
use phase9_eval::bitmap_delegate::{build_index, BitmapTantivyDelegate};
use phase9_eval::wands_relevance::ndcg_recall_mrr;

const N_PRODUCTS: usize = 3000;
const K: usize = 10;
const MIN_ENUM_FREQUENCY: usize = 1;

fn main() {
    println!(
        "=== Issue #38 E2: unseen-vertical (synthetic automotive parts) full-pipeline eval ==="
    );

    let products = automotive::generate_catalog(N_PRODUCTS);
    let (queries, judgments) = automotive::generate_workload(&products);
    ground_truth::assert_self_consistent(&products, &judgments);

    let ingested = ingest::build_catalog(&products);
    let catalog = &ingested.catalog;
    let index = CatalogIndex::build(catalog);

    let profile = CatalogProfile::build(
        catalog,
        &ingested.brands,
        &ingested.product_types,
        &ingested.categories,
    );
    let lexicon = compile_lexicon(&profile, MIN_ENUM_FREQUENCY);

    let built = build_index(catalog).expect("in-memory tantivy index build");
    let delegate = BitmapTantivyDelegate::new(
        &built.index,
        vec![built.title_field, built.description_field],
    )
    .expect("tantivy delegate build");

    // Reverse lookup: `execute_planned` returns `commerce_core` internal
    // `VariantId`s; ground truth is keyed by this crate's own
    // `SynthVariant::external_id` strings (the same id space
    // `ndcg_recall_mrr` expects, matching WANDS's own product-id-keyed
    // judgment convention).
    let variant_external_id: HashMap<VariantId, String> = ingested
        .external_variant_id
        .iter()
        .map(|(ext, &(_, vid))| (vid, ext.clone()))
        .collect();

    let policy = PlannerPolicy {
        selectivity_threshold: 0.05,
        delegate_oversample: 20,
    };

    // No real dataset file exists for a synthetic, seed-generated catalog
    // -- the generator source itself *is* the versioned dataset, so it is
    // what `RunManifest` fingerprints here, a deliberate, disclosed
    // deviation from the file-fingerprint convention real-data
    // experiments (E1, Phase 9) use.
    let manifest = RunManifest::capture(
        "I38-E2",
        "native_compiled_pipeline",
        std::path::Path::new("crates/issue38-e2e3-eval/src/automotive.rs"),
        std::path::Path::new("crates/issue38-e2e3-eval/src/automotive.rs"),
        serde_json::json!({
            "n_products": N_PRODUCTS,
            "min_enum_frequency": MIN_ENUM_FREQUENCY,
            "selectivity_threshold": policy.selectivity_threshold,
            "delegate_oversample": policy.delegate_oversample,
            "k": K,
        }),
        automotive::SEED,
        1,
        1,
    );
    manifest.print();
    println!(
        "catalog: {} products, {} distinct product types",
        catalog.products.len(),
        ingested.product_types.len()
    );
    println!("workload: {} queries", queries.len());

    // Warmup pass over the whole workload (discarded) -- P2-E16/P9-E02/
    // P9-E04 discipline: an unwarmed first-touch measurement (Tantivy
    // reader/segment caching, allocator warmup) is not a fair steady-state
    // latency sample.
    for q in &queries {
        let compiled = compile_query(&q.text, &lexicon);
        let _ = execute_planned(
            &compiled,
            catalog,
            &index,
            Some(&delegate as &dyn LexicalDelegate),
            K,
            &policy,
        );
    }

    let mut outcome_counts: BTreeMap<&'static str, usize> = BTreeMap::new();
    let mut failure_counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut candidate_sizes: Vec<f64> = Vec::new();
    let mut latencies_ms: Vec<f64> = Vec::new();
    let mut ndcgs: Vec<f64> = Vec::new();
    let mut recalls: Vec<f64> = Vec::new();
    let mut mrrs: Vec<f64> = Vec::new();
    let mut queries_with_any_hit = 0usize;
    let mut per_template_ndcg: BTreeMap<&'static str, Vec<f64>> = BTreeMap::new();
    let mut fitment_ndcgs: Vec<f64> = Vec::new();
    // "exact_lookup" (part-number search) is tracked separately from the
    // aggregate below: `part_number` is a *variant*-level `Text`
    // attribute, but the reused `phase9_eval::bitmap_delegate::build_index`
    // only indexes *product*-level `Text` attributes into its Tantivy
    // fields (its own doc comment states this scope explicitly -- not a
    // new defect found here). `commerce_core::plan::execute_planned`'s
    // `Hybrid`/`Punt` outcomes rank purely via the external
    // `LexicalDelegate`, never falling back to `CatalogIndex`'s own
    // internal `lexical_postings` (which *does* include variant text, via
    // `effective_attributes`, but is not wired into `plan`/`execute_planned`
    // at all -- confirmed by direct read of `plan/mod.rs`). So this
    // template's near-zero NDCG is a real, disclosed finding about this
    // reused evaluation delegate's product-vs-variant text scope
    // (directly the kind of "schema-management assumption" this
    // experiment was asked to surface), not evidence against the
    // fitment/schema-generalization question E2 exists to answer --
    // reported separately so it cannot silently dominate the aggregate
    // (31/57 = 54% of this workload).
    let mut ndcgs_excluding_exact_lookup: Vec<f64> = Vec::new();

    for q in &queries {
        let Some(judged) = judgments.get(&q.id) else {
            continue;
        };
        let compiled = compile_query(&q.text, &lexicon);

        let planned_only = plan(&compiled, &index, catalog.products.len(), &policy);
        let outcome_label = match planned_only.outcome {
            ExecutionOutcome::FastPath => "fast_path",
            ExecutionOutcome::Hybrid => "hybrid",
            ExecutionOutcome::Punt => "punt",
        };
        *outcome_counts.entry(outcome_label).or_insert(0) += 1;

        let class = failure_taxonomy::classify(&compiled, planned_only.outcome);
        *failure_counts.entry(class.to_string()).or_insert(0) += 1;

        let candidates = index.indexed_candidates(&compiled.constraints);
        candidate_sizes.push(candidates.len() as f64);

        let t0 = std::time::Instant::now();
        let (_, hits) = execute_planned(
            &compiled,
            catalog,
            &index,
            Some(&delegate as &dyn LexicalDelegate),
            K,
            &policy,
        );
        latencies_ms.push(t0.elapsed().as_secs_f64() * 1000.0);

        if !hits.is_empty() {
            queries_with_any_hit += 1;
        }

        let hit_external_ids: Vec<String> = hits
            .iter()
            .filter_map(|h| variant_external_id.get(&h.variant).cloned())
            .collect();
        let (ndcg, recall, mrr) = ndcg_recall_mrr(&hit_external_ids, judged, K);
        ndcgs.push(ndcg);
        recalls.push(recall);
        mrrs.push(mrr);
        per_template_ndcg.entry(q.template).or_default().push(ndcg);
        if q.template == "fitment_exact" {
            fitment_ndcgs.push(ndcg);
        }
        if q.template != "exact_lookup" {
            ndcgs_excluding_exact_lookup.push(ndcg);
        }
    }

    let total = ndcgs.len();
    println!("\n--- coverage / routing ---");
    println!("queries evaluated: {total}");
    println!(
        "queries with >=1 hit returned: {queries_with_any_hit} ({:.1}%)",
        100.0 * queries_with_any_hit as f64 / total as f64
    );
    for (outcome, count) in &outcome_counts {
        println!(
            "  outcome {outcome}: {count} ({:.1}%)",
            100.0 * *count as f64 / total as f64
        );
    }
    let punt_count = *outcome_counts.get("punt").unwrap_or(&0);
    println!(
        "fallback rate (Punt share of all queries): {:.1}%",
        100.0 * punt_count as f64 / total as f64
    );

    println!("\n--- failure taxonomy ---");
    for (class, count) in &failure_counts {
        println!(
            "  {class}: {count} ({:.1}%)",
            100.0 * *count as f64 / total as f64
        );
    }

    println!("\n--- correctness ---");
    Distribution::compute(&ndcgs).print(&format!("NDCG@{K} (all templates)"), "");
    Distribution::compute(&recalls).print(&format!("Recall@{K}"), "");
    Distribution::compute(&mrrs).print("MRR", "");
    if !ndcgs_excluding_exact_lookup.is_empty() {
        Distribution::compute(&ndcgs_excluding_exact_lookup).print(
            &format!("NDCG@{K} (excluding exact_lookup, see note below)"),
            "",
        );
    }
    println!(
        "  NOTE: exact_lookup (variant-level part-number search) scores near-zero NDCG because \
         the reused BitmapTantivyDelegate only indexes product-level Text attributes -- a \
         disclosed evaluation-delegate scope finding, not a compile()/plan() defect. See this \
         binary's own source comment above `ndcgs_excluding_exact_lookup`."
    );

    println!("\n--- per-template NDCG@{K} (mean) ---");
    for (template, values) in &per_template_ndcg {
        let d = Distribution::compute(values);
        println!(
            "  {template}: n={} mean={:.4} p50={:.4}",
            d.n, d.mean, d.p50
        );
    }

    if !fitment_ndcgs.is_empty() {
        println!("\n--- fitment (genuinely new structural relationship) ---");
        Distribution::compute(&fitment_ndcgs).print(&format!("fitment_exact NDCG@{K}"), "");
    } else {
        println!("\n--- fitment: NO fitment_exact queries evaluated (unexpected) ---");
    }

    println!("\n--- candidate-set size (index.indexed_candidates) ---");
    Distribution::compute(&candidate_sizes).print("candidates", " docs");

    println!("\n--- latency (execute_planned, end-to-end, single-call per query) ---");
    Distribution::compute(&latencies_ms).print("execute_planned", "ms");

    let output_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "docs/research/artifacts/i38_e2_run1/summary.json".to_string());
    let summary = serde_json::json!({
        "experiment": "I38-E2",
        "n_products": N_PRODUCTS,
        "n_queries": total,
        "outcome_counts": outcome_counts,
        "failure_taxonomy": failure_counts,
        "fallback_rate": punt_count as f64 / total as f64,
        "queries_with_any_hit_fraction": queries_with_any_hit as f64 / total as f64,
        "ndcg_mean": Distribution::compute(&ndcgs).mean,
        "ndcg_mean_excluding_exact_lookup": if ndcgs_excluding_exact_lookup.is_empty() { None } else { Some(Distribution::compute(&ndcgs_excluding_exact_lookup).mean) },
        "recall_mean": Distribution::compute(&recalls).mean,
        "mrr_mean": Distribution::compute(&mrrs).mean,
        "fitment_exact_ndcg_mean": if fitment_ndcgs.is_empty() { None } else { Some(Distribution::compute(&fitment_ndcgs).mean) },
        "candidate_size_p50": Distribution::compute(&candidate_sizes).p50,
        "latency_ms_p50": Distribution::compute(&latencies_ms).p50,
        "latency_ms_p95": Distribution::compute(&latencies_ms).p95,
    });
    if let Some(parent) = std::path::Path::new(&output_path).parent() {
        std::fs::create_dir_all(parent).expect("create artifacts dir");
    }
    std::fs::write(
        &output_path,
        serde_json::to_string_pretty(&summary).unwrap(),
    )
    .expect("write summary json");
    println!("\nsummary written to {output_path}");

    println!("\n=== I38-E2 complete ===");
}
