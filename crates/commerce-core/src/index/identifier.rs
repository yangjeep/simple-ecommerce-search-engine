//! Identifier serving primitive: Issue #42 R3
//! (`docs/experiments/ISSUE42_LOG.md#i42-r3`, `docs/adr/0013-identifier-serving-primitive.md`)
//! measured that `plan::LexicalHit` had no way to name a specific
//! `VariantId`, so `plan::verify_and_truncate` always resolved a lexical
//! hit to "the product's first variant that happens to satisfy every
//! structural constraint" -- arbitrary relative to whatever text a
//! delegate actually matched, a real cross-variant correctness defect for
//! an exact-identifier query (CLAUDE.md: "Product/variant correctness is
//! non-negotiable"). R3's calibrated classifier gating a dedicated exact-
//! match dictionary (Treatment C) cleared every preregistered GO-gate
//! criterion -- 100% Recall@1 / 0% false-match on a real identifier field,
//! correct rejection of prefix/near-miss adversarial queries, and correct
//! abstention on every non-identifier field -- with build/update cost far
//! lower than a general per-variant text index (Treatment B). This module
//! ports that mechanism into production; see the ADR for the full decision
//! and alternatives considered.

use std::collections::{BTreeMap, HashMap};

use crate::domain::{effective_attributes, AttributeValue, Catalog, ProductId, VariantId};

/// Per-field statistics [`compute_field_stats`] measures over a whole
/// [`Catalog`], regardless of the field's [`AttributeValue`] variant --
/// the input [`IdentifierClassifier::accepts`] gates on, never a field's
/// name or declared type.
#[derive(Debug, Clone, PartialEq)]
pub struct FieldStats {
    pub field: String,
    pub total_occurrences: usize,
    pub distinct_normalized_values: usize,
    pub uniqueness_ratio: f64,
    pub mean_entropy_bits: f64,
    /// Fraction of this field's values sharing the single most common
    /// character-class "shape" (every alphabetic char normalized to `A`,
    /// every digit to `9`, every other character kept as-is -- e.g. a real
    /// part number like `"IA-1234-BP"` signs as `"AA-9999-AA"`). Measured
    /// and reported for transparency, but does **not** gate
    /// [`IdentifierClassifier::accepts`]: R3's second correction round
    /// (`docs/experiments/ISSUE42_LOG.md#i42-r3`) tried format-consistency
    /// as a second required signal and found it scores the real identifier
    /// field *lower* than several genuine non-identifiers on real data
    /// (brand/product-type name segments of varying word count spread a
    /// real identifier's own occurrences across multiple signatures) --
    /// gating on it would have incorrectly rejected the real identifier
    /// field. That negative result is recorded, not erased (CLAUDE.md:
    /// "Record failed experiments"): the statistic stays computed and
    /// reported here, but only `uniqueness_ratio` and `variant_scoped`
    /// gate acceptance.
    pub format_consistency: f64,
    pub variant_scoped: bool,
}

fn normalize_identifier(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

fn attribute_string(value: &AttributeValue) -> Option<String> {
    match value {
        AttributeValue::Text(s) => Some(s.clone()),
        AttributeValue::Enum(s) => Some(s.clone()),
        AttributeValue::Boolean(b) => Some(b.to_string()),
        AttributeValue::Numeric(n) => Some(format!("{n}")),
        AttributeValue::MultiEnum(_) => None,
    }
}

/// Maps every alphabetic character to `A` and every digit to `9`, leaving
/// every other character (hyphens, spaces, ...) as-is -- a coarse
/// identifier-shape signature, cheap to compute and independent of a
/// field's actual character values (only their *class* and position).
/// Feeds [`FieldStats::format_consistency`] only; see that field's own doc
/// comment for why it is measured but does not gate.
fn format_signature(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphabetic() {
                'A'
            } else if c.is_ascii_digit() {
                '9'
            } else {
                c
            }
        })
        .collect()
}

fn shannon_entropy_bits(s: &str) -> f64 {
    if s.is_empty() {
        return 0.0;
    }
    // `BTreeMap`, not `HashMap`: summing floating-point terms in a
    // deterministic (sorted-by-char) order is required for this project's
    // "byte-identical across runs" standard (CLAUDE.md: "Keep benchmark
    // inputs deterministic and versioned") -- `issue42-eval::r3_experimental`
    // caught a real last-ULP-level cross-run divergence traced to `HashMap`'s
    // randomized-per-process iteration order here; this port preserves the
    // same discipline.
    let mut counts: BTreeMap<char, usize> = BTreeMap::new();
    for c in s.chars() {
        *counts.entry(c).or_insert(0) += 1;
    }
    let n = s.chars().count() as f64;
    -counts
        .values()
        .map(|&count| {
            let p = count as f64 / n;
            p * p.log2()
        })
        .sum::<f64>()
}

/// Scans every (product, variant) pair's effective attributes (product-
/// level merged with variant-level, variant winning -- [`effective_attributes`])
/// across the whole `catalog`, grouping raw occurrences by attribute name
/// regardless of [`AttributeValue`] variant (stringified) -- so a low-
/// cardinality `Enum` field is measured on equal footing with a genuine
/// `Text` identifier, and rejected on its own statistics, not by a type-
/// based shortcut.
pub fn compute_field_stats(catalog: &Catalog) -> BTreeMap<String, FieldStats> {
    let mut raw_values: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut variant_scoped: BTreeMap<String, bool> = BTreeMap::new();

    for product in &catalog.products {
        for variant in &product.variants {
            let attrs = effective_attributes(product, variant);
            for (name, value) in &attrs {
                if let Some(s) = attribute_string(value) {
                    raw_values.entry(name.clone()).or_default().push(s);
                }
            }
            for name in variant.attributes.keys() {
                variant_scoped.insert(name.clone(), true);
            }
        }
    }

    raw_values
        .into_iter()
        .map(|(field, values)| {
            let total_occurrences = values.len();
            let distinct: std::collections::BTreeSet<String> =
                values.iter().map(|v| normalize_identifier(v)).collect();
            let distinct_normalized_values = distinct.len();
            let uniqueness_ratio = distinct_normalized_values as f64 / total_occurrences as f64;
            let mean_entropy_bits = values.iter().map(|v| shannon_entropy_bits(v)).sum::<f64>()
                / total_occurrences as f64;
            // `BTreeMap`, not `HashMap`: the same deterministic-iteration
            // requirement as `shannon_entropy_bits` above -- picking "the"
            // most common signature via `HashMap` iteration order could
            // silently pick a different tied-for-first signature across
            // runs.
            let mut signature_counts: BTreeMap<String, usize> = BTreeMap::new();
            for v in &values {
                *signature_counts.entry(format_signature(v)).or_insert(0) += 1;
            }
            let format_consistency = signature_counts.values().copied().max().unwrap_or(0) as f64
                / total_occurrences as f64;
            let stats = FieldStats {
                field: field.clone(),
                total_occurrences,
                distinct_normalized_values,
                uniqueness_ratio,
                mean_entropy_bits,
                format_consistency,
                variant_scoped: *variant_scoped.get(&field).unwrap_or(&false),
            };
            (field, stats)
        })
        .collect()
}

/// R3's calibrated cutoff (`docs/experiments/ISSUE42_LOG.md#i42-r3`,
/// "Calibration" section): chosen by inspecting only the calibration
/// catalog's own [`FieldStats`] before any held-out metric existed, and
/// used verbatim on the held-out set, never re-tuned. It sits with wide
/// margin between the real identifier field measured there
/// (uniqueness_ratio=0.998) and the two misleading fields (~0.002/0.00067).
pub const MIN_UNIQUENESS_RATIO: f64 = 0.95;

/// **Added during production integration, not part of R3's own
/// experimental evidence.** R3's calibration/held-out catalogs both had
/// 1,500+ products, so a spurious small-sample accept was never exercised
/// there. `commerce_core`'s own test suite (`crates/commerce-core/src/fixtures.rs`,
/// `crates/commerce-core/tests/*.rs`) includes tiny hand-authored catalogs,
/// some with only 2-10 products -- a field's `uniqueness_ratio` computed
/// over a handful of occurrences can clear [`MIN_UNIQUENESS_RATIO`] purely
/// by chance for a field that is not semantically an identifier at all
/// (with `n` occurrences all distinct, `uniqueness_ratio` is always
/// `1.0` regardless of `n`). Requiring at least this many occurrences
/// before a field is even considered closes that gap. `100` sits well
/// below R3's own calibration set's 1,500 occurrences for `part_number`,
/// so it cannot change R3's own experimental conclusion (`part_number`
/// still clears it with wide margin) -- it only prevents a tiny
/// fixture/test catalog from spuriously accepting a field due to
/// small-sample noise.
pub const MIN_IDENTIFIER_SAMPLE_SIZE: usize = 100;

/// Gates whether a field's measured [`FieldStats`] look identifier-like
/// enough to build an [`IdentifierDictionary`] for it. See
/// `docs/adr/0013-identifier-serving-primitive.md` for the full decision
/// history, including a recorded negative result: R3's second correction
/// round (`docs/experiments/ISSUE42_LOG.md#i42-r3`) tried
/// `FieldStats::format_consistency` as a second required gate signal and
/// found it would have *incorrectly rejected* the real identifier field on
/// real data -- that statistic is still computed and reported (see its own
/// doc comment) but does not gate here.
pub struct IdentifierClassifier;

impl IdentifierClassifier {
    /// A field is accepted only when it clears the uniqueness-ratio
    /// cutoff, is variant-scoped, *and* clears [`MIN_IDENTIFIER_SAMPLE_SIZE`].
    ///
    /// The first two conditions are R3's own fully-corrected classifier
    /// (`docs/experiments/ISSUE42_LOG.md#i42-r3`'s "Second correction
    /// round"): uniqueness ratio alone was found, by a fresh adversarial
    /// review, to leave a genuine near-miss (a real, non-identifier
    /// Numeric attribute measuring uniqueness_ratio=0.94, only 0.01 below
    /// cutoff) with no second signal to catch it; `variant_scoped`
    /// discriminates correctly for a structural reason -- a real
    /// identifier is set directly on each `Variant`, while a product-level
    /// attribute like the near-miss above is not -- confirmed to leave
    /// every other classification on both the calibration and held-out
    /// sets unchanged.
    ///
    /// The third condition ([`MIN_IDENTIFIER_SAMPLE_SIZE`]) is a new
    /// safeguard added during production integration -- see that
    /// constant's own doc comment for why it is needed here and was never
    /// exercised by R3's own evaluation.
    pub fn accepts(stats: &FieldStats) -> bool {
        stats.uniqueness_ratio >= MIN_UNIQUENESS_RATIO
            && stats.variant_scoped
            && stats.total_occurrences >= MIN_IDENTIFIER_SAMPLE_SIZE
    }
}

/// A dedicated exact/normalized-key dictionary for one accepted field,
/// mapping a normalized identifier value to every `(ProductId, VariantId)`
/// that carries it -- a real collision (two variants sharing one
/// identifier value) surfaces every match, never silently arbitrated to
/// one.
#[derive(Debug, Clone, PartialEq)]
pub struct IdentifierDictionary {
    field: String,
    index: HashMap<String, Vec<(ProductId, VariantId)>>,
}

impl IdentifierDictionary {
    pub fn build(catalog: &Catalog, field: &str) -> Self {
        let mut index: HashMap<String, Vec<(ProductId, VariantId)>> = HashMap::new();
        for product in &catalog.products {
            for variant in &product.variants {
                let attrs = effective_attributes(product, variant);
                if let Some(value) = attrs.get(field) {
                    if let Some(s) = attribute_string(value) {
                        index
                            .entry(normalize_identifier(&s))
                            .or_default()
                            .push((product.id, variant.id));
                    }
                }
            }
        }
        IdentifierDictionary {
            field: field.to_string(),
            index,
        }
    }

    pub fn field(&self) -> &str {
        &self.field
    }

    pub fn entry_count(&self) -> usize {
        self.index.len()
    }

    /// A real incremental update -- a single `HashMap` insert, not a full
    /// rebuild.
    pub fn insert_one(&mut self, product: ProductId, variant: VariantId, value: &str) {
        self.index
            .entry(normalize_identifier(value))
            .or_default()
            .push((product, variant));
    }

    /// Exact, normalized lookup -- collisions return every matching
    /// `(ProductId, VariantId)`, never silently arbitrated to one.
    pub fn lookup(&self, query_text: &str) -> Vec<(ProductId, VariantId)> {
        self.index
            .get(&normalize_identifier(query_text))
            .cloned()
            .unwrap_or_default()
    }
}
