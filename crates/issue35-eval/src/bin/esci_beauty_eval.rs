//! Issue #35 (`docs/experiments/ISSUE35_ESCI_BEAUTY_PROTOCOL.md`): a
//! third, materially different unseen-vertical slice (beauty/personal
//! care), completing Issue #35's own stated Workstream D goal of "at
//! least three materially different verticals" alongside
//! `esci_electronics_eval` and `esci_automotive_eval`.
//!
//! Reproduction: acquire the dataset first
//! (`bash scripts/datasets/fetch_esci_beauty.sh &&
//! python3 scripts/datasets/filter_esci_beauty.py &&
//! python3 scripts/datasets/solr_index_esci_beauty.py`), then
//! `cargo build --release -p issue35-eval &&
//! ./target/release/esci_beauty_eval`.

fn main() {
    let solr_base_url = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "http://localhost:8983/solr/esci_beauty_bench".to_string());
    issue35_eval::eval::run_vertical_eval(
        "beauty",
        "dataset_cache/esci_beauty/esci_beauty_products.jsonl",
        "dataset_cache/esci_beauty/esci_beauty_queries.jsonl",
        &solr_base_url,
    );
}
