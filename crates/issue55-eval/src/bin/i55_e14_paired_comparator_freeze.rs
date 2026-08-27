//! Issue #55 checkpoint-14 follow-up, Priority 1A of the Architecture
//! Falsification Loop (Issue #55 issue body, "cleanly resolve checkpoint
//! 14 before more optimization"): `docs/decisions/ISSUE55_HYPONYM_LEAF_ONLY_DECISION.md`
//! reported `structural_routed` NDCG reversing from -25.05% to +5.37%
//! when leaf-only `ProductTypeAny` hyponym expansion was wired in, but in
//! the same before/after pair Solr's OWN NDCG on that traffic also moved
//! (0.3939 -> 0.3455) -- and the native treatment should never itself
//! change Solr's ranking. That checkpoint's own raw artifacts
//! (`docs/research/artifacts/i55_product_type_hyponym/p9_e02_after_revert.txt`,
//! `p9_e02_after_leaf_fix.txt`) show identical `structural_routed` counts
//! (n=21) before and after, but "same count" was never checked against
//! "same query IDs" -- exactly the ambiguity this experiment resolves.
//!
//! **Root-cause hypothesis, stated before this binary was run**: `p9_e02`
//! builds each query's Solr `(q, fq)` pair from that SAME query's
//! *compiled* `residual_lexical`/`constraints`
//! (`wands_solr_query_for` in `p9_e02_wands_physical_advantage.rs`) --
//! which are downstream of the very lexicon the `ProductTypeAny`
//! treatment changes. So Solr is not actually a frozen, independent
//! comparator across the before/after pair: enabling hyponym expansion
//! can change what TEXT is left in `residual_lexical` (more of the query
//! consumed into a `ProductTypeAny` constraint) and what `fq` filters are
//! sent, which can change Solr's own ranking even though Solr's index and
//! ranking algorithm never changed. This is the leading candidate
//! explanation for the observed Solr NDCG drift, to be confirmed or
//! falsified directly below, not assumed.
//!
//! **Design**: build ONE catalog/native-index (shared, treatment-
//! independent) and TWO lexicons from the exact same `CatalogProfile` via
//! the new `compile_lexicon_with_product_type_hyponyms` toggle
//! (`crates/commerce-core/src/cold_start/profile.rs`) --
//! `baseline` (hyponyms off, reproduces the pre-checkpoint-14 production
//! behavior `p9_e02_after_revert.txt` measured) and `treatment` (hyponyms
//! on, current production, what `p9_e02_after_leaf_fix.txt` measured).
//! For every WANDS query with judgments:
//!
//! 1. Compile under both lexicons; run `execute_planned` under both to
//!    get each treatment's own routing outcome and native hits.
//! 2. Freeze the query's structural-routed membership under EACH
//!    treatment separately, and report the exact set overlap/difference
//!    (not just counts).
//! 3. For every query that is structural-routed under EITHER treatment,
//!    fire the Solr query TWICE per repetition -- once built from the
//!    baseline compile, once from the treatment compile -- against the
//!    same live Solr core, in the same run, REPEAT_RUNS times, to
//!    separate (a) Solr-answer changes caused by the compiler changing
//!    the query text it sends Solr, from (b) genuine run-to-run Solr-side
//!    nondeterminism (JVM/cache warmup, tie-breaking) on an IDENTICAL
//!    query text.
//! 4. Report per-query paired deltas for native and for each Solr
//!    variant, split by FastPath vs Hybrid (per the issue's own named
//!    "FastPath: native materially worse; Hybrid: native materially
//!    better" hypothesis), plus the aggregate reproduction of checkpoint
//!    14's own -25.05%/+5.37% headline as a sanity check that this
//!    harness is measuring the same thing before trusting the paired
//!    breakdown built on top of it.
//!
//! Reproduction: `cargo build --release -p issue55-eval &&
//! ./target/release/i55_e14_paired_comparator_freeze [solr_base_url] [repeat_runs]`

use std::collections::{BTreeMap, BTreeSet};

use commerce_core::cold_start::{
    compile_lexicon_with_promoted_hyponyms, promote_all_hyponym_candidates_unadjudicated,
    CatalogProfile,
};
use commerce_core::control_plane::PromotedHyponyms;
use commerce_core::domain::{CategoryId, Constraint, ProductId, ProductTypeId};
use commerce_core::index::CatalogIndex;
use commerce_core::ir::{compile, CommerceQuery, ResolvedConstraint, StructuralConstraint};
use commerce_core::plan::{execute_planned, ExecutionOutcome, PlannerPolicy};
use phase9_eval::bitmap_delegate::{build_index, BitmapTantivyDelegate, BuiltIndex};
use phase9_eval::wands_relevance::{ndcg_recall_mrr, WandsLabel};

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

/// Identical construction to `p9_e02_wands_physical_advantage.rs`'s own
/// `wands_solr_query_for` -- reused verbatim (not reimplemented) so this
/// experiment's Solr queries are apples-to-apples with checkpoint 14's
/// own recorded numbers, which is exactly what the aggregate-reproduction
/// sanity check below depends on.
fn wands_solr_query_for(
    query_text: &str,
    residual_lexical: &[String],
    constraints: &[ResolvedConstraint],
    category_name_by_id: &BTreeMap<CategoryId, String>,
    product_type_name_by_id: &BTreeMap<ProductTypeId, String>,
) -> (String, Vec<String>) {
    let mut fq = Vec::new();
    for c in constraints {
        match c {
            ResolvedConstraint::Structural(StructuralConstraint::Category(id)) => {
                if let Some(name) = category_name_by_id.get(id) {
                    fq.push(format!(
                        "category_leaf:/{}/",
                        round1_eval::solr::case_insensitive_field_regex(name)
                    ));
                }
            }
            ResolvedConstraint::Structural(StructuralConstraint::ProductType(id)) => {
                if let Some(name) = product_type_name_by_id.get(id) {
                    fq.push(format!(
                        "product_class:/{}/",
                        round1_eval::solr::case_insensitive_field_regex(name)
                    ));
                }
            }
            ResolvedConstraint::Structural(StructuralConstraint::ProductTypeAny(ids)) => {
                // Same OR-of-regex construction `p9_e02` itself would need
                // for a `ProductTypeAny` fq (checkpoint 14 predates this
                // constraint reaching `wands_solr_query_for` in production,
                // so there is no prior version to match byte-for-byte;
                // this is the natural fq translation of "any of these
                // product types" -- an OR of the same per-id regex fq
                // the `ProductType` arm already uses).
                let names: Vec<&String> = ids
                    .iter()
                    .filter_map(|id| product_type_name_by_id.get(id))
                    .collect();
                if !names.is_empty() {
                    let alternation = names
                        .iter()
                        .map(|n| round1_eval::solr::case_insensitive_field_regex(n))
                        .collect::<Vec<_>>()
                        .join("|");
                    fq.push(format!("product_class:/({alternation})/"));
                }
            }
            ResolvedConstraint::Attribute(Constraint::Enum { attribute, value }) => {
                fq.push(format!(
                    "{attribute}:/{}/",
                    round1_eval::solr::case_insensitive_field_regex(value)
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Routing {
    FastPath,
    Hybrid,
    Punt,
}

impl Routing {
    fn from_outcome(o: ExecutionOutcome) -> Self {
        match o {
            ExecutionOutcome::FastPath => Routing::FastPath,
            ExecutionOutcome::Hybrid => Routing::Hybrid,
            ExecutionOutcome::Punt => Routing::Punt,
        }
    }
    fn is_structural(self) -> bool {
        matches!(self, Routing::FastPath | Routing::Hybrid)
    }
    fn label(self) -> &'static str {
        match self {
            Routing::FastPath => "FastPath",
            Routing::Hybrid => "Hybrid",
            Routing::Punt => "Punt",
        }
    }
}

struct TreatmentRun {
    routing: Routing,
    native_ndcg: f64,
    native_ids: Vec<String>,
    compiled: CommerceQuery,
}

/// Everything shared across every query/treatment evaluation this run --
/// bundled to stay under clippy's `too_many_arguments` bar without
/// threading eight separate parameters through `run_treatment`.
struct EvalContext<'a> {
    catalog: &'a commerce_core::domain::Catalog,
    index: &'a CatalogIndex,
    delegate: &'a BitmapTantivyDelegate,
    policy: &'a PlannerPolicy,
    wands_id_by_product_id: &'a BTreeMap<ProductId, String>,
}

fn run_treatment(
    ctx: &EvalContext,
    text: &str,
    lexicon: &commerce_core::ir::SemanticLexicon,
    judged: &BTreeMap<String, WandsLabel>,
) -> TreatmentRun {
    let compiled = compile(text, lexicon);
    let (planned, hits) = execute_planned(
        &compiled,
        ctx.catalog,
        ctx.index,
        Some(ctx.delegate),
        K,
        ctx.policy,
        None,
    );
    let native_ids: Vec<String> = hits
        .iter()
        .filter_map(|h| ctx.wands_id_by_product_id.get(&h.product).cloned())
        .collect();
    let (native_ndcg, _, _) = ndcg_recall_mrr(&native_ids, judged, K);
    TreatmentRun {
        routing: Routing::from_outcome(planned.outcome),
        native_ndcg,
        native_ids,
        compiled,
    }
}

/// Fires the Solr query `repeat_runs` times against `solr_base_url`,
/// scoring each repetition independently against `judged` -- gives both a
/// mean and a per-run spread, so a single-run number is never mistaken
/// for a stable measurement. Every repetition that fails (transport/parse
/// error, or a Solr-side query error) is recorded and EXCLUDED from the
/// mean, never scored as NDCG=0.0 -- the same research-hygiene rule this
/// checkpoint's own P0.2 sibling fix applies to `issue35-eval`.
fn solr_ndcg_repeated(
    solr_base_url: &str,
    q: &str,
    fq: &[String],
    judged: &BTreeMap<String, WandsLabel>,
    repeat_runs: usize,
) -> (Vec<f64>, usize) {
    let mut ndcgs = Vec::with_capacity(repeat_runs);
    let mut failures = 0usize;
    for _ in 0..repeat_runs {
        match round1_eval::solr::solr_search(solr_base_url, q, fq, K) {
            Some(result) => {
                let (ndcg, _, _) = ndcg_recall_mrr(&result.ids, judged, K);
                ndcgs.push(ndcg);
            }
            None => failures += 1,
        }
    }
    (ndcgs, failures)
}

fn mean(v: &[f64]) -> f64 {
    if v.is_empty() {
        0.0
    } else {
        v.iter().sum::<f64>() / v.len() as f64
    }
}

fn stdev(v: &[f64]) -> f64 {
    if v.len() < 2 {
        return 0.0;
    }
    let m = mean(v);
    (v.iter().map(|x| (x - m).powi(2)).sum::<f64>() / (v.len() - 1) as f64).sqrt()
}

fn main() {
    println!("=== Issue #55 checkpoint-14 follow-up: paired comparator freeze (Priority 1A) ===");

    let mut args = std::env::args().skip(1);
    let solr_base_url = args
        .next()
        .unwrap_or_else(|| "http://localhost:8983/solr/wands_bench".to_string());
    let repeat_runs: usize = args.next().and_then(|s| s.parse().ok()).unwrap_or(3).max(1);
    println!("solr_base_url={solr_base_url}  repeat_runs={repeat_runs}");

    println!("loading WANDS catalog...");
    let products = phase6a_eval::data::load_catalog(std::path::Path::new(CATALOG_PATH));
    let ingested = phase6a_eval::catalog::build_catalog(&products);
    println!(
        "catalog: {} products, {} categories, {} product types",
        ingested.catalog.products.len(),
        ingested.categories.len(),
        ingested.product_types.len()
    );

    let category_name_by_id: BTreeMap<CategoryId, String> = ingested
        .categories
        .iter()
        .map(|c| (c.id, c.name.clone()))
        .collect();
    let product_type_name_by_id: BTreeMap<ProductTypeId, String> = ingested
        .product_types
        .iter()
        .map(|pt| (pt.id, pt.name.clone()))
        .collect();
    let wands_id_by_product_id: BTreeMap<ProductId, String> = ingested
        .wands_id_to_product_id
        .iter()
        .map(|(wands_id, pid)| (*pid, wands_id.clone()))
        .collect();

    println!("building native index (shared, treatment-independent)...");
    let index = CatalogIndex::build(&ingested.catalog);
    let profile = CatalogProfile::build(
        &ingested.catalog,
        &[],
        &ingested.product_types,
        &ingested.categories,
    );
    // The one variable under test: same profile, same min_enum_frequency,
    // hyponym expansion toggled -- everything else about the lexicon is
    // identical by construction (see profile.rs's own
    // promoted_hyponyms_empty_matches_compile_lexicon regression test for
    // the proof that an empty `PromotedHyponyms` here is exactly
    // `compile_lexicon`). This binary measures the leaf-only hyponym
    // *expansion mechanism* checkpoint 14 introduced, in isolation from
    // Issue #55 A1's later promotion-adjudication gate -- so "treatment"
    // here deliberately reproduces the pre-A1 unconditional-auto-install
    // behavior via `promote_all_hyponym_candidates_unadjudicated`, not a
    // real adjudicated promotion set (see A2 for that).
    let baseline_lexicon = compile_lexicon_with_promoted_hyponyms(
        &profile,
        MIN_ENUM_FREQUENCY,
        &PromotedHyponyms::default(),
    );
    let all_candidates_promoted = promote_all_hyponym_candidates_unadjudicated(&profile);
    let treatment_lexicon = compile_lexicon_with_promoted_hyponyms(
        &profile,
        MIN_ENUM_FREQUENCY,
        &all_candidates_promoted,
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
    let judged_all = load_labels(LABEL_PATH);

    // Step 1: run BOTH treatments over every judged query, freeze each
    // treatment's own routing + native NDCG. No Solr call yet -- this is
    // the purely-native half of the paired comparison and does not depend
    // on Solr availability at all.
    struct Row {
        query_id: String,
        text: String,
        baseline: TreatmentRun,
        treatment: TreatmentRun,
    }
    let ctx = EvalContext {
        catalog: &ingested.catalog,
        index: &index,
        delegate: &delegate,
        policy: &policy,
        wands_id_by_product_id: &wands_id_by_product_id,
    };
    let mut rows: Vec<Row> = Vec::new();
    for q in &queries {
        let Some(judged) = judged_all.get(&q.query_id) else {
            continue;
        };
        let baseline = run_treatment(&ctx, &q.text, &baseline_lexicon, judged);
        let treatment = run_treatment(&ctx, &q.text, &treatment_lexicon, judged);
        rows.push(Row {
            query_id: q.query_id.clone(),
            text: q.text.clone(),
            baseline,
            treatment,
        });
    }
    println!("evaluated {} queries with judgments", rows.len());

    // Step 2: freeze cohort membership per treatment and report the exact
    // overlap -- the crux of "verify whether the before/after cohort is
    // literally identical, not merely the same count."
    let baseline_structural: BTreeSet<&str> = rows
        .iter()
        .filter(|r| r.baseline.routing.is_structural())
        .map(|r| r.query_id.as_str())
        .collect();
    let treatment_structural: BTreeSet<&str> = rows
        .iter()
        .filter(|r| r.treatment.routing.is_structural())
        .map(|r| r.query_id.as_str())
        .collect();
    let both: BTreeSet<&str> = baseline_structural
        .intersection(&treatment_structural)
        .copied()
        .collect();
    let only_baseline: Vec<&str> = baseline_structural
        .difference(&treatment_structural)
        .copied()
        .collect();
    let only_treatment: Vec<&str> = treatment_structural
        .difference(&baseline_structural)
        .copied()
        .collect();

    println!();
    println!("=== cohort freeze: structural_routed (FastPath+Hybrid) query-ID sets ===");
    println!(
        "baseline (hyponyms OFF, pre-checkpoint-14): n={}",
        baseline_structural.len()
    );
    println!(
        "treatment (hyponyms ON, current production): n={}",
        treatment_structural.len()
    );
    println!("identical cohort (same query IDs in both): {}", both.len());
    println!(
        "in baseline's structural_routed cohort but NOT treatment's: {} {:?}",
        only_baseline.len(),
        only_baseline
    );
    println!(
        "in treatment's structural_routed cohort but NOT baseline's: {} {:?}",
        only_treatment.len(),
        only_treatment
    );
    if only_baseline.is_empty() && only_treatment.is_empty() {
        println!(
            "=== COHORT IDENTICAL: the n=21 match in checkpoint 14's own artifacts was not a \
             coincidence -- it is the exact same 21 query IDs both before and after ==="
        );
    } else {
        println!(
            "=== COHORT DIFFERS: checkpoint 14's before/after aggregate compared genuinely \
             different query populations under the same n -- this alone can explain part or all \
             of the observed Solr-side NDCG drift, independent of any Solr nondeterminism ==="
        );
    }

    // Step 3: for every query structural-routed under EITHER treatment,
    // fire Solr repeat_runs times for EACH treatment's own compiled query
    // text, in the same run, back-to-back.
    let union: BTreeSet<&str> = baseline_structural
        .union(&treatment_structural)
        .copied()
        .collect();
    println!();
    println!(
        "=== firing Solr for {} queries (union of both cohorts) x 2 query variants x {repeat_runs} \
         repetitions = {} live Solr calls ===",
        union.len(),
        union.len() * 2 * repeat_runs
    );

    struct SolrPair {
        baseline_q: String,
        baseline_fq: Vec<String>,
        treatment_q: String,
        treatment_fq: Vec<String>,
        query_text_changed: bool,
    }
    let mut solr_pairs: BTreeMap<String, SolrPair> = BTreeMap::new();
    for r in rows.iter().filter(|r| union.contains(r.query_id.as_str())) {
        let (bq, bfq) = wands_solr_query_for(
            &r.text,
            &r.baseline.compiled.residual_lexical,
            &r.baseline.compiled.constraints,
            &category_name_by_id,
            &product_type_name_by_id,
        );
        let (tq, tfq) = wands_solr_query_for(
            &r.text,
            &r.treatment.compiled.residual_lexical,
            &r.treatment.compiled.constraints,
            &category_name_by_id,
            &product_type_name_by_id,
        );
        let query_text_changed = bq != tq || bfq != tfq;
        solr_pairs.insert(
            r.query_id.clone(),
            SolrPair {
                baseline_q: bq,
                baseline_fq: bfq,
                treatment_q: tq,
                treatment_fq: tfq,
                query_text_changed,
            },
        );
    }
    let changed_count = solr_pairs.values().filter(|p| p.query_text_changed).count();
    println!(
        "of {} queries, the compiled Solr (q, fq) itself DIFFERS between baseline and treatment \
         for {changed_count} of them ({:.1}%) -- these are queries where enabling ProductTypeAny \
         changed what text/filters get sent to Solr, not just what native does with the result",
        solr_pairs.len(),
        100.0 * changed_count as f64 / solr_pairs.len().max(1) as f64
    );

    struct SolrScored {
        baseline_ndcgs: Vec<f64>,
        baseline_failures: usize,
        treatment_ndcgs: Vec<f64>,
        treatment_failures: usize,
    }
    let mut solr_scored: BTreeMap<String, SolrScored> = BTreeMap::new();
    for (qid, pair) in &solr_pairs {
        let judged = judged_all
            .get(qid)
            .expect("query already filtered to judged set");
        let (baseline_ndcgs, baseline_failures) = solr_ndcg_repeated(
            &solr_base_url,
            &pair.baseline_q,
            &pair.baseline_fq,
            judged,
            repeat_runs,
        );
        let (treatment_ndcgs, treatment_failures) = solr_ndcg_repeated(
            &solr_base_url,
            &pair.treatment_q,
            &pair.treatment_fq,
            judged,
            repeat_runs,
        );
        solr_scored.insert(
            qid.clone(),
            SolrScored {
                baseline_ndcgs,
                baseline_failures,
                treatment_ndcgs,
                treatment_failures,
            },
        );
    }
    let total_solr_failures: usize = solr_scored
        .values()
        .map(|s| s.baseline_failures + s.treatment_failures)
        .sum();
    if total_solr_failures > 0 {
        eprintln!(
            "\n=== SOLR HARNESS FAILURE: {total_solr_failures} Solr calls in this experiment failed \
             (transport/parse error) and were excluded from their query's mean, never scored as \
             NDCG=0.0 -- see per-query rows below for which queries/variants were affected. This \
             experiment's own numbers are conditioned on the successful subset; a nonzero failure \
             count here means fewer than repeat_runs={repeat_runs} samples back some cells ==="
        );
    }

    // Step 4: per-query paired report.
    println!();
    println!(
        "{:<10} {:<7} {:<9} {:>9} {:>9} {:>9} {:>9} {:>9} {:>9} {:<8}",
        "query_id",
        "routing",
        "route_chg",
        "nat_base",
        "nat_treat",
        "nat_delta",
        "solr_base",
        "solr_treat",
        "solr_delta",
        "q_changed"
    );
    for r in rows.iter().filter(|r| union.contains(r.query_id.as_str())) {
        let scored = &solr_scored[&r.query_id];
        let solr_base_mean = mean(&scored.baseline_ndcgs);
        let solr_treat_mean = mean(&scored.treatment_ndcgs);
        let route_changed = r.baseline.routing != r.treatment.routing;
        println!(
            "{:<10} {:<7} {:<9} {:>9.4} {:>9.4} {:>+9.4} {:>9.4} {:>9.4} {:>+9.4} {:<8}",
            r.query_id,
            r.treatment.routing.label(),
            if route_changed {
                format!(
                    "{}->{}",
                    r.baseline.routing.label(),
                    r.treatment.routing.label()
                )
            } else {
                "-".to_string()
            },
            r.baseline.native_ndcg,
            r.treatment.native_ndcg,
            r.treatment.native_ndcg - r.baseline.native_ndcg,
            solr_base_mean,
            solr_treat_mean,
            solr_treat_mean - solr_base_mean,
            solr_pairs[&r.query_id].query_text_changed,
        );
    }

    // Step 5: Solr run-to-run variance on an IDENTICAL query text --
    // isolates genuine Solr-side nondeterminism from the compiler-driven
    // query-text-change confound quantified above.
    println!();
    println!("=== Solr run-to-run variance on an IDENTICAL query text (repeat_runs={repeat_runs} each) ===");
    let mut all_baseline_stdevs = Vec::new();
    let mut all_treatment_stdevs = Vec::new();
    for scored in solr_scored.values() {
        if scored.baseline_ndcgs.len() >= 2 {
            all_baseline_stdevs.push(stdev(&scored.baseline_ndcgs));
        }
        if scored.treatment_ndcgs.len() >= 2 {
            all_treatment_stdevs.push(stdev(&scored.treatment_ndcgs));
        }
    }
    println!(
        "mean per-query stdev of Solr NDCG across {repeat_runs} repeated identical-text calls: \
         baseline-query-text={:.6}  treatment-query-text={:.6}  (near-zero means Solr itself is \
         deterministic here; the compiler-driven query-text change above is then the whole \
         explanation for any Solr NDCG delta, not Solr nondeterminism)",
        mean(&all_baseline_stdevs),
        mean(&all_treatment_stdevs),
    );

    // Step 6: aggregate reproduction of checkpoint 14's own headline, as a
    // sanity check that this harness is measuring the same thing.
    println!();
    println!(
        "=== aggregate reproduction check (should track checkpoint 14's own -25.05% / +5.37%) ==="
    );
    let structural_rows: Vec<&Row> = rows
        .iter()
        .filter(|r| union.contains(r.query_id.as_str()))
        .collect();
    for (label, is_baseline_cohort) in [
        ("baseline (hyponyms OFF)", true),
        ("treatment (hyponyms ON)", false),
    ] {
        let cohort: Vec<&&Row> = structural_rows
            .iter()
            .filter(|r| {
                if is_baseline_cohort {
                    r.baseline.routing.is_structural()
                } else {
                    r.treatment.routing.is_structural()
                }
            })
            .collect();
        let native_mean = mean(
            &cohort
                .iter()
                .map(|r| {
                    if is_baseline_cohort {
                        r.baseline.native_ndcg
                    } else {
                        r.treatment.native_ndcg
                    }
                })
                .collect::<Vec<_>>(),
        );
        let solr_mean = mean(
            &cohort
                .iter()
                .map(|r| {
                    let s = &solr_scored[&r.query_id];
                    mean(if is_baseline_cohort {
                        &s.baseline_ndcgs
                    } else {
                        &s.treatment_ndcgs
                    })
                })
                .collect::<Vec<_>>(),
        );
        let gap = if solr_mean > 0.0 {
            (native_mean - solr_mean) / solr_mean * 100.0
        } else {
            0.0
        };
        println!(
            "{label}: n={}, native NDCG@10={native_mean:.4}, solr NDCG@10={solr_mean:.4}, relative gap={gap:+.2}%",
            cohort.len()
        );
    }

    // Step 7: FastPath vs Hybrid split, using the TREATMENT's own routing
    // (production truth), per the issue's own named hypothesis: FastPath
    // native materially worse than Solr; Hybrid native materially better.
    println!();
    println!("=== FastPath vs Hybrid split (treatment routing, production truth) ===");
    for routing in [Routing::FastPath, Routing::Hybrid] {
        let cohort: Vec<&&Row> = structural_rows
            .iter()
            .filter(|r| r.treatment.routing == routing)
            .collect();
        if cohort.is_empty() {
            println!(
                "{}: n=0 (no queries route here under treatment)",
                routing.label()
            );
            continue;
        }
        let native_mean = mean(
            &cohort
                .iter()
                .map(|r| r.treatment.native_ndcg)
                .collect::<Vec<_>>(),
        );
        let solr_mean = mean(
            &cohort
                .iter()
                .map(|r| mean(&solr_scored[&r.query_id].treatment_ndcgs))
                .collect::<Vec<_>>(),
        );
        let gap = if solr_mean > 0.0 {
            (native_mean - solr_mean) / solr_mean * 100.0
        } else {
            0.0
        };
        println!(
            "{}: n={}, native NDCG@10={native_mean:.4}, solr NDCG@10={solr_mean:.4}, relative gap={gap:+.2}%",
            routing.label(),
            cohort.len()
        );
    }

    // Step 8: qualitative sample -- the 5 queries with the largest
    // |native NDCG delta|, showing which actual WANDS product IDs native
    // returned under each treatment. Aggregate NDCG deltas alone do not
    // show whether a change is "the same products, reordered" or "a
    // different candidate set entirely" -- this project's own standing
    // discipline (e.g. `ISSUE55_PRODUCT_TYPE_HYPONYM_DECISION.md`'s own
    // real-vocabulary audit) is to check qualitatively, not infer from the
    // aggregate number alone.
    println!();
    println!("=== qualitative sample: 5 largest |native NDCG delta| queries (structural-routed cohort) ===");
    let mut by_abs_delta: Vec<&Row> = structural_rows.to_vec();
    by_abs_delta.sort_by(|a, b| {
        let da = (a.treatment.native_ndcg - a.baseline.native_ndcg).abs();
        let db = (b.treatment.native_ndcg - b.baseline.native_ndcg).abs();
        db.total_cmp(&da)
    });
    for r in by_abs_delta.iter().take(5) {
        println!(
            "query_id={} text={:?} baseline_ndcg={:.4} treatment_ndcg={:.4}",
            r.query_id, r.text, r.baseline.native_ndcg, r.treatment.native_ndcg
        );
        println!(
            "  baseline top-3 native ids:  {:?}",
            r.baseline.native_ids.iter().take(3).collect::<Vec<_>>()
        );
        println!(
            "  treatment top-3 native ids: {:?}",
            r.treatment.native_ids.iter().take(3).collect::<Vec<_>>()
        );
    }
}
