//! Issue #55 whole-workload follow-up: P9-E04 explicitly excludes
//! candidate sets above `MAX_CANDIDATES` (5000) from its H3 isolation.
//! P9-E02's own real WANDS run has exactly 2 of its 7 `FastPath`-routed
//! queries ("driftwood mirror", "marble") with *zero* structural
//! constraints, so `indexed_candidates` returns the entire 42,994-product
//! catalog -- P9-E04 never measures `execute_ranked`'s cost on this
//! specific, real, in-scope-for-Issue-#55 tail case.
//!
//! A single-shot measurement of these two queries embedded in P9-E02's
//! 480-query pass showed native's own latency going *up* after both
//! Issue #55 fixes (mean ~1.37ms before -> ~2.46ms after, across 6 runs
//! each), the opposite of every other measurement this project has taken
//! of these fixes. This binary repeats `execute_ranked` many times for
//! exactly these two full-catalog queries, in one process, to separate a
//! real regression from single-shot process-level noise.
use commerce_core::cold_start::{compile_lexicon, CatalogProfile};
use commerce_core::index::CatalogIndex;
use commerce_core::ir::compile;

const K: usize = 10;
const REPS: usize = 200;

fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let idx = ((sorted.len() - 1) as f64 * p).round() as usize;
    sorted[idx]
}

fn main() {
    println!("=== P9-E05: full-catalog (zero-constraint) execute_ranked tail latency, real WANDS queries ===");

    let raw_products =
        phase6a_eval::data::load_catalog(std::path::Path::new("dataset_cache/wands/catalog.jsonl"));
    let ingested = phase6a_eval::catalog::build_catalog(&raw_products);
    let index = CatalogIndex::build(&ingested.catalog);
    let profile = CatalogProfile::build(
        &ingested.catalog,
        &[],
        &ingested.product_types,
        &ingested.categories,
    );
    let lexicon = compile_lexicon(&profile, 1);

    println!("catalog: {} products", ingested.catalog.products.len());

    for query_text in ["driftwood mirror", "marble"] {
        let compiled = compile(query_text, &lexicon);
        assert!(
            compiled.constraints.is_empty(),
            "expected zero-constraint query for {query_text:?}, got {:?}",
            compiled.constraints
        );
        println!(
            "  compiled: preferences={:?} residual_lexical={:?}",
            compiled.preferences, compiled.residual_lexical
        );

        // warmup, discarded -- matches this project's own established
        // convention of not trusting a first-call measurement.
        for _ in 0..10 {
            let _ = index.execute_ranked(&compiled, &ingested.catalog, K);
        }

        let mut latencies_ms = Vec::with_capacity(REPS);
        for _ in 0..REPS {
            let t0 = std::time::Instant::now();
            let hits = index.execute_ranked(&compiled, &ingested.catalog, K);
            let elapsed_ms = t0.elapsed().as_secs_f64() * 1000.0;
            std::hint::black_box(&hits);
            latencies_ms.push(elapsed_ms);
        }
        latencies_ms.sort_by(|a, b| a.total_cmp(b));
        let mean = latencies_ms.iter().sum::<f64>() / latencies_ms.len() as f64;
        let median = percentile(&latencies_ms, 0.5);
        let p95 = percentile(&latencies_ms, 0.95);
        let min = latencies_ms.first().copied().unwrap_or(0.0);
        let max = latencies_ms.last().copied().unwrap_or(0.0);
        println!(
            "query={query_text:?} candidates=42994 reps={REPS}: mean={mean:.4}ms median={median:.4}ms p95={p95:.4}ms min={min:.4}ms max={max:.4}ms"
        );
    }
}
