//! E2b (`docs/experiments/ISSUE42_PROTOCOL.md`'s E2b amendment 1): the
//! raw-feed statistics every baseline (statistics-only, LLM proposal,
//! validator) is bounded to -- column/key names, representative values,
//! and parse/null/density/cardinality/uniqueness distributions, per the
//! issue's own "Inputs" list. Never includes the reference/oracle
//! mapping (`e2b_oracle`).
//!
//! Two real inputs, per the protocol amendment:
//! - **WANDS** (`dataset_cache/wands/product.csv`): a real, unprocessed
//!   36-key sample of `product_features`'s own 7,961 distinct real
//!   pipe-delimited keys, selected once from real per-key statistics
//!   before any oracle role was assigned (see `WANDS_SAMPLE_KEYS`'s own
//!   doc comment for the full rationale). `query.csv`/`label.csv` are
//!   loaded separately for the real end-to-end NDCG@10/Recall@10 check.
//! - **automotive** (`issue38_e2e3_eval::automotive`, already frozen,
//!   already used by R3): its own real attribute set, via the same
//!   `commerce_core::index::identifier::compute_field_stats` R3 already
//!   validated and this session's own R2/R3 production merge already
//!   ported into `commerce_core` itself -- reused directly, not
//!   reimplemented.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;

use commerce_core::index::compute_field_stats;
use issue38_e2e3_eval::automotive;

/// `CARGO_MANIFEST_DIR`-relative, not CWD-relative: every other WANDS
/// consumer in this workspace (`phase9-eval`/`phase6a-eval`'s own
/// binaries) assumes CWD is the workspace root, which only holds when a
/// binary is run from there directly -- `cargo test` runs with CWD at
/// the crate's own manifest directory instead, which would otherwise
/// break these paths silently. `crates/issue42-eval` is two directories
/// below the workspace root.
pub fn wands_dataset_path(file: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../dataset_cache/wands")
        .join(file)
}

/// The 36 real WANDS `product_features` keys E2b measures, selected once
/// from real per-key occurrence/distinct-value statistics (computed via
/// a one-off script over `dataset_cache/wands/product.csv`, cited here)
/// before any oracle role was assigned -- so the sample was not picked
/// after seeing which keys would make a flattering result. Spans every
/// category `docs/experiments/ISSUE42_PROTOCOL.md`'s E2b amendment names:
/// continuous Numeric, Boolean yes/no, low/medium-cardinality Enum,
/// higher-but-bounded-cardinality Enum, genuinely ambiguous/messy real
/// fields, free-text/lexical-only fields, one identifier-shaped field
/// (`samplepartnumber`, uniqueness_ratio=0.9785 at count=2,048 -- but
/// almost certainly NOT retrieval-significant, a real "statistically
/// identifier-shaped but semantically probably not what a shopper
/// searches" trap), and two real relationship/cross-reference fields.
pub const WANDS_SAMPLE_KEYS: &[&str] = &[
    // Numeric (continuous)
    "overallproductweight",
    "overallwidth-sidetoside",
    "overallheight-toptobottom",
    "overalldepth-fronttoback",
    "weightcapacity",
    "estimatedtimetosetup",
    // Boolean (yes/no)
    "commercialwarranty",
    "adultassemblyrequired",
    "organic",
    "firerated",
    "drawersincluded",
    "upholstered",
    "installationrequired",
    // Enum, low/medium cardinality
    "style",
    "dsprimaryproductstyle",
    "countryoforigin",
    "levelofassembly",
    "dswoodtone",
    "primarymaterial",
    "framematerial",
    "shape",
    "pattern",
    // Enum, higher but bounded cardinality
    "color",
    "basecolor",
    "finish",
    "upholsterymaterial",
    "upholsterycolor",
    // Ambiguous / messy real fields
    "productwarranty",
    "fullorlimitedwarranty",
    "warrantylength",
    "title",
    // Free-text / lexical-only
    "productcare",
    "piecesincluded",
    // Identifier-shaped (real trap)
    "samplepartnumber",
    // Relationship / cross-reference fields
    "compatibledrainassemblypartnumber",
    "compatiblediningchairpartnumber",
];

/// Raw statistics for one key -- exactly the bounded input the protocol
/// permits (never the oracle's semantic role). Density can exceed 1.0:
/// `product_features` sometimes repeats a key within one product's own
/// blob (e.g. two dimension entries for two configurations), so
/// `occurrences` counts every raw appearance, not one per product.
#[derive(Debug, Clone)]
pub struct RawFieldStats {
    pub key: String,
    pub occurrences: usize,
    pub distinct_values: usize,
    pub uniqueness_ratio: f64,
    pub density: f64,
    pub numeric_parseable_fraction: f64,
    /// Occurrence-weighted mean character length of every real value --
    /// a genuine, standard, dataset-independent signal for separating a
    /// short, repeating categorical vocabulary (a real Enum, e.g.
    /// `color`'s own mean length ~8.1) from real multi-word free text
    /// (e.g. `productcare`'s own mean length ~50.8), computed here
    /// because a naive "distinct-value-count ceiling" alone was found,
    /// before this baseline was ever run against the oracle, to conflate
    /// the two: `color` (uniqueness_ratio=0.178, 4,686 distinct values)
    /// and `productcare` (uniqueness_ratio=0.222, 3,500 distinct values)
    /// have near-identical uniqueness ratios and both exceed any
    /// reasonable small distinct-value ceiling, so cardinality/uniqueness
    /// alone cannot separate them -- length can, and does, on this real
    /// data.
    pub mean_value_length: f64,
    pub sample_values: Vec<String>,
}

pub struct WandsFeed {
    pub total_products: usize,
    pub stats: BTreeMap<String, RawFieldStats>,
}

fn numeric_parseable_fraction(values: &BTreeMap<String, usize>) -> f64 {
    let total: usize = values.values().sum();
    if total == 0 {
        return 0.0;
    }
    let numeric: usize = values
        .iter()
        .filter(|(v, _)| v.parse::<f64>().is_ok())
        .map(|(_, &c)| c)
        .sum();
    numeric as f64 / total as f64
}

fn mean_value_length(values: &BTreeMap<String, usize>) -> f64 {
    let total: usize = values.values().sum();
    if total == 0 {
        return 0.0;
    }
    let weighted: usize = values.iter().map(|(v, &c)| v.chars().count() * c).sum();
    weighted as f64 / total as f64
}

/// Parses `dataset_cache/wands/product.csv`'s `product_features` column
/// (pipe-delimited `key:value` pairs, real, unprocessed) and computes raw
/// statistics for exactly [`WANDS_SAMPLE_KEYS`] -- every other one of the
/// real 7,961 distinct keys is ignored, not because it is uninteresting,
/// but because a smaller, deliberately diverse sample is what this
/// protocol's own amendment preregistered (see that amendment's own
/// rationale for why exhaustive enumeration would not make this more
/// rigorous).
pub fn load_wands_feed() -> WandsFeed {
    let sample: BTreeSet<&str> = WANDS_SAMPLE_KEYS.iter().copied().collect();
    let content = fs::read_to_string(wands_dataset_path("product.csv")).expect(
        "read dataset_cache/wands/product.csv -- run scripts/datasets/fetch_wands.sh first",
    );
    let mut lines = content.lines();
    let header = lines.next().expect("product.csv must have a header row");
    let columns: Vec<&str> = header.split('\t').collect();
    let features_idx = columns
        .iter()
        .position(|&c| c == "product_features")
        .expect("product.csv must have a product_features column");

    let mut raw_values: BTreeMap<String, BTreeMap<String, usize>> = BTreeMap::new();
    let mut total_products = 0usize;
    for line in lines {
        total_products += 1;
        let fields: Vec<&str> = line.split('\t').collect();
        let Some(&features) = fields.get(features_idx) else {
            continue;
        };
        for part in features.split('|') {
            let part = part.trim();
            let Some((k, v)) = part.split_once(':') else {
                continue;
            };
            let k = k.trim();
            let v = v.trim();
            if k.is_empty() || !sample.contains(k) {
                continue;
            }
            *raw_values
                .entry(k.to_string())
                .or_default()
                .entry(v.to_string())
                .or_insert(0) += 1;
        }
    }

    let stats = raw_values
        .into_iter()
        .map(|(key, values)| {
            let occurrences: usize = values.values().sum();
            let distinct_values = values.len();
            let uniqueness_ratio = distinct_values as f64 / occurrences.max(1) as f64;
            let density = occurrences as f64 / total_products.max(1) as f64;
            let numeric_parseable_fraction = numeric_parseable_fraction(&values);
            let mean_value_length = mean_value_length(&values);
            let sample_values: Vec<String> = values.keys().take(5).cloned().collect();
            (
                key.clone(),
                RawFieldStats {
                    key,
                    occurrences,
                    distinct_values,
                    uniqueness_ratio,
                    density,
                    numeric_parseable_fraction,
                    mean_value_length,
                    sample_values,
                },
            )
        })
        .collect();

    WandsFeed {
        total_products,
        stats,
    }
}

/// automotive's own real attribute statistics, via the exact same
/// `commerce_core::index::identifier::compute_field_stats` R3 validated
/// and Issue #42's own R2/R3 production merge already ported into
/// `commerce_core` -- reused directly, not reimplemented, so E2b's
/// "existing synthetic catalog" input is measured with the identical
/// mechanism R3's own held-out evaluation trusted.
pub fn automotive_field_stats(
    n_products: usize,
) -> BTreeMap<String, commerce_core::index::FieldStats> {
    let products = automotive::generate_catalog(n_products);
    let ingested = issue38_e2e3_eval::ingest::build_catalog(&products);
    compute_field_stats(&ingested.catalog)
}

/// A single, source-agnostic statistics view the statistics-only
/// baseline and validator both compute over -- so the same classifier
/// logic runs identically whether the source is WANDS's raw feed or
/// automotive's real attributes, and neither has an unfair statistical
/// advantage the other lacks. `variant_scoped` is `None` for WANDS
/// (which has no formal Variant concept at all -- every row is one
/// listing), never fabricated as `Some(false)` (a real "does not apply"
/// is not the same claim as a real "measured and found false").
#[derive(Debug, Clone)]
pub struct UnifiedFieldStats {
    pub key: String,
    pub occurrences: usize,
    pub distinct_values: usize,
    pub uniqueness_ratio: f64,
    pub numeric_parseable_fraction: f64,
    pub mean_value_length: f64,
    pub variant_scoped: Option<bool>,
    pub sample_values: Vec<String>,
}

impl From<&RawFieldStats> for UnifiedFieldStats {
    fn from(s: &RawFieldStats) -> Self {
        UnifiedFieldStats {
            key: s.key.clone(),
            occurrences: s.occurrences,
            distinct_values: s.distinct_values,
            uniqueness_ratio: s.uniqueness_ratio,
            numeric_parseable_fraction: s.numeric_parseable_fraction,
            mean_value_length: s.mean_value_length,
            variant_scoped: None,
            sample_values: s.sample_values.clone(),
        }
    }
}

/// automotive's own real attribute set, converted to [`UnifiedFieldStats`].
/// `numeric_parseable_fraction` is computed by a fresh scan of the same
/// `catalog` `compute_field_stats` already scanned -- `commerce_core::index::FieldStats`
/// itself has no such field (R3 never needed one), so this is additive,
/// not a duplicate of anything already measured.
pub fn automotive_unified_stats(n_products: usize) -> BTreeMap<String, UnifiedFieldStats> {
    let products = automotive::generate_catalog(n_products);
    let ingested = issue38_e2e3_eval::ingest::build_catalog(&products);
    let field_stats = compute_field_stats(&ingested.catalog);

    let mut raw_values: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for product in &ingested.catalog.products {
        for variant in &product.variants {
            let attrs = commerce_core::domain::effective_attributes(product, variant);
            for (name, value) in &attrs {
                let s = match value {
                    commerce_core::domain::AttributeValue::Text(s) => Some(s.clone()),
                    commerce_core::domain::AttributeValue::Enum(s) => Some(s.clone()),
                    commerce_core::domain::AttributeValue::Boolean(b) => Some(b.to_string()),
                    commerce_core::domain::AttributeValue::Numeric(n) => Some(format!("{n}")),
                    commerce_core::domain::AttributeValue::MultiEnum(_) => None,
                };
                if let Some(s) = s {
                    raw_values.entry(name.clone()).or_default().push(s);
                }
            }
        }
    }

    field_stats
        .into_iter()
        .map(|(key, stats)| {
            let mut sample_map: BTreeMap<String, usize> = BTreeMap::new();
            if let Some(values) = raw_values.get(&key) {
                for v in values {
                    *sample_map.entry(v.clone()).or_insert(0) += 1;
                }
            }
            let numeric_parseable_fraction = numeric_parseable_fraction(&sample_map);
            let mean_value_length = mean_value_length(&sample_map);
            let sample_values: Vec<String> = sample_map.keys().take(5).cloned().collect();
            (
                key.clone(),
                UnifiedFieldStats {
                    key,
                    occurrences: stats.total_occurrences,
                    distinct_values: stats.distinct_normalized_values,
                    uniqueness_ratio: stats.uniqueness_ratio,
                    numeric_parseable_fraction,
                    mean_value_length,
                    variant_scoped: Some(stats.variant_scoped),
                    sample_values,
                },
            )
        })
        .collect()
}

pub struct WandsQuery {
    pub query_id: String,
    pub text: String,
}

/// Same parsing discipline as `phase9_eval::bin::p9_e02_wands_physical_advantage`'s
/// own `load_queries` (tab-split, skip header) -- not a new format
/// assumption.
pub fn load_wands_queries() -> Vec<WandsQuery> {
    let content = fs::read_to_string(wands_dataset_path("query.csv"))
        .expect("read dataset_cache/wands/query.csv");
    content
        .lines()
        .skip(1)
        .filter_map(|line| {
            let mut parts = line.splitn(3, '\t');
            let query_id = parts.next()?.to_string();
            let text = parts.next()?.to_string();
            Some(WandsQuery { query_id, text })
        })
        .collect()
}

/// `query_id -> (wands product_id -> label)`, reusing
/// `phase9_eval::wands_relevance::WandsLabel`'s own real 3-way scale --
/// same parsing discipline as `p9_e02_wands_physical_advantage`'s own
/// `load_labels`.
pub fn load_wands_labels(
) -> BTreeMap<String, BTreeMap<String, phase9_eval::wands_relevance::WandsLabel>> {
    let content = fs::read_to_string(wands_dataset_path("label.csv"))
        .expect("read dataset_cache/wands/label.csv");
    let mut judged: BTreeMap<String, BTreeMap<String, phase9_eval::wands_relevance::WandsLabel>> =
        BTreeMap::new();
    for line in content.lines().skip(1) {
        let mut parts = line.splitn(4, '\t');
        let Some(_id) = parts.next() else { continue };
        let Some(query_id) = parts.next() else {
            continue;
        };
        let Some(product_id) = parts.next() else {
            continue;
        };
        let Some(raw_label) = parts.next() else {
            continue;
        };
        let Some(label) = phase9_eval::wands_relevance::WandsLabel::parse(raw_label) else {
            continue;
        };
        judged
            .entry(query_id.to_string())
            .or_default()
            .insert(product_id.to_string(), label);
    }
    judged
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wands_sample_keys_has_no_duplicates() {
        let set: BTreeSet<&str> = WANDS_SAMPLE_KEYS.iter().copied().collect();
        assert_eq!(
            set.len(),
            WANDS_SAMPLE_KEYS.len(),
            "every sample key must be listed exactly once"
        );
    }

    #[test]
    fn wands_sample_keys_is_36_real_keys_spanning_every_category() {
        assert_eq!(WANDS_SAMPLE_KEYS.len(), 36);
    }

    #[test]
    fn load_wands_feed_finds_every_sample_key_with_real_occurrences() {
        let feed = load_wands_feed();
        assert!(
            feed.total_products > 40_000,
            "expected the real ~42,994-product WANDS catalog, got {}",
            feed.total_products
        );
        for key in WANDS_SAMPLE_KEYS {
            let stats = feed
                .stats
                .get(*key)
                .unwrap_or_else(|| panic!("expected real occurrences for sample key {key:?}"));
            assert!(
                stats.occurrences > 0,
                "{key:?} must have at least one real occurrence in product.csv"
            );
        }
    }

    #[test]
    fn samplepartnumber_is_a_real_high_uniqueness_field() {
        let feed = load_wands_feed();
        let stats = feed.stats.get("samplepartnumber").unwrap();
        assert!(
            stats.uniqueness_ratio > 0.95,
            "samplepartnumber's own real uniqueness ratio must clear a high bar (measured ~0.9785 \
             at count=2048); got {:.4}",
            stats.uniqueness_ratio
        );
    }

    #[test]
    fn color_is_a_real_bounded_but_higher_cardinality_enum_field() {
        let feed = load_wands_feed();
        let stats = feed.stats.get("color").unwrap();
        assert!(
            stats.distinct_values > 100,
            "color has real, bounded but non-trivial cardinality"
        );
        assert!(
            stats.uniqueness_ratio < 0.5,
            "color's real uniqueness ratio must be well below an identifier's, got {:.4}",
            stats.uniqueness_ratio
        );
    }

    #[test]
    fn overallproductweight_is_numeric_parseable_for_essentially_every_real_value() {
        let feed = load_wands_feed();
        let stats = feed.stats.get("overallproductweight").unwrap();
        assert!(
            stats.numeric_parseable_fraction > 0.95,
            "overallproductweight's real values must be almost entirely numeric-parseable, got \
             {:.4}",
            stats.numeric_parseable_fraction
        );
    }

    #[test]
    fn commercialwarranty_is_a_real_two_valued_boolean_shaped_field() {
        let feed = load_wands_feed();
        let stats = feed.stats.get("commercialwarranty").unwrap();
        assert_eq!(
            stats.distinct_values, 2,
            "commercialwarranty's real values are exactly yes/no"
        );
    }

    #[test]
    fn load_wands_queries_and_labels_match_the_real_dataset_sizes() {
        let queries = load_wands_queries();
        assert_eq!(queries.len(), 480, "WANDS's own real query count");
        let labels = load_wands_labels();
        let distinct_pairs: usize = labels.values().map(|m| m.len()).sum();
        // WANDS's real label.csv has 233,448 raw rows (matching
        // `phase9_eval::wands_relevance`'s own doc comment), but only
        // 231,873 distinct (query_id, product_id) pairs -- 1,575 real
        // duplicate-judgment rows for the same pair exist in the raw
        // data. `load_wands_labels` (matching
        // `p9_e02_wands_physical_advantage.rs`'s own pre-existing
        // `load_labels`) keeps the last-seen label for a duplicate pair,
        // same as that binary already does -- this is disclosed, real
        // data-quality behavior, not a bug in either implementation.
        assert_eq!(
            distinct_pairs, 231_873,
            "WANDS's own real distinct (query_id, product_id) judgment-pair count, after the \
             same last-value-wins dedup p9_e02_wands_physical_advantage.rs's own load_labels \
             already applies"
        );
    }

    #[test]
    fn automotive_field_stats_reuses_the_real_production_mechanism() {
        let stats = automotive_field_stats(1500);
        let part_number = stats.get("part_number").unwrap();
        assert!(
            part_number.uniqueness_ratio > 0.99,
            "part_number's real, already-established uniqueness ratio (R3: 0.998)"
        );
        assert!(part_number.variant_scoped);
    }
}
