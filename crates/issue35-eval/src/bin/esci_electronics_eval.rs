//! Issue #35 (`docs/experiments/ISSUE35_ESCI_ELECTRONICS_PROTOCOL.md`):
//! does this project's existing, unmodified discovery/serving pipeline
//! behave safely and sanely on a real, genuinely different commerce
//! vertical (electronics, via a real ESCI slice) with zero
//! `commerce-core` changes and zero hand-authored vertical ontology?
//!
//! Reproduction: acquire the dataset first
//! (`bash scripts/datasets/fetch_esci_electronics.sh &&
//! python3 scripts/datasets/filter_esci_electronics.py &&
//! python3 scripts/datasets/solr_index_esci_electronics.py`), then
//! `cargo build --release -p issue35-eval &&
//! ./target/release/esci_electronics_eval`.
//!
//! Thin wrapper around `issue35_eval::eval::run_vertical_eval` (the
//! shared measurement procedure a second/third vertical slice binary
//! also calls) -- kept as its own binary so this checkpoint's own
//! recorded reproduction command keeps working unchanged.

fn main() {
    let solr_base_url = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "http://localhost:8983/solr/esci_electronics_bench".to_string());
    issue35_eval::eval::run_vertical_eval(
        "electronics",
        "dataset_cache/esci_electronics/esci_electronics_products.jsonl",
        "dataset_cache/esci_electronics/esci_electronics_queries.jsonl",
        &solr_base_url,
    );
}
