//! Issue #51's own named next step
//! (`docs/decisions/ISSUE51_DECISION.md`): "rerun R1's *full* gate (not
//! just the isolated corroboration-decision cost measured here) against
//! a catalog of realistic size." Protocol:
//! `docs/experiments/ISSUE51_FULLGATE_SCALE_PROTOCOL.md`.
//!
//! Reruns R1/Issue #42's own preregistered GO gate (correctness +
//! `<=5%` `execute_planned` latency overhead vs. Treatment A) with the
//! exact same 9 query rows, fixture product IDs, and measurement
//! methodology as `r1_typed_ambiguity_eval.rs`, but against the same
//! fixture scaled up with harmless decoy products (reusing
//! `i51_e00_catalog_scale_diagnostic.rs`'s own decoy-construction logic)
//! to approximate this project's real WANDS catalog scale (~43,000
//! products) instead of R1's frozen 5-product fixture. Deliberately a
//! separate binary from `r1_typed_ambiguity_eval.rs` (unlike Issue #51's
//! own Treatment E, which was added to the *same* binary to keep a
//! same-run comparison) since this is not a same-run A/B against the
//! original: it is the *same* preregistered gate rerun once at a
//! different, fixed scale -- there is nothing to compare against within
//! this run itself, only against R1's own already-published N=5 numbers.
//!
//! Reproduction: `cargo build --release -p issue42-eval &&
//! ./target/release/r1_full_gate_scale_rerun`

use std::collections::BTreeSet;

use commerce_core::cold_start::{compile_lexicon, CatalogProfile};
use commerce_core::domain::{
    attributes, Catalog, Constraint, Inventory, NumericOp, Price, Product, ProductId,
    ProductTypeId, Variant, VariantId,
};
use commerce_core::index::CatalogIndex;
use commerce_core::ir::{Preference, ResolvedConstraint, SemanticLexicon};
use commerce_core::plan::{ExecutionOutcome, LexicalDelegate, PlannedHit, PlannerPolicy};
use issue42_eval::oracle::{self, AttrRequirement, QueryIntent};
use issue42_eval::r1_experimental::{
    build_attribute_kind_registry, resolve_a, resolve_b, resolve_c, resolve_d, resolve_e,
    run_treatment, AttributeKindRegistry, Resolution,
};
use issue42_eval::r1_workload::{
    build_typed_ambiguity_catalog, AMBIGUOUS_SIZE_VALUE, FITMENT_PHRASE, IDENTIFIER_VALUE,
};
use phase9_eval::bitmap_delegate::{build_index, BitmapTantivyDelegate};

const K: usize = 10;
const MIN_ENUM_FREQUENCY: usize = 1;
const LATENCY_BATCH: usize = 200;
const LATENCY_TRIALS: usize = 7;
/// Chosen to approximate this project's real WANDS catalog scale
/// (42,994 products) rather than an arbitrary round number: 5 real
/// fixture products + 42,990 inert decoys = 42,995.
const DECOY_COUNT: usize = 42_990;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Treatment {
    A,
    B,
    C,
    D,
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
    NegativeZeroSizeConstraint,
    NegativeZeroHits,
}

struct Row {
    id: usize,
    text: &'static str,
    class: RowClass,
    relevant: Option<(ProductId, VariantId)>,
}

/// **Not** `i51_e00_catalog_scale_diagnostic.rs`'s own `scaled_catalog`
/// -- that helper's decoys share product types 1/2/3 (Jeans/Wiper
/// Blades/Brake Pads) with a `"size"` attribute, which is harmless for
/// measuring `resolve_d`/`resolve_e` in isolation (as that diagnostic
/// does) but is **not** harmless once the full `execute_planned` gate
/// runs: a first attempt at this rerun using that exact decoy shape
/// found NDCG collapsing (1.0 -> 0.6667/0.3333) and routing outcomes
/// changing (row 3 flipping from `fast_path` to `punt`) -- caught before
/// trusting any number, not silently patched over. Root cause: (1)
/// `CatalogProfile::build` (`crates/commerce-core/src/cold_start/profile.rs:92-101`)
/// indexes attribute values from **every** product regardless of
/// product type, so 14,330-per-type decoy `"size"` enum values pollute
/// the very lexicon vocabulary the real rows' "22"/corroboration
/// resolution depends on; (2) `constraint_kind_registered_on_product_type`
/// (`crates/issue42-eval/src/r1_experimental.rs:141-144`) filters by
/// product type but still touches every catalog product to do so, so
/// sharing product types 1/2/3 was never necessary for the scan-cost
/// effect this rerun needs -- only *existing* as products was. Decoys
/// here instead use a distinct, unregistered product type/brand/category
/// (`9999`, absent from `fixture.product_types`/`brands`/`categories`,
/// so unreachable by name via the lexicon) and **zero attributes** at
/// both product and variant level, so nothing about them can be indexed
/// into the profile or matched by any of the 9 real rows' constraints --
/// while still inflating `catalog.products.len()`, which is all
/// `constraint_kind_registered_on_product_type`'s own `.iter()` scan (and
/// `CatalogIndex`/lexicon/Tantivy build) actually costs against.
const DECOY_PRODUCT_TYPE: u32 = 9999;
const DECOY_BRAND: u32 = 9999;
const DECOY_CATEGORY: u32 = 9999;

fn scaled_catalog(base: &Catalog, decoy_count: usize) -> Catalog {
    let mut products = base.products.clone();
    for next_id in (1_000_000u64..).take(decoy_count) {
        let decoy = Product {
            id: ProductId(next_id),
            product_type: ProductTypeId(DECOY_PRODUCT_TYPE),
            brand: commerce_core::domain::BrandId(DECOY_BRAND),
            category: commerce_core::domain::CategoryId(DECOY_CATEGORY),
            title: format!("Decoy product {next_id}"),
            attributes: attributes([]),
            variants: vec![Variant {
                id: VariantId(next_id * 10),
                attributes: attributes([]),
                // Exactly $34.00 -- rows 4/5 ("under $34"/"over $34") use
                // strict `<`/`>` (`crates/commerce-core/src/ir/structural.rs:36-37`),
                // so a decoy priced exactly at the threshold matches
                // neither, unlike an earlier attempt ($10.00) that made
                // "under $34" an accidental full-catalog scan dominating
                // ~97% of total measured latency and drowning out the
                // rows this experiment actually needs to distinguish
                // between treatments on -- caught via a per-row latency
                // breakdown before trusting the aggregate number.
                price: Price::usd(3_400),
                inventory: Inventory::in_stock(1),
            }],
        };
        products.push(decoy);
    }
    Catalog { products }
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
    let idcg: f64 = (0..relevant.len().min(k))
        .map(|i| 1.0 / (i as f64 + 2.0).log2())
        .sum();
    if idcg > 0.0 {
        dcg / idcg
    } else {
        0.0
    }
}

struct EvalContext<'a> {
    lexicon: &'a SemanticLexicon,
    catalog: &'a Catalog,
    index: &'a CatalogIndex,
    delegate: &'a BitmapTantivyDelegate,
    policy: &'a PlannerPolicy,
    registry: &'a AttributeKindRegistry,
}

/// Per-row breakdown, added while investigating a surprising result
/// (Treatments D/E measuring *faster* than baseline Treatment A at
/// realistic scale) -- not part of the preregistered gate itself, but
/// needed to honestly explain rather than merely report that direction
/// before trusting it.
fn one_latency_trial_per_row(treatment: Treatment, rows: &[Row], ctx: &EvalContext) -> Vec<f64> {
    rows.iter()
        .map(|row| {
            let resolution =
                resolve_for(treatment, row.text, ctx.lexicon, ctx.catalog, ctx.registry);
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
            t0.elapsed().as_secs_f64() * 1000.0 / LATENCY_BATCH as f64
        })
        .collect()
}

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
    println!("=== Issue #51 full-gate rerun at realistic catalog scale ===");

    let fixture = build_typed_ambiguity_catalog();
    let catalog = scaled_catalog(&fixture.catalog, DECOY_COUNT);
    println!(
        "catalog: {} products ({} real fixture products + {} inert decoys, \
         approximating this project's real WANDS scale of 42,994)",
        catalog.products.len(),
        fixture.catalog.products.len(),
        DECOY_COUNT
    );

    let index = CatalogIndex::build(&catalog);
    let profile = CatalogProfile::build(
        &catalog,
        &fixture.brands,
        &fixture.product_types,
        &fixture.categories,
    );
    let lexicon = compile_lexicon(&profile, MIN_ENUM_FREQUENCY);
    let built_index_start = std::time::Instant::now();
    let built = build_index(&catalog).expect("in-memory tantivy index build");
    let built_index_ms = built_index_start.elapsed().as_secs_f64() * 1000.0;
    let delegate = BitmapTantivyDelegate::new(
        &built.index,
        vec![built.title_field, built.description_field],
    )
    .expect("tantivy delegate build");
    let policy = PlannerPolicy {
        selectivity_threshold: 0.05,
        delegate_oversample: 20,
    };

    let registry_build_start = std::time::Instant::now();
    let registry = build_attribute_kind_registry(&catalog);
    let registry_build_ms = registry_build_start.elapsed().as_secs_f64() * 1000.0;
    println!(
        "one-time ingestion/compile-time setup (not part of the query-time overhead gate): \
         CatalogIndex+Tantivy build={built_index_ms:.2}ms, Treatment E registry build={registry_build_ms:.5}ms"
    );

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
    issue42_eval::regression::assert_positive(&catalog, &jeans_intent, "row 1/2 Jeans Enum size");
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
        &catalog,
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
    issue42_eval::regression::assert_positive(&catalog, &fitment_intent, "row 6 fitment MultiEnum");

    let numeric_value: f64 = AMBIGUOUS_SIZE_VALUE.parse().unwrap();
    let rows = [
        Row {
            id: 1,
            text: "size 22",
            class: RowClass::AmbiguousUncorroborated,
            relevant: None,
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
            let resolution = resolve_for(treatment, row.text, &lexicon, &catalog, &registry);
            let (planned, hits) = run_treatment(
                &resolution,
                &catalog,
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
            let candidate_counts: Vec<u64> = resolution
                .queries
                .iter()
                .map(|q| index.indexed_candidates(&q.constraints).len())
                .collect();
            println!(
                "row {:>2} ({:>28}): {} sub-queries, outcomes={:?}, hits={}, candidates={:?}, \
                 constraints={:?}, preferences_len={:?}",
                row.id,
                row.text,
                resolution.queries.len(),
                outcomes,
                hits.len(),
                candidate_counts,
                resolution
                    .queries
                    .iter()
                    .map(|q| q.constraints.len())
                    .collect::<Vec<_>>(),
                resolution
                    .queries
                    .iter()
                    .map(|q| q.preferences.len())
                    .collect::<Vec<_>>(),
            );

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

    println!(
        "\n--- per-row latency breakdown (single trial, diagnostic only -- explains, does not \
         replace, the median-of-{LATENCY_TRIALS}-trials gate numbers below) ---"
    );
    let diag_ctx = EvalContext {
        lexicon: &lexicon,
        catalog: &catalog,
        index: &index,
        delegate: &delegate,
        policy: &policy,
        registry: &registry,
    };
    for treatment in [Treatment::A, Treatment::D, Treatment::E] {
        let per_row = one_latency_trial_per_row(treatment, &rows, &diag_ctx);
        for (row, ms) in rows.iter().zip(per_row.iter()) {
            println!(
                "  Treatment {} row {:>2} ({:>28}): {ms:.5}ms",
                treatment.label(),
                row.id,
                row.text
            );
        }
        println!(
            "  Treatment {} row total: {:.5}ms",
            treatment.label(),
            per_row.iter().sum::<f64>()
        );
    }

    println!("\n--- latency (median of {LATENCY_TRIALS} independent batched trials) ---");
    let ctx = EvalContext {
        lexicon: &lexicon,
        catalog: &catalog,
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

    println!("\n--- GO gate evaluation (identical gate to R1/#51, realistic scale) ---");
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
}
