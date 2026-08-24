//! Phase 6A sanity check: ingest the real WANDS catalog, build
//! `CatalogIndex`, and report basic real-data statistics (product/
//! category/product_type cardinality, index build time/size) before any
//! benchmark work -- mirrors `round1_eval::bin::profile_catalog`'s role
//! for ESCI in Round 1.
//!
//! Usage: cargo run --release -p phase6a-eval --bin p6a_profile_catalog
//!        [path/to/catalog.jsonl]

use std::path::PathBuf;
use std::time::Instant;

use commerce_core::index::CatalogIndex;
use phase6a_eval::{catalog, data};

fn main() {
    let path = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("dataset_cache/wands/catalog.jsonl"));

    println!("loading {path:?}...");
    let load_start = Instant::now();
    let products = data::load_catalog(&path);
    println!(
        "loaded {} products in {:.2}s",
        products.len(),
        load_start.elapsed().as_secs_f64()
    );

    let build_start = Instant::now();
    let ingested = catalog::build_catalog(&products);
    println!(
        "mapped to commerce_core::Catalog in {:.2}s",
        build_start.elapsed().as_secs_f64()
    );
    println!(
        "distinct categories (leaf-level): {}",
        ingested.categories.len()
    );
    println!(
        "distinct product_types (product_class): {}",
        ingested.product_types.len()
    );

    let index_start = Instant::now();
    let index = CatalogIndex::build(&ingested.catalog);
    println!(
        "CatalogIndex::build in {:.2}s, approximate_size={} bytes",
        index_start.elapsed().as_secs_f64(),
        index.approximate_size_bytes()
    );

    let all = index.indexed_candidates(&[]);
    println!(
        "indexed_candidates([]) = {} (should equal product count)",
        all.len()
    );

    let color_facets = commerce_core::index::CatalogIndex::facet_counts(&index, "color", &all);
    println!(
        "distinct color values with >=1 candidate: {}",
        color_facets.len()
    );
    let top5: Vec<_> = {
        let mut v: Vec<_> = color_facets.iter().collect();
        v.sort_by(|a, b| b.1.cmp(a.1));
        v.into_iter().take(5).collect()
    };
    println!("top 5 colors: {top5:?}");

    let cat_facets = index.category_facet_counts(&all);
    println!(
        "distinct leaf categories with >=1 candidate: {}",
        cat_facets.len()
    );
    let pt_facets = index.product_type_facet_counts(&all);
    println!(
        "distinct product_types with >=1 candidate: {}",
        pt_facets.len()
    );
}
