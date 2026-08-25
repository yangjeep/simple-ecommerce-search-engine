//! Issue #42 R1: typed ambiguity and corroborated resolution
//! (`docs/experiments/ISSUE42_PROTOCOL.md`'s R1 section), extended by
//! Issue #51 (`docs/experiments/ISSUE51_PROTOCOL.md`) to add Treatment E.
//!
//! Runs the preregistered workload through all five treatments
//! (`issue42_eval::r1_experimental`), evaluates each against the
//! independent oracle (`issue42_eval::oracle`) -- never against any
//! generator's own judgment map -- and checks the preregistered GO gate.
//! No production code (`commerce_core::ir::query::compile`,
//! `commerce_core::plan::execute_planned`) is modified; both are called
//! as-is via `r1_experimental`'s treatment functions.
//!
//! **Disclosed change (Issue #51)**: Treatments A/B/C/D's own decision
//! logic in `r1_experimental.rs` is byte-for-byte unchanged from R1 --
//! verified directly by `treatment_e_matches_treatment_d_on_every_r1_workload_row`
//! and friends, not merely asserted. What changed in *this binary* is
//! additive: a new `Treatment::E` variant, its registry-build step, and
//! passing that registry through `resolve_for`/`one_latency_trial`'s
//! existing call sites. Rerunning this binary reproduces A/B/C/D's own
//! original numbers unchanged (same fixture, same measurement
//! methodology, same process) while adding E measured fairly alongside
//! them in the same run -- deliberately not split into a second binary,
//! since a cross-process/cross-build confound would undermine exactly
//! the kind of fair, same-run overhead comparison this experiment needs.
//!
//! Reproduction: `cargo build --release -p issue42-eval &&
//! ./target/release/r1_typed_ambiguity_eval [output_summary_json_path]`

use std::collections::BTreeSet;
use std::env;
use std::fs;

use commerce_core::cold_start::{compile_lexicon, CatalogProfile};
use commerce_core::domain::{Catalog, Constraint, NumericOp, ProductId, VariantId};
use commerce_core::index::CatalogIndex;
use commerce_core::ir::{Preference, ResolvedConstraint, SemanticLexicon};
use commerce_core::plan::{ExecutionOutcome, LexicalDelegate, PlannedHit, PlannerPolicy};
use issue42_eval::oracle::{self, AttrRequirement, QueryIntent};
use issue42_eval::r1_experimental::{
    build_attribute_kind_registry, resolve_a, resolve_b, resolve_c,
    resolve_c_isolated_no_residual_push, resolve_d, resolve_e, run_treatment,
    AttributeKindRegistry, Resolution,
};
use issue42_eval::r1_workload::{
    build_typed_ambiguity_catalog, AMBIGUOUS_SIZE_VALUE, FITMENT_PHRASE, IDENTIFIER_VALUE,
};
use phase9_eval::bitmap_delegate::{build_index, BitmapTantivyDelegate};

const K: usize = 10;
const MIN_ENUM_FREQUENCY: usize = 1;
const LATENCY_BATCH: usize = 200;
/// Independent latency trials per treatment, median-combined. This
/// fixture's absolute per-call costs are a few microseconds -- comparable
/// to `Instant::now()`'s own call overhead and OS scheduler jitter, per
/// E1's own documented reasoning for its batching discipline
/// (`crates/issue38-eval/src/bin/e1_hardcoded_vs_compiled_vs_generic_vs_solr.rs`).
/// A *single* 200-call batch was observed to occasionally report a
/// *negative* overhead for Treatment B relative to A despite B provably
/// doing strictly more work (two real `execute_planned` calls, not one)
/// -- a clear signal the single-batch measurement floor had been
/// reached, not a real speed advantage. Taking the median of several
/// independent batches is a real, disclosed correction to this
/// experiment's own methodology, made before any GO/REVISE verdict was
/// recorded from it (Issue #42 rule 9's "no silent replacement" does not
/// apply retroactively here since no verdict had been finalized yet).
const LATENCY_TRIALS: usize = 7;
const BASELINE_SHA: &str = "fe2e52e0fe872a0f4ab86c63ccc839e61de8f3e6";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Treatment {
    A,
    B,
    C,
    D,
    /// Issue #51: Treatment D's own decision logic, with the query-time
    /// catalog scan replaced by a precomputed registry lookup.
    E,
}

impl Treatment {
    const ALL: [Treatment; 5] = [
        Treatment::A,
        Treatment::B,
        Treatment::C,
        Treatment::D,
        Treatment::E,
    ];
    fn label(self) -> &'static str {
        match self {
            Treatment::A => "A",
            Treatment::B => "B",
            Treatment::C => "C",
            Treatment::D => "D",
            Treatment::E => "E",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum RowClass {
    AmbiguousUncorroborated,
    Corroborated,
    PriceRegressionGuard,
    IdentifierRegressionGuard,
    /// Row 9: "no valid numeric OR enum interpretation exists for
    /// 'purple' as a size; must resolve to zero hard constraints /
    /// residual text only" -- checked via
    /// `negative_row_has_zero_size_hard_constraints`.
    NegativeZeroSizeConstraint,
    /// Row 10: "outside any real generated size range in either family;
    /// must not match anything, must not error" -- a weaker requirement
    /// than row 9's (a harmless Numeric(size=999999) hard constraint is
    /// fine; it simply matches nothing), checked via hit count alone.
    NegativeZeroHits,
}

struct Row {
    id: usize,
    text: &'static str,
    class: RowClass,
    /// The single (ProductId, VariantId) the oracle confirms is the
    /// correct relevant match for a *resolved* query, if this row has one
    /// (corroborated/ambiguous rows do; regression-guard/negative rows
    /// have no size-typed relevant target at all).
    relevant: Option<(ProductId, VariantId)>,
}

fn is_size_numeric_hard(c: &ResolvedConstraint, value: f64) -> bool {
    matches!(
        c,
        ResolvedConstraint::Attribute(Constraint::Numeric { attribute, op: NumericOp::Eq, value: v })
            if attribute == "size" && (*v - value).abs() < 1e-9
    )
}

fn is_size_enum_hard(c: &ResolvedConstraint, value: &str) -> bool {
    matches!(
        c,
        ResolvedConstraint::Attribute(Constraint::Enum { attribute, value: v })
            if attribute == "size" && v == value
    )
}

fn has_size_preference(resolution: &Resolution, value_str: &str) -> bool {
    resolution.queries.iter().any(|q| {
        q.preferences.iter().any(|p| {
            matches!(p, Preference::Boost { attribute, value, .. } if attribute == "size" && value == value_str)
        })
    })
}

/// Row 1's GO-gate check: a treatment must not silently pick one family
/// as the sole hard filter. PASS if both interpretations survive as hard
/// constraints (a real OR), if neither does (both demoted), or if exactly
/// one is hard and the other remains reachable as a preference.
fn row1_does_not_silently_pick_one_family(
    resolution: &Resolution,
    numeric_value: f64,
    enum_value: &str,
) -> bool {
    let numeric_hard = resolution.queries.iter().any(|q| {
        q.constraints
            .iter()
            .any(|c| is_size_numeric_hard(c, numeric_value))
    });
    let enum_hard = resolution.queries.iter().any(|q| {
        q.constraints
            .iter()
            .any(|c| is_size_enum_hard(c, enum_value))
    });
    match (numeric_hard, enum_hard) {
        (true, true) => true,
        (false, false) => true,
        (true, false) => has_size_preference(resolution, enum_value),
        (false, true) => has_size_preference(resolution, &numeric_value.to_string()),
    }
}

/// Rows 9/10's GO-gate check: zero hard constraints from the
/// ambiguous-numeric mechanism specifically (an unrelated structural
/// constraint from a different mechanism is not a violation -- neither
/// row produces one here).
fn negative_row_has_zero_size_hard_constraints(resolution: &Resolution) -> bool {
    resolution.queries.iter().all(|q| {
        !q.constraints.iter().any(|c| {
            matches!(c, ResolvedConstraint::Attribute(Constraint::Numeric { attribute, .. }) if attribute == "size")
                || matches!(c, ResolvedConstraint::Attribute(Constraint::Enum { attribute, .. }) if attribute == "size")
        })
    })
}

fn ndcg_at_k(hits: &[PlannedHit], relevant: &BTreeSet<(ProductId, VariantId)>, k: usize) -> f64 {
    if relevant.is_empty() {
        return 0.0;
    }
    let dcg: f64 = hits
        .iter()
        .take(k)
        .enumerate()
        .map(|(i, h)| {
            let gain = if relevant.contains(&(h.product, h.variant)) {
                1.0
            } else {
                0.0
            };
            gain / (i as f64 + 2.0).log2()
        })
        .sum();
    // Ideal: every relevant item (gain 1.0) ranked first, up to k.
    let idcg: f64 = (0..relevant.len().min(k))
        .map(|i| 1.0 / (i as f64 + 2.0).log2())
        .sum();
    if idcg > 0.0 {
        dcg / idcg
    } else {
        0.0
    }
}

/// Bundles the fixed per-run context (built once in `main`, never
/// mutated) that every treatment's resolution/execution needs -- keeps
/// `one_latency_trial` and similar functions under clippy's argument-count
/// lint without resorting to `#[allow]`.
struct EvalContext<'a> {
    lexicon: &'a SemanticLexicon,
    catalog: &'a Catalog,
    index: &'a CatalogIndex,
    delegate: &'a BitmapTantivyDelegate,
    policy: &'a PlannerPolicy,
    registry: &'a AttributeKindRegistry,
}

/// Sums one batched, `black_box`-guarded latency trial across every row
/// for `treatment` -- the total per-call cost of running the whole R1
/// workload once under this treatment.
fn one_latency_trial(treatment: Treatment, rows: &[Row], ctx: &EvalContext) -> f64 {
    let mut total_ms = 0.0;
    for row in rows {
        let resolution = resolve_for(treatment, row.text, ctx.lexicon, ctx.catalog, ctx.registry);
        let t0 = std::time::Instant::now();
        for _ in 0..LATENCY_BATCH {
            let (_planned, hits) = std::hint::black_box(run_treatment(
                std::hint::black_box(&resolution),
                ctx.catalog,
                ctx.index,
                Some(ctx.delegate as &dyn LexicalDelegate),
                K,
                ctx.policy,
            ));
            std::hint::black_box(hits.len());
        }
        total_ms += t0.elapsed().as_secs_f64() * 1000.0 / LATENCY_BATCH as f64;
    }
    total_ms
}

fn median(mut values: Vec<f64>) -> f64 {
    values.sort_by(|a, b| a.total_cmp(b));
    values[values.len() / 2]
}

fn resolve_for(
    treatment: Treatment,
    text: &str,
    lexicon: &SemanticLexicon,
    catalog: &Catalog,
    registry: &AttributeKindRegistry,
) -> Resolution {
    match treatment {
        Treatment::A => resolve_a(text, lexicon),
        Treatment::B => resolve_b(text, lexicon),
        Treatment::C => resolve_c(text, lexicon),
        Treatment::D => resolve_d(text, lexicon, catalog),
        Treatment::E => resolve_e(text, lexicon, registry),
    }
}

fn main() {
    println!("=== Issue #42 R1: typed ambiguity and corroborated resolution ===");
    println!("baseline_sha: {BASELINE_SHA}");

    let fixture = build_typed_ambiguity_catalog();
    let catalog = &fixture.catalog;
    let index = CatalogIndex::build(catalog);
    let profile = CatalogProfile::build(
        catalog,
        &fixture.brands,
        &fixture.product_types,
        &fixture.categories,
    );
    let lexicon = compile_lexicon(&profile, MIN_ENUM_FREQUENCY);
    let built = build_index(catalog).expect("in-memory tantivy index build");
    let delegate = BitmapTantivyDelegate::new(
        &built.index,
        vec![built.title_field, built.description_field],
    )
    .expect("tantivy delegate build");
    let policy = PlannerPolicy {
        selectivity_threshold: 0.05,
        delegate_oversample: 20,
    };

    // Issue #51: Treatment E's registry, built once here -- the
    // ingestion/compile-time step -- and timed separately from the
    // per-query serving-overhead measurement the <=5% gate applies to
    // (disclosed, not hidden, even though it does not count against it).
    let registry_build_start = std::time::Instant::now();
    let registry = build_attribute_kind_registry(catalog);
    let registry_build_ms = registry_build_start.elapsed().as_secs_f64() * 1000.0;
    println!(
        "Treatment E registry build (one-time, ingestion/compile-time, not part of the \
         query-time overhead gate): {registry_build_ms:.5}ms for {} catalog products",
        catalog.products.len()
    );

    // Every claimed positive below is independently re-verified against
    // the oracle at run time (not merely asserted here in prose) --
    // failure panics loudly rather than silently producing a bogus GO/
    // REVISE verdict from a case that doesn't actually exist.
    let jeans_intent = QueryIntent {
        product_type: Some(oracle::resolve_product_type(
            &fixture.product_types,
            "Jeans",
        )),
        attributes: vec![AttrRequirement::EnumEquals {
            attribute: "size".to_string(),
            value: AMBIGUOUS_SIZE_VALUE.to_string(),
        }],
    };
    issue42_eval::regression::assert_positive(catalog, &jeans_intent, "row 1/2 Jeans Enum size");
    let wiper_intent = QueryIntent {
        product_type: Some(oracle::resolve_product_type(
            &fixture.product_types,
            "Wiper Blades",
        )),
        attributes: vec![AttrRequirement::NumericEquals {
            attribute: "size".to_string(),
            value: AMBIGUOUS_SIZE_VALUE.parse().unwrap(),
            epsilon: 1e-9,
        }],
    };
    issue42_eval::regression::assert_positive(
        catalog,
        &wiper_intent,
        "row 1/3 Wiper Blades Numeric size",
    );
    let fitment_intent = QueryIntent {
        product_type: Some(oracle::resolve_product_type(
            &fixture.product_types,
            "Brake Pads",
        )),
        attributes: vec![AttrRequirement::MultiEnumContains {
            attribute: "compatible_fitment".to_string(),
            value: FITMENT_PHRASE.to_string(),
        }],
    };
    issue42_eval::regression::assert_positive(catalog, &fitment_intent, "row 6 fitment MultiEnum");

    let numeric_value: f64 = AMBIGUOUS_SIZE_VALUE.parse().unwrap();
    let rows = [
        Row {
            id: 1,
            text: "size 22",
            class: RowClass::AmbiguousUncorroborated,
            relevant: None, // ambiguous: no single correct target family
        },
        Row {
            id: 2,
            text: "size 22 jeans",
            class: RowClass::Corroborated,
            relevant: Some(fixture.jeans_variant),
        },
        Row {
            id: 3,
            text: "size 22 wiper blades",
            class: RowClass::Corroborated,
            relevant: Some(fixture.wiper_variant),
        },
        Row {
            id: 4,
            text: "under $34",
            class: RowClass::PriceRegressionGuard,
            relevant: None,
        },
        Row {
            id: 5,
            text: "over $34",
            class: RowClass::PriceRegressionGuard,
            relevant: None,
        },
        Row {
            id: 6,
            text: "2015 honda civic brake pads",
            class: RowClass::Corroborated,
            relevant: Some(fixture.brake_pads_variant),
        },
        Row {
            id: 7,
            text: "part number IA-1234-BP",
            class: RowClass::IdentifierRegressionGuard,
            relevant: Some(fixture.identifier_variant),
        },
        Row {
            id: 9,
            text: "size purple",
            class: RowClass::NegativeZeroSizeConstraint,
            relevant: None,
        },
        Row {
            id: 10,
            text: "size 999999",
            class: RowClass::NegativeZeroHits,
            relevant: None,
        },
    ];
    assert_eq!(IDENTIFIER_VALUE, "IA-1234-BP");

    let mut wrong_family_false_positives: std::collections::BTreeMap<&'static str, usize> =
        std::collections::BTreeMap::new();
    let mut row1_violations: Vec<&'static str> = Vec::new();
    let mut negative_violations: Vec<(usize, &'static str)> = Vec::new();
    let mut corroborated_ndcgs: std::collections::BTreeMap<&'static str, Vec<f64>> =
        std::collections::BTreeMap::new();
    let mut latency_ms: std::collections::BTreeMap<&'static str, f64> =
        std::collections::BTreeMap::new();

    for treatment in Treatment::ALL {
        println!("\n--- Treatment {} ---", treatment.label());
        for row in &rows {
            let resolution = resolve_for(treatment, row.text, &lexicon, catalog, &registry);
            let (planned, hits) = run_treatment(
                &resolution,
                catalog,
                &index,
                Some(&delegate as &dyn LexicalDelegate),
                K,
                &policy,
            );

            let outcomes: Vec<&str> = planned
                .iter()
                .map(|p| match p.outcome {
                    ExecutionOutcome::FastPath => "fast_path",
                    ExecutionOutcome::Hybrid => "hybrid",
                    ExecutionOutcome::Punt => "punt",
                })
                .collect();
            println!(
                "row {:>2} ({:>20}): {} sub-queries, outcomes={:?}, hits={}",
                row.id,
                row.text,
                resolution.queries.len(),
                outcomes,
                hits.len(),
            );

            // Wrong-family false-positive check: any hit that is neither
            // the row's own confirmed-relevant target nor (for the
            // ambiguous row) either real interpretation's own product is
            // a wrong-family false positive.
            let allowed: BTreeSet<(ProductId, VariantId)> = match row.class {
                RowClass::AmbiguousUncorroborated => {
                    BTreeSet::from([fixture.jeans_variant, fixture.wiper_variant])
                }
                _ => row.relevant.into_iter().collect(),
            };
            for hit in &hits {
                let key = (hit.product, hit.variant);
                if !allowed.is_empty() && !allowed.contains(&key) {
                    *wrong_family_false_positives
                        .entry(treatment.label())
                        .or_insert(0) += 1;
                    println!("  WRONG-FAMILY FALSE POSITIVE: {key:?}");
                }
            }

            match row.class {
                RowClass::AmbiguousUncorroborated => {
                    if !row1_does_not_silently_pick_one_family(
                        &resolution,
                        numeric_value,
                        AMBIGUOUS_SIZE_VALUE,
                    ) {
                        row1_violations.push(treatment.label());
                    }
                }
                RowClass::Corroborated => {
                    let relevant: BTreeSet<_> = row.relevant.into_iter().collect();
                    let ndcg = ndcg_at_k(&hits, &relevant, K);
                    corroborated_ndcgs
                        .entry(treatment.label())
                        .or_default()
                        .push(ndcg);
                }
                RowClass::NegativeZeroSizeConstraint => {
                    if !negative_row_has_zero_size_hard_constraints(&resolution) {
                        negative_violations.push((row.id, treatment.label()));
                    }
                }
                RowClass::NegativeZeroHits => {
                    if !hits.is_empty() {
                        negative_violations.push((row.id, treatment.label()));
                    }
                }
                RowClass::PriceRegressionGuard | RowClass::IdentifierRegressionGuard => {}
            }
        }
    }

    // Diagnostic added after an adversarial review of R1's results found
    // that Treatment C's poor corroborated-row NDCG is confounded with
    // execute_planned's strict residual-lexical veto (the same mechanism
    // R2 exists to address): resolve_c pushes the demoted token into
    // residual_lexical, which downgrades an otherwise-FastPath-eligible
    // query (a single ProductType entity constraint, already sufficient
    // to identify the correct product on its own) to Hybrid/Punt, where
    // the delegate finds nothing and the structural narrowing's own
    // result is discarded. This is NOT part of Treatment C's own
    // preregistered GO-gate measurement above -- it is a side diagnostic
    // isolating how much of the C-vs-D NDCG gap is attributable to that
    // confound rather than to C's own defining property (never using
    // corroborating context to select an interpretation).
    println!("\n--- diagnostic (NOT part of the preregistered GO gate): isolated C without the residual_lexical push ---");
    let mut isolated_c_ndcgs: Vec<f64> = Vec::new();
    for row in &rows {
        if row.class != RowClass::Corroborated {
            continue;
        }
        let resolution = resolve_c_isolated_no_residual_push(row.text, &lexicon);
        let (planned, hits) = run_treatment(
            &resolution,
            catalog,
            &index,
            Some(&delegate as &dyn LexicalDelegate),
            K,
            &policy,
        );
        let outcomes: Vec<&str> = planned
            .iter()
            .map(|p| match p.outcome {
                ExecutionOutcome::FastPath => "fast_path",
                ExecutionOutcome::Hybrid => "hybrid",
                ExecutionOutcome::Punt => "punt",
            })
            .collect();
        let relevant: BTreeSet<_> = row.relevant.into_iter().collect();
        let ndcg = ndcg_at_k(&hits, &relevant, K);
        isolated_c_ndcgs.push(ndcg);
        println!(
            "row {:>2} ({:>20}): outcomes={:?}, hits={}, ndcg={:.4}",
            row.id,
            row.text,
            outcomes,
            hits.len(),
            ndcg
        );
    }
    let isolated_c_mean = isolated_c_ndcgs.iter().sum::<f64>() / isolated_c_ndcgs.len() as f64;
    let c_mean = corroborated_ndcgs
        .get(Treatment::C.label())
        .map(|v| v.iter().sum::<f64>() / v.len() as f64)
        .unwrap_or(0.0);
    println!(
        "isolated-C mean corroborated NDCG@10 = {isolated_c_mean:.4} vs. Treatment C's own \
         measured {c_mean:.4} -- the gap between these two numbers is attributable to the \
         residual-lexical-veto confound, not to C's own corroboration-blindness"
    );

    println!("\n--- latency (median of {LATENCY_TRIALS} independent batched trials) ---");
    let ctx = EvalContext {
        lexicon: &lexicon,
        catalog,
        index: &index,
        delegate: &delegate,
        policy: &policy,
        registry: &registry,
    };
    for treatment in Treatment::ALL {
        let trials: Vec<f64> = (0..LATENCY_TRIALS)
            .map(|_| one_latency_trial(treatment, &rows, &ctx))
            .collect();
        let med = median(trials.clone());
        println!(
            "Treatment {}: trials={:?} median={:.5}ms",
            treatment.label(),
            trials.iter().map(|v| format!("{v:.5}")).collect::<Vec<_>>(),
            med
        );
        latency_ms.insert(treatment.label(), med);
    }

    println!("\n--- GO gate evaluation ---");
    println!(
        "wrong-family false positives, per treatment: {:?} (require: 0 for that treatment)",
        wrong_family_false_positives
    );
    println!("row 1 silent-single-family violations: {row1_violations:?} (require: empty)");
    println!("negative-row hard-constraint violations: {negative_violations:?} (require: empty)");

    let baseline_latency = latency_ms[Treatment::A.label()];
    for treatment in Treatment::ALL {
        let ndcgs = corroborated_ndcgs
            .get(treatment.label())
            .cloned()
            .unwrap_or_default();
        let mean_ndcg = if ndcgs.is_empty() {
            0.0
        } else {
            ndcgs.iter().sum::<f64>() / ndcgs.len() as f64
        };
        let overhead_pct =
            100.0 * (latency_ms[treatment.label()] - baseline_latency) / baseline_latency;
        let own_wrong_family_fps = *wrong_family_false_positives
            .get(treatment.label())
            .unwrap_or(&0);
        let row1_ok = !row1_violations.contains(&treatment.label());
        let negatives_ok = !negative_violations
            .iter()
            .any(|(_, t)| *t == treatment.label());
        let go = own_wrong_family_fps == 0
            && mean_ndcg >= 0.95
            && row1_ok
            && negatives_ok
            && (treatment == Treatment::A || overhead_pct <= 5.0);
        println!(
            "Treatment {}: corroborated mean NDCG@10={mean_ndcg:.4} (require >=0.95), \
             latency overhead vs A={overhead_pct:.1}% (require <=5% except for A itself), \
             wrong_family_fps={own_wrong_family_fps} (require 0), row1_ok={row1_ok}, \
             negatives_ok={negatives_ok} => GO gate {}",
            treatment.label(),
            if go { "PASS" } else { "FAIL" }
        );
    }

    println!(
        "\nNote: wrong-family false-positive count is measured against this experiment's \
         small (4-product, 1-candidate-per-family) fixture -- Constraint::matches's own \
         AttributeValue-kind catch-all (commerce_core/src/domain/constraint.rs) structurally \
         guarantees a type-mismatched constraint can never match, so this fixture cannot by \
         itself stress-test a *larger*-catalog coincidental cross-family collision; that \
         remains a disclosed limitation, not fabricated as already covered."
    );

    let summary = serde_json::json!({
        "experiment_id": "I42-R1",
        "baseline_sha": BASELINE_SHA,
        "wrong_family_false_positives": wrong_family_false_positives,
        "row1_violations": row1_violations,
        "negative_violations": negative_violations.iter().map(|(r,t)| format!("row{r}/{t}")).collect::<Vec<_>>(),
        "corroborated_ndcg_mean": corroborated_ndcgs.iter().map(|(t, v)| {
            (t.to_string(), v.iter().sum::<f64>() / v.len() as f64)
        }).collect::<std::collections::BTreeMap<_,_>>(),
        "latency_ms_per_call": latency_ms,
    });
    println!("\n{}", serde_json::to_string_pretty(&summary).unwrap());

    if let Some(path) = env::args().nth(1) {
        fs::write(&path, serde_json::to_string_pretty(&summary).unwrap())
            .expect("write summary json");
        println!("summary written to {path}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use commerce_core::domain::ProductId as PId;
    use commerce_core::domain::VariantId as VId;
    use commerce_core::ir::CommerceQuery;

    fn query_with_constraints(constraints: Vec<ResolvedConstraint>) -> CommerceQuery {
        CommerceQuery {
            constraints,
            preferences: vec![],
            ambiguous: vec![],
            residual_lexical: vec![],
        }
    }

    #[test]
    fn median_of_odd_count_is_the_middle_value() {
        assert_eq!(median(vec![3.0, 1.0, 2.0]), 2.0);
        assert_eq!(median(vec![5.0]), 5.0);
    }

    #[test]
    fn ndcg_at_k_is_one_for_a_perfect_top_rank_and_zero_for_no_relevant_items() {
        let relevant = BTreeSet::from([(PId(1), VId(1))]);
        let perfect = vec![PlannedHit {
            product: PId(1),
            variant: VId(1),
            score: 1.0,
        }];
        assert!((ndcg_at_k(&perfect, &relevant, 10) - 1.0).abs() < 1e-9);

        let miss = vec![PlannedHit {
            product: PId(2),
            variant: VId(2),
            score: 1.0,
        }];
        assert_eq!(ndcg_at_k(&miss, &relevant, 10), 0.0);

        assert_eq!(ndcg_at_k(&[], &BTreeSet::new(), 10), 0.0);
    }

    #[test]
    fn ndcg_at_k_ranks_a_later_hit_lower_than_an_earlier_one() {
        let relevant = BTreeSet::from([(PId(1), VId(1))]);
        let early = vec![
            PlannedHit {
                product: PId(1),
                variant: VId(1),
                score: 1.0,
            },
            PlannedHit {
                product: PId(2),
                variant: VId(2),
                score: 1.0,
            },
        ];
        let late = vec![
            PlannedHit {
                product: PId(2),
                variant: VId(2),
                score: 1.0,
            },
            PlannedHit {
                product: PId(1),
                variant: VId(1),
                score: 1.0,
            },
        ];
        assert!(ndcg_at_k(&early, &relevant, 10) > ndcg_at_k(&late, &relevant, 10));
    }

    #[test]
    fn row1_check_passes_when_both_interpretations_are_hard_constraints() {
        let resolution = Resolution {
            queries: vec![
                query_with_constraints(vec![ResolvedConstraint::Attribute(Constraint::Numeric {
                    attribute: "size".to_string(),
                    op: NumericOp::Eq,
                    value: 22.0,
                })]),
                query_with_constraints(vec![ResolvedConstraint::Attribute(Constraint::Enum {
                    attribute: "size".to_string(),
                    value: "22".to_string(),
                })]),
            ],
        };
        assert!(row1_does_not_silently_pick_one_family(
            &resolution,
            22.0,
            "22"
        ));
    }

    #[test]
    fn row1_check_passes_when_neither_interpretation_is_hard() {
        let resolution = Resolution {
            queries: vec![query_with_constraints(vec![])],
        };
        assert!(row1_does_not_silently_pick_one_family(
            &resolution,
            22.0,
            "22"
        ));
    }

    #[test]
    fn row1_check_fails_when_exactly_one_interpretation_is_hard_with_no_fallback_preference() {
        let resolution = Resolution {
            queries: vec![query_with_constraints(vec![ResolvedConstraint::Attribute(
                Constraint::Numeric {
                    attribute: "size".to_string(),
                    op: NumericOp::Eq,
                    value: 22.0,
                },
            )])],
        };
        assert!(!row1_does_not_silently_pick_one_family(
            &resolution,
            22.0,
            "22"
        ));
    }

    #[test]
    fn row1_check_passes_when_one_is_hard_and_the_other_survives_as_a_preference() {
        let mut q =
            query_with_constraints(vec![ResolvedConstraint::Attribute(Constraint::Numeric {
                attribute: "size".to_string(),
                op: NumericOp::Eq,
                value: 22.0,
            })]);
        q.preferences.push(Preference::Boost {
            attribute: "size".to_string(),
            value: "22".to_string(),
            weight: 0.5,
        });
        let resolution = Resolution { queries: vec![q] };
        assert!(row1_does_not_silently_pick_one_family(
            &resolution,
            22.0,
            "22"
        ));
    }

    /// Symmetric counterpart of the test above: the Enum interpretation
    /// kept hard, the Numeric one surviving only as a preference. An
    /// adversarial review found this (false, true) branch of
    /// `row1_does_not_silently_pick_one_family` had no dedicated test --
    /// no treatment in the actual R1 run exercises it (only Treatment A
    /// ever keeps a numeric-only hard constraint for row 1), but the
    /// branch itself must still be independently verified, not left
    /// untested merely because it happens not to fire on this fixture.
    #[test]
    fn row1_check_passes_when_enum_is_hard_and_numeric_survives_as_a_preference() {
        let mut q = query_with_constraints(vec![ResolvedConstraint::Attribute(Constraint::Enum {
            attribute: "size".to_string(),
            value: "22".to_string(),
        })]);
        q.preferences.push(Preference::Boost {
            attribute: "size".to_string(),
            value: "22".to_string(),
            weight: 0.5,
        });
        let resolution = Resolution { queries: vec![q] };
        assert!(row1_does_not_silently_pick_one_family(
            &resolution,
            22.0,
            "22"
        ));
    }

    #[test]
    fn negative_zero_size_constraint_check_detects_a_surviving_size_constraint() {
        let clean = Resolution {
            queries: vec![query_with_constraints(vec![])],
        };
        assert!(negative_row_has_zero_size_hard_constraints(&clean));

        let violating = Resolution {
            queries: vec![query_with_constraints(vec![ResolvedConstraint::Attribute(
                Constraint::Numeric {
                    attribute: "size".to_string(),
                    op: NumericOp::Eq,
                    value: 999999.0,
                },
            )])],
        };
        assert!(!negative_row_has_zero_size_hard_constraints(&violating));
    }
}
