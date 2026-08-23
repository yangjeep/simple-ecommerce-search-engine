//! R3 experimental treatments (`docs/experiments/ISSUE42_PROTOCOL.md`'s R3
//! section): identifier serving primitive.
//!
//! Unlike R1/R2, these treatments do not route through
//! `commerce_core::plan::execute_planned`/`LexicalDelegate` at all -- per
//! `ISSUE42_PROTOCOL.md`'s dated correction under R3's own Treatment B
//! description, `commerce_core::plan::LexicalHit` has no `VariantId`
//! field, and `verify_and_truncate`'s per-variant resolution is vacuously
//! true whenever `query.constraints` is empty (exactly the case for an
//! identifier-only query, since `compile()` has no dedicated "part
//! number" keyword branch at all) -- so today's pipeline cannot express
//! "this specific variant is what matched" at all, not merely omit doing
//! so. Every treatment below is its own self-contained index-and-lookup
//! path returning [`IdentifierHit`] (which does carry a real
//! `VariantId`) directly, never a `commerce_core::plan::LexicalHit`.
//! `commerce_core::plan`/`commerce_core::ir` are not modified.

use std::collections::{BTreeMap, HashMap};

use commerce_core::domain::{effective_attributes, AttributeValue, Catalog, ProductId, VariantId};
use phase9_eval::bitmap_delegate::BitmapTantivyDelegate;
use tantivy::query::QueryParser;
use tantivy::schema::{Field, Schema, Value, FAST, STORED, TEXT};
use tantivy::{collector::TopDocs, Directory, HasLen, Index, IndexReader, TantivyDocument};

/// One resolved identifier-lookup hit. Always carries the specific
/// variant it resolved to -- see the module doc comment for why this,
/// not `commerce_core::plan::LexicalHit`, is this experiment's own hit
/// type.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct IdentifierHit {
    pub product: ProductId,
    pub variant: VariantId,
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

// ---------------------------------------------------------------------
// Treatment A: real, unmodified `BitmapTantivyDelegate` over the real,
// unmodified `phase9_eval::bitmap_delegate::build_index` (product-level
// `title`/`Text` attributes only). Reused verbatim, not reimplemented, so
// "current behavior" cannot itself be a source of divergence. Resolves a
// raw hit to the product's *first* variant -- mirroring
// `verify_and_truncate`'s own vacuous-constraint behavior today (see the
// module doc comment), since Treatment A never indexes anything that
// could distinguish one variant from another anyway.
// ---------------------------------------------------------------------

pub fn execute_a(
    catalog: &Catalog,
    delegate: &BitmapTantivyDelegate,
    query_text: &str,
    limit: usize,
) -> Vec<IdentifierHit> {
    use commerce_core::plan::LexicalDelegate;
    let raw = delegate.search(&[query_text.to_string()], None, limit);
    raw.into_iter()
        .filter_map(|hit| {
            let product = catalog.products.iter().find(|p| p.id == hit.product)?;
            let variant = product.variants.first()?;
            Some(IdentifierHit {
                product: product.id,
                variant: variant.id,
            })
        })
        .collect()
}

// ---------------------------------------------------------------------
// Treatment B: an experimental copy of `build_index` that additionally
// indexes every variant's own effective Text attributes (via
// `effective_attributes`), one Tantivy document PER VARIANT (not per
// product), with the owning `ProductId`/`VariantId` stored directly on
// each document -- so a match resolves to the exact variant that
// contains the matched text, not merely its parent product. A
// general-purpose text index with no notion of "this token is a
// complete identifier": a query for a real identifier's own prefix can
// still match, since Tantivy's default tokenizer splits an identifier
// like "IA-1234-BP" into sub-tokens ("ia", "1234", "bp") that a prefix
// query's own tokens are a subset of.
// ---------------------------------------------------------------------

pub struct VariantTextIndex {
    index: Index,
    reader: IndexReader,
    query_parser: QueryParser,
    product_ordinal_field: Field,
    variant_ordinal_field: Field,
}

fn variant_index_schema() -> (Schema, Field, Field, Field) {
    let mut builder = Schema::builder();
    let product_ordinal = builder.add_u64_field("product_ordinal", FAST | STORED);
    let variant_ordinal = builder.add_u64_field("variant_ordinal", FAST | STORED);
    let text = builder.add_text_field("text", TEXT);
    (builder.build(), product_ordinal, variant_ordinal, text)
}

impl VariantTextIndex {
    pub fn build(catalog: &Catalog) -> tantivy::Result<Self> {
        let (schema, product_ordinal_field, variant_ordinal_field, text_field) =
            variant_index_schema();
        let index = Index::create_in_ram(schema);
        // Single-threaded, matching `phase9_eval::bitmap_delegate::build_index`'s
        // own determinism discipline (`Index::writer_for_tests`'s own doc
        // comment: multi-threaded indexing does not give a deterministic
        // DocId allocation).
        let mut writer = index.writer_with_num_threads(1, 64_000_000)?;
        for product in &catalog.products {
            for variant in &product.variants {
                let attrs = effective_attributes(product, variant);
                let mut text_parts = vec![product.title.clone()];
                for value in attrs.values() {
                    if let Some(s) = attribute_string(value) {
                        text_parts.push(s);
                    }
                }
                let mut doc = TantivyDocument::default();
                doc.add_u64(product_ordinal_field, product.id.0);
                doc.add_u64(variant_ordinal_field, variant.id.0);
                doc.add_text(text_field, text_parts.join(" "));
                writer.add_document(doc)?;
            }
        }
        writer.commit()?;
        let reader = index.reader()?;
        let query_parser = QueryParser::for_index(&index, vec![text_field]);
        Ok(Self {
            index,
            reader,
            query_parser,
            product_ordinal_field,
            variant_ordinal_field,
        })
    }

    pub fn search(&self, query_text: &str, limit: usize) -> Vec<IdentifierHit> {
        let (parsed, _errors) = self.query_parser.parse_query_lenient(query_text);
        let searcher = self.reader.searcher();
        let Ok(top_docs) = searcher.search(&parsed, &TopDocs::with_limit(limit)) else {
            return Vec::new();
        };
        top_docs
            .into_iter()
            .filter_map(|(_score, address)| {
                let retrieved: TantivyDocument = searcher.doc(address).ok()?;
                let product = retrieved.get_first(self.product_ordinal_field)?.as_u64()?;
                let variant = retrieved.get_first(self.variant_ordinal_field)?.as_u64()?;
                Some(IdentifierHit {
                    product: ProductId(product),
                    variant: VariantId(variant),
                })
            })
            .collect()
    }

    /// Real Tantivy segment file sizes (bytes), summed across every file
    /// the index currently has committed -- an actual index-size
    /// measurement (via `Directory::open_read`'s own reported
    /// `FileSlice::len()`, not an OS filesystem stat, since
    /// `Index::create_in_ram`'s directory has no real filesystem files
    /// at all), not an estimate.
    pub fn index_size_bytes(&self) -> u64 {
        let directory = self.index.directory();
        directory
            .list_managed_files()
            .into_iter()
            .filter_map(|path| directory.open_read(&path).ok())
            .map(|slice| slice.len() as u64)
            .sum()
    }
}

pub fn execute_b(index: &VariantTextIndex, query_text: &str, limit: usize) -> Vec<IdentifierHit> {
    index.search(query_text, limit)
}

// ---------------------------------------------------------------------
// Treatment C: a dedicated exact/normalized-key dictionary, built only
// for fields the classifier accepts. `IdentifierClassifier` computes
// per-field statistics from a whole `Catalog` (uniqueness ratio, mean
// per-value Shannon entropy, and whether the field is ever set directly
// on a Variant, not merely inherited from its Product) and calibrated
// cutoffs decide acceptance -- never the field's name.
// ---------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct FieldStats {
    pub field: String,
    pub total_occurrences: usize,
    pub distinct_normalized_values: usize,
    pub uniqueness_ratio: f64,
    pub mean_entropy_bits: f64,
    pub variant_scoped: bool,
}

fn shannon_entropy_bits(s: &str) -> f64 {
    if s.is_empty() {
        return 0.0;
    }
    let mut counts: HashMap<char, usize> = HashMap::new();
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
/// level merged with variant-level, variant winning) across the whole
/// `catalog`, grouping raw occurrences by attribute name regardless of
/// `AttributeValue` variant (stringified) -- so a low-cardinality `Enum`
/// field is measured on equal footing with a genuine `Text` identifier,
/// and rejected on its own statistics (near-zero uniqueness), not by a
/// type-based shortcut that would make the "does not key off names"
/// check in `ISSUE42_PROTOCOL.md`'s R3 calibration section vacuous.
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
            let stats = FieldStats {
                field: field.clone(),
                total_occurrences,
                distinct_normalized_values,
                uniqueness_ratio,
                mean_entropy_bits,
                variant_scoped: *variant_scoped.get(&field).unwrap_or(&false),
            };
            (field, stats)
        })
        .collect()
}

/// Calibrated once, by inspecting only the calibration catalog's own
/// [`FieldStats`] (`ISSUE42_PROTOCOL.md`'s R3 "Calibration / held-out
/// split": "Classifier cutoffs... are chosen by inspecting only this
/// calibration set's statistics before any held-out metric is
/// computed"). On `issue42_eval::r3_workload::build_calibration_and_held_out`'s
/// 1500-product calibration catalog, `compute_field_stats` measured the
/// three real fields as: `part_number` (uniqueness_ratio=0.998 --
/// 1497 distinct normalized values across 1500 occurrences; the 3
/// natural collisions are automotive's own real
/// `rng.gen_range(1000..9999)` draw occasionally repeating, not a
/// fixture defect, confirmed by direct computation, not assumed to be
/// exactly 1.0 -- mean_entropy_bits≈2.91), `sku_code` (uniqueness_ratio=0.002
/// -- 3 distinct values across 1500 occurrences, mean_entropy_bits=0.0,
/// since a single-character Enum value has no internal character
/// variation at all), and `product_fingerprint` (uniqueness_ratio≈0.00067
/// -- 1 distinct value across 1500 occurrences, despite mean_entropy_bits≈3.84
/// -- a real per-string-entropy value *higher* than part_number's own,
/// proving entropy alone cannot separate these two fields and
/// uniqueness ratio is the load-bearing cutoff here, not a redundant
/// one). A cutoff of `0.95` sits with wide margin between the two
/// misleading fields (~0.002/0.00067) and the real identifier (0.998),
/// and is used verbatim on the held-out set, not tuned against anything
/// held out.
pub const MIN_UNIQUENESS_RATIO: f64 = 0.95;

pub struct IdentifierClassifier;

impl IdentifierClassifier {
    /// A field is accepted only on its measured uniqueness ratio -- see
    /// `MIN_UNIQUENESS_RATIO`'s own doc comment for why entropy is
    /// measured and reported (per the protocol's own required input
    /// statistics) but is not this classifier's gating cutoff: on the
    /// calibration set, the misleading non-unique field's *entropy* was
    /// actually higher than the real identifier's, so entropy alone
    /// cannot distinguish them -- uniqueness ratio can and does.
    pub fn accepts(stats: &FieldStats) -> bool {
        stats.uniqueness_ratio >= MIN_UNIQUENESS_RATIO
    }
}

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

    /// Exact, normalized lookup -- collisions return every matching
    /// `(ProductId, VariantId)`, never silently arbitrated to one.
    pub fn lookup(&self, query_text: &str) -> Vec<IdentifierHit> {
        self.index
            .get(&normalize_identifier(query_text))
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .map(|(product, variant)| IdentifierHit { product, variant })
            .collect()
    }
}

pub fn execute_c(dictionary: &IdentifierDictionary, query_text: &str) -> Vec<IdentifierHit> {
    dictionary.lookup(query_text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::r3_workload;
    use phase9_eval::bitmap_delegate::build_index;

    #[test]
    fn variant_text_index_resolves_to_the_exact_variant_not_the_products_first_one() {
        let (_cal, held) = r3_workload::build_calibration_and_held_out();
        let index = VariantTextIndex::build(&held.catalog).unwrap();
        let stress_product = held
            .catalog
            .products
            .iter()
            .find(|p| p.id == held.many_variant_product)
            .unwrap();
        // Query for the *last* variant's identifier specifically -- if
        // this resolved to "the product's first variant" (Treatment A's
        // own vacuous-constraint behavior), it would return variant 0,
        // not the one actually asked for.
        let last_variant = stress_product.variants.last().unwrap();
        let AttributeValue::Text(target_pn) = last_variant.attributes.get("part_number").unwrap()
        else {
            panic!("expected Text");
        };
        let hits = index.search(target_pn, 10);
        assert!(
            hits.iter().any(|h| h.variant == last_variant.id),
            "expected the last variant ({:?}) among hits for its own identifier {target_pn:?}, \
             got {hits:?}",
            last_variant.id
        );
    }

    #[test]
    fn treatment_a_never_finds_a_variant_level_identifier_at_all() {
        let (_cal, held) = r3_workload::build_calibration_and_held_out();
        let built = build_index(&held.catalog).unwrap();
        let delegate = BitmapTantivyDelegate::new(
            &built.index,
            vec![built.title_field, built.description_field],
        )
        .unwrap();
        let (_p, _v, real_identifier) = &held.known_identifier;
        let hits = execute_a(&held.catalog, &delegate, real_identifier, 10);
        assert!(
            hits.is_empty(),
            "Treatment A indexes only Product::title/product-level Text attributes -- \
             part_number is variant-level only, so a real identifier query must find nothing \
             at all, confirming H3-A; got {hits:?}"
        );
    }

    #[test]
    fn identifier_classifier_accepts_the_real_identifier_and_rejects_both_misleading_fields() {
        let (cal, _held) = r3_workload::build_calibration_and_held_out();
        let stats = compute_field_stats(&cal.catalog);
        let part_number = stats.get(r3_workload::REAL_IDENTIFIER_FIELD).unwrap();
        let low_card = stats
            .get(r3_workload::MISLEADING_LOW_CARDINALITY_FIELD)
            .unwrap();
        let non_unique = stats.get(r3_workload::MISLEADING_NON_UNIQUE_FIELD).unwrap();

        assert!(IdentifierClassifier::accepts(part_number));
        assert!(!IdentifierClassifier::accepts(low_card));
        assert!(!IdentifierClassifier::accepts(non_unique));
        assert!(
            non_unique.mean_entropy_bits > part_number.mean_entropy_bits,
            "this test's whole point: the misleading non-unique field's entropy is HIGHER than \
             the real identifier's, proving entropy alone could not have rejected it -- \
             uniqueness ratio is the field doing the real work"
        );
    }

    #[test]
    fn identifier_dictionary_surfaces_both_variants_of_a_real_collision() {
        let (_cal, held) = r3_workload::build_calibration_and_held_out();
        let dictionary =
            IdentifierDictionary::build(&held.catalog, r3_workload::REAL_IDENTIFIER_FIELD);
        let (a, b, shared_value) = &held.collision_pair;
        let hits = execute_c(&dictionary, shared_value);
        let hit_set: std::collections::BTreeSet<(ProductId, VariantId)> =
            hits.iter().map(|h| (h.product, h.variant)).collect();
        assert!(hit_set.contains(a));
        assert!(hit_set.contains(b));
        assert_eq!(
            hit_set.len(),
            2,
            "a real collision must surface exactly both variants, never silently drop one"
        );
    }

    #[test]
    fn identifier_dictionary_rejects_a_prefix_and_a_near_miss() {
        let (_cal, held) = r3_workload::build_calibration_and_held_out();
        let dictionary =
            IdentifierDictionary::build(&held.catalog, r3_workload::REAL_IDENTIFIER_FIELD);
        let (real, near_miss) = &held.near_miss;
        assert!(execute_c(&dictionary, near_miss).is_empty());
        let prefix = &real[..real.len() - 2];
        assert!(
            execute_c(&dictionary, prefix).is_empty(),
            "a partial prefix must never resolve to a false exact match"
        );
    }
}
