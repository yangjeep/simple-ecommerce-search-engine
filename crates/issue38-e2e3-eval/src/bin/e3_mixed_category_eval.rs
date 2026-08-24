//! Issue #38 E3: full-pipeline evaluation of a realistic mixed-category
//! merchant catalog (furniture + apparel + automotive in one catalog,
//! `issue38_e2e3_eval::mixed_merchant`) against the real, production
//! `commerce_core` pipeline, plus a direct schema-management diagnostic:
//! does catalog-agnostic ingestion (`CatalogIndex::build`,
//! `cold_start::CatalogProfile::build`/`compile_lexicon`) correctly decide
//! which features deserve bitmap/lexicon treatment *without* any
//! search-time category intelligence, when three families with
//! incompatible schemas, shared ambiguous field names, sparse attributes,
//! and noisy titles are ingested as one undifferentiated catalog?
//!
//! **Verified directly against source** (not asserted): `CatalogIndex::build`
//! (`crates/commerce-core/src/index/mod.rs`) bitmap-indexes every
//! `Enum`/`MultiEnum`/`Boolean` attribute value and sorts every `Numeric`
//! value purely by that value's own `AttributeValue` variant tag -- there
//! is no per-family/per-category conditional anywhere in that function.
//! This crate's own `SynthProduct::family` tag is never read by
//! `ingest::build_catalog` or by anything in `commerce_core`, so it cannot
//! leak into a "search-time category intelligence" shortcut (confirmed by
//! grep, not merely by convention). Ingestion's schema decisions are
//! therefore, verifiably, type-tag-driven and catalog-agnostic.
//!
//! **The named recall-gap finding, measured here (not merely read from
//! source)**: apparel's `size` (`AttributeValue::Enum`, e.g. `"34"` for
//! jeans waist) and automotive Wiper Blades' `size`
//! (`AttributeValue::Numeric`) share a field name but never collide at
//! the `CatalogProfile`/`SemanticLexicon` layer at all -- `Numeric` values
//! are profiled into a completely separate `numeric_values` map that
//! `compile_lexicon` never reads, so the Enum-typed lexicon entry for
//! `"34"` (if any) has exactly one source, apparel. The actual conflict is
//! entirely inside `ir::query::compile`'s hard-coded `"size N"` keyword
//! branch, which (a) never consults the lexicon at all for this token
//! pattern, and (b) writes directly into `result.constraints` rather than
//! the `lexicon_attribute_matches` list P9-E05's entity-corroboration
//! demotion rule inspects -- so a bare `"size 34"` query with no
//! accompanying entity phrase is *not* demoted to a preference the way an
//! equivalent bare lexicon-derived attribute match would be; it becomes an
//! unconditional hard `Numeric` filter and, since `residual_lexical` is
//! then empty, routes straight to `FastPath`. `Constraint::matches`'s
//! `_ => false` catch-all still makes this safe (never a false-positive
//! cross-type match), so this remains a disclosed **recall gap**, not a
//! correctness violation -- but it is a sharper mechanism than "the
//! lexicon can't see it": the keyword branch also bypasses the one
//! existing safety net against un-corroborated hard filters.
//!
//! No `commerce_core` production code is touched or reimplemented here.

use std::collections::{BTreeMap, HashMap};

use bench_harness::{Distribution, RunManifest};
use commerce_core::cold_start::{compile_lexicon, CatalogProfile};
use commerce_core::domain::VariantId;
use commerce_core::index::CatalogIndex;
use commerce_core::ir::compile as compile_query;
use commerce_core::plan::{
    execute_planned, plan, ExecutionOutcome, LexicalDelegate, PlannerPolicy,
};
use issue38_e2e3_eval::{failure_taxonomy, ground_truth, ingest, mixed_merchant};
use phase9_eval::bitmap_delegate::{build_index, BitmapTantivyDelegate};
use phase9_eval::wands_relevance::ndcg_recall_mrr;

const N_PER_FAMILY: usize = 1000;
const K: usize = 10;
const MIN_ENUM_FREQUENCY: usize = 1;

fn main() {
    println!("=== Issue #38 E3: mixed-category merchant full-pipeline eval ===");

    let products = mixed_merchant::generate_mixed_catalog(N_PER_FAMILY, N_PER_FAMILY, N_PER_FAMILY);
    let (queries, judgments) = mixed_merchant::generate_cross_category_workload(&products);
    ground_truth::assert_self_consistent(&products, &judgments);

    let family_by_external_id: HashMap<&str, &'static str> = products
        .iter()
        .map(|p| (p.variants[0].external_id.as_str(), p.family))
        .collect();

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

    // --- Schema-management diagnostic ---
    println!("\n--- schema-management diagnostic (catalog-agnostic ingestion) ---");
    println!(
        "product types discovered: {} (no per-family hardcoding -- CatalogProfile::build takes \
         only the already-ingested Catalog + registries, never SynthProduct::family)",
        profile.product_type_names().count()
    );
    println!("brand names discovered: {}", profile.brand_names().count());
    println!(
        "boolean attributes discovered: {}",
        profile.boolean_attributes().count()
    );
    println!(
        "distinct enum values registered in the lexicon: {}",
        profile.distinct_value_count()
    );
    let size_numeric_values = profile.numeric_values("size");
    println!(
        "'size' Numeric values profiled (automotive Wiper Blades only): {} distinct \
         (profiled separately from the Enum lexicon entirely -- CatalogProfile's numeric_values \
         map is never consulted by compile_lexicon)",
        size_numeric_values.len()
    );
    // The queried value comes from `mixed_merchant::size_conflict_anchors`,
    // the SAME derivation the `size_schema_conflict` workload itself uses
    // -- not a hard-coded literal. An earlier version of this diagnostic
    // hard-coded `lexicon.resolve("34")` directly, decoupled from
    // whatever value the workload's own RNG-derived anchor actually was;
    // it happened to agree at this crate's current (SEED, N_PER_FAMILY),
    // but nothing enforced that agreement, and the coincidence was
    // asserted as a measured fact in committed docs before an adversarial
    // review caught it (see `mixed_merchant.rs`'s own doc comment on
    // `size_conflict_anchors`).
    let (jeans_anchor, _wiper_anchor) = mixed_merchant::size_conflict_anchors(&products);
    let size_lexicon_sources: Option<usize> = jeans_anchor.as_deref().map(|anchor| {
        let count = lexicon.resolve(anchor).map(|c| c.len()).unwrap_or(0);
        println!(
            "lexicon.resolve({anchor:?}) candidate count: {count} (this run's actual \
             apparel-jeans size anchor, derived the same way the size_schema_conflict workload \
             derives it -- expected 1 if only apparel jeans registers this Enum value; confirms \
             automotive's Numeric size values never reach the lexicon at all, so there is no \
             'ambiguous candidate list' for the profiler to resolve; the gap is entirely in \
             compile()'s keyword branch, not here)"
        );
        count
    });
    if size_lexicon_sources.is_none() {
        println!(
            "no apparel Jeans product generated in this run -- size-conflict lexicon diagnostic skipped"
        );
    }

    let built = build_index(catalog).expect("in-memory tantivy index build");
    let delegate = BitmapTantivyDelegate::new(
        &built.index,
        vec![built.title_field, built.description_field],
    )
    .expect("tantivy delegate build");

    let variant_external_id: HashMap<VariantId, String> = ingested
        .external_variant_id
        .iter()
        .map(|(ext, &(_, vid))| (vid, ext.clone()))
        .collect();

    let policy = PlannerPolicy {
        selectivity_threshold: 0.05,
        delegate_oversample: 20,
    };

    let manifest = RunManifest::capture(
        "I38-E3",
        "native_compiled_pipeline",
        std::path::Path::new("crates/issue38-e2e3-eval/src/mixed_merchant.rs"),
        std::path::Path::new("crates/issue38-e2e3-eval/src/mixed_merchant.rs"),
        serde_json::json!({
            "n_per_family": N_PER_FAMILY,
            "min_enum_frequency": MIN_ENUM_FREQUENCY,
            "selectivity_threshold": policy.selectivity_threshold,
            "delegate_oversample": policy.delegate_oversample,
            "k": K,
        }),
        mixed_merchant::SEED,
        1,
        1,
    );
    println!();
    manifest.print();
    println!(
        "catalog: {} products ({} furniture + {} apparel + {} automotive)",
        catalog.products.len(),
        N_PER_FAMILY,
        N_PER_FAMILY,
        N_PER_FAMILY
    );
    println!("workload: {} queries", queries.len());

    for q in &queries {
        let compiled = compile_query(&q.text, &lexicon);
        let _ = execute_planned(
            &compiled,
            catalog,
            &index,
            Some(&delegate as &dyn LexicalDelegate),
            K,
            &policy,
            None,
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
    let mut per_template_outcome: BTreeMap<&'static str, BTreeMap<&'static str, usize>> =
        BTreeMap::new();
    // Directly measures the size-schema-conflict recall gap: for each
    // `size_schema_conflict` query, how many of the ground-truth-relevant
    // variants belong to each family, vs. how many of the *returned hits*
    // belong to each family -- a systematic apparel-side shortfall here is
    // the measured (not merely code-read) confirmation of the finding
    // above.
    let mut size_conflict_relevant_by_family: BTreeMap<&'static str, usize> = BTreeMap::new();
    let mut size_conflict_hits_by_family: BTreeMap<&'static str, usize> = BTreeMap::new();

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
        *per_template_outcome
            .entry(q.template)
            .or_default()
            .entry(outcome_label)
            .or_insert(0) += 1;

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
            None,
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

        if q.template == "size_schema_conflict" {
            for ext_id in judged.keys() {
                if let Some(&family) = family_by_external_id.get(ext_id.as_str()) {
                    *size_conflict_relevant_by_family.entry(family).or_insert(0) += 1;
                }
            }
            for ext_id in &hit_external_ids {
                if let Some(&family) = family_by_external_id.get(ext_id.as_str()) {
                    *size_conflict_hits_by_family.entry(family).or_insert(0) += 1;
                }
            }
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
    Distribution::compute(&ndcgs).print(&format!("NDCG@{K}"), "");
    Distribution::compute(&recalls).print(&format!("Recall@{K}"), "");
    Distribution::compute(&mrrs).print("MRR", "");

    println!("\n--- per-template NDCG@{K} (mean) + routing ---");
    for (template, values) in &per_template_ndcg {
        let d = Distribution::compute(values);
        let outcomes = per_template_outcome
            .get(template)
            .cloned()
            .unwrap_or_default();
        println!(
            "  {template}: n={} mean={:.4} p50={:.4} routing={:?}",
            d.n, d.mean, d.p50, outcomes
        );
    }

    println!("\n--- size-schema-conflict recall gap (measured, not just read from source) ---");
    println!("  ground-truth-relevant variants by family: {size_conflict_relevant_by_family:?}");
    println!("  returned hits by family:                  {size_conflict_hits_by_family:?}");
    let apparel_relevant = *size_conflict_relevant_by_family
        .get("apparel")
        .unwrap_or(&0);
    let apparel_hits = *size_conflict_hits_by_family.get("apparel").unwrap_or(&0);
    if apparel_relevant > 0 {
        println!(
            "  apparel-side recall on size_schema_conflict queries: {apparel_hits}/{apparel_relevant} \
             ({:.1}%) -- a low or zero value here, alongside nonzero automotive-side recall, is \
             the direct measured confirmation of the disclosed recall gap.",
            100.0 * apparel_hits as f64 / apparel_relevant as f64
        );
    }

    println!("\n--- candidate-set size (index.indexed_candidates) ---");
    Distribution::compute(&candidate_sizes).print("candidates", " docs");

    println!("\n--- latency (execute_planned, end-to-end, single-call per query) ---");
    Distribution::compute(&latencies_ms).print("execute_planned", "ms");

    let output_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "docs/research/artifacts/i38_e3_run1/summary.json".to_string());
    let summary = serde_json::json!({
        "experiment": "I38-E3",
        "n_per_family": N_PER_FAMILY,
        "n_queries": total,
        "outcome_counts": outcome_counts,
        "failure_taxonomy": failure_counts,
        "fallback_rate": punt_count as f64 / total as f64,
        "queries_with_any_hit_fraction": queries_with_any_hit as f64 / total as f64,
        "ndcg_mean": Distribution::compute(&ndcgs).mean,
        "recall_mean": Distribution::compute(&recalls).mean,
        "mrr_mean": Distribution::compute(&mrrs).mean,
        "candidate_size_p50": Distribution::compute(&candidate_sizes).p50,
        "latency_ms_p50": Distribution::compute(&latencies_ms).p50,
        "latency_ms_p95": Distribution::compute(&latencies_ms).p95,
        "size_conflict_relevant_by_family": size_conflict_relevant_by_family,
        "size_conflict_hits_by_family": size_conflict_hits_by_family,
        "size_conflict_jeans_anchor": jeans_anchor,
        "size_lexicon_candidate_count_for_jeans_anchor": size_lexicon_sources,
        "size_numeric_values_profiled": size_numeric_values.len(),
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

    println!("\n=== I38-E3 complete ===");
}
