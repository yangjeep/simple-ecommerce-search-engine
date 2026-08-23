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
    text_field: Field,
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
            text_field,
        })
    }

    /// Adds a single new variant's document and commits -- a real
    /// incremental update (Tantivy's own segment-merge machinery), not a
    /// full rebuild from scratch. Reopens the reader afterward so
    /// `search` sees the new document (`IndexReader`'s own default
    /// `ReloadPolicy` does not guarantee synchronous visibility
    /// immediately after `commit`).
    pub fn add_one(
        &mut self,
        product: ProductId,
        variant: VariantId,
        text: &str,
    ) -> tantivy::Result<()> {
        let mut writer = self.index.writer_with_num_threads(1, 64_000_000)?;
        let mut doc = TantivyDocument::default();
        doc.add_u64(self.product_ordinal_field, product.0);
        doc.add_u64(self.variant_ordinal_field, variant.0);
        doc.add_text(self.text_field, text);
        writer.add_document(doc)?;
        writer.commit()?;
        self.reader.reload()?;
        Ok(())
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
// per-value Shannon entropy, format consistency, and whether the field
// is ever set directly on a Variant, not merely inherited from its
// Product) -- never the field's name -- and gates acceptance on two of
// them (uniqueness ratio and variant scope; see `IdentifierClassifier::accepts`'s
// own doc comment for why entropy and format-consistency are measured
// and reported but do not gate, including a negative result: naive
// format-consistency measures were tried and found to incorrectly
// reject the real identifier field on this catalog's own data, not
// merely assumed to work).
// ---------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct FieldStats {
    pub field: String,
    pub total_occurrences: usize,
    pub distinct_normalized_values: usize,
    pub uniqueness_ratio: f64,
    pub mean_entropy_bits: f64,
    /// Fraction of this field's values sharing the single most common
    /// character-class "shape" (every alphabetic char normalized to `A`,
    /// every digit to `9`, every other character kept as-is -- e.g. a
    /// real `part_number` like `"IA-1234-BP"` signs as `"AA-9999-AA"`).
    /// Added during R3's second correction round (a fresh adversarial
    /// review found the originally-shipped classifier silently narrowed
    /// the protocol's preregistered multi-signal design down to
    /// uniqueness ratio alone, with no dated deviation note, and that
    /// `lumens` -- a genuine, non-identifier Numeric attribute -- scored
    /// uniqueness_ratio=0.94 on the calibration set, only 0.01 below the
    /// cutoff, a real margin risk the single-signal classifier had no
    /// second signal to catch). A rigid, repeatable format is exactly
    /// what a real identifier has and a loosely-formed numeric field
    /// (whose digit count varies with its value's own magnitude) does
    /// not.
    pub format_consistency: f64,
    pub variant_scoped: bool,
}

/// Maps every alphabetic character to `A` and every digit to `9`,
/// leaving every other character (hyphens, spaces, ...) as-is -- the
/// same coarse identifier-shape signature commercial identifier
/// detectors commonly use, cheap to compute and independent of a
/// field's actual character values (only their *class* and position).
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
    // deterministic (sorted-by-char) order, rather than `HashMap`'s
    // randomized-per-process iteration order, is required for this
    // project's own "byte-identical across runs" standard -- caught by
    // diffing 5 independent runs' summary JSON, which showed a
    // last-ULP-level difference in `material_grade`'s own
    // `mean_entropy_bits` between runs, traced to this function.
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
    /// A field is accepted only when it clears the uniqueness-ratio
    /// cutoff *and* is variant-scoped.
    ///
    /// **Second correction round** (a fresh adversarial review, before
    /// any production change was made on the strength of R3's GO
    /// verdict): the originally-shipped version of this function
    /// checked uniqueness ratio alone, silently narrowing the
    /// protocol's own preregistered multi-signal classifier design
    /// (uniqueness ratio, entropy, format-consistency, collision rate,
    /// Product-vs-Variant scope, exact-query rate) down to one signal,
    /// with no dated deviation note recording the narrowing -- itself a
    /// real, confirmed defect independent of any numeric consequence.
    /// The reviewer also found a concrete numeric consequence: on the
    /// calibration set, `lumens` (a genuine, non-identifier Numeric
    /// attribute) measured uniqueness_ratio=0.94, only 0.01 below the
    /// cutoff -- and traced this to sample size, not semantic
    /// identifier-ness (`lumens`'s ratio drops further, to 0.89, on the
    /// 2x-larger held-out set). A single-signal classifier has no
    /// second check to catch a similarly-sampled non-identifier field
    /// that happens to clear 0.95 by chance.
    ///
    /// Two candidate second signals were tried and *rejected* after
    /// measuring them for real, not assumed to work from their
    /// description in the protocol: `format_consistency` (this field's
    /// own doc comment) and an equivalent fixed-length-ratio variant
    /// both scored **part_number itself at ~0.51-0.56** -- LOWER than
    /// several genuine non-identifier fields (`product_fingerprint` and
    /// `sku_code` both score 1.0, trivially, since they are the
    /// identical value/a single character everywhere) -- because
    /// automotive's own brand names and product-type names vary in
    /// word count (`"TrueDrive"` -> one-letter code; `"Ironclad Auto"`
    /// -> two-letter code), so `part_number`'s own brand/type-code
    /// segments genuinely vary in length across the catalog, spreading
    /// its occurrences across multiple signatures/lengths with no
    /// single majority. Gating on either would have INCORRECTLY
    /// REJECTED the real identifier field -- a regression, not a fix.
    /// This negative result is recorded, not erased (`CLAUDE.md`:
    /// "Record failed experiments"), and both statistics remain
    /// computed and reported (`FieldStats::format_consistency`) for
    /// transparency, but neither gates.
    ///
    /// `variant_scoped` (already computed, but -- per the same
    /// review -- never actually read anywhere before this fix) is the
    /// signal that *does* discriminate correctly, and for a structural
    /// reason, not a numeric coincidence: automotive's generator
    /// (`crates/issue38-e2e3-eval/src/automotive.rs`) sets `part_number`/
    /// `warranty_months`/`compatible_fitment` directly on each
    /// `Variant`, and every other attribute (including `lumens`) only
    /// on the parent `Product`. Requiring `variant_scoped` rejects
    /// `lumens` outright regardless of its uniqueness-ratio margin,
    /// confirmed to leave every other classification on both the
    /// calibration and held-out sets unchanged (`warranty_months` is
    /// also variant-scoped but already correctly rejected on
    /// uniqueness ratio alone, so this addition introduces no new false
    /// accept).
    pub fn accepts(stats: &FieldStats) -> bool {
        stats.uniqueness_ratio >= MIN_UNIQUENESS_RATIO && stats.variant_scoped
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

    /// A real incremental update -- a single `HashMap` insert, not a
    /// full rebuild -- for the same new-variant scenario `VariantTextIndex::add_one`
    /// measures.
    pub fn insert_one(&mut self, product: ProductId, variant: VariantId, value: &str) {
        self.index
            .entry(normalize_identifier(value))
            .or_default()
            .push((product, variant));
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

    /// Second correction round: `lumens` is a real, non-identifier
    /// Numeric attribute (Headlight Bulbs' brightness) that a fresh
    /// adversarial review found scores uniqueness_ratio=0.94 on the
    /// calibration set -- only 0.01 below `MIN_UNIQUENESS_RATIO`, a
    /// genuine margin risk a single-signal classifier has no second
    /// check against. This test proves the fix (requiring
    /// `variant_scoped` too) rejects `lumens` for a structural reason
    /// (it is a product-level attribute in automotive's own generator,
    /// never set on any `Variant`), not merely because its uniqueness
    /// ratio happens to clear or miss a threshold by chance.
    #[test]
    fn identifier_classifier_rejects_lumens_a_genuine_near_miss_on_uniqueness_ratio_alone() {
        let (cal, _held) = r3_workload::build_calibration_and_held_out();
        let stats = compute_field_stats(&cal.catalog);
        let lumens = stats
            .get("lumens")
            .expect("automotive sets a Numeric lumens attribute");
        assert!(
            lumens.uniqueness_ratio >= 0.9 && lumens.uniqueness_ratio < MIN_UNIQUENESS_RATIO,
            "this test's premise is a genuine near-miss on uniqueness ratio alone; got {:.4} \
             (cutoff {MIN_UNIQUENESS_RATIO})",
            lumens.uniqueness_ratio
        );
        assert!(
            !lumens.variant_scoped,
            "lumens must be product-level only in this fixture -- the structural reason it is \
             correctly rejected regardless of its uniqueness-ratio margin"
        );
        assert!(!IdentifierClassifier::accepts(lumens));
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
