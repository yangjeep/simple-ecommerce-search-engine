//! JSONL loader for the real WANDS export
//! (`scripts/datasets/prepare_wands.py`). No network access here: this
//! module only reads `dataset_cache/wands/catalog.jsonl` (gitignored --
//! see `scripts/datasets/fetch_wands.sh` and `prepare_wands.py` for how
//! it's produced).

use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct WandsProduct {
    pub id: String,
    pub title: String,
    pub description: Option<String>,
    pub product_class: Option<String>,
    pub category_leaf: Option<String>,
    pub category_depth_1: Option<String>,
    pub category_depth_2: Option<String>,
    pub category_depth_3: Option<String>,
    pub category_depth_4: Option<String>,
    pub category_depth_5: Option<String>,
    pub category_depth_6: Option<String>,
    pub color: Option<String>,
    pub style: Option<String>,
    pub primarymaterial: Option<String>,
    pub material: Option<String>,
    pub shape: Option<String>,
    pub rating_count: Option<f64>,
    pub average_rating: Option<f64>,
    pub review_count: Option<f64>,
}

impl WandsProduct {
    /// The per-depth category fields as `(depth, value)` pairs, skipping
    /// absent levels -- used both for ingestion and for building the
    /// real per-depth distinct-node-count distribution workload
    /// selection reads from.
    pub fn category_depths(&self) -> Vec<(u8, &str)> {
        [
            (1u8, &self.category_depth_1),
            (2, &self.category_depth_2),
            (3, &self.category_depth_3),
            (4, &self.category_depth_4),
            (5, &self.category_depth_5),
            (6, &self.category_depth_6),
        ]
        .into_iter()
        .filter_map(|(d, v)| v.as_deref().map(|s| (d, s)))
        .collect()
    }
}

pub fn load_catalog(path: &Path) -> Vec<WandsProduct> {
    let file =
        File::open(path).unwrap_or_else(|e| panic!("failed to open {}: {e}", path.display()));
    BufReader::new(file)
        .lines()
        .map(|line| {
            let line = line.expect("read line");
            serde_json::from_str(&line).unwrap_or_else(|e| panic!("bad catalog line: {e}: {line}"))
        })
        .collect()
}
