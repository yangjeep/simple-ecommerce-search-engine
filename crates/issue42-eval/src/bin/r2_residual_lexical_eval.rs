//! Issue #42 R2: residual lexical semantics
//! (`docs/experiments/ISSUE42_PROTOCOL.md`'s R2 section).
//!
//! Runs the preregistered 8-row workload through all four treatments
//! (`issue42_eval::r2_experimental`), checked against the independent
//! oracle for the entity-level positive claims rows 1/3/5/7 depend on
//! (`Sofas`/`Boots` alone really have real products in the materialized
//! catalog) and against `r2_workload`'s own direct catalog-content
//! invariant tests for the lexical-content claims (which words appear in
//! which titles) the oracle's attribute-based `QueryIntent` scheme cannot
//! express -- R2 is fundamentally about free-text residual words, not
//! modeled `AttributeValue`s, so a parallel, equally-independent
//! catalog-content assertion (already unit-tested in `r2_workload.rs`) is
//! the right instrument there, not a forced fit into `oracle::classify`.
//! No production code (`commerce_core::plan::plan`/`execute_planned`) is
//! modified; every treatment calls it, or the same public primitives it
//! calls internally, unmodified, via `r2_experimental`.
//!
//! Reproduction: `cargo build --release -p issue42-eval &&
//! ./target/release/r2_residual_lexical_eval [output_summary_json_path]`

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::env;
use std::fs;

use commerce_core::cold_start::{compile_lexicon, CatalogProfile};
use commerce_core::domain::{Catalog, ProductId, VariantId};
use commerce_core::index::CatalogIndex;
use commerce_core::ir::{compile, CommerceQuery, SemanticLexicon};
use commerce_core::plan::{plan, LexicalDelegate, PlannedHit, PlannedQuery, PlannerPolicy};
use issue42_eval::oracle::{self, QueryIntent};
use issue42_eval::r2_experimental::{execute_a, execute_b, execute_c, execute_d, ResidualPolicy};
use issue42_eval::r2_workload;
use phase9_eval::bitmap_delegate::{build_index, BitmapTantivyDelegate};

const K: usize = 10;
const MIN_ENUM_FREQUENCY: usize = 1;
const LATENCY_BATCH: usize = 200;
/// Same discipline as R1 (`r1_typed_ambiguity_eval.rs`): a single batched
/// trial can hit the measurement floor at this fixture's few-microsecond
/// absolute scale, so several independent trials are taken and the
/// median reported, not a single number.
const LATENCY_TRIALS: usize = 7;
const BASELINE_SHA: &str = "fe2e52e0fe872a0f4ab86c63ccc839e61de8f3e6";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Treatment {
    A,
    B,
    C,
    D,
}

impl Treatment {
    const ALL: [Treatment; 4] = [Treatment::A, Treatment::B, Treatment::C, Treatment::D];
    fn label(self) -> &'static str {
        match self {
            Treatment::A => "A",
            Treatment::B => "B",
            Treatment::C => "C",
            Treatment::D => "D",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum RowClass {
    /// Rows 1/3/5/7: a real, catalog-uninformative residual term paired
    /// with a real structural entity. GO-gate recovery denominator.
    Benign,
    /// Rows 2/6: a residual term that either never occurs anywhere
    /// (`banana`) or occurs only under a different, specific product
    /// type (`velvet`) -- must NOT be recovered. GO-gate false-recovery
    /// denominator.
    Adversarial,
    /// Row 4: a misspelling of a real registered lexicon value.
    /// Out-of-scope for R2's own GO gate by the protocol's own text
    /// ("misspelling correction is explicitly out of scope for R2 --
    /// measured, not required to pass") -- printed for every treatment,
    /// contributes to no metric.
    Measured,
    /// Row 8: a purely lexical query with no structural anchor at all
    /// (`query.constraints.is_empty()`). Every treatment's own code
    /// explicitly defers to `execute_a` (i.e. the real `execute_planned`)
    /// on this path without inspecting it further -- this row proves
    /// that untouched-ness empirically, rather than merely by source
    /// inspection.
    RegressionGuard,
}

struct Row {
    id: usize,
    text: &'static str,
    class: RowClass,
    /// The full set of (ProductId, VariantId) pairs a *correct* recovery
    /// would return -- empty for Adversarial/Measured/RegressionGuard
    /// rows, which have no correct nonempty answer.
    relevant: BTreeSet<(ProductId, VariantId)>,
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

fn run_treatment(
    treatment: Treatment,
    query: &CommerceQuery,
    catalog: &Catalog,
    index: &CatalogIndex,
    delegate: &BitmapTantivyDelegate,
    policy: &PlannerPolicy,
    residual_policy: &ResidualPolicy,
) -> (PlannedQuery, Vec<PlannedHit>) {
    let d: Option<&dyn LexicalDelegate> = Some(delegate);
    match treatment {
        Treatment::A => execute_a(query, catalog, index, d, K, policy),
        Treatment::B => execute_b(query, catalog, index, d, K, policy),
        Treatment::C => execute_c(query, catalog, index, d, K, policy),
        Treatment::D => execute_d(query, catalog, index, d, K, policy, residual_policy),
    }
}

/// Sums one batched, `black_box`-guarded latency trial across every row
/// for `treatment` -- the total per-call cost of running the whole R2
/// workload once under this treatment.
#[allow(clippy::too_many_arguments)]
fn one_latency_trial(
    treatment: Treatment,
    rows: &[Row],
    lexicon: &SemanticLexicon,
    catalog: &Catalog,
    index: &CatalogIndex,
    delegate: &BitmapTantivyDelegate,
    policy: &PlannerPolicy,
    residual_policy: &ResidualPolicy,
) -> f64 {
    let mut total_ms = 0.0;
    for row in rows {
        let query = compile(row.text, lexicon);
        let t0 = std::time::Instant::now();
        for _ in 0..LATENCY_BATCH {
            let (_planned, hits) = std::hint::black_box(run_treatment(
                treatment,
                std::hint::black_box(&query),
                catalog,
                index,
                delegate,
                policy,
                residual_policy,
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

fn main() {
    println!("=== Issue #42 R2: residual lexical semantics ===");
    println!("baseline_sha: {BASELINE_SHA}");

    let fixture = r2_workload::build();
    let catalog = &fixture.catalog;
    let index = CatalogIndex::build(catalog);
    let profile = CatalogProfile::build(
        catalog,
        &fixture.brands,
        &fixture.product_types,
        &fixture.categories,
    );
    let lexicon = compile_lexicon(&profile, MIN_ENUM_FREQUENCY);
    let residual_policy = ResidualPolicy::compile(catalog);
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

    // Every claimed positive below is independently re-verified against
    // the oracle at run time (not merely asserted here in prose): the
    // Sofas/Boots entities themselves must have real products in the
    // actually materialized catalog, which is what rows 1/3/5/7 rely on
    // "the entity alone" to recover.
    let sofas_intent = QueryIntent {
        product_type: Some(oracle::resolve_product_type(
            &fixture.product_types,
            "Sofas",
        )),
        attributes: vec![],
    };
    issue42_eval::regression::assert_positive(
        catalog,
        &sofas_intent,
        "Sofas entity alone (rows 1/2/5/8's backing product type)",
    );
    let boots_intent = QueryIntent {
        product_type: Some(oracle::resolve_product_type(
            &fixture.product_types,
            "Boots",
        )),
        attributes: vec![],
    };
    issue42_eval::regression::assert_positive(
        catalog,
        &boots_intent,
        "Boots entity alone (rows 3/6/7's backing product type)",
    );
    // The adversarial/lexical-content claims (rows 2/4/6/8) are about
    // free-text title words, not modeled AttributeValues -- outside
    // `oracle::classify`'s attribute-based scheme by construction. They
    // are independently checked by `r2_workload`'s own direct
    // catalog-content invariant tests instead (already run by `cargo
    // test -p issue42-eval`, not re-asserted here): `banana_appears_nowhere_at_all`,
    // `velvet_is_exclusively_a_sofas_title_word`, and
    // `no_test_residual_word_is_also_a_registered_enum_value`.

    let sofas_relevant: BTreeSet<(ProductId, VariantId)> =
        BTreeSet::from([(ProductId(1), VariantId(10)), (ProductId(2), VariantId(20))]);
    let boots_relevant: BTreeSet<(ProductId, VariantId)> =
        BTreeSet::from([(ProductId(3), VariantId(30)), (ProductId(4), VariantId(40))]);

    let rows = [
        Row {
            id: 1,
            text: "furniture sofas",
            class: RowClass::Benign,
            relevant: sofas_relevant.clone(),
        },
        Row {
            id: 2,
            text: "banana sofas",
            class: RowClass::Adversarial,
            relevant: BTreeSet::new(),
        },
        Row {
            id: 3,
            text: "waterproof boots",
            class: RowClass::Benign,
            relevant: boots_relevant.clone(),
        },
        Row {
            id: 4,
            text: "leathr sofas",
            class: RowClass::Measured,
            relevant: BTreeSet::new(),
        },
        Row {
            id: 5,
            text: "bestseller sofas",
            class: RowClass::Benign,
            relevant: sofas_relevant.clone(),
        },
        Row {
            id: 6,
            text: "velvet boots",
            class: RowClass::Adversarial,
            relevant: BTreeSet::new(),
        },
        Row {
            id: 7,
            text: "clearance boots",
            class: RowClass::Benign,
            relevant: boots_relevant.clone(),
        },
        Row {
            id: 8,
            text: "banana",
            class: RowClass::RegressionGuard,
            relevant: BTreeSet::new(),
        },
        // Second correction round (fresh adversarial review, before any
        // production change was made on the strength of this experiment's
        // GO verdict): rows 1-8 above only ever compile a single
        // ProductType structural constraint, so none of them could expose
        // a real defect the reviewer found -- ResidualPolicy::classify
        // only ever sees the bare ProductTypeId, never the query's actual
        // (possibly narrower) structural candidate set. "velvet blue
        // sofas" compiles a COMPOUND constraint (ProductType(Sofas) AND
        // Enum(color=Blue)); its real candidate set is {P1} only (P1 is
        // Blue, P2 -- the only product "velvet" actually describes -- is
        // Purple). Before the fix (see
        // ResidualPolicy::classify/r2_experimental.rs), "velvet" being
        // observed anywhere under Sofas classified it Preferred, so
        // Treatment D incorrectly recovered P1 (a Blue Leather Sofa,
        // not velvet at all) for this query. This row is Adversarial,
        // exactly like 2/6, and must recover nothing.
        Row {
            id: 9,
            text: "velvet blue sofas",
            class: RowClass::Adversarial,
            relevant: BTreeSet::new(),
        },
    ];

    println!(
        "\n--- compiled query diagnostics (same compile() output every treatment starts from) ---"
    );
    for row in &rows {
        let query = compile(row.text, &lexicon);
        let planned = plan(&query, &index, catalog.products.len(), &policy);
        println!(
            "row {:>2} ({:>20}): constraints={:?} residual_lexical={:?} outcome={:?} selectivity={:?}",
            row.id, row.text, query.constraints, query.residual_lexical, planned.outcome, planned.selectivity
        );
    }

    let mut benign_recovered: BTreeMap<&'static str, usize> = BTreeMap::new();
    let mut adversarial_false_recovery: BTreeMap<&'static str, usize> = BTreeMap::new();
    let mut benign_ndcgs: BTreeMap<&'static str, Vec<f64>> = BTreeMap::new();
    let mut regression_guard_violations: Vec<&'static str> = Vec::new();
    let mut a_hits_cache: BTreeMap<usize, Vec<PlannedHit>> = BTreeMap::new();
    let mut hit_counts: BTreeMap<&'static str, BTreeMap<usize, usize>> = BTreeMap::new();

    for treatment in Treatment::ALL {
        println!("\n--- Treatment {} ---", treatment.label());
        for row in &rows {
            let query = compile(row.text, &lexicon);
            let (planned, hits) = run_treatment(
                treatment,
                &query,
                catalog,
                &index,
                &delegate,
                &policy,
                &residual_policy,
            );
            println!(
                "row {:>2} ({:>20}): outcome={:?} hits={} products={:?}",
                row.id,
                row.text,
                planned.outcome,
                hits.len(),
                hits.iter().map(|h| h.product).collect::<Vec<_>>(),
            );
            hit_counts
                .entry(treatment.label())
                .or_default()
                .insert(row.id, hits.len());

            if treatment == Treatment::A {
                a_hits_cache.insert(row.id, hits.clone());
            }

            match row.class {
                RowClass::Benign => {
                    let recovered = !hits.is_empty()
                        && hits
                            .iter()
                            .all(|h| row.relevant.contains(&(h.product, h.variant)));
                    if recovered {
                        *benign_recovered.entry(treatment.label()).or_insert(0) += 1;
                    } else if !hits.is_empty() {
                        println!(
                            "  UNEXPECTED: benign row recovered hits outside its own entity's \
                             relevant set (should be structurally impossible given a ProductType \
                             constraint) -- {:?}",
                            hits.iter().map(|h| h.product).collect::<Vec<_>>()
                        );
                    }
                    let ndcg = ndcg_at_k(&hits, &row.relevant, K);
                    benign_ndcgs
                        .entry(treatment.label())
                        .or_default()
                        .push(ndcg);
                }
                RowClass::Adversarial => {
                    if !hits.is_empty() {
                        *adversarial_false_recovery
                            .entry(treatment.label())
                            .or_insert(0) += 1;
                        println!(
                            "  FALSE RECOVERY: {} hit(s) for an adversarial row that should stay \
                             at zero results",
                            hits.len()
                        );
                    }
                }
                RowClass::RegressionGuard => {
                    if let Some(a_hits) = a_hits_cache.get(&row.id) {
                        if &hits != a_hits {
                            regression_guard_violations.push(treatment.label());
                            println!(
                                "  REGRESSION GUARD VIOLATION: row {} (no structural anchor at \
                                 all) must be byte-identical to Treatment A for every treatment; \
                                 Treatment {} diverged",
                                row.id,
                                treatment.label()
                            );
                        }
                    }
                }
                RowClass::Measured => {}
            }
        }
    }

    // Regression guard is a correctness invariant every treatment's own
    // code already claims to satisfy by construction (each explicitly
    // defers to `execute_a`/the real `execute_planned` whenever
    // `query.constraints.is_empty()`) -- a violation here means a
    // treatment's implementation touches a code path the protocol
    // requires it not to, which is a bug in this experiment's own code,
    // not a graded metric. Fails loudly rather than silently degrading
    // the GO-gate computation below.
    assert!(
        regression_guard_violations.is_empty(),
        "row 8 (no structural anchor) must be untouched by every treatment; violated by {regression_guard_violations:?}"
    );

    println!("\n--- candidate-set growth (hits per row, per treatment) ---");
    for row in &rows {
        let counts: Vec<(&str, usize)> = Treatment::ALL
            .iter()
            .map(|t| (t.label(), *hit_counts[t.label()].get(&row.id).unwrap_or(&0)))
            .collect();
        println!("row {:>2} ({:>20}): {:?}", row.id, row.text, counts);
    }

    println!("\n--- latency (median of {LATENCY_TRIALS} independent batched trials) ---");
    let mut latency_ms: BTreeMap<&'static str, f64> = BTreeMap::new();
    for treatment in Treatment::ALL {
        let trials: Vec<f64> = (0..LATENCY_TRIALS)
            .map(|_| {
                one_latency_trial(
                    treatment,
                    &rows,
                    &lexicon,
                    catalog,
                    &index,
                    &delegate,
                    &policy,
                    &residual_policy,
                )
            })
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
        "Note: with only 4 preregistered benign rows and 2 preregistered adversarial rows, the \
         protocol's '>=90% recovery'/'<=1% false recovery' thresholds are numerically equivalent \
         to 'all 4' and 'zero of 2' respectively -- no fractional pass point exists at this \
         denominator. Disclosed here, not hidden behind the percentage framing."
    );
    let baseline_latency = latency_ms[Treatment::A.label()];
    let benign_total = rows.iter().filter(|r| r.class == RowClass::Benign).count();
    let adversarial_total = rows
        .iter()
        .filter(|r| r.class == RowClass::Adversarial)
        .count();
    for treatment in Treatment::ALL {
        let recovered = *benign_recovered.get(treatment.label()).unwrap_or(&0);
        let recovery_rate = recovered as f64 / benign_total as f64;
        let false_recoveries = *adversarial_false_recovery
            .get(treatment.label())
            .unwrap_or(&0);
        let false_recovery_rate = false_recoveries as f64 / adversarial_total as f64;
        let mean_ndcg = benign_ndcgs
            .get(treatment.label())
            .map(|v| v.iter().sum::<f64>() / v.len() as f64)
            .unwrap_or(0.0);
        let overhead_pct =
            100.0 * (latency_ms[treatment.label()] - baseline_latency) / baseline_latency;
        // Every R2 treatment (`r2_experimental::execute_{a,b,c,d}`) calls
        // only `commerce_core::plan`/`commerce_core::index` primitives --
        // never `control_plane::provider::ModelProvider` or any other
        // model-call surface. This is a structural fact confirmed by
        // direct source inspection of `r2_experimental.rs` (no such import
        // exists anywhere in that file), not something instrumented at
        // runtime here.
        let zero_model_calls = true;
        let go = recovery_rate >= 0.90 && false_recovery_rate <= 0.01 && zero_model_calls;
        println!(
            "Treatment {}: benign recovery={recovered}/{benign_total} ({:.0}%, require >=90%), \
             adversarial false recovery={false_recoveries}/{adversarial_total} ({:.0}%, require \
             <=1%), mean benign NDCG@10={mean_ndcg:.4}, latency overhead vs A={overhead_pct:.1}%, \
             zero_model_calls={zero_model_calls} => GO gate {}",
            treatment.label(),
            recovery_rate * 100.0,
            false_recovery_rate * 100.0,
            if go { "PASS" } else { "FAIL" }
        );
    }

    println!(
        "\n--- row 4 (measured, out of scope for the GO gate): misspelling of a real registered \
         lexicon value ('leather' -> 'leathr') ---"
    );
    println!(
        "See the per-treatment output above for row 4's outcome/hits under each treatment -- \
         documented, not scored, per the protocol's own text ('misspelling correction is \
         explicitly out of scope for R2')."
    );

    let summary = serde_json::json!({
        "experiment_id": "I42-R2",
        "baseline_sha": BASELINE_SHA,
        "benign_recovered": benign_recovered,
        "benign_total": benign_total,
        "adversarial_false_recovery": adversarial_false_recovery,
        "adversarial_total": adversarial_total,
        "benign_ndcg_mean": benign_ndcgs.iter().map(|(t, v)| {
            (t.to_string(), v.iter().sum::<f64>() / v.len() as f64)
        }).collect::<BTreeMap<_,_>>(),
        "latency_ms_per_call": latency_ms,
        "hit_counts_per_row": hit_counts,
    });
    println!("\n{}", serde_json::to_string_pretty(&summary).unwrap());

    if let Some(path) = env::args().nth(1) {
        fs::write(&path, serde_json::to_string_pretty(&summary).unwrap())
            .expect("write summary json");
        println!("summary written to {path}");
    }
}
