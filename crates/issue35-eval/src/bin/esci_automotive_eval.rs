//! Issue #35 (`docs/experiments/ISSUE35_ESCI_AUTOMOTIVE_PROTOCOL.md`):
//! a second, materially different unseen-vertical slice (automotive
//! parts/accessories), testing the same H0 as the electronics slice
//! (`esci_electronics_eval`) on genuinely different real data, building
//! toward Issue #35's own stated Workstream D goal of "at least three
//! materially different verticals."
//!
//! Reproduction: acquire the dataset first
//! (`bash scripts/datasets/fetch_esci_automotive.sh &&
//! python3 scripts/datasets/filter_esci_automotive.py &&
//! python3 scripts/datasets/solr_index_esci_automotive.py`), then
//! `cargo build --release -p issue35-eval &&
//! ./target/release/esci_automotive_eval`.

fn main() {
    let solr_base_url = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "http://localhost:8983/solr/esci_automotive_bench".to_string());
    issue35_eval::eval::run_vertical_eval(
        "automotive",
        "dataset_cache/esci_automotive/esci_automotive_products.jsonl",
        "dataset_cache/esci_automotive/esci_automotive_queries.jsonl",
        &solr_base_url,
    );
}
