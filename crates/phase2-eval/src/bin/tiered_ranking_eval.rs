//! Issue #7 residual hypothesis #4: does a small ordered set of discrete
//! lexicographic ranking TIERS beat `commerce_core::index::rank`'s current
//! ranking mechanism -- summing compiled `Preference` boost weights
//! (`score_preferences`) into one `f64` per hit and sorting desc, with a
//! deterministic id tiebreak only when that sum ties?
//!
//! **Question**: on real per-hit inputs (matched-constraint count, compiled
//! `Preference` scores, and a real numeric custom-rank signal), does
//! re-ranking the *same* real candidate set with a tiered/lexicographic
//! comparator -- `(tier1, tier2, tier3)` as a tuple, an earlier tier's
//! difference always winning outright over any difference in a later tier
//! -- produce materially higher NDCG@10 than the shipping additive sum?
//!
//! **Hypothesis** (Issue #7 archaeology): Algolia's 8-criteria cascading
//! tie-break and Meilisearch's `Criterion` bucket-sort both converge on
//! discrete tiers instead of one summed number, because additive summing
//! lets a large difference in a later signal numerically outweigh a real
//! difference in an earlier, more decisive one. If that pattern matters
//! here too, a tiered comparator should beat additive-sum by a material
//! (>=5% relative) NDCG@10 margin on the same real candidate sets.
//!
//! **Evidence class: real.** Real 1,215,854-product ESCI catalog, real
//! 22,458 judged queries (`dataset_cache/export/`), a real
//! `commerce_core::cold_start::compile_lexicon`-derived lexicon (no
//! hand-authored vocabulary), and real `CatalogIndex::execute` retrieval.
//! No synthetic products, queries, or judgments.
//!
//! **Established real baselines, cited not re-derived** (see
//! `docs/experiments/PHASE2_LOG.md` P2-E05 and P2-E01,
//! `docs/experiments/ROUND1_LOG.md` R1-E04): P2-E01 measured standalone
//! Tantivy's real relevance ceiling at NDCG@10=0.3033 on this same real
//! catalog/judgment set; P2-E05 measured this project's own integrated
//! system (structural index + delegated Tantivy, `plan::execute_planned`)
//! at NDCG@10=0.2703 (89.1% of that ceiling) at
//! `min_enum_frequency=100` -- the exact lexicon config this experiment
//! reuses below, for direct comparability of *which lexicon* is in play,
//! though NOT of the resulting NDCG@10 numbers themselves (see the scope
//! note below); R1-E04 measured a real Apache Solr baseline at
//! NDCG@10=0.3052 on the same catalog/judgments.
//!
//! **Scope, stated up front so the numbers below are read correctly**:
//! this experiment isolates *only* "how do we order matches" (the ranking
//! question), not "which matches do we find" (the retrieval question).
//! Both arms rank the exact same real candidate set, retrieved once via
//! `CatalogIndex::execute` on the compiled query's structural/attribute
//! constraints alone -- no lexical delegate, no `plan::execute_planned`,
//! no Hybrid/Punt routing. That makes this run's absolute NDCG@10 numbers
//! NOT directly comparable to P2-E05's integrated pipeline numbers (P2-E05
//! also lets a Tantivy delegate contribute candidates); the baselines
//! above are cited for context on the lexicon config and the ceiling this
//! project is chasing, not as a number this run is expected to reproduce.
//!
//! **A real architectural fact this experiment surfaces, not hidden**:
//! `commerce_core::cold_start::profile::compile_lexicon`'s own doc comment
//! states the cold-start profiler "cannot propose `ir::Preference`s the
//! way a human curator did" -- confirmed empirically below (see the
//! "queries with >=1 compiled preference" counter, which prints 0 on every
//! run). Every real query compiled from this catalog's cold-start lexicon
//! therefore has `query.preferences == []`, which makes
//! `rank::score_preferences` return `0.0` for every hit, every query --
//! so on this real workload, the *shipping* additive-sum ranking
//! (arm (a) below) always ties at 0.0 and degenerates entirely to its
//! id-based tiebreak (an arbitrary order with no relevance signal at all).
//! Likewise tier1 (matched-constraint count) is provably constant within
//! one query's candidate set here: `CatalogIndex::execute` is a hard-AND
//! filter, so every hit it returns already satisfies 100% of the query's
//! constraints by construction (also confirmed empirically below). This
//! experiment's only real discriminating variable on this dataset is
//! therefore tier3 -- a real numeric custom-rank signal
//! (`brand_occurrence_count`, computed once from the real ingested
//! catalog) -- used as a lexicographic tiebreak in arm (b), versus arm
//! (a)'s implicit fallback to arbitrary product-id order. That is still a
//! faithful, real test of "does the shape of the tiebreak matter", just
//! not of "does bucketing beat summing *preference* weights specifically"
//! -- there are no real compiled preferences on this catalog's cold-start
//! lexicon to bucket or sum in the first place.
//!
//! **Falsification bar** (relevance-quality, not latency): <5% relative
//! NDCG@10 improvement of the tiered arm over the additive-sum
//! reproduction is reported as FALSIFIED/INCONCLUSIVE, not massaged into a
//! positive result.
//!
//! Usage: cargo run --release -p phase2-eval --bin tiered_ranking_eval
//!        [catalog.jsonl] [queries.jsonl]

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::Instant;

use commerce_core::cold_start::{compile_lexicon, CatalogProfile};
use commerce_core::domain::{
    effective_attributes, AttributeMap, AttributeValue, BrandId, Catalog, Product, ProductId,
    Variant, VariantId,
};
use commerce_core::index::CatalogIndex;
use commerce_core::ir::{compile, CommerceQuery, Preference, ResolvedConstraint};
use round1_eval::catalog;
use round1_eval::data::{self, EsciLabel};

const K: usize = 10;
/// Swept across the same threshold set `docs/experiments/PHASE2_LOG.md`
/// P2-E05 and P2-E02 already established evidence at, for multi-point real
/// evidence rather than one single run -- `100` is P2-E05's headline
/// config (NDCG@10=0.2703, 89.1% of P2-E01's Tantivy-alone ceiling); `1`
/// is "no filtering" (every value trusted, noisiest vocabulary, largest
/// evaluated query count).
const MIN_ENUM_FREQUENCIES: [usize; 4] = [1, 5, 25, 100];
/// A query whose only recognized constraint is a price constraint can, on
/// this real catalog, produce a near-universal candidate set: every real
/// ESCI product is ingested with the same sentinel `Price::usd(0)`
/// (`round1_eval::catalog`'s own documented limitation -- the source data
/// has no real price field), so `PriceUnderCents(any positive cents)`
/// matches essentially the entire 1,215,854-product catalog. Both ranking
/// arms would pay the same O(n log n) sort cost over that candidate set
/// (arm (a)'s `execute_ranked` scores+sorts the full candidate vector
/// exactly like arm (b) does), so this cap is a symmetric, documented
/// evaluation-scope decision -- applied identically to both arms before
/// either sees the query -- not a thumb on the scale for one arm.
const MAX_CANDIDATES: usize = 20_000;

fn relevance_gain(label: EsciLabel) -> f64 {
    match label {
        EsciLabel::Exact => 3.0,
        EsciLabel::Substitute => 2.0,
        EsciLabel::Complement => 1.0,
        EsciLabel::Irrelevant => 0.0,
    }
}

fn percentile_ms(sorted_ns: &[u128], p: f64) -> f64 {
    if sorted_ns.is_empty() {
        return 0.0;
    }
    let idx = ((sorted_ns.len() as f64 - 1.0) * p).round() as usize;
    sorted_ns[idx] as f64 / 1_000_000.0
}

fn percentile_usize(sorted: &[usize], p: f64) -> usize {
    if sorted.is_empty() {
        return 0;
    }
    let idx = ((sorted.len() as f64 - 1.0) * p).round() as usize;
    sorted[idx]
}

/// NDCG@10 exactly matching `tantivy_relevance_eval.rs`/
/// `planner_integration_eval.rs`'s formula: graded relevance E=3/S=2/C=1/I=0,
/// `log2(i+2)` discount, ideal ranking from ALL of this query's judged
/// grades (not just what was retrieved).
fn ndcg_at_k(ranked_asins: &[&String], judged: &HashMap<String, EsciLabel>, k: usize) -> f64 {
    let dcg: f64 = ranked_asins
        .iter()
        .take(k)
        .enumerate()
        .map(|(i, pid)| {
            let gain = judged
                .get(*pid)
                .map(|&label| relevance_gain(label))
                .unwrap_or(0.0);
            gain / (i as f64 + 2.0).log2()
        })
        .sum();
    let mut ideal_gains: Vec<f64> = judged.values().map(|&l| relevance_gain(l)).collect();
    ideal_gains.sort_by(|a, b| b.total_cmp(a));
    let idcg: f64 = ideal_gains
        .iter()
        .take(k)
        .enumerate()
        .map(|(i, &g)| g / (i as f64 + 2.0).log2())
        .sum();
    if idcg > 0.0 {
        dcg / idcg
    } else {
        0.0
    }
}

/// How many of `query`'s compiled constraints this specific hit satisfies,
/// computed per-hit (not assumed) by re-running the exact same per-constraint
/// checks `CommerceQuery::matches_variant` uses. On this codebase's real
/// `CatalogIndex::execute` (a hard-AND filter -- only fully-matching hits
/// are ever returned as candidates at all), this is expected to always
/// equal `query.constraints.len()` for every hit in one query's candidate
/// set; that expectation is checked empirically in `main`, not just assumed.
fn matched_constraint_count(
    query: &CommerceQuery,
    product: &Product,
    variant: &Variant,
    attrs: &AttributeMap,
) -> usize {
    query
        .constraints
        .iter()
        .filter(|c| match c {
            ResolvedConstraint::Attribute(constraint) => constraint.matches(attrs),
            ResolvedConstraint::Structural(structural) => structural.matches(product, variant),
        })
        .count()
}

/// Mirrors `commerce_core::index::rank`'s private `score_preferences` hit
/// test exactly (Enum equality / MultiEnum any / Text contains) so tier2 is
/// built from the identical match semantics the shipping additive scorer
/// uses -- but returns a discrete count of matched preferences rather than
/// a summed weight, per Issue #7's "a bucket, not a raw sum" tier
/// definition.
fn matched_preference_count(preferences: &[Preference], attrs: &AttributeMap) -> usize {
    preferences
        .iter()
        .filter(|pref| match pref {
            Preference::Boost {
                attribute, value, ..
            } => match attrs.get(attribute) {
                Some(AttributeValue::Enum(v)) => v == value,
                Some(AttributeValue::MultiEnum(vs)) => vs.iter().any(|v| v == value),
                Some(AttributeValue::Text(t)) => t.contains(value.as_str()),
                _ => false,
            },
            // Issue #6 P1-B added this variant after I7-E04 (this
            // experiment) already reached its falsified conclusion; not
            // attribute-map-shaped, so out of scope for this mirror.
            Preference::StructuralBoost(..) => false,
        })
        .count()
}

#[derive(Debug, Clone, Copy)]
struct TieredHit {
    product: ProductId,
    variant: VariantId,
    tier1: usize,
    tier2: usize,
    tier3: u64,
}

#[derive(Debug, Clone, Copy)]
struct TierStats {
    candidate_count: usize,
    tier1_varies: bool,
    tier2_varies: bool,
    tier3_varies: bool,
}

/// The new tiered/lexicographic scorer: sorts by `(tier1, tier2, tier3)` as
/// a tuple comparison, not a summed score -- an earlier tier's difference
/// always wins outright over any difference in a later tier. tier1 = how
/// many of this query's compiled constraints this hit satisfies; tier2 =
/// discrete count of compiled `Preference::Boost` matches (a bucket, not a
/// summed weight); tier3 = a real numeric custom-rank signal,
/// `brand_occurrence_count` (how many products in the real catalog share
/// this hit's brand, computed once up front). Final tiebreak is
/// product/variant id ascending -- the same determinism guarantee
/// `rank::execute_ranked` gives, applied only once every real tier is
/// exhausted.
fn tiered_rank(
    index: &CatalogIndex,
    query: &CommerceQuery,
    catalog: &Catalog,
    brand_counts: &HashMap<BrandId, u64>,
    k: usize,
) -> (Vec<(ProductId, VariantId)>, TierStats) {
    let mut scored: Vec<TieredHit> = index
        .execute(query, catalog)
        .into_iter()
        .map(|(product_id, variant_id)| {
            let (product, variant) = index
                .lookup_variant(catalog, variant_id)
                .expect("execute() only returns ids that exist in this catalog");
            let attrs = effective_attributes(product, variant);
            TieredHit {
                product: product_id,
                variant: variant_id,
                tier1: matched_constraint_count(query, product, variant, &attrs),
                tier2: matched_preference_count(&query.preferences, &attrs),
                tier3: brand_counts.get(&product.brand).copied().unwrap_or(0),
            }
        })
        .collect();

    let candidate_count = scored.len();
    let (tier1_varies, tier2_varies, tier3_varies) = if scored.is_empty() {
        (false, false, false)
    } else {
        (
            scored.iter().any(|h| h.tier1 != scored[0].tier1),
            scored.iter().any(|h| h.tier2 != scored[0].tier2),
            scored.iter().any(|h| h.tier3 != scored[0].tier3),
        )
    };

    scored.sort_by(|a, b| {
        b.tier1
            .cmp(&a.tier1)
            .then(b.tier2.cmp(&a.tier2))
            .then(b.tier3.cmp(&a.tier3))
            .then(a.product.0.cmp(&b.product.0))
            .then(a.variant.0.cmp(&b.variant.0))
    });
    scored.truncate(k);

    (
        scored.into_iter().map(|h| (h.product, h.variant)).collect(),
        TierStats {
            candidate_count,
            tier1_varies,
            tier2_varies,
            tier3_varies,
        },
    )
}

#[allow(clippy::too_many_arguments)]
fn run_for_threshold(
    min_enum_frequency: usize,
    ingested_catalog: &Catalog,
    index: &CatalogIndex,
    brand_counts: &HashMap<BrandId, u64>,
    profile: &CatalogProfile,
    judged_by_query: &HashMap<u64, (String, HashMap<String, EsciLabel>)>,
    product_id_to_asin: &HashMap<ProductId, String>,
) {
    println!();
    println!("################ min_enum_frequency={min_enum_frequency} ################");
    let lexicon = compile_lexicon(profile, min_enum_frequency);

    let mut skipped_no_relevant = 0usize;
    let mut skipped_no_constraints = 0usize;
    let mut skipped_too_large = 0usize;
    let mut evaluated = 0usize;
    let mut queries_with_nonempty_preferences = 0usize;
    let mut queries_where_tier1_varies = 0usize;
    let mut queries_where_tier2_varies = 0usize;
    let mut queries_where_tier3_varies = 0usize;
    let mut queries_where_topk_differs = 0usize;
    let mut candidate_set_sizes: Vec<usize> = Vec::new();

    let mut ndcgs_additive = Vec::new();
    let mut ndcgs_tiered = Vec::new();
    let mut latency_additive_ns: Vec<u128> = Vec::new();
    let mut latency_tiered_ns: Vec<u128> = Vec::new();

    let sweep_start = Instant::now();
    for (query_text, judged) in judged_by_query.values() {
        let relevant_ids: HashSet<&String> = judged
            .iter()
            .filter(|(_, label)| label.is_relevant())
            .map(|(pid, _)| pid)
            .collect();
        if relevant_ids.is_empty() {
            skipped_no_relevant += 1;
            continue;
        }

        let compiled = compile(query_text, &lexicon);
        if compiled.constraints.is_empty() {
            // `CatalogIndex::indexed_candidates` documents that it falls
            // back to the FULL catalog when there are no indexable
            // constraints at all -- ranking all 1.2M candidates for such a
            // query is a retrieval-scope question this experiment does not
            // isolate (see the doc comment's scope note). Skipped, counted,
            // not silently dropped.
            skipped_no_constraints += 1;
            continue;
        }

        let probe = index.execute(&compiled, ingested_catalog);
        if probe.len() > MAX_CANDIDATES {
            skipped_too_large += 1;
            continue;
        }

        if !compiled.preferences.is_empty() {
            queries_with_nonempty_preferences += 1;
        }

        // Arm (a): the literal shipping additive-sum mechanism, unmodified
        // -- calling `execute_ranked` directly rather than reimplementing
        // `score_preferences` gets the exact current behavior for real
        // comparison, per this experiment's design constraint.
        let t0 = Instant::now();
        let additive_hits = index.execute_ranked(&compiled, ingested_catalog, K);
        latency_additive_ns.push(t0.elapsed().as_nanos());

        // Arm (b): the new tiered/lexicographic scorer, over the same
        // real candidate set `execute_ranked` just scored.
        let t1 = Instant::now();
        let (tiered_hits, stats) = tiered_rank(index, &compiled, ingested_catalog, brand_counts, K);
        latency_tiered_ns.push(t1.elapsed().as_nanos());

        candidate_set_sizes.push(stats.candidate_count);
        if stats.tier1_varies {
            queries_where_tier1_varies += 1;
        }
        if stats.tier2_varies {
            queries_where_tier2_varies += 1;
        }
        if stats.tier3_varies {
            queries_where_tier3_varies += 1;
        }

        let additive_asins: Vec<&String> = additive_hits
            .iter()
            .filter_map(|h| product_id_to_asin.get(&h.product))
            .collect();
        let tiered_asins: Vec<&String> = tiered_hits
            .iter()
            .filter_map(|(p, _)| product_id_to_asin.get(p))
            .collect();

        if additive_asins != tiered_asins {
            queries_where_topk_differs += 1;
        }

        ndcgs_additive.push(ndcg_at_k(&additive_asins, judged, K));
        ndcgs_tiered.push(ndcg_at_k(&tiered_asins, judged, K));

        evaluated += 1;
    }
    let sweep_elapsed = sweep_start.elapsed();

    if evaluated == 0 {
        println!("no queries evaluated -- cannot compute NDCG@10 (check dataset paths / filters)");
        return;
    }

    latency_additive_ns.sort_unstable();
    latency_tiered_ns.sort_unstable();
    candidate_set_sizes.sort_unstable();

    let n = evaluated as f64;
    let mean_additive: f64 = ndcgs_additive.iter().sum::<f64>() / n;
    let mean_tiered: f64 = ndcgs_tiered.iter().sum::<f64>() / n;
    let relative_change_pct = if mean_additive > 0.0 {
        (mean_tiered - mean_additive) / mean_additive * 100.0
    } else {
        f64::NAN
    };

    println!();
    println!(
        "=== Real-data run: {evaluated} queries evaluated in {:.1}s ===",
        sweep_elapsed.as_secs_f64()
    );
    println!("  skipped (no Exact/Substitute judgment): {skipped_no_relevant}");
    println!("  skipped (zero compiled constraints -- would fall back to the full catalog as candidates): {skipped_no_constraints}");
    println!("  skipped (candidate set > {MAX_CANDIDATES} -- symmetric cap, both arms): {skipped_too_large}");
    println!();
    println!("=== Architecture-fact diagnostics (checked empirically, not assumed) ===");
    println!(
        "  queries with >=1 compiled Preference (compile_lexicon never emits Preference candidates -- expect 0): {queries_with_nonempty_preferences}/{evaluated}"
    );
    println!(
        "  queries where tier1 (matched-constraint count) varies within the candidate set (execute() is hard-AND -- expect 0): {queries_where_tier1_varies}/{evaluated}"
    );
    println!(
        "  queries where tier2 (preference-match count) varies within the candidate set (expect 0, same reason as above): {queries_where_tier2_varies}/{evaluated}"
    );
    println!(
        "  queries where tier3 (brand_occurrence_count) varies within the candidate set (this is the only real discriminator, expect >0): {queries_where_tier3_varies}/{evaluated}"
    );
    println!(
        "  queries where the two arms' top-{K} ordering actually differs: {queries_where_topk_differs}/{evaluated}"
    );
    println!(
        "  candidate set size: min={} p50={} p95={} max={}",
        candidate_set_sizes.first().copied().unwrap_or(0),
        percentile_usize(&candidate_set_sizes, 0.5),
        percentile_usize(&candidate_set_sizes, 0.95),
        candidate_set_sizes.last().copied().unwrap_or(0)
    );
    println!();
    println!("=== NDCG@10 (real ESCI judgments, same candidate set fed to both arms) ===");
    println!("  (a) additive-sum (commerce_core::index::rank, unmodified, via execute_ranked): {mean_additive:.4}");
    println!("  (b) tiered/lexicographic (tier1, tier2, tier3 tuple compare):                  {mean_tiered:.4}");
    println!("  relative change of (b) vs (a): {relative_change_pct:.2}%");
    println!();
    println!(
        "  latency (a) additive-sum   (n={}): p50={:.4}ms p95={:.4}ms p99={:.4}ms",
        latency_additive_ns.len(),
        percentile_ms(&latency_additive_ns, 0.5),
        percentile_ms(&latency_additive_ns, 0.95),
        percentile_ms(&latency_additive_ns, 0.99)
    );
    println!(
        "  latency (b) tiered         (n={}): p50={:.4}ms p95={:.4}ms p99={:.4}ms",
        latency_tiered_ns.len(),
        percentile_ms(&latency_tiered_ns, 0.5),
        percentile_ms(&latency_tiered_ns, 0.95),
        percentile_ms(&latency_tiered_ns, 0.99)
    );
    println!();
    if relative_change_pct.is_nan() {
        println!("=== VERDICT: INCONCLUSIVE (additive-sum arm's mean NDCG@10 is exactly 0.0 -- relative change is undefined) ===");
    } else if relative_change_pct >= 5.0 {
        println!("=== VERDICT: CONFIRMED -- tiered/lexicographic ranking beats additive-sum by {relative_change_pct:.2}% relative NDCG@10 (>=5% bar met) ===");
    } else {
        println!("=== VERDICT: FALSIFIED -- tiered/lexicographic ranking's real NDCG@10 change vs. additive-sum is {relative_change_pct:.2}% relative (<5% bar) ===");
    }
    println!();
    println!("(Cited context, not reproduced here -- see this file's doc comment for why these numbers aren't directly comparable to the ones above: P2-E01 Tantivy-alone standalone ceiling NDCG@10=0.3033; P2-E05 integrated system at min_enum_frequency=100, NDCG@10=0.2703 (89.1% of ceiling); R1-E04 real Solr baseline NDCG@10=0.3052.)");
}

fn main() {
    let mut args = std::env::args().skip(1);
    let catalog_path = args
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("dataset_cache/export/catalog.jsonl"));
    let queries_path = args
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("dataset_cache/export/queries.jsonl"));

    println!("loading + ingesting real catalog...");
    let products = data::load_catalog(&catalog_path);
    let ingested = catalog::build_catalog(&products);
    println!("{} real products ingested", ingested.catalog.products.len());

    println!("building commerce_core structural index...");
    let index = CatalogIndex::build(&ingested.catalog);

    let mut brand_counts: HashMap<BrandId, u64> = HashMap::new();
    for product in &ingested.catalog.products {
        *brand_counts.entry(product.brand).or_insert(0) += 1;
    }
    println!(
        "{} distinct brand ids counted for tier3's custom-rank signal (brand_occurrence_count)",
        brand_counts.len()
    );

    let profile = CatalogProfile::build(&ingested.catalog, &ingested.brands, &[], &[]);

    println!("loading real queries + judgments...");
    let judgments = data::load_queries(&queries_path);
    let mut judged_by_query: HashMap<u64, (String, HashMap<String, EsciLabel>)> = HashMap::new();
    for j in &judgments {
        judged_by_query
            .entry(j.query_id)
            .or_insert_with(|| (j.query.clone(), HashMap::new()))
            .1
            .insert(j.product_id.clone(), j.label);
    }
    let product_id_to_asin: HashMap<ProductId, String> = ingested
        .asin_to_product_id
        .iter()
        .map(|(asin, pid)| (*pid, asin.clone()))
        .collect();

    for &min_enum_frequency in &MIN_ENUM_FREQUENCIES {
        run_for_threshold(
            min_enum_frequency,
            &ingested.catalog,
            &index,
            &brand_counts,
            &profile,
            &judged_by_query,
            &product_id_to_asin,
        );
    }
}
