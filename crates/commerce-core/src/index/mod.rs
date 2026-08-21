//! Gate 3: specialized physical indexes over a [`Catalog`], built once from
//! catalog data and queried by a compiled [`CommerceQuery`]. Structural
//! filters (brand/product-type/category/price/typed Enum/Boolean/Numeric
//! attributes) are answered from bitmap/range structures; `Text` attribute
//! constraints are not bitmap-indexable in general (substring containment),
//! so they narrow-then-verify against the reduced candidate set instead of
//! ever being silently skipped or approximated — see [`CatalogIndex::execute`].

mod rank;

use std::collections::{BTreeMap, BTreeSet, HashMap};

use roaring::RoaringBitmap;

use crate::domain::{
    effective_attributes, AttributeMap, AttributeValue, BrandId, Catalog, CategoryId, Constraint,
    NumericOp, Product, ProductId, ProductTypeId, Variant, VariantId,
};
use crate::ir::{CommerceQuery, ResolvedConstraint, StructuralConstraint};

pub use rank::RankedHit;

type Ordinal = u32;

/// An immutable, build-once index over one [`Catalog`] snapshot. There is
/// no update path: a new catalog version means a new `CatalogIndex::build`
/// call, matching the "immutable/mmap-friendly bundle" bias in CLAUDE.md
/// (mmap itself is not implemented yet; this is in-memory only).
#[derive(Debug, Default)]
pub struct CatalogIndex {
    ordinals: Vec<(ProductId, VariantId)>,
    variant_location: HashMap<VariantId, (usize, usize)>,
    product_location: HashMap<ProductId, usize>,
    variant_ordinal: HashMap<VariantId, Ordinal>,

    enum_bitmaps: HashMap<(String, String), RoaringBitmap>,
    enum_values: HashMap<String, BTreeSet<String>>,
    bool_bitmaps: HashMap<(String, bool), RoaringBitmap>,
    numeric_index: HashMap<String, Vec<(f64, Ordinal)>>,

    // Issue #21 Phase 6D: a dictionary/ordinal-encoded parallel
    // representation of single-valued Enum attributes, motivated by
    // P6C-E01's finding that Lucene's own ordinal-based facet module
    // beats a naive per-candidate scan. `enum_dictionary` maps each
    // attribute to its distinct values in first-encountered order (the
    // index into this Vec is the value's dense ordinal); `enum_columns`
    // maps each attribute to a `Vec<u32>` indexed by variant ordinal,
    // storing that variant's value-ordinal (or `u32::MAX` if the
    // variant has no single-valued Enum value for this attribute).
    // Deliberately built only from `AttributeValue::Enum`, not
    // `MultiEnum`, to exactly match `facet_counts_by_scan`'s own
    // semantics (see `facet_counts_ordinal`).
    enum_dictionary: HashMap<String, Vec<String>>,
    enum_value_ordinal: HashMap<String, HashMap<String, u32>>,
    enum_columns: HashMap<String, Vec<u32>>,

    brand_bitmaps: HashMap<BrandId, RoaringBitmap>,
    product_type_bitmaps: HashMap<ProductTypeId, RoaringBitmap>,
    category_bitmaps: HashMap<CategoryId, RoaringBitmap>,
    price_index: Vec<(i64, Ordinal)>,

    lexical_postings: HashMap<String, RoaringBitmap>,
}

/// Split text into lowercased alphanumeric tokens. Public so callers that
/// need to look tokens up against [`CatalogIndex::lexical_and_candidates`]/
/// [`CatalogIndex::lexical_or_candidates`] tokenize identically to how the
/// index itself tokenized catalog text at build time (Round 1 R1-E07,
/// `docs/experiments/ROUND1_LOG.md`) — a mismatched tokenizer would make any
/// comparison meaningless.
pub fn tokenize(text: &str) -> impl Iterator<Item = String> + '_ {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty())
        .map(str::to_lowercase)
}

impl CatalogIndex {
    /// Build every index structure in one pass over `catalog`. Ordinals are
    /// assigned in catalog encounter order, so the build (and therefore
    /// every query result derived from ordinal iteration) is deterministic.
    pub fn build(catalog: &Catalog) -> Self {
        let mut idx = CatalogIndex::default();
        let mut numeric_raw: HashMap<String, Vec<(f64, Ordinal)>> = HashMap::new();
        let mut price_raw: Vec<(i64, Ordinal)> = Vec::new();
        let mut enum_column_raw: HashMap<String, Vec<(u32, Ordinal)>> = HashMap::new();

        for (p_idx, product) in catalog.products.iter().enumerate() {
            idx.product_location.insert(product.id, p_idx);
            for (v_idx, variant) in product.variants.iter().enumerate() {
                let ord = idx.ordinals.len() as Ordinal;
                idx.ordinals.push((product.id, variant.id));
                idx.variant_location.insert(variant.id, (p_idx, v_idx));
                idx.variant_ordinal.insert(variant.id, ord);

                idx.brand_bitmaps
                    .entry(product.brand)
                    .or_default()
                    .insert(ord);
                idx.product_type_bitmaps
                    .entry(product.product_type)
                    .or_default()
                    .insert(ord);
                idx.category_bitmaps
                    .entry(product.category)
                    .or_default()
                    .insert(ord);
                price_raw.push((variant.price.amount_cents, ord));

                idx.index_attributes(
                    &effective_attributes(product, variant),
                    ord,
                    &mut numeric_raw,
                    &mut enum_column_raw,
                );
                for token in tokenize(&product.title) {
                    idx.lexical_postings.entry(token).or_default().insert(ord);
                }
            }
        }

        for values in numeric_raw.values_mut() {
            values.sort_by(|a, b| a.0.total_cmp(&b.0));
        }
        idx.numeric_index = numeric_raw;
        price_raw.sort_by_key(|&(cents, _)| cents);
        idx.price_index = price_raw;

        let total = idx.ordinals.len();
        for (attribute, pairs) in enum_column_raw {
            let mut column = vec![u32::MAX; total];
            for (value_ord, ord) in pairs {
                column[ord as usize] = value_ord;
            }
            idx.enum_columns.insert(attribute, column);
        }
        idx
    }

    fn index_attributes(
        &mut self,
        attrs: &AttributeMap,
        ord: Ordinal,
        numeric_raw: &mut HashMap<String, Vec<(f64, Ordinal)>>,
        enum_column_raw: &mut HashMap<String, Vec<(u32, Ordinal)>>,
    ) {
        for (name, value) in attrs {
            match value {
                AttributeValue::Enum(v) => {
                    let value_ord = self.index_enum_value(name, v, ord);
                    enum_column_raw
                        .entry(name.clone())
                        .or_default()
                        .push((value_ord, ord));
                }
                AttributeValue::MultiEnum(vs) => {
                    for v in vs {
                        let _ = self.index_enum_value(name, v, ord);
                    }
                }
                AttributeValue::Boolean(b) => {
                    self.bool_bitmaps
                        .entry((name.clone(), *b))
                        .or_default()
                        .insert(ord);
                }
                AttributeValue::Numeric(n) => {
                    numeric_raw.entry(name.clone()).or_default().push((*n, ord));
                }
                AttributeValue::Text(t) => {
                    for token in tokenize(t) {
                        self.lexical_postings.entry(token).or_default().insert(ord);
                    }
                }
            }
        }
    }

    /// Indexes `(name, value)` for ordinal `ord` in every existing
    /// bitmap/vocabulary structure, and additionally assigns (or looks
    /// up) `value`'s dense dictionary ordinal for `name`, returning it
    /// so callers building the parallel single-valued `enum_columns`
    /// representation (see `facet_counts_ordinal`) don't need a second
    /// lookup.
    fn index_enum_value(&mut self, name: &str, value: &str, ord: Ordinal) -> u32 {
        self.enum_bitmaps
            .entry((name.to_string(), value.to_string()))
            .or_default()
            .insert(ord);
        self.enum_values
            .entry(name.to_string())
            .or_default()
            .insert(value.to_string());

        let dictionary = self.enum_dictionary.entry(name.to_string()).or_default();
        let value_ordinals = self.enum_value_ordinal.entry(name.to_string()).or_default();
        if let Some(&existing) = value_ordinals.get(value) {
            existing
        } else {
            let new_ordinal = dictionary.len() as u32;
            dictionary.push(value.to_string());
            value_ordinals.insert(value.to_string(), new_ordinal);
            new_ordinal
        }
    }

    /// Exact entity lookup by id: O(1) via a hash map, no scan.
    pub fn lookup_variant<'c>(
        &self,
        catalog: &'c Catalog,
        id: VariantId,
    ) -> Option<(&'c Product, &'c Variant)> {
        let &(p_idx, v_idx) = self.variant_location.get(&id)?;
        let product = &catalog.products[p_idx];
        Some((product, &product.variants[v_idx]))
    }

    pub fn lookup_product<'c>(&self, catalog: &'c Catalog, id: ProductId) -> Option<&'c Product> {
        let &p_idx = self.product_location.get(&id)?;
        Some(&catalog.products[p_idx])
    }

    /// The bitmap ordinal assigned to `variant_id` at build time, if it
    /// exists in this index. Needed by any external caller that wants to
    /// test membership of a specific variant against a bitmap returned by
    /// `indexed_candidates`/`lexical_and_candidates`/`lexical_or_candidates`
    /// without re-running a full query (e.g. Round 1 R1-E07's cross-
    /// reference against real relevance judgments,
    /// `docs/experiments/ROUND1_LOG.md`).
    pub fn ordinal_of(&self, variant_id: VariantId) -> Option<Ordinal> {
        self.variant_ordinal.get(&variant_id).copied()
    }

    /// The inverse of [`Self::ordinal_of`]: the `VariantId` a bitmap
    /// ordinal refers to. Needed by `state::execute_with_overlay`
    /// (Issue #8) to turn a candidate ordinal surviving structural-AND-
    /// availability intersection back into a variant to look up and
    /// verify, without exposing this index's internal `ordinals` vector.
    pub fn variant_id_at(&self, ordinal: Ordinal) -> Option<VariantId> {
        self.ordinals.get(ordinal as usize).map(|&(_, v)| v)
    }

    /// The distinct `ProductId`s referenced by a set of ordinals (typically
    /// `indexed_candidates(&query.constraints)`'s output). Needed by
    /// `plan::execute_planned` (Issue #6 priority 5) to hand a structural
    /// pre-filter to a lexical delegate keyed by product id, since a
    /// delegate such as an external text index has no concept of this
    /// index's internal bitmap ordinals.
    pub fn candidate_product_ids(&self, ordinals: &RoaringBitmap) -> BTreeSet<ProductId> {
        ordinals
            .iter()
            .map(|ord| self.ordinals[ord as usize].0)
            .collect()
    }

    fn all_ordinals(&self) -> RoaringBitmap {
        (0..self.ordinals.len() as Ordinal).collect()
    }

    fn structural_bitmap(&self, s: &StructuralConstraint) -> RoaringBitmap {
        match s {
            StructuralConstraint::Brand(id) => {
                self.brand_bitmaps.get(id).cloned().unwrap_or_default()
            }
            StructuralConstraint::BrandAny(ids) => {
                let mut bm = RoaringBitmap::new();
                for id in ids {
                    if let Some(b) = self.brand_bitmaps.get(id) {
                        bm |= b;
                    }
                }
                bm
            }
            StructuralConstraint::ProductType(id) => self
                .product_type_bitmaps
                .get(id)
                .cloned()
                .unwrap_or_default(),
            StructuralConstraint::Category(id) => {
                self.category_bitmaps.get(id).cloned().unwrap_or_default()
            }
            StructuralConstraint::PriceUnderCents(cents) => {
                let hi = self.price_index.partition_point(|&(p, _)| p < *cents);
                self.price_index[..hi].iter().map(|&(_, ord)| ord).collect()
            }
            StructuralConstraint::PriceOverCents(cents) => {
                let lo = self.price_index.partition_point(|&(p, _)| p <= *cents);
                self.price_index[lo..].iter().map(|&(_, ord)| ord).collect()
            }
        }
    }

    fn attribute_bitmap(&self, c: &Constraint) -> Option<RoaringBitmap> {
        match c {
            Constraint::Enum { attribute, value }
            | Constraint::MultiEnumContains { attribute, value } => Some(
                self.enum_bitmaps
                    .get(&(attribute.clone(), value.clone()))
                    .cloned()
                    .unwrap_or_default(),
            ),
            Constraint::Boolean { attribute, value } => Some(
                self.bool_bitmaps
                    .get(&(attribute.clone(), *value))
                    .cloned()
                    .unwrap_or_default(),
            ),
            Constraint::Numeric {
                attribute,
                op,
                value,
            } => Some(
                self.numeric_index
                    .get(attribute)
                    .map(|sorted| numeric_range(sorted, *op, *value))
                    .unwrap_or_default(),
            ),
            // Substring containment is not bitmap-indexable in general: the
            // caller must fall back to verifying this constraint directly
            // against the candidate set (see `execute`).
            Constraint::Text { .. } => None,
        }
    }

    /// Intersect the bitmaps for every constraint that *can* be answered
    /// from an index (everything except `Constraint::Text`). Returns the
    /// full ordinal set when there are no indexable constraints at all.
    pub fn indexed_candidates(&self, constraints: &[ResolvedConstraint]) -> RoaringBitmap {
        let mut acc: Option<RoaringBitmap> = None;
        for c in constraints {
            let bm = match c {
                ResolvedConstraint::Structural(s) => Some(self.structural_bitmap(s)),
                ResolvedConstraint::Attribute(a) => self.attribute_bitmap(a),
            };
            if let Some(bm) = bm {
                acc = Some(match acc {
                    Some(existing) => existing & bm,
                    None => bm,
                });
            }
        }
        acc.unwrap_or_else(|| self.all_ordinals())
    }

    /// Index-accelerated equivalent of `CommerceQuery::execute`: narrow via
    /// bitmaps/range structures first, then verify any `Constraint::Text`
    /// clause against only the surviving candidates. Must return exactly
    /// the same hit set as the linear scan (`tests/physical_index.rs`
    /// asserts this on every fixture) — the index only changes *how* a
    /// match is found, never *what* counts as one.
    pub fn execute(&self, query: &CommerceQuery, catalog: &Catalog) -> Vec<(ProductId, VariantId)> {
        let candidates = self.indexed_candidates(&query.constraints);
        let text_constraints: Vec<&Constraint> = query
            .constraints
            .iter()
            .filter_map(|c| match c {
                ResolvedConstraint::Attribute(inner @ Constraint::Text { .. }) => Some(inner),
                _ => None,
            })
            .collect();

        let mut hits = Vec::new();
        for ord in candidates.iter() {
            let (product_id, variant_id) = self.ordinals[ord as usize];
            if text_constraints.is_empty() {
                hits.push((product_id, variant_id));
                continue;
            }
            let (product, variant) = self
                .lookup_variant(catalog, variant_id)
                .expect("ordinal was built from this catalog");
            let attrs = effective_attributes(product, variant);
            if text_constraints.iter().all(|c| c.matches(&attrs)) {
                hits.push((product_id, variant_id));
            }
        }
        hits
    }

    /// Whole-word token lookup against `lexical_postings`, requiring every
    /// token to be present (AND). This is a *different* matching paradigm
    /// from `Constraint::Text`/`execute()`'s substring containment on one
    /// named attribute: `lexical_postings` is attribute-agnostic (built
    /// from every `Text` attribute plus the product title) and exact-token,
    /// not substring — "water" will not match a stored token "waterproof"
    /// here the way `Constraint::Text{contains:"water"}` would via
    /// substring. No query path (`execute`/`compile`) uses this; it exists
    /// so Round 1 R1-E07 (`docs/experiments/ROUND1_LOG.md`) can measure
    /// whether the postings structure Gate 3 already built is a viable
    /// fast-candidate-retrieval primitive, independent of ranking.
    pub fn lexical_and_candidates(&self, tokens: &[String]) -> RoaringBitmap {
        let mut acc: Option<RoaringBitmap> = None;
        for token in tokens {
            let bm = self
                .lexical_postings
                .get(token)
                .cloned()
                .unwrap_or_default();
            acc = Some(match acc {
                Some(existing) => existing & bm,
                None => bm,
            });
        }
        acc.unwrap_or_default()
    }

    /// Same token index as [`CatalogIndex::lexical_and_candidates`], but
    /// requiring only at least one token to be present (OR) — the
    /// candidate-generation shape a real lexical/BM25 engine uses before
    /// ranking narrows results, rather than requiring every query token to
    /// appear verbatim.
    pub fn lexical_or_candidates(&self, tokens: &[String]) -> RoaringBitmap {
        let mut acc = RoaringBitmap::new();
        for token in tokens {
            if let Some(bm) = self.lexical_postings.get(token) {
                acc |= bm;
            }
        }
        acc
    }

    /// Facet counts for one attribute over an arbitrary candidate set
    /// (typically `indexed_candidates(&query.constraints)`): how many
    /// candidates carry each known value of `attribute`. Values with zero
    /// candidates are omitted.
    pub fn facet_counts(
        &self,
        attribute: &str,
        candidates: &RoaringBitmap,
    ) -> BTreeMap<String, u64> {
        let mut counts = BTreeMap::new();
        let Some(values) = self.enum_values.get(attribute) else {
            return counts;
        };
        for value in values {
            if let Some(bm) = self
                .enum_bitmaps
                .get(&(attribute.to_string(), value.clone()))
            {
                let count = (candidates & bm).len();
                if count > 0 {
                    counts.insert(value.clone(), count);
                }
            }
        }
        counts
    }

    /// Issue #17 (Phase 5): the same faceting operation as
    /// [`Self::facet_counts`], but for [`crate::domain::BrandId`] rather
    /// than a generic string-valued `Enum` attribute. Brand is a first-class
    /// structural field with its own dedicated `brand_bitmaps` index (see
    /// [`Self::structural_bitmap`]), not one of the generic
    /// `enum_bitmaps` `facet_counts` reads -- so a "which brands appear
    /// under this candidate set" facet (e.g. brand facet under an active
    /// color filter) needs this sibling method rather than being
    /// answerable through `facet_counts("brand", ...)`, which would
    /// always return empty.
    pub fn brand_facet_counts(&self, candidates: &RoaringBitmap) -> BTreeMap<BrandId, u64> {
        let mut counts = BTreeMap::new();
        for (&brand_id, bm) in &self.brand_bitmaps {
            let count = (candidates & bm).len();
            if count > 0 {
                counts.insert(brand_id, count);
            }
        }
        counts
    }

    /// Issue #17 (Phase 5): an alternative faceting strategy for
    /// [`Self::facet_counts`], scanning the (typically small) *candidate*
    /// set directly rather than the field's entire global vocabulary.
    /// `facet_counts` costs `O(distinct values for this attribute across
    /// the whole catalog)` regardless of how small `candidates` is --
    /// real-data measurement (P5-E00) found this costs 35-420ms on a
    /// ~175k/206k-distinct-value real field even when the candidate set
    /// itself is only a few thousand ordinals, dramatically slower than a
    /// tuned Solr JSON Facet API call (1-3ms) on the identical request.
    /// This method instead costs `O(|candidates|)`: for a genuinely small
    /// candidate set (the common real case for a filtered PLP facet
    /// request), that is far cheaper than scanning the whole vocabulary,
    /// testing whether the slowness above was fundamental or an
    /// avoidable representation choice. Additive: does not change
    /// `facet_counts`'s existing behavior/tests.
    pub fn facet_counts_by_scan(
        &self,
        candidates: &RoaringBitmap,
        catalog: &Catalog,
        attribute: &str,
    ) -> BTreeMap<String, u64> {
        let mut counts = BTreeMap::new();
        for ord in candidates.iter() {
            let (_, variant_id) = self.ordinals[ord as usize];
            let Some((product, variant)) = self.lookup_variant(catalog, variant_id) else {
                continue;
            };
            if let Some(AttributeValue::Enum(value)) =
                effective_attributes(product, variant).get(attribute)
            {
                *counts.entry(value.clone()).or_insert(0) += 1;
            }
        }
        counts
    }

    /// Issue #21 Phase 6D: an ordinal/dictionary-based facet-counting
    /// strategy, motivated by P6C-E01's finding
    /// (`docs/experiments/PHASE6C_LOG.md`) that Lucene's own dedicated
    /// facet module -- which pre-builds a per-field dictionary mapping
    /// each distinct value to a dense integer ordinal, then counts by
    /// incrementing a flat array indexed by that ordinal -- beats both
    /// Solr and a naive per-candidate scan in most measured checkpoints.
    /// Unlike [`Self::facet_counts_by_scan`], this needs no `Catalog`
    /// parameter and does no per-candidate `effective_attributes`
    /// clone, `String` hash, or map entry/insert during the scan itself
    /// -- counting is a flat `Vec<u64>` increment per candidate, with
    /// `String` cloning deferred to only the (typically few) non-zero
    /// result buckets at the end. Exact semantics match
    /// `facet_counts_by_scan` precisely, including its
    /// `MultiEnum`-values-are-not-counted behavior (`enum_columns` is
    /// built only from `AttributeValue::Enum`, see `index_attributes`)
    /// -- see `facet_counts_ordinal_matches_facet_counts_by_scan_exactly`
    /// in `tests/physical_index.rs`.
    pub fn facet_counts_ordinal(
        &self,
        candidates: &RoaringBitmap,
        attribute: &str,
    ) -> BTreeMap<String, u64> {
        let mut result = BTreeMap::new();
        let Some(dictionary) = self.enum_dictionary.get(attribute) else {
            return result;
        };
        let Some(column) = self.enum_columns.get(attribute) else {
            return result;
        };
        let mut counts = vec![0u64; dictionary.len()];
        for ord in candidates.iter() {
            if let Some(&value_ord) = column.get(ord as usize) {
                if value_ord != u32::MAX {
                    counts[value_ord as usize] += 1;
                }
            }
        }
        for (value_ord, count) in counts.into_iter().enumerate() {
            if count > 0 {
                result.insert(dictionary[value_ord].clone(), count);
            }
        }
        result
    }

    /// The scan-based sibling of [`Self::brand_facet_counts`], for the
    /// same reason [`Self::facet_counts_by_scan`] exists alongside
    /// [`Self::facet_counts`].
    pub fn brand_facet_counts_by_scan(
        &self,
        candidates: &RoaringBitmap,
        catalog: &Catalog,
    ) -> BTreeMap<BrandId, u64> {
        let mut counts = BTreeMap::new();
        for ord in candidates.iter() {
            let (product_id, _) = self.ordinals[ord as usize];
            if let Some(product) = self.lookup_product(catalog, product_id) {
                *counts.entry(product.brand).or_insert(0) += 1;
            }
        }
        counts
    }

    /// Phase 6A (Issue #23): `category` and `product_type` are, like
    /// `brand`, first-class structural fields with their own dedicated
    /// `category_bitmaps`/`product_type_bitmaps` indexes -- populated by
    /// every catalog since `CatalogIndex::build` has always built them,
    /// but never exercised with real, non-sentinel values until WANDS
    /// (ESCI always assigns `CategoryId(0)`/`ProductTypeId(0)`; see
    /// `round1_eval::catalog`). These four methods complete the same
    /// global-vocabulary-scan / candidate-scan pair Phase 5 built for
    /// `brand`, for exactly the same reason: `facet_counts("category", ...)`
    /// would always return empty, since category/product_type are not
    /// `enum_bitmaps` attributes.
    pub fn category_facet_counts(&self, candidates: &RoaringBitmap) -> BTreeMap<CategoryId, u64> {
        let mut counts = BTreeMap::new();
        for (&category_id, bm) in &self.category_bitmaps {
            let count = (candidates & bm).len();
            if count > 0 {
                counts.insert(category_id, count);
            }
        }
        counts
    }

    pub fn category_facet_counts_by_scan(
        &self,
        candidates: &RoaringBitmap,
        catalog: &Catalog,
    ) -> BTreeMap<CategoryId, u64> {
        let mut counts = BTreeMap::new();
        for ord in candidates.iter() {
            let (product_id, _) = self.ordinals[ord as usize];
            if let Some(product) = self.lookup_product(catalog, product_id) {
                *counts.entry(product.category).or_insert(0) += 1;
            }
        }
        counts
    }

    pub fn product_type_facet_counts(
        &self,
        candidates: &RoaringBitmap,
    ) -> BTreeMap<ProductTypeId, u64> {
        let mut counts = BTreeMap::new();
        for (&product_type_id, bm) in &self.product_type_bitmaps {
            let count = (candidates & bm).len();
            if count > 0 {
                counts.insert(product_type_id, count);
            }
        }
        counts
    }

    pub fn product_type_facet_counts_by_scan(
        &self,
        candidates: &RoaringBitmap,
        catalog: &Catalog,
    ) -> BTreeMap<ProductTypeId, u64> {
        let mut counts = BTreeMap::new();
        for ord in candidates.iter() {
            let (product_id, _) = self.ordinals[ord as usize];
            if let Some(product) = self.lookup_product(catalog, product_id) {
                *counts.entry(product.product_type).or_insert(0) += 1;
            }
        }
        counts
    }

    /// Top-K ranking (Gate 3): correctness-verified hits from `execute`,
    /// scored by summing compiled `Preference` weights and sorted
    /// deterministically (score desc, then product/variant id asc so ties
    /// never depend on hash-map iteration order).
    pub fn execute_ranked(
        &self,
        query: &CommerceQuery,
        catalog: &Catalog,
        k: usize,
    ) -> Vec<RankedHit> {
        rank::execute_ranked(self, query, catalog, k)
    }

    /// Issue #14 P3-E03: `execute_ranked`, further narrowed by an
    /// externally-supplied ordinal bitmap (e.g. `lexical_and_candidates`'s
    /// AND-narrowing over residual free-text tokens). See
    /// `rank::execute_ranked_narrowed_by`'s doc comment for the full
    /// correctness/safety contract.
    pub fn execute_ranked_narrowed_by(
        &self,
        query: &CommerceQuery,
        narrow_by: &RoaringBitmap,
        catalog: &Catalog,
        k: usize,
    ) -> Vec<RankedHit> {
        rank::execute_ranked_narrowed_by(self, query, narrow_by, catalog, k)
    }

    /// Approximate on-heap index size in bytes (Gate 7's "index size"
    /// metric): the sum of every `RoaringBitmap`'s
    /// [`RoaringBitmap::serialized_size`] plus the flat byte size of the
    /// ordinal/numeric/price vectors. This is a comparable-across-runs
    /// estimate, not a precise allocator-level accounting — `HashMap`
    /// bucket overhead, `String` heap allocations (attribute/value names),
    /// and allocator bookkeeping are not itemized.
    pub fn approximate_size_bytes(&self) -> usize {
        let bitmap_bytes: usize = self
            .enum_bitmaps
            .values()
            .map(RoaringBitmap::serialized_size)
            .sum::<usize>()
            + self
                .bool_bitmaps
                .values()
                .map(RoaringBitmap::serialized_size)
                .sum::<usize>()
            + self
                .brand_bitmaps
                .values()
                .map(RoaringBitmap::serialized_size)
                .sum::<usize>()
            + self
                .product_type_bitmaps
                .values()
                .map(RoaringBitmap::serialized_size)
                .sum::<usize>()
            + self
                .category_bitmaps
                .values()
                .map(RoaringBitmap::serialized_size)
                .sum::<usize>()
            + self
                .lexical_postings
                .values()
                .map(RoaringBitmap::serialized_size)
                .sum::<usize>();

        let ordinals_bytes = self.ordinals.len() * std::mem::size_of::<(ProductId, VariantId)>();
        let numeric_bytes: usize = self
            .numeric_index
            .values()
            .map(|v| v.len() * std::mem::size_of::<(f64, Ordinal)>())
            .sum();
        let price_bytes = self.price_index.len() * std::mem::size_of::<(i64, Ordinal)>();

        bitmap_bytes + ordinals_bytes + numeric_bytes + price_bytes
    }
}

fn numeric_range(sorted: &[(f64, Ordinal)], op: NumericOp, value: f64) -> RoaringBitmap {
    let lo = sorted.partition_point(|&(v, _)| v < value);
    let hi = sorted.partition_point(|&(v, _)| v <= value);
    let slice = match op {
        NumericOp::Eq => &sorted[lo..hi],
        NumericOp::Lt => &sorted[..lo],
        NumericOp::Lte => &sorted[..hi],
        NumericOp::Gt => &sorted[hi..],
        NumericOp::Gte => &sorted[lo..],
    };
    slice.iter().map(|&(_, ord)| ord).collect()
}
