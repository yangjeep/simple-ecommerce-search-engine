//! JSONL loaders for the real ESCI export (`scripts/round1/export_esci.py`).
//! No network access here: this module only reads files already on disk
//! under `dataset_cache/export/` (gitignored — see
//! `docs/experiments/ROUND1_LOG.md` for how they're produced).

use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct RealProduct {
    pub id: String,
    pub title: String,
    pub description: Option<String>,
    pub bullets: Option<String>,
    pub brand: Option<String>,
    pub color: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EsciLabel {
    Exact,
    Substitute,
    Complement,
    Irrelevant,
}

impl EsciLabel {
    fn parse(raw: &str) -> Option<Self> {
        match raw {
            "E" => Some(EsciLabel::Exact),
            "S" => Some(EsciLabel::Substitute),
            "C" => Some(EsciLabel::Complement),
            "I" => Some(EsciLabel::Irrelevant),
            _ => None,
        }
    }

    /// A coarse binary view used for precision/relevance scoring: Exact or
    /// Substitute count as a relevant result for the shopper's query,
    /// Complement/Irrelevant do not. This mirrors how the ESCI benchmark's
    /// own binary-relevance track is commonly derived from the 4-way label.
    pub fn is_relevant(self) -> bool {
        matches!(self, EsciLabel::Exact | EsciLabel::Substitute)
    }
}

#[derive(Debug, Clone)]
pub struct JudgedExample {
    pub query_id: u64,
    pub query: String,
    pub product_id: String,
    pub label: EsciLabel,
}

#[derive(Deserialize)]
struct RawExample {
    query_id: u64,
    query: String,
    product_id: String,
    label: String,
}

fn open_lines(path: &Path) -> BufReader<File> {
    let file =
        File::open(path).unwrap_or_else(|e| panic!("failed to open {}: {e}", path.display()));
    BufReader::new(file)
}

pub fn load_catalog(path: &Path) -> Vec<RealProduct> {
    open_lines(path)
        .lines()
        .map(|line| {
            let line = line.expect("read line");
            serde_json::from_str(&line).unwrap_or_else(|e| panic!("bad catalog line: {e}: {line}"))
        })
        .collect()
}

/// Load judged (query, product, label) triples, dropping any row whose
/// label doesn't parse as one of the four ESCI classes (none observed in
/// practice, but the loader must not silently misinterpret a malformed
/// row as a specific relevance judgment).
pub fn load_queries(path: &Path) -> Vec<JudgedExample> {
    open_lines(path)
        .lines()
        .filter_map(|line| {
            let line = line.expect("read line");
            let raw: RawExample = serde_json::from_str(&line)
                .unwrap_or_else(|e| panic!("bad query line: {e}: {line}"));
            EsciLabel::parse(&raw.label).map(|label| JudgedExample {
                query_id: raw.query_id,
                query: raw.query,
                product_id: raw.product_id,
                label,
            })
        })
        .collect()
}
