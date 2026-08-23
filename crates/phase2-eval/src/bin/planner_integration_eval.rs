//! P2-E05: does `commerce_core::plan` (Issue #6 priority 5's
//! FastPath/Hybrid/Punt execution contract) actually compose the
//! structural index with a delegated Tantivy engine correctly and
//! usefully on real data -- not just standalone, as P2-E01 validated, but
//! as one integrated system a shopper query actually goes through?
//!
//! Reuses: `round1_eval`'s real catalog/query loaders and
//! `commerce_core::cold_start::compile_lexicon`'s canonicalization
//! machinery (P2-E02, extended by this experiment to cover brand
//! vocabulary too -- see the `min_enum_frequency` sweep below);
//! `tantivy_relevance_eval.rs`'s index schema/build/metric definitions
//! (kept self-contained here, matching this crate's existing
//! one-binary-per-experiment convention) so relevance numbers are
//! directly comparable to P2-E01 and R1-E04's Solr baseline, not just
//! similarly named.
//!
//! Usage: cargo run --release -p phase2-eval --bin planner_integration_eval
//!        [catalog.jsonl] [queries.jsonl] [index_dir]

use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::PathBuf;
use std::time::Instant;

use commerce_core::cold_start::{compile_lexicon, CatalogProfile};
use commerce_core::domain::ProductId;
use commerce_core::index::CatalogIndex;
use commerce_core::ir::compile;
use commerce_core::plan::{
    execute_planned, ExecutionOutcome, LexicalDelegate, LexicalHit, PlannerPolicy,
};
use round1_eval::data::{self, EsciLabel};
use round1_eval::{catalog, classify};

use tantivy::collector::TopDocs;
use tantivy::query::QueryParser;
use tantivy::schema::{Schema, Value, STORED, STRING, TEXT};
use tantivy::{doc, Index, IndexWriter, ReloadPolicy, TantivyDocument};

const K: usize = 10;

fn relevance_gain(label: EsciLabel) -> f64 {
    match label {
        EsciLabel::Exact => 3.0,
        EsciLabel::Substitute => 2.0,
        EsciLabel::Complement => 1.0,
        EsciLabel::Irrelevant => 0.0,
    }
}

fn percentile_ms(sorted_ns: &[u128], p: f64) -> f64 {
    if sorted_ns.is_empty() {
        return 0.0;
    }
    let idx = ((sorted_ns.len() as f64 - 1.0) * p).round() as usize;
    sorted_ns[idx] as f64 / 1_000_000.0
}

fn build_tantivy_index(
    products: &[data::RealProduct],
    index_dir: &PathBuf,
) -> tantivy::Result<Index> {
    if index_dir.exists() {
        std::fs::remove_dir_all(index_dir).expect("clear stale index dir");
    }
    std::fs::create_dir_all(index_dir).expect("create index dir");

    let mut schema_builder = Schema::builder();
    let id_field = schema_builder.add_text_field("id", STRING | STORED);
    let text_field = schema_builder.add_text_field("all_text", TEXT);
    let schema = schema_builder.build();

    let index = Index::create_in_dir(index_dir, schema)?;
    let mut writer: IndexWriter = index.writer(512_000_000)?;
    for product in products {
        let all_text = format!(
            "{} {} {}",
            product.title,
            product.description.as_deref().unwrap_or(""),
            product.bullets.as_deref().unwrap_or("")
        );
        writer.add_document(doc!(
            id_field => product.id.clone(),
            text_field => all_text,
        ))?;
    }
    writer.commit()?;
    Ok(index)
}

/// Wraps the same real Tantivy index P2-E01 validated standalone as a
/// `commerce_core::plan::LexicalDelegate`.
///
/// **First version (real-data-caught, recorded not discarded):** this
/// deliberately did *not* push `restrict_to` down into the Tantivy query
/// -- `limit` carried `execute_planned`'s oversample factor, and
/// `commerce_core::plan`'s own `verify_and_truncate` did the actual
/// restriction. A real-data smoke run (500 real queries) showed this
/// fails badly: 8/8 `Hybrid` queries in that sample returned fewer than
/// `k` verified hits, and full-integration relevance collapsed to
/// NDCG@10=0.0536 (vs. P2-E01's standalone Tantivy NDCG@10=0.3033) --
/// because Tantivy's *global* top-N ranked by BM25 relevance essentially
/// never overlaps with a genuinely *selective* structural candidate set
/// (the exact condition `Hybrid` routes on) purely by chance. Oversampling
/// further would not fix this in general -- a structural filter selective
/// enough to trigger `Hybrid` can leave zero of its candidates anywhere
/// near Tantivy's global top few thousand results for a generic query.
///
/// **Fixed version (this one):** when `restrict_to` is present, the id
/// field is used as a real filter pushed into the query itself --
/// `BooleanQuery` combining the free-text query with a `TermSetQuery` over
/// `restrict_to`'s ASINs, both `Occur::Must` -- so Tantivy only ever scores
/// and ranks documents that are actually structurally eligible, rather
/// than ranking the whole corpus and hoping the restriction survives
/// truncation. `Punt` (no `restrict_to`) is unaffected and still searches
/// the whole corpus, matching P2-E01 exactly.
struct TantivyDelegate<'a> {
    searcher: tantivy::Searcher,
    query_parser: QueryParser,
    id_field: tantivy::schema::Field,
    asin_to_product_id: &'a HashMap<String, ProductId>,
    product_id_to_asin: &'a HashMap<ProductId, String>,
}

impl LexicalDelegate for TantivyDelegate<'_> {
    fn search(
        &self,
        terms: &[String],
        restrict_to: Option<&BTreeSet<ProductId>>,
        limit: usize,
    ) -> Vec<LexicalHit> {
        if terms.is_empty() {
            return Vec::new();
        }
        let text = terms.join(" ");
        let (text_query, _errors) = self.query_parser.parse_query_lenient(&text);

        let top_docs = match restrict_to {
            None => self
                .searcher
                .search(&text_query, &TopDocs::with_limit(limit))
                .unwrap_or_default(),
            Some(allowed) => {
                let filter_terms = allowed.iter().filter_map(|pid| {
                    self.product_id_to_asin
                        .get(pid)
                        .map(|asin| tantivy::Term::from_field_text(self.id_field, asin))
                });
                let filter_query = tantivy::query::TermSetQuery::new(filter_terms);
                let combined = tantivy::query::BooleanQuery::new(vec![
                    (tantivy::query::Occur::Must, text_query),
                    (
                        tantivy::query::Occur::Must,
                        Box::new(filter_query) as Box<dyn tantivy::query::Query>,
                    ),
                ]);
                self.searcher
                    .search(&combined, &TopDocs::with_limit(limit))
                    .unwrap_or_default()
            }
        };

        top_docs
            .into_iter()
            .filter_map(|(score, addr)| {
                let doc: TantivyDocument = self.searcher.doc(addr).ok()?;
                let asin = doc.get_first(self.id_field)?.as_str()?;
                let product = *self.asin_to_product_id.get(asin)?;
                Some(LexicalHit {
                    product,
                    score: score as f64,
                    variant: None,
                })
            })
            .collect()
    }
}

fn main() -> tantivy::Result<()> {
    let mut args = std::env::args().skip(1);
    let catalog_path = args
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("dataset_cache/export/catalog.jsonl"));
    let queries_path = args
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("dataset_cache/export/queries.jsonl"));
    let index_dir = args
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("dataset_cache/tantivy_planner_index"));

    println!("loading + ingesting real catalog...");
    let products = data::load_catalog(&catalog_path);
    let ingested = catalog::build_catalog(&products);
    println!("{} real products ingested", ingested.catalog.products.len());

    println!("building commerce_core structural index...");
    let index = CatalogIndex::build(&ingested.catalog);

    println!("building Tantivy index (same schema/config as P2-E01)...");
    let t0 = Instant::now();
    let tantivy_index = build_tantivy_index(&products, &index_dir)?;
    println!("Tantivy index built in {:.1}s", t0.elapsed().as_secs_f64());
    let schema = tantivy_index.schema();
    let id_field = schema.get_field("id").unwrap();
    let text_field = schema.get_field("all_text").unwrap();
    let reader = tantivy_index
        .reader_builder()
        .reload_policy(ReloadPolicy::OnCommitWithDelay)
        .try_into()?;
    let product_id_to_asin: HashMap<ProductId, String> = ingested
        .asin_to_product_id
        .iter()
        .map(|(asin, pid)| (*pid, asin.clone()))
        .collect();
    let delegate = TantivyDelegate {
        searcher: reader.searcher(),
        query_parser: QueryParser::for_index(&tantivy_index, vec![text_field]),
        id_field,
        asin_to_product_id: &ingested.asin_to_product_id,
        product_id_to_asin: &product_id_to_asin,
    };

    let profile = CatalogProfile::build(&ingested.catalog, &ingested.brands, &[], &[]);

    println!("loading real queries + judgments...");
    let judgments = data::load_queries(&queries_path);
    let mut judged_by_query: HashMap<u64, (String, HashMap<String, EsciLabel>)> = HashMap::new();
    for j in &judgments {
        judged_by_query
            .entry(j.query_id)
            .or_insert_with(|| (j.query.clone(), HashMap::new()))
            .1
            .insert(j.product_id.clone(), j.label);
    }
    let known_ids: HashSet<&str> = ingested
        .asin_to_product_id
        .keys()
        .map(String::as_str)
        .collect();

    // The selectivity_threshold sweep (Hybrid vs. Punt routing) is fixed
    // at one representative value here: a first pass sweeping {0.01, 0.05,
    // 0.20} produced byte-for-byte identical outcome distributions and
    // relevance numbers at every value (see docs/experiments/PHASE2_LOG.md
    // P2-E05) -- the real, unfiltered brand vocabulary's candidate sets
    // were overwhelmingly singleton products (one-off junk brand
    // strings), so selectivity never varied enough to cross any threshold
    // in that range differently. What actually varies results, found via
    // that same run, is `min_enum_frequency` -- now applied to brand
    // vocabulary too (`commerce_core::cold_start::compile_lexicon`) -- so
    // that is what this run sweeps instead.
    let selectivity_threshold = 0.05;
    let policy = PlannerPolicy {
        selectivity_threshold,
        delegate_oversample: 20,
    };
    for &min_enum_frequency in &[1usize, 5, 25, 100] {
        let lexicon = compile_lexicon(&profile, min_enum_frequency);
        let mut outcome_counts: HashMap<&str, usize> = HashMap::new();
        let mut ndcgs = Vec::new();
        let mut recalls = Vec::new();
        let mut mrrs = Vec::new();
        let mut zero_result = 0usize;
        let mut evaluated = 0usize;
        let mut latency_samples = Vec::new();
        let mut hybrid_short_of_k = 0usize;
        let mut hybrid_total = 0usize;

        let sweep_start = Instant::now();
        for (query_text, judged) in judged_by_query.values() {
            let relevant_ids: HashSet<&String> = judged
                .iter()
                .filter(|(_, label)| label.is_relevant())
                .map(|(pid, _)| pid)
                .collect();
            if relevant_ids.is_empty() {
                continue;
            }

            let compiled = compile(query_text, &lexicon);
            let _class = classify::classify(query_text, &compiled, &known_ids);

            let start = Instant::now();
            let (planned, hits) = execute_planned(
                &compiled,
                &ingested.catalog,
                &index,
                Some(&delegate),
                K,
                &policy,
                None,
            );
            latency_samples.push(start.elapsed().as_nanos());

            *outcome_counts
                .entry(match planned.outcome {
                    ExecutionOutcome::FastPath => "FastPath",
                    ExecutionOutcome::Hybrid => "Hybrid",
                    ExecutionOutcome::Punt => "Punt",
                })
                .or_insert(0) += 1;
            if planned.outcome == ExecutionOutcome::Hybrid {
                hybrid_total += 1;
                if hits.len() < K {
                    hybrid_short_of_k += 1;
                }
            }

            evaluated += 1;
            if hits.is_empty() {
                zero_result += 1;
            }

            let ranked_ids: Vec<&String> = hits
                .iter()
                .filter_map(|h| product_id_to_asin.get(&h.product))
                .collect();

            let dcg: f64 = ranked_ids
                .iter()
                .enumerate()
                .map(|(i, pid)| {
                    let gain = judged
                        .get(*pid)
                        .map(|&label| relevance_gain(label))
                        .unwrap_or(0.0);
                    gain / (i as f64 + 2.0).log2()
                })
                .sum();
            let mut ideal_gains: Vec<f64> = judged.values().map(|&l| relevance_gain(l)).collect();
            ideal_gains.sort_by(|a, b| b.total_cmp(a));
            let idcg: f64 = ideal_gains
                .iter()
                .take(K)
                .enumerate()
                .map(|(i, &g)| g / (i as f64 + 2.0).log2())
                .sum();
            ndcgs.push(if idcg > 0.0 { dcg / idcg } else { 0.0 });

            let hit = ranked_ids
                .iter()
                .filter(|pid| relevant_ids.contains(**pid))
                .count();
            recalls.push(hit as f64 / relevant_ids.len() as f64);

            let rr = ranked_ids
                .iter()
                .position(|pid| relevant_ids.contains(*pid))
                .map(|pos| 1.0 / (pos as f64 + 1.0))
                .unwrap_or(0.0);
            mrrs.push(rr);
        }

        latency_samples.sort_unstable();
        let n = ndcgs.len() as f64;
        println!();
        println!("=== min_enum_frequency={min_enum_frequency} (selectivity_threshold={selectivity_threshold:.2} fixed; {:.1}s for {evaluated} queries) ===", sweep_start.elapsed().as_secs_f64());
        println!("  outcome distribution: {outcome_counts:?}");
        println!(
            "  Hybrid queries returning <{K} hits despite oversampled delegate search: {hybrid_short_of_k}/{hybrid_total}"
        );
        println!(
            "  zero-result rate: {}/{} ({:.2}%)",
            zero_result,
            evaluated,
            zero_result as f64 / evaluated as f64 * 100.0
        );
        println!("  NDCG@10:   {:.4}", ndcgs.iter().sum::<f64>() / n);
        println!("  Recall@10: {:.4}", recalls.iter().sum::<f64>() / n);
        println!("  MRR:       {:.4}", mrrs.iter().sum::<f64>() / n);
        println!(
            "  latency (n={}): p50={:.4}ms  p95={:.4}ms  p99={:.4}ms",
            latency_samples.len(),
            percentile_ms(&latency_samples, 0.5),
            percentile_ms(&latency_samples, 0.95),
            percentile_ms(&latency_samples, 0.99)
        );
    }

    println!();
    println!("(P2-E01 Tantivy-alone, full 22,458-query set: zero-result=0.6%, NDCG@10=0.3033, Recall@10=0.1801, MRR=0.4838, p50=1.09ms)");
    println!("(R1-E04 Solr baseline, 1,000-query sample: zero-result=0.2%, NDCG@10=0.3052, Recall@10=0.1811, MRR=0.4910, p50=1486us)");

    Ok(())
}
