//! Fresh variant-document adapter over the locked public Tantivy 0.22 API.
//! Not the historical product-document delegate: no u32 product-ID narrowing,
//! lenient parse, swallowed errors, top-N cutoff, or sibling text substitution.
use crate::{Error, MAX_LEXICAL_SCORE};
use commerce_core::domain::{effective_attributes, AttributeValue, Catalog};
use std::collections::{BTreeMap, BTreeSet};
use tantivy::collector::TopDocs;
use tantivy::query::{AllQuery, BooleanQuery, EmptyQuery, Query, QueryParser, TermQuery};
use tantivy::schema::{Field, IndexRecordOption, Schema, Value, STORED, TEXT};
use tantivy::{Index, IndexReader, TantivyDocument, Term};

pub(crate) struct Bm25Index {
    index: Index,
    reader: IndexReader,
    variant: Field,
    title: Field,
    text: Field,
    count: usize,
}
impl std::fmt::Debug for Bm25Index {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Bm25Index")
            .field("count", &self.count)
            .finish_non_exhaustive()
    }
}
impl Bm25Index {
    pub fn build(catalog: &Catalog) -> Result<Self, Error> {
        let mut schema = Schema::builder();
        let variant = schema.add_u64_field("variant_id", STORED);
        let title = schema.add_text_field("title", TEXT);
        let text = schema.add_text_field("text", TEXT);
        let index = Index::create_in_ram(schema.build());
        let mut writer = index.writer_with_num_threads(1, 64_000_000).map_err(err)?;
        let mut count = 0;
        for product in &catalog.products {
            for v in &product.variants {
                let mut doc = TantivyDocument::default();
                doc.add_u64(variant, v.id.0);
                doc.add_text(title, &product.title);
                for value in effective_attributes(product, v).values() {
                    if let AttributeValue::Text(value) = value {
                        doc.add_text(text, value);
                    }
                }
                writer.add_document(doc).map_err(err)?;
                count += 1;
            }
        }
        writer.commit().map_err(err)?;
        drop(writer);
        let reader = index.reader().map_err(err)?;
        Ok(Self {
            index,
            reader,
            variant,
            title,
            text,
            count,
        })
    }
    pub fn search(&self, q: &str) -> Result<BTreeMap<u64, (f64, i32)>, Error> {
        let mut parser = QueryParser::for_index(&self.index, vec![self.title, self.text]);
        parser.set_conjunction_by_default();
        // Blank browse is explicit; punctuation-only input is still parsed.
        let query: Box<dyn tantivy::query::Query> = if q.trim().is_empty() {
            Box::new(tantivy::query::AllQuery)
        } else {
            parser.parse_query(q).map_err(err)?
        };
        self.collect(query.as_ref())
    }
    /// Literal free text is analyzed exactly like indexed fields, never parsed as DSL.
    pub fn search_literal(&self, q: &str) -> Result<BTreeMap<u64, (f64, i32)>, Error> {
        if q.trim().is_empty() {
            return self.collect(&AllQuery);
        }
        // Both immutable schema fields use TEXT and its registered default analyzer.
        // Retrieve that analyzer rather than approximating it with a second tokenizer.
        let mut analyzer = self.index.tokenizer_for_field(self.title).map_err(err)?;
        let mut stream = analyzer.token_stream(q);
        let mut seen = BTreeSet::new();
        let mut clauses: Vec<Box<dyn Query>> = Vec::new();
        while stream.advance() {
            let token = &stream.token().text;
            if !seen.insert(token.clone()) {
                continue;
            }
            let fields: Vec<Box<dyn Query>> = [self.title, self.text]
                .into_iter()
                .map(|field| {
                    Box::new(TermQuery::new(
                        Term::from_field_text(field, token),
                        IndexRecordOption::WithFreqs,
                    )) as Box<dyn Query>
                })
                .collect();
            clauses.push(Box::new(BooleanQuery::union(fields)));
        }
        if clauses.is_empty() {
            // A nonblank string removed entirely by the analyzer is NOT browse.
            return self.collect(&EmptyQuery);
        }
        self.collect(&BooleanQuery::intersection(clauses))
    }
    fn collect(&self, query: &dyn Query) -> Result<BTreeMap<u64, (f64, i32)>, Error> {
        if self.count == 0 {
            return Ok(BTreeMap::new());
        }
        let searcher = self.reader.searcher();
        let hits = searcher
            .search(query, &TopDocs::with_limit(self.count))
            .map_err(err)?;
        let mut result = BTreeMap::new();
        for (score, address) in hits {
            if !score.is_finite() || score < 0.0 || f64::from(score) > MAX_LEXICAL_SCORE {
                return Err(Error(
                    "BM25 score must be finite in [0, 1e12] to preserve supplied trace replay"
                        .into(),
                ));
            }
            let doc: TantivyDocument = searcher.doc(address).map_err(err)?;
            let id = doc
                .get_first(self.variant)
                .and_then(|v| v.as_u64())
                .ok_or_else(|| Error("BM25 stored variant ID missing".into()))?;
            if result
                .insert(id, (if score == 0.0 { 0.0 } else { f64::from(score) }, 0))
                .is_some()
            {
                return Err(Error("duplicate BM25 variant ID".into()));
            }
        }
        Ok(result)
    }
}
fn err(error: impl std::fmt::Display) -> Error {
    Error(format!("BM25: {error}"))
}
