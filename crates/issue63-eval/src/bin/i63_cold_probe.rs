//! Issue #63 post-hoc diagnostic (NOT preregistered; no verdict depends on
//! it). Part A's native server executes the same `plp::execute` code
//! 2.3-6.4x slower than the hot-loop microbenchmark. This probe times the
//! same operations (i) back to back, as the microbenchmark does, and (ii)
//! with a 64 MiB streaming pass over an unrelated buffer between
//! operations (excluded from the timed window), which evicts the working
//! set from every cache level first. If (ii) approaches the in-server
//! numbers, cache-cold execution explains the gap.
//!
//! Usage: i63_cold_probe --catalog <jsonl> --out <json> [--ops 200]

use std::collections::HashMap;
use std::hint::black_box;
use std::path::PathBuf;

use commerce_core::domain::{CategoryId, Constraint};
use commerce_core::index::CatalogIndex;
use commerce_core::ir::ResolvedConstraint;
use comparator_eval::translate::StructuralNames;
use issue61_eval::{load_dataset, Dataset};
use issue63_eval::bench::thread_cpu_ns;
use issue63_eval::{RHO_S, TAU_F};
use issue79_eval::cells::{all_cells, SORT_FIELDS};
use issue79_eval::plp::{
    execute, CandidateMode, FacetMode, PlpContext, PlpRequest, SortMode, SortStructures,
};

const EVICT_BYTES: usize = 64 << 20;

fn arg(args: &[String], name: &str) -> Option<String> {
    args.windows(2).find(|w| w[0] == name).map(|w| w[1].clone())
}

fn timed<R>(ops: usize, evict: Option<&mut Vec<u8>>, mut op: impl FnMut() -> R) -> f64 {
    let mut evict = evict;
    for _ in 0..20 {
        black_box(op());
    }
    let mut total = 0u64;
    for i in 0..ops {
        if let Some(buffer) = evict.as_deref_mut() {
            for chunk in buffer.chunks_mut(64) {
                chunk[0] = chunk[0].wrapping_add(i as u8);
            }
            black_box(&buffer);
        }
        let started = thread_cpu_ns();
        black_box(op());
        total += thread_cpu_ns() - started;
    }
    total as f64 / ops as f64 / 1e3
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let catalog = PathBuf::from(arg(&args, "--catalog").expect("--catalog"));
    let out = PathBuf::from(arg(&args, "--out").expect("--out"));
    let ops: usize = arg(&args, "--ops").map_or(200, |v| v.parse().expect("ops"));
    let data = load_dataset(&catalog, Dataset::Wands).expect("load");
    let index = CatalogIndex::build(&data.catalog);
    let mut structures = SortStructures::build(&index, &SORT_FIELDS, &SORT_FIELDS);
    structures.all_ordinals = Some(index.all_ordinals_bitmap());
    let category_id_by_leaf: HashMap<String, CategoryId> = data
        .catalog
        .products
        .iter()
        .filter_map(|p| {
            data.category_name(p.category)
                .map(|n| (n.to_owned(), p.category))
        })
        .collect();
    let ctx = PlpContext {
        catalog: &data.catalog,
        index: &index,
        category_id_by_leaf: &category_id_by_leaf,
        source_id_by_product: &data.source_id_by_product,
        structures: &structures,
        tau_f: Some(TAU_F),
        rho_s: Some(RHO_S),
    };
    let fh3 = all_cells()
        .into_iter()
        .find(|c| c.name == "facet_high_cardinality_color")
        .expect("fh3");
    let request = |cand_mode| PlpRequest {
        category: None,
        filters: Vec::new(),
        ranges: Vec::new(),
        facets: fh3.facets.iter().map(|f| (*f).to_owned()).collect(),
        sort: None,
        top_k: fh3.top_k,
        offset: 0,
        facet_mode: FacetMode::Hybrid,
        sort_mode: SortMode::Hybrid,
        cand_mode,
    };
    let white = [ResolvedConstraint::Attribute(Constraint::Enum {
        attribute: "color".to_owned(),
        value: "white".to_owned(),
    })];
    let mut buffer = vec![0u8; EVICT_BYTES];
    let mut rows = Vec::new();
    let mut probe = |name: &str, run: &mut dyn FnMut() -> usize, buffer: &mut Vec<u8>| {
        let hot = timed(ops, None, &mut *run);
        let cold = timed(ops, Some(buffer), &mut *run);
        println!(
            "{name}: hot {hot:.1} µs, cold {cold:.1} µs, cold/hot {:.2}x",
            cold / hot
        );
        rows.push(serde_json::json!({"op": name, "hot_us": hot, "cold_us": cold, "cold_over_hot": cold / hot}));
    };
    probe(
        "FH3 FINAL (p0) pipeline",
        &mut || {
            execute(&ctx, &request(CandidateMode::P0))
                .expect("x")
                .num_found
        },
        &mut buffer,
    );
    probe(
        "FH3 N+ (p0r) pipeline",
        &mut || {
            execute(&ctx, &request(CandidateMode::P0r))
                .expect("x")
                .num_found
        },
        &mut buffer,
    );
    probe(
        "P0 all_ordinals construction",
        &mut || index.all_ordinals_bitmap().len() as usize,
        &mut buffer,
    );
    probe(
        "color facet_counts_ordinal over full",
        &mut || {
            let all = index.all_ordinals_bitmap_by_range();
            index.facet_counts_ordinal(&all, "color").len()
        },
        &mut buffer,
    );
    probe(
        "FD1 candidates (color=white clone)",
        &mut || index.indexed_candidates(&white).len() as usize,
        &mut buffer,
    );
    let report = serde_json::json!({
        "diagnostic": "post-hoc cold-cache probe, not preregistered",
        "evict_bytes": EVICT_BYTES,
        "ops": ops,
        "catalog": catalog.display().to_string(),
        "rows": rows,
    });
    std::fs::write(&out, serde_json::to_string_pretty(&report).expect("json")).expect("write");
}
