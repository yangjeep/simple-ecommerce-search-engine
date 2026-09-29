//! Issue #63 (amendment 1, sections 3-4): native-internal microbenchmarks.
//!
//! Usage:
//!   i63_primitives --catalog <jsonl> --tier 100k|500k --run <n> --out <json>
//!     [--parts b1,b2,c] [--min-ops 200] [--min-cpu-ms 2000]
//!
//! - B1: match-all candidate representations P0/P0r/P1/P2(/P2b) --
//!   construction CPU/allocations/bytes, and the downstream in-process `/plp`
//!   pipeline (`issue79_eval::plp::execute`, FINAL = hybrid:hybrid at the
//!   frozen tau/rho) for every #79 cell (500k) or FH1-FH4 (100k), each
//!   response checked equal to P0's before timing.
//! - B2 (500k only): the facet-counting grid (attribute x candidate set x
//!   strategy/decomposition probe).
//! - C: structural primitives, each checked against a linear-scan oracle.
//!
//! Single-threaded; run it pinned to one CPU inside the #79 scope envelope.
//! Any correctness mismatch is recorded and makes the process exit 1 after
//! writing the report (wrong-but-fast disqualifies; it is never dropped).

use std::collections::{BTreeMap, HashMap};
use std::hint::black_box;
use std::path::PathBuf;
use std::time::Instant;

use commerce_core::domain::{
    effective_attributes, AttributeValue, Catalog, CategoryId, Constraint, NumericOp, VariantId,
};
use commerce_core::index::{tokenize, CatalogIndex};
use commerce_core::ir::{ResolvedConstraint, StructuralConstraint};
use comparator_eval::translate::StructuralNames;
use issue61_eval::{load_dataset, Dataset};
use issue63_eval::alloc::CountingAllocator;
use issue63_eval::bench::{measure, Sample, MIN_CPU_NS, MIN_OPS};
use issue63_eval::bytes;
use issue63_eval::{synthetic, EXPERIMENT_ID, NOMINAL_GHZ, RAW_SCHEMA_VERSION, RHO_S, TAU_F};
use issue79_eval::cells::{all_cells, reference_cells, Cell, CATEGORY_BROAD, MODERN, SORT_FIELDS};
use issue79_eval::oracle::Oracle;
use issue79_eval::plp::{
    execute, CandidateMode, FacetMode, PlpContext, PlpRequest, PlpResponse, SortMode,
    SortStructures,
};
use roaring::RoaringBitmap;
use serde::Serialize;

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

const CAND_MODES: [CandidateMode; 5] = [
    CandidateMode::P0,
    CandidateMode::P0r,
    CandidateMode::P1,
    CandidateMode::P2,
    CandidateMode::P2b,
];
const FH_CELLS: [&str; 4] = [
    "facet_low_cardinality_style",
    "facet_medium_cardinality_primarymaterial",
    "facet_high_cardinality_color",
    "facet_disjunctive_multi_dim",
];
const B2_ATTRIBUTES: [&str; 5] = ["style", "shape", "material", "primarymaterial", "color"];
const B2_DENSITIES: [f64; 7] = [0.5, 0.25, 0.1, 0.03, 0.01, 0.003, 0.001];
const B2_SEED: u64 = 63;

struct Args {
    catalog: PathBuf,
    tier: String,
    run: u32,
    out: PathBuf,
    parts: Vec<String>,
    min_ops: u64,
    min_cpu_ns: u64,
}

fn parse_args() -> Result<Args, String> {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    let mut map = BTreeMap::new();
    let mut iter = raw.iter();
    while let Some(flag) = iter.next() {
        let value = iter
            .next()
            .ok_or_else(|| format!("missing value for {flag}"))?;
        map.insert(flag.clone(), value.clone());
    }
    let get = |name: &str| map.get(name).cloned().ok_or(format!("missing {name}"));
    Ok(Args {
        catalog: PathBuf::from(get("--catalog")?),
        tier: get("--tier")?,
        run: get("--run")?.parse().map_err(|e| format!("--run: {e}"))?,
        out: PathBuf::from(get("--out")?),
        parts: map
            .get("--parts")
            .map_or("b1,b2,c", String::as_str)
            .split(',')
            .map(str::to_owned)
            .collect(),
        min_ops: map
            .get("--min-ops")
            .map_or(Ok(MIN_OPS), |v| v.parse())
            .map_err(|e| format!("--min-ops: {e}"))?,
        min_cpu_ns: map
            .get("--min-cpu-ms")
            .map_or(Ok(MIN_CPU_NS), |v| {
                v.parse::<u64>().map(|ms| ms * 1_000_000)
            })
            .map_err(|e| format!("--min-cpu-ms: {e}"))?,
    })
}

#[derive(Serialize)]
struct Point {
    part: &'static str,
    name: String,
    variant: String,
    /// Elements the primitive works over (|C|, lookups per op, ...).
    work_items: u64,
    result_cardinality: u64,
    bytes_touched_estimate: u64,
    sample: Sample,
    ns_per_item: f64,
    nominal_cycles_per_item: f64,
    extra: serde_json::Value,
}

#[derive(Serialize)]
struct Correctness {
    part: &'static str,
    name: String,
    ok: bool,
    detail: Option<String>,
}

#[derive(Serialize)]
struct Report {
    schema_version: u32,
    experiment_id: &'static str,
    tier: String,
    run: u32,
    catalog: String,
    ordinals: usize,
    min_ops: u64,
    min_cpu_ns: u64,
    git_sha: String,
    hostname: String,
    started_utc: u64,
    load_ms: f64,
    index_build_ms: f64,
    points: Vec<Point>,
    correctness: Vec<Correctness>,
    all_correct: bool,
    total_wall_s: f64,
}

struct Bench {
    min_ops: u64,
    min_cpu_ns: u64,
    points: Vec<Point>,
    correctness: Vec<Correctness>,
}

impl Bench {
    #[allow(clippy::too_many_arguments)]
    fn point<R>(
        &mut self,
        part: &'static str,
        name: &str,
        variant: &str,
        work_items: u64,
        result_cardinality: u64,
        bytes_touched_estimate: u64,
        extra: serde_json::Value,
        op: impl FnMut() -> R,
    ) {
        let sample = measure(self.min_ops, self.min_cpu_ns, op);
        let ns_per_item = sample.cpu_ns_per_op / work_items.max(1) as f64;
        eprintln!(
            "  {part} {name} {variant}: {:.1} ns/op ({} ops, {:.2} allocs/op)",
            sample.cpu_ns_per_op, sample.ops, sample.allocations_per_op
        );
        self.points.push(Point {
            part,
            name: name.to_owned(),
            variant: variant.to_owned(),
            work_items,
            result_cardinality,
            bytes_touched_estimate,
            ns_per_item,
            nominal_cycles_per_item: ns_per_item * NOMINAL_GHZ,
            sample,
            extra,
        });
    }

    fn check(&mut self, part: &'static str, name: &str, ok: bool, detail: Option<String>) {
        if !ok {
            eprintln!("  CORRECTNESS FAILURE {part} {name}: {detail:?}");
        }
        self.correctness.push(Correctness {
            part,
            name: name.to_owned(),
            ok,
            detail,
        });
    }
}

fn command_stdout(program: &str, args: &[&str]) -> String {
    std::process::Command::new(program)
        .args(args)
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_default()
        .trim()
        .to_owned()
}

fn plp_request(cell: &Cell, cand_mode: CandidateMode) -> PlpRequest {
    PlpRequest {
        category: cell.category.map(str::to_owned),
        filters: cell
            .filters
            .iter()
            .map(|(a, v)| ((*a).to_owned(), (*v).to_owned()))
            .collect(),
        ranges: cell
            .ranges
            .iter()
            .map(|(a, o, v)| ((*a).to_owned(), (*o).to_owned(), *v))
            .collect(),
        facets: cell.facets.iter().map(|f| (*f).to_owned()).collect(),
        sort: cell.sort.map(|(f, d)| (f.to_owned(), d)),
        top_k: cell.top_k,
        offset: 0,
        facet_mode: FacetMode::Hybrid,
        sort_mode: SortMode::Hybrid,
        cand_mode,
    }
}

/// The base constraints `plp::execute` builds for a cell (#77's
/// `build_constraints`: category, then enum filters, then numeric ranges).
fn constraints_for(
    cell: &Cell,
    category_id_by_leaf: &HashMap<String, CategoryId>,
) -> Vec<ResolvedConstraint> {
    let mut constraints = Vec::new();
    if let Some(leaf) = cell.category {
        constraints.push(ResolvedConstraint::Structural(
            StructuralConstraint::Category(category_id_by_leaf[leaf]),
        ));
    }
    for (attribute, value) in &cell.filters {
        constraints.push(ResolvedConstraint::Attribute(Constraint::Enum {
            attribute: (*attribute).to_owned(),
            value: (*value).to_owned(),
        }));
    }
    for (attribute, op, value) in &cell.ranges {
        assert_eq!(*op, "gte", "only gte ranges are used");
        constraints.push(ResolvedConstraint::Attribute(Constraint::Numeric {
            attribute: (*attribute).to_owned(),
            op: NumericOp::Gte,
            value: *value,
        }));
    }
    constraints
}

fn same_response(a: &PlpResponse, b: &PlpResponse) -> bool {
    a.num_found == b.num_found && a.facets == b.facets && a.docs == b.docs
}

// ---------------------------------------------------------------- B1 ----

fn run_b1(bench: &mut Bench, ctx: &PlpContext<'_>, tier: &str) {
    let index = ctx.index;
    let n = index.ordinal_count() as u32;
    let prebuilt = ctx
        .structures
        .all_ordinals
        .as_ref()
        .expect("prebuilt P1 bitmap");
    let p0 = index.all_ordinals_bitmap();
    let p0r = index.all_ordinals_bitmap_by_range();
    let p2 = index.candidate_set(&[]);
    bench.check(
        "b1",
        "representations_identical",
        p0 == p0r && &p0 == prebuilt && p2.materialize() == p0 && p2.len() == u64::from(n),
        None,
    );
    let result_bytes = bytes::bitmap_bytes(&p0);
    let serialized = p0.serialized_size() as u64;
    let persistent = |bytes: u64| serde_json::json!({ "persistent_bytes": bytes, "serialized_size": serialized });
    bench.point(
        "b1",
        "construct",
        "p0",
        u64::from(n),
        u64::from(n),
        bytes::per_element_materialization_bytes(n),
        persistent(0),
        || index.all_ordinals_bitmap(),
    );
    bench.point(
        "b1",
        "construct",
        "p0r",
        u64::from(n),
        u64::from(n),
        result_bytes,
        persistent(0),
        || index.all_ordinals_bitmap_by_range(),
    );
    bench.point(
        "b1",
        "construct",
        "p1",
        u64::from(n),
        u64::from(n),
        0,
        persistent(serialized),
        || black_box(prebuilt).len(),
    );
    bench.point(
        "b1",
        "construct",
        "p2",
        u64::from(n),
        u64::from(n),
        0,
        persistent(0),
        || index.candidate_set(&[]).len(),
    );

    let cells: Vec<Cell> = if tier == "500k" {
        all_cells()
    } else {
        all_cells()
            .into_iter()
            .filter(|c| FH_CELLS.contains(&c.name))
            .collect()
    };
    for cell in &cells {
        let baseline = execute(ctx, &plp_request(cell, CandidateMode::P0)).expect("p0 execute");
        for cand in CAND_MODES {
            let req = plp_request(cell, cand);
            let response = match execute(ctx, &req) {
                Ok(r) => r,
                Err(error) => {
                    bench.check(
                        "b1",
                        &format!("{}:{}", cell.name, cand.as_str()),
                        false,
                        Some(error),
                    );
                    continue;
                }
            };
            let equal = same_response(&response, &baseline);
            bench.check(
                "b1",
                &format!("{}:{}", cell.name, cand.as_str()),
                equal,
                (!equal)
                    .then(|| format!("num_found {} vs {}", response.num_found, baseline.num_found)),
            );
            if !equal {
                continue;
            }
            // Phase timers (decomposition only): mean over 50 extra calls.
            let mut phases = [0.0f64; 4];
            for _ in 0..50 {
                let d = execute(ctx, &req).expect("execute").diag;
                phases[0] += d.candidates_us / 50.0;
                phases[1] += d.facets_us / 50.0;
                phases[2] += d.sort_us / 50.0;
                phases[3] += d.total_us / 50.0;
            }
            let extra = serde_json::json!({
                "cell": cell.name,
                "role": cell.role.as_str(),
                "family": cell.family.as_str(),
                "num_found": response.num_found,
                "base_match_all": response.diag.base_match_all,
                "facet_paths": response.diag.facet_diag.iter().map(|f| f.path.clone()).collect::<Vec<_>>(),
                "sort_path": response.diag.sort_path,
                "phase_us": {"candidates": phases[0], "facets": phases[1], "sort": phases[2], "total": phases[3]},
            });
            bench.point(
                "b1",
                "pipeline",
                &format!("{}:{}", cell.name, cand.as_str()),
                response.num_found as u64,
                response.num_found as u64,
                0,
                extra,
                || execute(ctx, &req).expect("execute"),
            );
        }
    }
}

// ---------------------------------------------------------------- B2 ----

struct XorShift(u64);
impl XorShift {
    fn next_f64(&mut self) -> f64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        (x >> 11) as f64 / (1u64 << 53) as f64
    }
}

fn b2_sets(
    index: &CatalogIndex,
    category_id_by_leaf: &HashMap<String, CategoryId>,
) -> Vec<(String, String, RoaringBitmap)> {
    let n = index.ordinal_count() as u32;
    let mut sets: Vec<(String, String, RoaringBitmap)> =
        vec![("full".into(), "full".into(), index.all_ordinals_bitmap())];
    for cell in all_cells().into_iter().chain(reference_cells()) {
        let bitmap = index.indexed_candidates(&constraints_for(&cell, category_id_by_leaf));
        if bitmap.len() == u64::from(n) || sets.iter().any(|(_, _, b)| *b == bitmap) {
            continue;
        }
        sets.push((format!("real:{}", cell.name), "real".into(), bitmap));
    }
    for density in B2_DENSITIES {
        let mut rng = XorShift(B2_SEED ^ density.to_bits());
        let random: RoaringBitmap = (0..n).filter(|_| rng.next_f64() < density).collect();
        sets.push((format!("random:{density}"), "random".into(), random));
        let prefix: RoaringBitmap = (0..((f64::from(n) * density) as u32)).collect();
        sets.push((format!("prefix:{density}"), "prefix".into(), prefix));
    }
    sets
}

fn run_b2(
    bench: &mut Bench,
    index: &CatalogIndex,
    category_id_by_leaf: &HashMap<String, CategoryId>,
) {
    let sets = b2_sets(index, category_id_by_leaf);
    for (label, kind, set) in &sets {
        let c = set.len();
        let set_bytes = bytes::bitmap_bytes(set);
        bench.point(
            "b2",
            label,
            "d1_iterate",
            c,
            c,
            set_bytes,
            serde_json::json!({"set_kind": kind, "candidates": c}),
            || {
                let mut s = 0u64;
                for o in set {
                    s = s.wrapping_add(u64::from(o));
                }
                s
            },
        );
        for attr in B2_ATTRIBUTES {
            let column = index.enum_column(attr).expect("single-valued enum column");
            let dictionary = index.enum_dictionary(attr).expect("dictionary");
            let v = dictionary.len();
            let value_bitmaps: Vec<&RoaringBitmap> = dictionary
                .iter()
                .filter_map(|value| index.enum_value_bitmap(attr, value))
                .collect();
            let ordinal = index.facet_counts_ordinal(set, attr);
            let bitmap = index.facet_counts_bitmap(set, attr);
            let mut precomputed = vec![0u64; v];
            for o in set {
                let value = column[o as usize];
                if value != u32::MAX {
                    precomputed[value as usize] += 1;
                }
            }
            let from_counts: BTreeMap<String, u64> = precomputed
                .iter()
                .enumerate()
                .filter(|(_, &c)| c > 0)
                .map(|(i, &c)| (dictionary[i].clone(), c))
                .collect();
            let name = format!("{label}|{attr}");
            bench.check(
                "b2",
                &name,
                ordinal == bitmap && ordinal == from_counts,
                (ordinal != bitmap || ordinal != from_counts)
                    .then(|| "strategy outputs differ".to_owned()),
            );
            let nonzero = ordinal.len() as u64;
            let output_bytes: u64 = ordinal.keys().map(|k| k.len() as u64).sum();
            let gather = bytes::column_gather_bytes(set, 4);
            let extra = serde_json::json!({
                "set_kind": kind, "attribute": attr, "cardinality_v": v, "candidates": c,
                "density": c as f64 / index.ordinal_count() as f64, "nonzero_values": nonzero,
                "x_c_over_v": c as f64 / v as f64, "output_string_bytes": output_bytes,
                "column_gather_bytes": gather, "candidate_bytes": set_bytes,
            });
            bench.point(
                "b2",
                &name,
                "ordinal",
                c,
                nonzero,
                set_bytes + gather + 8 * v as u64 + output_bytes,
                extra.clone(),
                || index.facet_counts_ordinal(set, attr),
            );
            bench.point(
                "b2",
                &name,
                "bitmap",
                v as u64,
                nonzero,
                bytes::bitmap_facet_bytes(set, value_bitmaps.iter().copied()) + output_bytes,
                extra.clone(),
                || index.facet_counts_bitmap(set, attr),
            );
            bench.point(
                "b2",
                &name,
                "d2_iterate_gather",
                c,
                nonzero,
                set_bytes + gather,
                extra.clone(),
                || {
                    let mut s = 0u64;
                    for o in set {
                        s = s.wrapping_add(u64::from(column[o as usize]));
                    }
                    s
                },
            );
            bench.point(
                "b2",
                &name,
                "d3_count_u64",
                c,
                nonzero,
                set_bytes + gather + 8 * v as u64,
                extra.clone(),
                || {
                    let mut counts = vec![0u64; v];
                    for o in set {
                        let value = column[o as usize];
                        if value != u32::MAX {
                            counts[value as usize] += 1;
                        }
                    }
                    counts
                },
            );
            bench.point(
                "b2",
                &name,
                "d3_count_u32",
                c,
                nonzero,
                set_bytes + gather + 4 * v as u64,
                extra.clone(),
                || {
                    let mut counts = vec![0u32; v];
                    for o in set {
                        let value = column[o as usize];
                        if value != u32::MAX {
                            counts[value as usize] += 1;
                        }
                    }
                    counts
                },
            );
            bench.point(
                "b2",
                &name,
                "d5_materialize",
                v as u64,
                nonzero,
                8 * v as u64 + output_bytes,
                extra.clone(),
                || {
                    let mut out = BTreeMap::new();
                    for (i, &count) in precomputed.iter().enumerate() {
                        if count > 0 {
                            out.insert(dictionary[i].clone(), count);
                        }
                    }
                    out
                },
            );
            if kind == "full" {
                let dense_ordinal = index.facet_counts_ordinal_all(attr);
                let dense_bitmap = index.facet_counts_bitmap_all(attr);
                bench.check(
                    "b2",
                    &format!("{name}|dense"),
                    dense_ordinal == ordinal && dense_bitmap == ordinal,
                    None,
                );
                let column_bytes = 4 * column.len() as u64;
                // `len()` reads each container's stored cardinality only:
                // estimated as one 32-byte container header per chunk.
                let header_bytes: u64 = value_bitmaps
                    .iter()
                    .map(|b| 32 * bytes::chunk_cardinalities(b).len() as u64)
                    .sum();
                bench.point(
                    "b2",
                    &name,
                    "dense_ordinal_all",
                    c,
                    nonzero,
                    column_bytes + 8 * v as u64 + output_bytes,
                    extra.clone(),
                    || index.facet_counts_ordinal_all(attr),
                );
                bench.point(
                    "b2",
                    &name,
                    "dense_bitmap_len_all",
                    v as u64,
                    nonzero,
                    header_bytes + output_bytes,
                    extra.clone(),
                    || index.facet_counts_bitmap_all(attr),
                );
                bench.point(
                    "b2",
                    &name,
                    "d3_dense_count_u64",
                    c,
                    nonzero,
                    column_bytes + 8 * v as u64,
                    extra,
                    || {
                        let mut counts = vec![0u64; v];
                        for &value in column {
                            if value != u32::MAX {
                                counts[value as usize] += 1;
                            }
                        }
                        counts
                    },
                );
            }
        }
    }
}

// ----------------------------------------------------------------- C ----

fn oracle_ordinals(
    oracle: &Oracle,
    cell: &Cell,
    category_id_by_leaf: &HashMap<String, CategoryId>,
    source_ids: &HashMap<commerce_core::domain::ProductId, String>,
) -> Vec<u32> {
    let mut req = plp_request(cell, CandidateMode::P0);
    req.facets.clear();
    req.sort = None;
    oracle
        .expected(&req, category_id_by_leaf, source_ids)
        .expect("oracle")
        .candidate_ordinals
}

#[allow(clippy::too_many_lines)]
fn run_c(
    bench: &mut Bench,
    catalog: &Catalog,
    index: &CatalogIndex,
    oracle: &Oracle,
    category_id_by_leaf: &HashMap<String, CategoryId>,
    source_ids: &HashMap<commerce_core::domain::ProductId, String>,
    tier: &str,
) {
    let n = index.ordinal_count();
    // Exact lookup: 1,000 variant ids spread over the ordinal space.
    let ids: Vec<VariantId> = (0..1000)
        .map(|i| index.variant_id_at((i * n / 1000) as u32).expect("ordinal"))
        .collect();
    let mut expected_lookup = Vec::new();
    for id in &ids {
        let found = catalog.products.iter().find_map(|p| {
            p.variants
                .iter()
                .find(|v| v.id == *id)
                .map(|v| (p.id, v.id))
        });
        expected_lookup.push(found);
    }
    let actual_lookup: Vec<_> = ids
        .iter()
        .map(|id| {
            index
                .lookup_variant(catalog, *id)
                .map(|(p, v)| (p.id, v.id))
        })
        .collect();
    bench.check(
        "c",
        "exact_lookup",
        actual_lookup == expected_lookup && index.ordinal_of(ids[0]).is_some(),
        None,
    );
    bench.point(
        "c", "exact_lookup", "variant_id_to_ordinal_and_record", 1000, 1000, 1000 * 4 * bytes::CACHE_LINE,
        serde_json::json!({"lookups_per_op": 1000, "identifier_fields_accepted": index.identifier_field_count()}),
        || {
            let mut hits = 0u64;
            for id in &ids {
                if let (Some(_), Some(_)) = (index.ordinal_of(*id), index.lookup_variant(catalog, *id)) {
                    hits += 1;
                }
            }
            hits
        },
    );

    // Filters, conjunctions and ranges, as `/plp` builds them.
    let depth3 = [
        ("color", "white"),
        ("style", MODERN),
        ("primarymaterial", "metal"),
    ];
    let mut depth4 = depth3.to_vec();
    depth4.push(("shape", "square"));
    let review_sorted = index.numeric_sorted("review_count").expect("review_count");
    let p99 = review_sorted[(review_sorted.len() as f64 * 0.99) as usize].0;
    let make = |name: &'static str,
                category: Option<&'static str>,
                filters: Vec<(&'static str, &'static str)>,
                ranges: Vec<(&'static str, &'static str, f64)>| Cell {
        name,
        category,
        filters,
        ranges,
        ..reference_cells().remove(0)
    };
    let cases = vec![
        make(
            "single_bitmap_color_white",
            None,
            vec![("color", "white")],
            vec![],
        ),
        make(
            "single_bitmap_category_accent_chairs",
            Some(CATEGORY_BROAD),
            vec![],
            vec![],
        ),
        make("conjunction_3way", None, depth3.to_vec(), vec![]),
        make(
            "conjunction_5way",
            None,
            depth4.clone(),
            vec![("average_rating", "gte", 4.0)],
        ),
        make(
            "numeric_range_broad_rating_gte_4",
            None,
            vec![],
            vec![("average_rating", "gte", 4.0)],
        ),
        make(
            "numeric_range_narrow_review_count_p99",
            None,
            vec![],
            vec![("review_count", "gte", p99)],
        ),
    ];
    for case in &cases {
        let constraints = constraints_for(case, category_id_by_leaf);
        let result = index.indexed_candidates(&constraints);
        let expected = oracle_ordinals(oracle, case, category_id_by_leaf, source_ids);
        let actual: Vec<u32> = result.iter().collect();
        bench.check(
            "c",
            case.name,
            actual == expected,
            (actual != expected).then(|| format!("{} vs {}", actual.len(), expected.len())),
        );
        // Inputs read (approximately): each constraint's bitmap or range
        // slice, plus the output written.
        let mut read = 0u64;
        for c in &constraints {
            let single = index.indexed_candidates(std::slice::from_ref(c));
            read += match c {
                ResolvedConstraint::Attribute(Constraint::Numeric { .. }) => single.len() * 16,
                _ => bytes::bitmap_bytes(&single),
            };
        }
        let written = bytes::bitmap_bytes(&result);
        bench.point(
            "c",
            case.name,
            "indexed_candidates",
            constraints.len() as u64,
            result.len(),
            read + written,
            serde_json::json!({"constraints": constraints.len(), "p99_review_count": p99}),
            || index.indexed_candidates(&constraints),
        );
    }
    let white = index.enum_value_bitmap("color", "white").expect("white");
    bench.point(
        "c",
        "single_bitmap_color_white",
        "borrowed_len",
        1,
        white.len(),
        0,
        serde_json::json!({"note": "borrow + stored cardinality, no clone"}),
        || black_box(white).len(),
    );

    // Lexical residual (reference only).
    for query in [vec!["wood", "bed"], vec!["outdoor", "dining", "table"]] {
        let tokens: Vec<String> = query.iter().map(|t| (*t).to_owned()).collect();
        let result = index.lexical_and_candidates(&tokens);
        let mut expected = Vec::new();
        let mut ordinal = 0u32;
        for product in &catalog.products {
            for variant in &product.variants {
                let mut words: Vec<String> = tokenize(&product.title).collect();
                for value in effective_attributes(product, variant).values() {
                    if let AttributeValue::Text(t) = value {
                        words.extend(tokenize(t));
                    }
                }
                if tokens.iter().all(|t| words.contains(t)) {
                    expected.push(ordinal);
                }
                ordinal += 1;
            }
        }
        let actual: Vec<u32> = result.iter().collect();
        let name = format!("lexical_and:{}", query.join("+"));
        bench.check(
            "c",
            &name,
            actual == expected,
            (actual != expected).then(|| format!("{} vs {}", actual.len(), expected.len())),
        );
        let read: u64 = tokens
            .iter()
            .map(|t| bytes::bitmap_bytes(&index.lexical_and_candidates(std::slice::from_ref(t))))
            .sum();
        bench.point(
            "c",
            &name,
            "lexical_and_candidates",
            tokens.len() as u64,
            result.len(),
            read,
            serde_json::json!({"reference_only": true}),
            || index.lexical_and_candidates(&tokens),
        );
    }

    // Same-variant conjunction on the synthetic multi-variant expansion of
    // the 100k catalog (amendment 1, section 4).
    if tier == "100k" {
        let expanded = synthetic::expand(catalog);
        let expanded_index = CatalogIndex::build(&expanded);
        let (matches, traps) = synthetic::oracle(&expanded);
        let constraints: Vec<ResolvedConstraint> = [
            ("variant_color", synthetic::QUERY_COLOR),
            ("variant_size", synthetic::QUERY_SIZE),
        ]
        .iter()
        .map(|(a, v)| {
            ResolvedConstraint::Attribute(Constraint::Enum {
                attribute: (*a).to_owned(),
                value: (*v).to_owned(),
            })
        })
        .collect();
        let result = expanded_index.indexed_candidates(&constraints);
        let hits: Vec<VariantId> = result
            .iter()
            .map(|o| expanded_index.variant_id_at(o).expect("ordinal"))
            .collect();
        let false_matches = hits.iter().filter(|h| !matches.contains(h)).count();
        bench.check(
            "c",
            "same_variant_conjunction",
            hits == matches && false_matches == 0,
            Some(format!(
                "matches={} traps={} false_matches={false_matches}",
                matches.len(),
                traps
            )),
        );
        let read: u64 = constraints
            .iter()
            .map(|c| {
                bytes::bitmap_bytes(&expanded_index.indexed_candidates(std::slice::from_ref(c)))
            })
            .sum::<u64>()
            + bytes::bitmap_bytes(&result);
        bench.point(
            "c",
            "same_variant_conjunction",
            "indexed_candidates",
            2,
            result.len(),
            read,
            serde_json::json!({
                "synthetic": true, "variants": expanded_index.ordinal_count(),
                "products": expanded.products.len(), "trap_products": traps,
                "cross_variant_false_matches": false_matches,
            }),
            || expanded_index.indexed_candidates(&constraints),
        );
    }
}

fn main() {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("i63_primitives: {e}");
            std::process::exit(2);
        }
    };
    let wall_started = Instant::now();
    let started_utc = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let load_started = Instant::now();
    let data = load_dataset(&args.catalog, Dataset::Wands).expect("load catalog");
    let load_ms = load_started.elapsed().as_secs_f64() * 1e3;
    let build_started = Instant::now();
    let index = CatalogIndex::build(&data.catalog);
    let index_build_ms = build_started.elapsed().as_secs_f64() * 1e3;
    let mut structures = SortStructures::build(&index, &SORT_FIELDS, &SORT_FIELDS);
    structures.all_ordinals = Some(index.all_ordinals_bitmap());
    let category_id_by_leaf: HashMap<String, CategoryId> = data
        .catalog
        .products
        .iter()
        .filter_map(|p| {
            data.category_name(p.category)
                .map(|name| (name.to_owned(), p.category))
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
    let mut bench = Bench {
        min_ops: args.min_ops,
        min_cpu_ns: args.min_cpu_ns,
        points: Vec::new(),
        correctness: Vec::new(),
    };
    eprintln!(
        "i63_primitives tier={} run={} ordinals={}",
        args.tier,
        args.run,
        index.ordinal_count()
    );
    if args.parts.iter().any(|p| p == "b1") {
        run_b1(&mut bench, &ctx, &args.tier);
    }
    if args.parts.iter().any(|p| p == "b2") && args.tier == "500k" {
        run_b2(&mut bench, &index, &category_id_by_leaf);
    }
    if args.parts.iter().any(|p| p == "c") {
        let oracle = Oracle::new(&data.catalog);
        run_c(
            &mut bench,
            &data.catalog,
            &index,
            &oracle,
            &category_id_by_leaf,
            &data.source_id_by_product,
            &args.tier,
        );
    }
    let all_correct = bench.correctness.iter().all(|c| c.ok);
    let report = Report {
        schema_version: RAW_SCHEMA_VERSION,
        experiment_id: EXPERIMENT_ID,
        tier: args.tier.clone(),
        run: args.run,
        catalog: args.catalog.display().to_string(),
        ordinals: index.ordinal_count(),
        min_ops: args.min_ops,
        min_cpu_ns: args.min_cpu_ns,
        git_sha: command_stdout("git", &["rev-parse", "HEAD"]),
        hostname: command_stdout("hostname", &[]),
        started_utc,
        load_ms,
        index_build_ms,
        points: bench.points,
        correctness: bench.correctness,
        all_correct,
        total_wall_s: wall_started.elapsed().as_secs_f64(),
    };
    std::fs::write(
        &args.out,
        serde_json::to_string_pretty(&report).expect("json"),
    )
    .expect("write report");
    println!(
        "I63_PRIMITIVES tier={} run={} points={} correctness_checks={} all_correct={} wall_s={:.0}",
        report.tier,
        report.run,
        report.points.len(),
        report.correctness.len(),
        report.all_correct,
        report.total_wall_s
    );
    if !all_correct {
        std::process::exit(1);
    }
}
