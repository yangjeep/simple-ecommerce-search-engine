//! Issue #34 Phase 9 (disclosed defect #2, `PHASE2_DECISION.md`): the
//! reference `TantivyDelegate` implementations used throughout Phase 2
//! (`crates/phase2-eval/src/bin/p1d_physical_advantage_eval.rs` and
//! siblings) restrict `Hybrid`'s candidate set by building a fresh Lucene
//! `TermSetQuery` from up to ~60K `Term`s *per query* -- collecting every
//! candidate's id string, sorting/deduping them, and building an FST
//! automaton (`tantivy::query::set_query`'s `Map`), all before the query
//! can even run. P2-E17's own adversarial review found this "undermines
//! much of Hybrid's own narrow-first-is-cheap cost advantage."
//!
//! This is a bitmap-based replacement: `commerce_core::plan::execute_planned`
//! already re-verifies `restrict_to` membership itself in
//! `verify_and_truncate` (a plain `BTreeSet<ProductId>::contains` check,
//! independent of whatever a `LexicalDelegate` does internally) -- so a
//! delegate is free to implement `restrict_to` however is cheapest,
//! correctness never depends on it (confirmed by direct source read of
//! `plan/mod.rs`, not assumed). Instead of a per-query term-set/FST build,
//! this stores each document's native `ProductId` as a `FAST` u64 field at
//! index-build time (once), and restricts a query by wrapping the inner
//! text query's `Scorer` in a filter that checks `RoaringBitmap::contains`
//! against that fast field per candidate doc -- an O(1) membership check
//! per doc the inner query would visit anyway, no per-query set/FST
//! construction at all.

use std::sync::Arc;

use commerce_core::domain::{AttributeValue, Catalog, ProductId};
use commerce_core::plan::{LexicalDelegate, LexicalHit};
use roaring::RoaringBitmap;
use tantivy::collector::TopDocs;
use tantivy::query::{EnableScoring, Query, QueryClone, QueryParser, Scorer, Weight};
use tantivy::schema::{Field, Schema, FAST, STORED, TEXT};
use tantivy::{DocId, DocSet, Index, IndexReader, Score, SegmentReader, TantivyError, TERMINATED};

const PRODUCT_ORDINAL_FIELD: &str = "product_ordinal";
const TITLE_FIELD: &str = "title";
const DESCRIPTION_FIELD: &str = "description";

fn schema() -> (Schema, Field, Field, Field) {
    let mut builder = Schema::builder();
    let ordinal = builder.add_u64_field(PRODUCT_ORDINAL_FIELD, FAST | STORED);
    let title = builder.add_text_field(TITLE_FIELD, TEXT | STORED);
    let description = builder.add_text_field(DESCRIPTION_FIELD, TEXT);
    (builder.build(), ordinal, title, description)
}

/// Named, not positional: `build_index`'s four outputs are all `Field`
/// handles of the same type, and this project has already once caught
/// itself mis-destructuring an equivalent tuple in a test (transposing
/// `title_field`/`ordinal_field` produces no compile error, only a
/// silently-wrong `QueryParser` and zero real hits) -- named fields make
/// that class of mistake impossible instead of merely disclosed.
pub struct BuiltIndex {
    pub index: Index,
    pub ordinal_field: Field,
    pub title_field: Field,
    pub description_field: Field,
}

/// Builds an in-memory Tantivy index from a real `commerce_core::domain::Catalog`
/// -- any catalog, not WANDS-specific: this reuses `Product::title` and any
/// `AttributeValue::Text` product-level attribute (e.g. WANDS's
/// `description`), the same fields `execute_ranked`'s own default ranking
/// signal (Issue #34 P9-E00) reads. Each document's `product_ordinal`
/// field is set to the real `ProductId.0` assigned during catalog
/// ingestion -- not a fresh loop counter, and not assumed to equal
/// Tantivy's own internal `DocId` order (which is not guaranteed stable
/// across segments) -- so `BitmapRestrictQuery` never needs a separate
/// `ProductId` <-> `DocId` translation table, per-segment or otherwise.
pub fn build_index(catalog: &Catalog) -> tantivy::Result<BuiltIndex> {
    let (schema, ordinal_field, title_field, description_field) = schema();
    let index = Index::create_in_ram(schema);
    // Single-threaded indexing, not `index.writer(...)`'s own default of
    // `min(num_cpus::get(), 8)` threads: found necessary for genuine
    // run-to-run determinism, not merely a style preference. Tantivy's own
    // source (`Index::writer_for_tests`'s doc comment) states plainly that
    // multi-threaded indexing does not give a deterministic DocId
    // allocation. This module's own `product_ordinal` field already
    // protects *correctness* (which document is which) against that --
    // but a real, measured effect survived anyway: `TopDocs`' score-tie
    // ordering among multiple equally-scored documents can still depend on
    // DocId order, which is exactly the case a multi-word free-text query
    // against several same-scoring titles produces. Caught by rerunning
    // `issue38-e2e3-eval`'s E2 binary 5 times and diffing raw output
    // byte-for-byte (not merely eyeballing summary stats): one template's
    // mean NDCG genuinely differed between two runs of the identical
    // binary against the identical seeded catalog (0.5386 vs 0.5869),
    // contradicting this project's own repeated "byte-identical across 5
    // runs" claim for every prior experiment built on this delegate. A
    // 3,000-product in-memory catalog indexes in a few milliseconds either
    // way, so single-threaded indexing costs nothing measurable here in
    // exchange for the determinism Issue #42 explicitly requires ("one
    // deterministic headless reproduction command").
    let mut writer = index.writer_with_num_threads(1, 64_000_000)?;
    for product in &catalog.products {
        let mut doc = tantivy::TantivyDocument::default();
        doc.add_u64(ordinal_field, product.id.0);
        doc.add_text(title_field, &product.title);
        for value in product.attributes.values() {
            if let AttributeValue::Text(t) = value {
                doc.add_text(description_field, t);
            }
        }
        writer.add_document(doc)?;
    }
    writer.commit()?;
    Ok(BuiltIndex {
        index,
        ordinal_field,
        title_field,
        description_field,
    })
}

/// A `Query` that restricts an inner query to documents whose
/// `product_ordinal` fast-field value is a member of `allowed`. The score
/// of every surviving document is exactly the inner query's own score --
/// this only filters, it never re-scores.
struct BitmapRestrictQuery {
    inner: Box<dyn Query>,
    allowed: Arc<RoaringBitmap>,
}

impl Clone for BitmapRestrictQuery {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.box_clone(),
            allowed: Arc::clone(&self.allowed),
        }
    }
}

impl std::fmt::Debug for BitmapRestrictQuery {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "BitmapRestrictQuery(allowed_count={}, inner={:?})",
            self.allowed.len(),
            self.inner
        )
    }
}

impl Query for BitmapRestrictQuery {
    fn weight(&self, enable_scoring: EnableScoring<'_>) -> tantivy::Result<Box<dyn Weight>> {
        let inner_weight = self.inner.weight(enable_scoring)?;
        Ok(Box::new(BitmapRestrictWeight {
            inner_weight,
            allowed: Arc::clone(&self.allowed),
        }))
    }

    fn query_terms<'a>(&'a self, visitor: &mut dyn FnMut(&'a tantivy::Term, bool)) {
        self.inner.query_terms(visitor);
    }
}

struct BitmapRestrictWeight {
    inner_weight: Box<dyn Weight>,
    allowed: Arc<RoaringBitmap>,
}

impl Weight for BitmapRestrictWeight {
    fn scorer(&self, reader: &SegmentReader, boost: Score) -> tantivy::Result<Box<dyn Scorer>> {
        let inner_scorer = self.inner_weight.scorer(reader, boost)?;
        let ordinal_column = reader
            .fast_fields()
            .u64(PRODUCT_ORDINAL_FIELD)
            .map_err(|e| TantivyError::SchemaError(e.to_string()))?;
        Ok(Box::new(BitmapFilterScorer::new(
            inner_scorer,
            ordinal_column,
            Arc::clone(&self.allowed),
        )))
    }

    fn explain(
        &self,
        reader: &SegmentReader,
        doc: DocId,
    ) -> tantivy::Result<tantivy::query::Explanation> {
        self.inner_weight.explain(reader, doc)
    }
}

struct BitmapFilterScorer {
    inner: Box<dyn Scorer>,
    ordinal_column: tantivy::columnar::Column<u64>,
    allowed: Arc<RoaringBitmap>,
}

impl BitmapFilterScorer {
    fn new(
        inner: Box<dyn Scorer>,
        ordinal_column: tantivy::columnar::Column<u64>,
        allowed: Arc<RoaringBitmap>,
    ) -> Self {
        let mut me = Self {
            inner,
            ordinal_column,
            allowed,
        };
        me.skip_to_next_allowed();
        me
    }

    fn is_allowed(&self, doc: DocId) -> bool {
        match self.ordinal_column.first(doc) {
            Some(ordinal) if ordinal <= u32::MAX as u64 => self.allowed.contains(ordinal as u32),
            _ => false,
        }
    }

    fn skip_to_next_allowed(&mut self) {
        while self.inner.doc() != TERMINATED && !self.is_allowed(self.inner.doc()) {
            self.inner.advance();
        }
    }
}

impl DocSet for BitmapFilterScorer {
    fn advance(&mut self) -> DocId {
        self.inner.advance();
        self.skip_to_next_allowed();
        self.inner.doc()
    }

    fn doc(&self) -> DocId {
        self.inner.doc()
    }

    fn size_hint(&self) -> u32 {
        self.inner.size_hint()
    }
}

impl Scorer for BitmapFilterScorer {
    fn score(&mut self) -> Score {
        self.inner.score()
    }
}

/// A `LexicalDelegate` backed by the bitmap-filtered Tantivy index above,
/// a drop-in replacement for Phase 2's `TantivyDelegate` reference
/// implementations wherever `restrict_to` is present (the `Hybrid` case).
pub struct BitmapTantivyDelegate {
    reader: IndexReader,
    query_parser: QueryParser,
}

impl BitmapTantivyDelegate {
    /// `default_fields` are the text fields a bare query term is matched
    /// against (mirrors `tantivy::query::QueryParser::for_index`'s own
    /// parameter) -- typically `[title_field, description_field]` for a
    /// real catalog, but deliberately not hard-coded to exactly two named
    /// fields, so a single-field index (e.g. a title-only microbenchmark
    /// comparing this delegate's restriction mechanism against
    /// `TermSetQuery` on equal footing) can reuse this constructor too.
    pub fn new(index: &Index, default_fields: Vec<Field>) -> tantivy::Result<Self> {
        let reader = index.reader()?;
        let query_parser = QueryParser::for_index(index, default_fields);
        Ok(Self {
            reader,
            query_parser,
        })
    }
}

impl LexicalDelegate for BitmapTantivyDelegate {
    fn search(
        &self,
        terms: &[String],
        restrict_to: Option<&std::collections::BTreeSet<ProductId>>,
        limit: usize,
    ) -> Vec<LexicalHit> {
        let text = terms.join(" ");
        let (text_query, _errors) = self.query_parser.parse_query_lenient(&text);

        let final_query: Box<dyn Query> = match restrict_to {
            Some(allowed) if !allowed.is_empty() => {
                let mut bitmap = RoaringBitmap::new();
                for pid in allowed {
                    if pid.0 <= u32::MAX as u64 {
                        bitmap.insert(pid.0 as u32);
                    }
                }
                Box::new(BitmapRestrictQuery {
                    inner: text_query.box_clone(),
                    allowed: Arc::new(bitmap),
                })
            }
            Some(_) => return Vec::new(),
            None => text_query,
        };

        let searcher = self.reader.searcher();
        let Ok(top_docs) = searcher.search(&final_query, &TopDocs::with_limit(limit)) else {
            return Vec::new();
        };

        top_docs
            .into_iter()
            .filter_map(|(score, addr)| {
                let segment_reader = searcher.segment_reader(addr.segment_ord);
                let ordinal_column = segment_reader
                    .fast_fields()
                    .u64(PRODUCT_ORDINAL_FIELD)
                    .ok()?;
                let ordinal = ordinal_column.first(addr.doc_id)?;
                Some(LexicalHit {
                    product: ProductId(ordinal),
                    score: score as f64,
                    variant: None,
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use commerce_core::domain::{
        attributes, BrandId, CategoryId, Inventory, Price, Product, ProductId, ProductTypeId,
        Variant, VariantId,
    };
    use std::collections::BTreeSet;

    fn product(ordinal: u64, title: &str, description: &str) -> Product {
        Product {
            id: ProductId(ordinal),
            product_type: ProductTypeId(1),
            brand: BrandId(0),
            category: CategoryId(1),
            title: title.to_string(),
            attributes: attributes(vec![(
                "description",
                AttributeValue::Text(description.to_string()),
            )]),
            variants: vec![Variant {
                id: VariantId(ordinal),
                attributes: attributes(vec![]),
                price: Price::usd(0),
                inventory: Inventory::in_stock(0),
            }],
        }
    }

    fn test_catalog() -> Catalog {
        Catalog {
            products: vec![
                product(10, "solid wood platform bed", "a sturdy bed frame"),
                product(20, "all-clad slow cooker", "a kitchen appliance"),
                product(30, "wood dining table", "a six-seat dining table"),
            ],
        }
    }

    #[test]
    fn unrestricted_search_finds_the_real_text_match() {
        let catalog = test_catalog();
        let built = build_index(&catalog).unwrap();
        let index = built.index;
        let title_field = built.title_field;
        let description_field = built.description_field;
        let delegate =
            BitmapTantivyDelegate::new(&index, vec![title_field, description_field]).unwrap();

        let hits = delegate.search(&["cooker".to_string()], None, 10);
        assert_eq!(hits.len(), 1, "{hits:?}");
        assert_eq!(hits[0].product, ProductId(20));
    }

    #[test]
    fn restrict_to_excludes_a_real_text_match_outside_the_bitmap() {
        let catalog = test_catalog();
        let built = build_index(&catalog).unwrap();
        let index = built.index;
        let title_field = built.title_field;
        let description_field = built.description_field;
        let delegate =
            BitmapTantivyDelegate::new(&index, vec![title_field, description_field]).unwrap();

        // "wood" matches products 10 and 30's titles; restrict_to only
        // allows product 30 -- the bitmap filter, not the text query
        // itself, must be what excludes product 10.
        let restrict_to: BTreeSet<ProductId> = BTreeSet::from([ProductId(30)]);
        let hits = delegate.search(&["wood".to_string()], Some(&restrict_to), 10);
        assert_eq!(hits.len(), 1, "{hits:?}");
        assert_eq!(hits[0].product, ProductId(30));
    }

    #[test]
    fn restrict_to_matching_nothing_returns_no_hits() {
        let catalog = test_catalog();
        let built = build_index(&catalog).unwrap();
        let index = built.index;
        let title_field = built.title_field;
        let description_field = built.description_field;
        let delegate =
            BitmapTantivyDelegate::new(&index, vec![title_field, description_field]).unwrap();

        let restrict_to: BTreeSet<ProductId> = BTreeSet::from([ProductId(999)]);
        let hits = delegate.search(&["wood".to_string()], Some(&restrict_to), 10);
        assert!(hits.is_empty(), "{hits:?}");
    }

    #[test]
    fn empty_restrict_to_set_returns_no_hits_without_querying_tantivy_at_all() {
        let catalog = test_catalog();
        let built = build_index(&catalog).unwrap();
        let index = built.index;
        let title_field = built.title_field;
        let description_field = built.description_field;
        let delegate =
            BitmapTantivyDelegate::new(&index, vec![title_field, description_field]).unwrap();

        let restrict_to: BTreeSet<ProductId> = BTreeSet::new();
        let hits = delegate.search(&["wood".to_string()], Some(&restrict_to), 10);
        assert!(hits.is_empty(), "{hits:?}");
    }

    #[test]
    fn score_is_exactly_the_inner_text_querys_score_not_rewritten() {
        let catalog = test_catalog();
        let built = build_index(&catalog).unwrap();
        let index = built.index;
        let title_field = built.title_field;
        let description_field = built.description_field;
        let delegate =
            BitmapTantivyDelegate::new(&index, vec![title_field, description_field]).unwrap();

        let unrestricted = delegate.search(&["wood".to_string()], None, 10);
        let restrict_to: BTreeSet<ProductId> = BTreeSet::from([ProductId(10), ProductId(30)]);
        let restricted = delegate.search(&["wood".to_string()], Some(&restrict_to), 10);

        let mut unrestricted_scores: Vec<(ProductId, f64)> =
            unrestricted.iter().map(|h| (h.product, h.score)).collect();
        let mut restricted_scores: Vec<(ProductId, f64)> =
            restricted.iter().map(|h| (h.product, h.score)).collect();
        unrestricted_scores.sort_by_key(|(p, _)| p.0);
        restricted_scores.sort_by_key(|(p, _)| p.0);
        assert_eq!(
            unrestricted_scores, restricted_scores,
            "the bitmap filter must not change any surviving hit's score"
        );
    }
}
