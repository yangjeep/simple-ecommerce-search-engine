//! Issue #65: builds the frozen A-F request pools with oracle expectations
//! (amendment 1 section 8) and writes them to one JSON file.
//!
//! Usage: i65_workload --catalog <jsonl> --queries <query.csv> --out <pools.json>

use std::collections::HashMap;
use std::path::Path;

use commerce_core::domain::CategoryId;
use comparator_eval::translate::StructuralNames;
use issue61_eval::{load_dataset, Dataset};
use issue65_eval::workload::build;

fn arg(args: &[String], name: &str) -> String {
    args.windows(2)
        .find(|w| w[0] == name)
        .map(|w| w[1].clone())
        .unwrap_or_else(|| panic!("missing {name}"))
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let catalog_path = arg(&args, "--catalog");
    let data = load_dataset(Path::new(&catalog_path), Dataset::Wands).expect("load");
    let category_id_by_leaf: HashMap<String, CategoryId> = data
        .catalog
        .products
        .iter()
        .filter_map(|p| {
            data.category_name(p.category)
                .map(|n| (n.to_owned(), p.category))
        })
        .collect();
    // WANDS query.csv: tab-separated `query_id  query  query_class`, header row.
    let queries: Vec<String> = std::fs::read_to_string(arg(&args, "--queries"))
        .expect("queries")
        .lines()
        .skip(1)
        .filter_map(|l| l.split('\t').nth(1).map(|q| q.trim().to_owned()))
        .filter(|q| !q.is_empty())
        .collect();
    let pools = build(
        &data.catalog,
        &catalog_path,
        &category_id_by_leaf,
        &data.source_id_by_product,
        &queries,
    );
    let mut counts: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
    for r in &pools.requests {
        *counts.entry(r.class.as_str()).or_insert(0) += 1;
    }
    std::fs::write(
        arg(&args, "--out"),
        serde_json::to_string_pretty(&pools).expect("json"),
    )
    .expect("write");
    println!(
        "I65_WORKLOAD requests={} classes={counts:?} pools_sha256={}",
        pools.requests.len(),
        pools.hash()
    );
}
