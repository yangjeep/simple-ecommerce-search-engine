//! Issue #63 (amendment 1, section 2): the equal-work verification. Every
//! engine dump written under `I63_DUMP_DIR` (`<engine>__<cell>.json`, or
//! `native__<cell>__<mode>.json`) is compared with the independent oracle's
//! `num_found` and complete facet maps for that cell.
//!
//! Usage: i63_facet_equivalence --catalog <jsonl> --dump-dir <dir> --out <json>
//!
//! Exit code 1 if any dump is `NOT_EQUIVALENT_WORK` or unreadable; the
//! report is always written first.

use std::collections::HashMap;
use std::path::PathBuf;

use commerce_core::domain::CategoryId;
use comparator_eval::translate::StructuralNames;
use issue61_eval::{load_dataset, Dataset};
use issue63_eval::equivalence::{compare, Verdict};
use issue79_eval::cells::{all_cells, reference_cells, Cell};
use issue79_eval::oracle::Oracle;
use issue79_eval::plp::{CandidateMode, FacetMode, PlpRequest, SortMode};
use serde::Serialize;

#[derive(Serialize)]
struct Report {
    catalog: String,
    dump_dir: String,
    verdicts: Vec<Verdict>,
    unreadable: Vec<String>,
    all_equivalent: bool,
}

fn arg(args: &[String], name: &str) -> Option<String> {
    args.windows(2).find(|w| w[0] == name).map(|w| w[1].clone())
}

fn request(cell: &Cell) -> PlpRequest {
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
        facet_mode: FacetMode::Legacy,
        sort_mode: SortMode::Legacy,
        cand_mode: CandidateMode::P0,
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let catalog = PathBuf::from(arg(&args, "--catalog").expect("--catalog"));
    let dump_dir = PathBuf::from(arg(&args, "--dump-dir").expect("--dump-dir"));
    let out = PathBuf::from(arg(&args, "--out").expect("--out"));

    let data = load_dataset(&catalog, Dataset::Wands).expect("load catalog");
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
    let cells: Vec<Cell> = all_cells().into_iter().chain(reference_cells()).collect();
    let mut expected_cache = HashMap::new();

    let mut files: Vec<PathBuf> = std::fs::read_dir(&dump_dir)
        .expect("read dump dir")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    files.sort();
    let mut verdicts = Vec::new();
    let mut unreadable = Vec::new();
    for path in files {
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or_default();
        let mut parts = stem.split("__");
        let (Some(engine), Some(cell_name)) = (parts.next(), parts.next()) else {
            unreadable.push(format!("{}: bad file name", path.display()));
            continue;
        };
        let engine = match parts.next() {
            Some(mode) => format!("{engine}:{mode}"),
            None => engine.to_owned(),
        };
        let Some(cell) = cells.iter().find(|c| c.name == cell_name) else {
            unreadable.push(format!("{}: unknown cell {cell_name}", path.display()));
            continue;
        };
        let dump: serde_json::Value = match std::fs::read_to_string(&path)
            .map_err(|e| e.to_string())
            .and_then(|t| serde_json::from_str(&t).map_err(|e| e.to_string()))
        {
            Ok(v) => v,
            Err(e) => {
                unreadable.push(format!("{}: {e}", path.display()));
                continue;
            }
        };
        let expected = expected_cache.entry(cell.name).or_insert_with(|| {
            oracle
                .expected(&request(cell), &category_id_by_leaf, &data.source_id_by_product)
                .expect("oracle")
        });
        verdicts.push(compare(
            &engine,
            cell.name,
            &dump,
            expected.num_found as u64,
            &expected.facets,
        ));
    }
    let all_equivalent = unreadable.is_empty() && verdicts.iter().all(|v| v.equivalent);
    for v in &verdicts {
        println!(
            "{} {} {}: num_found {:?}/{} facets_exact={} id_only={} requests={:?}",
            if v.equivalent {
                "EQUIVALENT"
            } else {
                "NOT_EQUIVALENT_WORK"
            },
            v.engine,
            v.cell,
            v.num_found_returned,
            v.num_found_expected,
            v.facets_exact,
            v.hits_id_only,
            v.backend_requests
        );
    }
    for u in &unreadable {
        println!("UNREADABLE {u}");
    }
    let report = Report {
        catalog: catalog.display().to_string(),
        dump_dir: dump_dir.display().to_string(),
        verdicts,
        unreadable,
        all_equivalent,
    };
    std::fs::write(&out, serde_json::to_string_pretty(&report).expect("json")).expect("write");
    println!("I63_EQUIVALENCE all_equivalent={}", report.all_equivalent);
    if !report.all_equivalent {
        std::process::exit(1);
    }
}
