//! Issue #51 follow-up named by `docs/decisions/ISSUE51_FULLGATE_SCALE_DECISION.md`:
//! is row 1's ("size 22", genuinely ambiguous, no corroborating entity)
//! ~50x `Punt`-path cost gap vs. Treatment A inherent to correctly
//! delegating to real lexical search when structural evidence is
//! insufficient, or does it contain a fixable treatment-side
//! inefficiency on top of the delegate's own real cost? Protocol:
//! `docs/experiments/ISSUE51_PUNT_COST_FLOOR_PROTOCOL.md`.
//!
//! Reuses the exact same realistic-scale (~43,000-product) catalog
//! construction as `r1_full_gate_scale_rerun.rs` (same decoy shape/count,
//! same fixture), so the comparison against that checkpoint's own
//! recorded numbers is apples to apples. This binary adds one new
//! measurement: the **isolated floor** cost of exactly the two real
//! operations `execute_planned`'s `Punt` arm performs for row 1
//! (`index.identifier_lookup` + `BitmapTantivyDelegate::search`), called
//! directly via public API with no `CommerceQuery`/`compile()`/`resolve_e`/
//! `execute_planned`/`plan()` in the timed region at all.
//!
//! Reproduction: `cargo build --release -p issue42-eval &&
//! ./target/release/r1_punt_cost_floor`

use commerce_core::cold_start::{compile_lexicon, CatalogProfile};
use commerce_core::domain::{
    attributes, Catalog, Inventory, Price, Product, ProductId, ProductTypeId, Variant, VariantId,
};
use commerce_core::index::CatalogIndex;
use commerce_core::plan::{LexicalDelegate, PlannerPolicy};
use issue42_eval::r1_experimental::{
    build_attribute_kind_registry, resolve_a, resolve_e, run_treatment,
};
use issue42_eval::r1_workload::build_typed_ambiguity_catalog;
use phase9_eval::bitmap_delegate::{build_index, BitmapTantivyDelegate};

const K: usize = 10;
const MIN_ENUM_FREQUENCY: usize = 1;
const LATENCY_BATCH: usize = 200;
const LATENCY_TRIALS: usize = 7;
/// Identical to `r1_full_gate_scale_rerun.rs`'s own constants, so this
/// binary's catalog is byte-for-byte the same scale/shape.
const DECOY_COUNT: usize = 42_990;
const DECOY_PRODUCT_TYPE: u32 = 9999;
const DECOY_BRAND: u32 = 9999;
const DECOY_CATEGORY: u32 = 9999;

/// Duplicated from `r1_full_gate_scale_rerun.rs` rather than shared:
/// each binary is a self-contained, independently reproducible
/// measurement (this project's established pattern for eval binaries,
/// e.g. that binary's own doc comment on why it does not import
/// `i51_e00_catalog_scale_diagnostic.rs`'s decoy helper either).
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
                price: Price::usd(3_400),
                inventory: Inventory::in_stock(1),
            }],
        };
        products.push(decoy);
    }
    Catalog { products }
}

fn median(mut values: Vec<f64>) -> f64 {
    values.sort_by(|a, b| a.total_cmp(b));
    values[values.len() / 2]
}

fn main() {
    println!("=== Issue #51 Punt-path delegate-cost floor isolation ===");

    let fixture = build_typed_ambiguity_catalog();
    let catalog = scaled_catalog(&fixture.catalog, DECOY_COUNT);
    println!(
        "catalog: {} products (matches r1_full_gate_scale_rerun.rs's own scale)",
        catalog.products.len()
    );

    let index = CatalogIndex::build(&catalog);
    let profile = CatalogProfile::build(
        &catalog,
        &fixture.brands,
        &fixture.product_types,
        &fixture.categories,
    );
    let lexicon = compile_lexicon(&profile, MIN_ENUM_FREQUENCY);
    let built = build_index(&catalog).expect("in-memory tantivy index build");
    let delegate = BitmapTantivyDelegate::new(
        &built.index,
        vec![built.title_field, built.description_field],
    )
    .expect("tantivy delegate build");
    let policy = PlannerPolicy {
        selectivity_threshold: 0.05,
        delegate_oversample: 20,
    };
    let registry = build_attribute_kind_registry(&catalog);

    const ROW1_TEXT: &str = "size 22";
    const ROW1_TERM: &str = "22";

    // --- Reproduction gate: same-process sanity check that this binary's
    // own methodology reproduces r1_full_gate_scale_rerun.rs's recorded
    // magnitude for row 1 before trusting any new floor number. ---
    println!("\n--- reproduction gate: Treatment A vs Treatment E, row 1 only ---");
    let a_resolution = resolve_a(ROW1_TEXT, &lexicon);
    let e_resolution = resolve_e(ROW1_TEXT, &lexicon, &registry);

    let treatment_a_trials: Vec<f64> = (0..LATENCY_TRIALS)
        .map(|_| {
            let t0 = std::time::Instant::now();
            for _ in 0..LATENCY_BATCH {
                let (_planned, hits) = std::hint::black_box(run_treatment(
                    std::hint::black_box(&a_resolution),
                    &catalog,
                    &index,
                    Some(&delegate as &dyn LexicalDelegate),
                    K,
                    &policy,
                ));
                std::hint::black_box(hits.len());
            }
            t0.elapsed().as_secs_f64() * 1000.0 / LATENCY_BATCH as f64
        })
        .collect();
    let treatment_e_trials: Vec<f64> = (0..LATENCY_TRIALS)
        .map(|_| {
            let t0 = std::time::Instant::now();
            for _ in 0..LATENCY_BATCH {
                let (_planned, hits) = std::hint::black_box(run_treatment(
                    std::hint::black_box(&e_resolution),
                    &catalog,
                    &index,
                    Some(&delegate as &dyn LexicalDelegate),
                    K,
                    &policy,
                ));
                std::hint::black_box(hits.len());
            }
            t0.elapsed().as_secs_f64() * 1000.0 / LATENCY_BATCH as f64
        })
        .collect();
    let treatment_a_ms = median(treatment_a_trials.clone());
    let treatment_e_ms = median(treatment_e_trials.clone());
    println!(
        "Treatment A row 1: trials={:?} median={treatment_a_ms:.5}ms",
        treatment_a_trials
            .iter()
            .map(|v| format!("{v:.5}"))
            .collect::<Vec<_>>()
    );
    println!(
        "Treatment E row 1: trials={:?} median={treatment_e_ms:.5}ms",
        treatment_e_trials
            .iter()
            .map(|v| format!("{v:.5}"))
            .collect::<Vec<_>>()
    );
    println!(
        "reproduction check: prior checkpoint recorded A~0.0001-0.0003ms, E~0.0097-0.0104ms \
         (docs/decisions/ISSUE51_FULLGATE_SCALE_DECISION.md)"
    );

    // --- The isolated floor: exactly what execute_planned's Punt arm
    // does for row 1 (identifier_hits' own identifier_lookup loop, then
    // the delegate call with the same non-oversampled limit=k the Punt
    // arm already uses when query.constraints is empty), called
    // directly via public API -- no CommerceQuery/compile()/resolve_e/
    // execute_planned/plan() anywhere in the timed region. ---
    println!("\n--- isolated floor: identifier_lookup + delegate.search, called directly ---");
    let floor_trials: Vec<f64> = (0..LATENCY_TRIALS)
        .map(|_| {
            let t0 = std::time::Instant::now();
            for _ in 0..LATENCY_BATCH {
                let hits = std::hint::black_box(index.identifier_lookup(ROW1_TERM));
                std::hint::black_box(hits.len());
                let raw = std::hint::black_box(delegate.search(
                    std::hint::black_box(&[ROW1_TERM.to_string()]),
                    None,
                    K,
                ));
                std::hint::black_box(raw.len());
            }
            t0.elapsed().as_secs_f64() * 1000.0 / LATENCY_BATCH as f64
        })
        .collect();
    let floor_ms = median(floor_trials.clone());
    println!(
        "isolated floor: trials={:?} median={floor_ms:.5}ms",
        floor_trials
            .iter()
            .map(|v| format!("{v:.5}"))
            .collect::<Vec<_>>()
    );

    // --- Sanity check: confirm the isolated calls actually exercise the
    // same real code paths. "22" turns out to have zero exact-token
    // matches in this synthetic corpus (decoy titles embed it only
    // inside a larger numeric token like "1000022", which Tantivy's
    // tokenizer does not split, and no real fixture title/description
    // contains the standalone token "22") -- this is not a validity
    // problem (the isolated call and the production Punt-path call are
    // the exact same function against the exact same index with the
    // exact same argument, so the comparison stays apples-to-apples
    // regardless of hit count), but it does raise a fair question:
    // is ~9 microseconds mostly term-dictionary-lookup/parse overhead
    // that would be paid even for a miss, or would a genuine multi-match
    // query cost about the same (supporting "inherent cost of a real
    // search") or dramatically more (undercutting it)? Measured directly
    // below rather than assumed. ---
    let sample_identifier_hits = index.identifier_lookup(ROW1_TERM);
    let sample_delegate_hits = delegate.search(&[ROW1_TERM.to_string()], None, K);
    println!(
        "\nsanity check: identifier_lookup(\"22\") returned {} hits (expected 0 -- \"22\" is not \
         a registered identifier); delegate.search([\"22\"], None, {K}) returned {} hits \
         (0 is expected here -- see comment above; validity does not depend on a nonzero count \
         since this is the identical call the production Punt path makes)",
        sample_identifier_hits.len(),
        sample_delegate_hits.len()
    );

    // --- Does a genuine multi-match query cost about the same, or does
    // the zero-hit case understate the real floor? "decoy" appears in
    // every one of the 42,990 decoy titles, forcing Tantivy to actually
    // traverse a huge posting list and run real BM25 top-K selection --
    // as adversarial a real-hit case as this corpus can produce. ---
    println!("\n--- comparison: isolated floor for a term with real matches (\"decoy\") ---");
    let real_hit_count = delegate.search(&["decoy".to_string()], None, K).len();
    let real_hit_trials: Vec<f64> = (0..LATENCY_TRIALS)
        .map(|_| {
            let t0 = std::time::Instant::now();
            for _ in 0..LATENCY_BATCH {
                let raw = std::hint::black_box(delegate.search(
                    std::hint::black_box(&["decoy".to_string()]),
                    None,
                    K,
                ));
                std::hint::black_box(raw.len());
            }
            t0.elapsed().as_secs_f64() * 1000.0 / LATENCY_BATCH as f64
        })
        .collect();
    let real_hit_ms = median(real_hit_trials.clone());
    println!(
        "delegate.search([\"decoy\"], None, {K}) returns {real_hit_count} hits (a real, large \
         posting-list traversal + BM25 top-{K} selection, the most adversarial real-hit case \
         this corpus can produce); trials={:?} median={real_hit_ms:.5}ms",
        real_hit_trials
            .iter()
            .map(|v| format!("{v:.5}"))
            .collect::<Vec<_>>()
    );

    println!("\n--- decision ---");
    let floor_pct_of_measured = 100.0 * floor_ms / treatment_e_ms;
    println!(
        "isolated floor ({floor_ms:.5}ms) is {floor_pct_of_measured:.1}% of Treatment E's \
         measured row-1 cost ({treatment_e_ms:.5}ms)"
    );
    println!(
        "real-hit comparison: a genuine large-posting-list match (\"decoy\", {real_hit_ms:.5}ms) \
         vs. the zero-hit \"22\" case ({floor_ms:.5}ms) -- ratio {:.2}x, confirming whether the \
         zero-hit floor understates a real match's cost",
        real_hit_ms / floor_ms
    );
    if floor_pct_of_measured >= 80.0 {
        println!(
            "=== H0 CONFIRMED (>=80%): the gap is dominated by the delegate's own inherent, \
             necessary cost of a real lexical fallback -- R1's <=5% overhead bar, applied to a \
             workload containing a genuinely ambiguous, uncorroborated query, is structurally \
             unclearable by any correctness-preserving treatment ==="
        );
    } else if floor_pct_of_measured < 50.0 {
        println!(
            "=== H1 CONFIRMED (<50%): a material, currently-unidentified treatment-side \
             overhead sits on top of the delegate's real cost -- a genuine, fixable \
             optimization opportunity ==="
        );
    } else {
        println!(
            "=== AMBIGUOUS ZONE (50-80%): both a real inherent cost and a partial fixable \
             overhead contribute; no clean H0/H1 verdict ==="
        );
    }
}
