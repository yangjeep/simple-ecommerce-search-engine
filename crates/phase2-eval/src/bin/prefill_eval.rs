//! Issue #6 P1-C: does predictive semantic prefill (inferring latent brand
//! structure from catalog-derived title-phrase co-occurrence, not literal
//! query text) move real traffic from `Punt`->`Hybrid` or increase usable
//! structural information, while preserving relevance -- measured
//! end-to-end against the exact same real corpus and harness P1-B used, so
//! the two are directly comparable.
//!
//! Motivated by P2-E11's real root-cause diagnostic: franchise/media-
//! property-vs-manufacturer mismatches ("Pokemon" query, "Ultra Pro"
//! actual brand) and missing brand data were real, sizeable contributors
//! to the brand-filter recall gap that no string-similarity enforcement
//! mechanism (P1-B) could address. This tests whether catalog-derived
//! co-occurrence can.
//!
//! Usage: cargo run --release -p phase2-eval --bin prefill_eval
//!        [catalog.jsonl] [queries.jsonl] [index_dir]

use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::PathBuf;
use std::time::Instant;

use commerce_core::cold_start::{
    apply_predictive_prefill, compile_lexicon, CatalogProfile, PrefillPolicy, TitlePhraseIndex,
};
use commerce_core::domain::{BrandId, ProductId};
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
    // Dedicated title-only field: predictive prefill's premise is "which
    // products carry this phrase in their *title*," a tighter, less noisy
    // signal than the full title+description+bullets blob the lexical
    // delegate searches.
    let title_field = schema_builder.add_text_field("title", TEXT);
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
            title_field => product.title.clone(),
        ))?;
    }
    writer.commit()?;
    Ok(index)
}

/// Identical shape to `alias_enforcement_eval.rs`'s `TantivyDelegate`.
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

/// `TitlePhraseIndex` backed by a Tantivy phrase query against the
/// title-only field, with a same-process cache: real query text repeats
/// phrases often enough (e.g. "phone case" across many queries) that
/// caching cuts Tantivy round-trips substantially without changing any
/// result.
struct TantivyTitlePhraseIndex<'a> {
    searcher: tantivy::Searcher,
    title_query_parser: QueryParser,
    id_field: tantivy::schema::Field,
    asin_to_product_id: &'a HashMap<String, ProductId>,
    cache: RefCell<HashMap<String, Vec<ProductId>>>,
}

impl TitlePhraseIndex for TantivyTitlePhraseIndex<'_> {
    fn products_containing_phrase(&self, phrase: &str, limit: usize) -> Vec<ProductId> {
        if let Some(cached) = self.cache.borrow().get(phrase) {
            return cached.iter().take(limit).copied().collect();
        }
        let quoted = format!("\"{}\"", phrase.replace('"', ""));
        let (query, _errors) = self.title_query_parser.parse_query_lenient(&quoted);
        let top_docs = self
            .searcher
            .search(&query, &TopDocs::with_limit(limit))
            .unwrap_or_default();
        let ids: Vec<ProductId> = top_docs
            .into_iter()
            .filter_map(|(_, addr)| {
                let doc: TantivyDocument = self.searcher.doc(addr).ok()?;
                let asin = doc.get_first(self.id_field)?.as_str()?;
                self.asin_to_product_id.get(asin).copied()
            })
            .collect();
        self.cache
            .borrow_mut()
            .insert(phrase.to_string(), ids.clone());
        ids
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Mode {
    Baseline,
    WithPrefill,
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
        .unwrap_or_else(|| PathBuf::from("dataset_cache/tantivy_prefill_index"));

    println!("loading + ingesting real catalog...");
    let products = data::load_catalog(&catalog_path);
    let ingested = catalog::build_catalog(&products);
    println!("{} real products ingested", ingested.catalog.products.len());

    println!("building commerce_core structural index...");
    let index = CatalogIndex::build(&ingested.catalog);

    println!("building Tantivy index (all_text + title fields)...");
    let t0 = Instant::now();
    let tantivy_index = build_tantivy_index(&products, &index_dir)?;
    println!("Tantivy index built in {:.1}s", t0.elapsed().as_secs_f64());
    let schema = tantivy_index.schema();
    let id_field = schema.get_field("id").unwrap();
    let text_field = schema.get_field("all_text").unwrap();
    let title_field = schema.get_field("title").unwrap();
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
    let phrase_index = TantivyTitlePhraseIndex {
        searcher: reader.searcher(),
        title_query_parser: QueryParser::for_index(&tantivy_index, vec![title_field]),
        id_field,
        asin_to_product_id: &ingested.asin_to_product_id,
        cache: RefCell::new(HashMap::new()),
    };

    let profile = CatalogProfile::build(&ingested.catalog, &ingested.brands, &[], &[]);
    let brand_name_by_id: HashMap<BrandId, String> = ingested
        .brands
        .iter()
        .map(|b| (b.id, b.name.clone()))
        .collect();

    println!("loading real queries + judgments...");
    let judgments = data::load_queries(&queries_path);
    let mut judged_by_query: HashMap<u64, (String, HashMap<String, EsciLabel>)> = HashMap::new();
    let mut judgments_by_query: HashMap<u64, Vec<&data::JudgedExample>> = HashMap::new();
    for j in &judgments {
        judged_by_query
            .entry(j.query_id)
            .or_insert_with(|| (j.query.clone(), HashMap::new()))
            .1
            .insert(j.product_id.clone(), j.label);
        judgments_by_query.entry(j.query_id).or_default().push(j);
    }
    let known_ids: HashSet<&str> = ingested
        .asin_to_product_id
        .keys()
        .map(String::as_str)
        .collect();

    let min_enum_frequency = 25; // P1-B's primary comparison threshold
    let selectivity_threshold = 0.05; // fixed per P2-E05's finding
    let planner_policy = PlannerPolicy {
        selectivity_threshold,
        delegate_oversample: 20,
    };
    let prefill_policy = PrefillPolicy {
        ngram_sizes: vec![2, 3],
        sample_limit: 50,
        high_confidence_min_purity: 0.9,
        high_confidence_min_occurrence: 20,
        medium_confidence_min_purity: 0.65,
        medium_confidence_min_occurrence: 8,
        preference_weight: 1.0,
    };
    let lexicon = compile_lexicon(&profile, min_enum_frequency);

    let mut route_moved_punt_to_hybrid = 0usize;
    let mut route_moved_punt_to_fastpath = 0usize;
    let mut queries_with_a_prediction = 0usize;

    for mode in [Mode::Baseline, Mode::WithPrefill] {
        let mut outcome_counts: HashMap<&str, usize> = HashMap::new();
        let mut ndcgs = Vec::new();
        let mut recalls = Vec::new();
        let mut mrrs = Vec::new();
        let mut zero_result = 0usize;
        let mut evaluated = 0usize;
        let mut latency_samples = Vec::new();
        let mut hybrid_selectivity_sum = 0.0;
        let mut hybrid_selectivity_n = 0usize;

        // Structural filter recall (P2-E07-E11-comparable), recomputed
        // per mode since prefill can add a Brand constraint baseline
        // never had.
        let mut compiled_by_query: HashMap<
            u64,
            (classify::QueryClass, commerce_core::ir::CommerceQuery),
        > = HashMap::new();

        let sweep_start = Instant::now();
        for (query_id, (query_text, judged)) in &judged_by_query {
            let relevant_ids: HashSet<&String> = judged
                .iter()
                .filter(|(_, label)| label.is_relevant())
                .map(|(pid, _)| pid)
                .collect();
            if relevant_ids.is_empty() {
                continue;
            }

            let mut compiled = compile(query_text, &lexicon);
            let before_had_constraint = !compiled.constraints.is_empty();

            if mode == Mode::WithPrefill {
                let preferences_before = compiled.preferences.len();
                apply_predictive_prefill(
                    &mut compiled,
                    query_text,
                    &ingested.catalog,
                    &index,
                    &phrase_index,
                    &brand_name_by_id,
                    &prefill_policy,
                );
                let gained_constraint = !before_had_constraint && !compiled.constraints.is_empty();
                let gained_preference = compiled.preferences.len() > preferences_before;
                if gained_constraint || gained_preference {
                    queries_with_a_prediction += 1;
                }
            }

            let class = classify::classify(query_text, &compiled, &known_ids);
            compiled_by_query.insert(*query_id, (class, compiled.clone()));

            let start = Instant::now();
            let (planned, hits) = execute_planned(
                &compiled,
                &ingested.catalog,
                &index,
                Some(&delegate),
                K,
                &planner_policy,
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
            if let Some(sel) = planned.selectivity {
                if planned.outcome == ExecutionOutcome::Hybrid {
                    hybrid_selectivity_sum += sel;
                    hybrid_selectivity_n += 1;
                }
            }
            if mode == Mode::WithPrefill && !before_had_constraint {
                match planned.outcome {
                    ExecutionOutcome::Hybrid => route_moved_punt_to_hybrid += 1,
                    ExecutionOutcome::FastPath => route_moved_punt_to_fastpath += 1,
                    ExecutionOutcome::Punt => {}
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

        let precision_report = classify::measure_precision(
            &ingested.catalog,
            &ingested.asin_to_product_id,
            &judgments_by_query,
            &compiled_by_query,
            classify::AggregationRule::ExistingAnd,
        );

        latency_samples.sort_unstable();
        let n = ndcgs.len() as f64;
        println!();
        println!(
            "=== mode={mode:?}  min_enum_frequency={min_enum_frequency} ({:.1}s for {evaluated} queries) ===",
            sweep_start.elapsed().as_secs_f64()
        );
        println!("  outcome distribution: {outcome_counts:?}");
        println!(
            "  avg Hybrid selectivity: {:.4} (n={hybrid_selectivity_n})",
            if hybrid_selectivity_n > 0 {
                hybrid_selectivity_sum / hybrid_selectivity_n as f64
            } else {
                0.0
            }
        );
        println!(
            "  structural filter recall vs Exact+Substitute: {:.1}%  vs Exact only: {:.1}%  precision: {:.1}% ({} queries measured)",
            precision_report.filter_recall() * 100.0,
            precision_report.exact_recall() * 100.0,
            precision_report.precision() * 100.0,
            precision_report.queries_measured
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
        let p50 = percentile_ms(&latency_samples, 0.5);
        println!(
            "  latency (n={}): p50={:.4}ms  p95={:.4}ms  p99={:.4}ms   QPS/core proxy: {:.0}",
            latency_samples.len(),
            p50,
            percentile_ms(&latency_samples, 0.95),
            percentile_ms(&latency_samples, 0.99),
            if p50 > 0.0 { 1000.0 / p50 } else { 0.0 }
        );
        if mode == Mode::WithPrefill {
            println!(
                "  prefill effect: {queries_with_a_prediction} queries gained a NEW hard Brand constraint; \
                 of those, {route_moved_punt_to_hybrid} moved to Hybrid, {route_moved_punt_to_fastpath} to FastPath \
                 (both were Punt-shaped, i.e. had no structural constraint, before prefill)"
            );
        }
    }

    println!();
    println!("(P2-E11 baseline at the same threshold: NDCG@10=0.2278, Recall@10=0.1354, zero-result=9.55%, outcome FastPath=328/Hybrid=5589/Punt=16541)");

    Ok(())
}
