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
        idx
    }

    fn index_attributes(
        &mut self,
        attrs: &AttributeMap,
        ord: Ordinal,
        numeric_raw: &mut HashMap<String, Vec<(f64, Ordinal)>>,
    ) {
        for (name, value) in attrs {
            match value {
                AttributeValue::Enum(v) => self.index_enum_value(name, v, ord),
                AttributeValue::MultiEnum(vs) => {
                    for v in vs {
                        self.index_enum_value(name, v, ord);
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

    fn index_enum_value(&mut self, name: &str, value: &str, ord: Ordinal) {
        self.enum_bitmaps
            .entry((name.to_string(), value.to_string()))
            .or_default()
            .insert(ord);
        self.enum_values
            .entry(name.to_string())
            .or_default()
            .insert(value.to_string());
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

    fn all_ordinals(&self) -> RoaringBitmap {
        (0..self.ordinals.len() as Ordinal).collect()
    }

    fn structural_bitmap(&self, s: &StructuralConstraint) -> RoaringBitmap {
        match s {
            StructuralConstraint::Brand(id) => {
                self.brand_bitmaps.get(id).cloned().unwrap_or_default()
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
