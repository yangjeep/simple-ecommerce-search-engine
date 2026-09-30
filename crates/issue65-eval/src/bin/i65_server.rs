//! Issue #65 N1: minimal concurrent native server and H1 router
//! (amendment 1 section 4). See `issue65_eval::server`.
//!
//! Usage: i65_server --catalog <jsonl> --port <p> --workers <W>
//!          [--solr-url http://127.0.0.1:8985]
//!
//! Builds the same physical structures as #79/#63/#64's N+ (sort columns and
//! presence bitmaps on average_rating/review_count/rating_count, frozen
//! tau_F/rho_S) and prints one JSON ready line.

use std::collections::HashMap;
use std::net::TcpListener;
use std::sync::Arc;
use std::time::Instant;

use commerce_core::domain::{CategoryId, Constraint, ProductId};
use commerce_core::index::CatalogIndex;
use commerce_core::ir::ResolvedConstraint;
use comparator_eval::translate::StructuralNames;
use issue61_eval::{load_dataset, Dataset};
use issue65_eval::server::{serve, State};
use issue65_eval::{RHO_S, TAU_F};
use issue77_eval::fixture::{build_fixture_catalog, oracle_queries};
use issue79_eval::cells::SORT_FIELDS;
use issue79_eval::plp::SortStructures;

fn arg(args: &[String], name: &str) -> Option<String> {
    args.windows(2).find(|w| w[0] == name).map(|w| w[1].clone())
}

/// #77's same-product cross-variant fixture check, unchanged in substance.
fn fixture_check() -> impl Fn() -> String + Send + Sync {
    let catalog = build_fixture_catalog();
    let index = CatalogIndex::build(&catalog);
    let labels: HashMap<u64, String> = catalog
        .products
        .iter()
        .zip(["A", "B", "C"])
        .map(|(p, l)| (p.id.0, l.to_owned()))
        .collect();
    move || {
        let mut all_passed = true;
        let mut checks = Vec::new();
        for query in oracle_queries() {
            let constraints: Vec<ResolvedConstraint> = query
                .filters
                .iter()
                .map(|(field, value)| {
                    ResolvedConstraint::Attribute(if *field == "available" {
                        Constraint::Boolean {
                            attribute: (*field).to_owned(),
                            value: *value == "true",
                        }
                    } else {
                        Constraint::Enum {
                            attribute: (*field).to_owned(),
                            value: (*value).to_owned(),
                        }
                    })
                })
                .collect();
            let candidates = index.indexed_candidates(&constraints);
            let mut actual: Vec<String> = index
                .candidate_product_ids(&candidates)
                .iter()
                .filter_map(|p| labels.get(&p.0).cloned())
                .collect();
            actual.sort();
            let mut expected: Vec<String> = query
                .expected_product_ids
                .iter()
                .map(|s| (*s).to_owned())
                .collect();
            expected.sort();
            let passed = actual == expected;
            all_passed &= passed;
            checks.push(serde_json::json!({"query": query.name, "passed": passed}));
        }
        serde_json::json!({"all_passed": all_passed, "checks": checks}).to_string()
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let catalog_path = arg(&args, "--catalog").expect("--catalog");
    let port: u16 = arg(&args, "--port").expect("--port").parse().expect("port");
    let workers: usize = arg(&args, "--workers").map_or(3, |w| w.parse().expect("workers"));
    let solr_url = arg(&args, "--solr-url");

    let started = Instant::now();
    let data = load_dataset(std::path::Path::new(&catalog_path), Dataset::Wands).expect("load");
    let load_ms = started.elapsed().as_secs_f64() * 1e3;
    let built = Instant::now();
    let index = CatalogIndex::build(&data.catalog);
    let structures = SortStructures::build(&index, &SORT_FIELDS, &SORT_FIELDS);
    let build_ms = built.elapsed().as_secs_f64() * 1e3;
    let category_id_by_leaf: HashMap<String, CategoryId> = data
        .catalog
        .products
        .iter()
        .filter_map(|p| {
            data.category_name(p.category)
                .map(|n| (n.to_owned(), p.category))
        })
        .collect();
    let product_by_source_id: HashMap<String, ProductId> = data
        .source_id_by_product
        .iter()
        .map(|(p, s)| (s.clone(), *p))
        .collect();
    let docs = data.catalog.products.len();
    let state = Arc::new(State {
        index,
        category_id_by_leaf,
        source_id_by_product: data.source_id_by_product,
        product_by_source_id,
        structures,
        tau_f: Some(TAU_F),
        rho_s: Some(RHO_S),
        solr_url: solr_url.clone(),
        correctness: Some(Box::new(fixture_check())),
        catalog: data.catalog,
    });
    let listener = TcpListener::bind(("0.0.0.0", port)).expect("bind");
    println!(
        "{}",
        serde_json::json!({
            "i65_ready": true, "docs": docs, "workers": workers, "solr_url": solr_url,
            "load_ms": load_ms, "build_ms": build_ms, "tau_f": TAU_F, "rho_s": RHO_S,
            "cand_mode": "p0r (requested per request)",
        })
    );
    std::io::Write::flush(&mut std::io::stdout()).expect("flush");
    serve(listener, state, workers);
}
