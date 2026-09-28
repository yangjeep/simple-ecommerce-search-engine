//! Issue #79 (Infra E3b) correctness gate (preregistration section 6).
//!
//! Usage: e3b_correctness_gate --catalog <jsonl> --out <json>
//!          [--tau-f <x>] [--rho-s <x>]
//!
//! Loads the catalog, builds the index plus every optional E3b structure,
//! and for every preregistered cell -- plus extra asc / offset probes --
//! runs every variant through the same `plp::execute` the server uses,
//! comparing against the independent `oracle` (a `Catalog` scan):
//! filter result ordinals, `num_found`, facet maps (all five WANDS facet
//! fields, disjunctive self-exclusion, nulls never counted, zeros omitted),
//! and the exact top-K `(id, sort_value)` sequence. Hybrid modes are
//! exercised at several constants (both branches) as well as at any
//! calibrated constant passed in. Checks are attributed per component: a
//! non-legacy facet mode must match facets/filter-IDs/num_found exactly, a
//! non-legacy sort mode must match the exact top-K docs. Mismatches coming
//! only from a *legacy* (N0) component are recorded as baseline divergences,
//! not candidate failures -- N0's asc comparator puts missing values first
//! (Option ordering), which the preregistered semantics define as last.
//! Exit code 1 if any candidate component fails.

use commerce_core::domain::CategoryId;
use commerce_core::index::CatalogIndex;
use comparator_eval::translate::StructuralNames;
use issue61_eval::{load_dataset, Dataset};
use issue79_eval::cells::{all_cells, Cell, Family, SORT_FIELDS};
use issue79_eval::oracle::Oracle;
use issue79_eval::plp::{execute, FacetMode, PlpContext, PlpRequest, SortMode, SortStructures};
use serde::Serialize;
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Serialize)]
struct Check {
    case: String,
    facet_mode: FacetMode,
    sort_mode: SortMode,
    tau_f: Option<f64>,
    rho_s: Option<f64>,
    is_baseline: bool,
    /// Every component this variant changed matches the oracle.
    candidate_components_ok: bool,
    /// A legacy (N0) component this variant kept diverges from the oracle.
    baseline_component_divergence: bool,
    num_found_ok: bool,
    candidates_ok: bool,
    facets_ok: bool,
    docs_ok: bool,
    passed: bool,
    detail: Option<String>,
}

#[derive(Serialize)]
struct Report {
    catalog: String,
    rows: usize,
    nan_numeric_values: usize,
    single_valued_enum: HashMap<String, bool>,
    checks: Vec<Check>,
    candidate_checks: usize,
    candidate_failures: usize,
    baseline_mismatches: usize,
    all_candidates_passed: bool,
}

fn request_for(cell: &Cell, offset: usize, sort_override: Option<(&str, bool)>) -> PlpRequest {
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
        sort: sort_override.or(cell.sort).map(|(f, d)| (f.to_owned(), d)),
        top_k: cell.top_k,
        offset,
        facet_mode: FacetMode::Legacy,
        sort_mode: SortMode::Legacy,
    }
}

fn arg(args: &[String], name: &str) -> Option<String> {
    args.windows(2)
        .find(|pair| pair[0] == name)
        .map(|pair| pair[1].clone())
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let catalog_path = PathBuf::from(arg(&args, "--catalog").expect("--catalog"));
    let out = PathBuf::from(arg(&args, "--out").expect("--out"));
    let calibrated_tau = arg(&args, "--tau-f").map(|v| v.parse::<f64>().expect("tau"));
    let calibrated_rho = arg(&args, "--rho-s").map(|v| v.parse::<f64>().expect("rho"));

    let data = load_dataset(&catalog_path, Dataset::Wands).expect("load catalog");
    let index = CatalogIndex::build(&data.catalog);
    let structures = SortStructures::build(&index, &SORT_FIELDS, &SORT_FIELDS);
    let category_id_by_leaf: HashMap<String, CategoryId> = data
        .catalog
        .products
        .iter()
        .filter_map(|p| {
            data.category_name(p.category)
                .map(|name| (name.to_owned(), p.category))
        })
        .collect();
    let oracle = Oracle::new(&data.catalog);
    assert_eq!(
        oracle.row_count(),
        index.ordinal_count(),
        "oracle row/ordinal mismatch"
    );

    // Cases: every preregistered cell, plus probes that exercise asc,
    // offset and nulls-last on the sort paths.
    let mut cases: Vec<(String, PlpRequest)> = Vec::new();
    for cell in all_cells() {
        cases.push((cell.name.to_owned(), request_for(&cell, 0, None)));
        if cell.family == Family::Sort {
            let (field, descending) = cell.sort.expect("sort cell");
            cases.push((
                format!("{}+flipped_direction", cell.name),
                request_for(&cell, 0, Some((field, !descending))),
            ));
            cases.push((
                format!("{}+offset10", cell.name),
                request_for(&cell, 10, None),
            ));
        } else {
            cases.push((
                format!("{}+sorted_rating_count_asc", cell.name),
                request_for(&cell, 0, Some(("rating_count", false))),
            ));
        }
    }

    let mut taus: Vec<Option<f64>> = vec![Some(0.0), Some(1.0), Some(100.0), Some(1e12)];
    let mut rhos: Vec<Option<f64>> = vec![Some(0.0), Some(1.0), Some(1e12)];
    if calibrated_tau.is_some() {
        taus.push(calibrated_tau);
    }
    if calibrated_rho.is_some() {
        rhos.push(calibrated_rho);
    }

    let mut checks = Vec::new();
    for (case, base_req) in &cases {
        let expected = oracle
            .expected(base_req, &category_id_by_leaf, &data.source_id_by_product)
            .expect("oracle");
        // Filter result IDs are compared once per case, directly against the
        // index's candidate bitmap (every variant shares that retrieval).
        let mut variants: Vec<(FacetMode, SortMode, Option<f64>, Option<f64>)> = Vec::new();
        for facet in [FacetMode::Legacy, FacetMode::Ordinal, FacetMode::Bitmap] {
            for sort in [SortMode::Legacy, SortMode::Topk, SortMode::Presorted] {
                variants.push((facet, sort, None, None));
            }
        }
        for tau in &taus {
            for rho in &rhos {
                variants.push((FacetMode::Hybrid, SortMode::Hybrid, *tau, *rho));
            }
        }
        for (facet_mode, sort_mode, tau_f, rho_s) in variants {
            let req = PlpRequest {
                facet_mode,
                sort_mode,
                ..base_req.clone()
            };
            let ctx = PlpContext {
                catalog: &data.catalog,
                index: &index,
                category_id_by_leaf: &category_id_by_leaf,
                source_id_by_product: &data.source_id_by_product,
                structures: &structures,
                tau_f,
                rho_s,
            };
            let is_baseline = facet_mode == FacetMode::Legacy && sort_mode == SortMode::Legacy;
            let mut check = Check {
                case: case.clone(),
                facet_mode,
                sort_mode,
                tau_f,
                rho_s,
                is_baseline,
                candidate_components_ok: false,
                baseline_component_divergence: false,
                num_found_ok: false,
                candidates_ok: false,
                facets_ok: false,
                docs_ok: false,
                passed: false,
                detail: None,
            };
            match execute(&ctx, &req) {
                Ok(response) => {
                    check.num_found_ok = response.num_found == expected.num_found;
                    check.facets_ok = response.facets == expected.facets;
                    check.docs_ok = response.docs == expected.docs;
                    let constraints_ok = {
                        // Recompute the base candidate bitmap the same way
                        // `execute` does and compare ordinals to the oracle.
                        let bitmap_ordinals: Vec<u32> = {
                            use commerce_core::domain::{Constraint, NumericOp};
                            use commerce_core::ir::{ResolvedConstraint, StructuralConstraint};
                            let mut cs = Vec::new();
                            if let Some(leaf) = &req.category {
                                cs.push(ResolvedConstraint::Structural(
                                    StructuralConstraint::Category(category_id_by_leaf[leaf]),
                                ));
                            }
                            for (a, v) in &req.filters {
                                cs.push(ResolvedConstraint::Attribute(Constraint::Enum {
                                    attribute: a.clone(),
                                    value: v.clone(),
                                }));
                            }
                            for (a, o, v) in &req.ranges {
                                assert_eq!(o, "gte", "gate only builds gte ranges");
                                cs.push(ResolvedConstraint::Attribute(Constraint::Numeric {
                                    attribute: a.clone(),
                                    op: NumericOp::Gte,
                                    value: *v,
                                }));
                            }
                            index.indexed_candidates(&cs).iter().collect()
                        };
                        bitmap_ordinals == expected.candidate_ordinals
                    };
                    check.candidates_ok = constraints_ok;
                    check.passed = check.num_found_ok
                        && check.candidates_ok
                        && check.facets_ok
                        && check.docs_ok;
                    let retrieval_ok = check.num_found_ok && check.candidates_ok;
                    let facet_changed = facet_mode != FacetMode::Legacy;
                    let sort_changed = sort_mode != SortMode::Legacy;
                    check.candidate_components_ok = retrieval_ok
                        && (!facet_changed || check.facets_ok)
                        && (!sort_changed || check.docs_ok);
                    check.baseline_component_divergence =
                        (!facet_changed && !check.facets_ok) || (!sort_changed && !check.docs_ok);
                    if !check.passed {
                        check.detail = Some(format!(
                            "num_found {} vs {}; first doc {:?} vs {:?}",
                            response.num_found,
                            expected.num_found,
                            response.docs.first(),
                            expected.docs.first()
                        ));
                    }
                }
                Err(error) => check.detail = Some(error),
            }
            checks.push(check);
        }
    }

    let candidate: Vec<&Check> = checks.iter().filter(|c| !c.is_baseline).collect();
    let candidate_failures = candidate
        .iter()
        .filter(|c| !c.candidate_components_ok)
        .count();
    let report = Report {
        catalog: catalog_path.display().to_string(),
        rows: oracle.row_count(),
        nan_numeric_values: oracle.nan_numeric_values(),
        single_valued_enum: ["color", "style", "primarymaterial", "material", "shape"]
            .iter()
            .map(|f| ((*f).to_owned(), index.attribute_is_single_valued_enum(f)))
            .collect(),
        candidate_checks: candidate.len(),
        candidate_failures,
        baseline_mismatches: checks
            .iter()
            .filter(|c| c.baseline_component_divergence)
            .count(),
        all_candidates_passed: candidate_failures == 0,
        checks,
    };
    std::fs::write(&out, serde_json::to_string_pretty(&report).expect("json")).expect("write");
    println!(
        "E3B_GATE rows={} nan={} candidate_checks={} candidate_failures={} baseline_mismatches={}",
        report.rows,
        report.nan_numeric_values,
        report.candidate_checks,
        report.candidate_failures,
        report.baseline_mismatches
    );
    for c in report.checks.iter().filter(|c| !c.candidate_components_ok) {
        println!(
            "  {} {} {:?}/{:?} tau={:?} rho={:?} :: {:?}",
            if c.is_baseline || c.baseline_component_divergence {
                "BASELINE_DIVERGENCE"
            } else {
                "CANDIDATE_FAILURE"
            },
            c.case,
            c.facet_mode,
            c.sort_mode,
            c.tau_f,
            c.rho_s,
            c.detail
        );
    }
    if report.nan_numeric_values != 0 || !report.all_candidates_passed {
        std::process::exit(1);
    }
}
