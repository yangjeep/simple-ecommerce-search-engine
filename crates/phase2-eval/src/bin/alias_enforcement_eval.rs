//! Issue #6 P1-B: does confidence-tiered enforcement (alias-normalized hard
//! `Constraint`, fuzzy soft `Preference`) preserve more real recall than
//! `compile_lexicon`'s single-BrandId exact match — Issue #9/P2-E07-E10's
//! baseline — while still routing a meaningful share of real traffic to
//! `FastPath`/`Hybrid` (not collapsing to `Punt`-only, which would just be
//! "delegate to Tantivy for everything" wearing a commerce-native costume)?
//!
//! Reuses `planner_integration_eval.rs`'s exact harness (same real catalog/
//! query loader, same Tantivy delegate, same `execute_planned` call, same
//! NDCG@10/Recall@10/MRR/latency metrics) so results are directly
//! comparable, and `round1_eval::classify::measure_precision` (P2-E07-E10's
//! own structural-filter-recall measurement) so the enforcement swap's
//! effect on raw filter recall is measured with the identical function
//! that produced the numbers this experiment is trying to beat.
//!
//! Three lexicon modes, same `min_enum_frequency` trust gate held fixed
//! across all three so only the *enforcement* variable changes:
//!
//! - `baseline`: `compile_lexicon` (Issue #9's exact-BrandId hard match).
//! - `alias_only`: `compile_lexicon_with_alias_enforcement` with
//!   `fuzzy_max_edit_distance=0` (tier 1 alone: deterministic
//!   alias-normalized hard match, tier 2 disabled).
//! - `alias_fuzzy`: same, `fuzzy_max_edit_distance=1` (tier 1 + tier 2:
//!   adds a soft `Preference::StructuralBoost` for brand-shaped query
//!   terms that fuzzy-match a trusted alias group).
//!
//! Usage: cargo run --release -p phase2-eval --bin alias_enforcement_eval
//!        [catalog.jsonl] [queries.jsonl] [index_dir]

use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::PathBuf;
use std::time::Instant;

use commerce_core::cold_start::{
    compile_lexicon, compile_lexicon_with_alias_enforcement, CatalogProfile,
};
use commerce_core::domain::ProductId;
use commerce_core::index::CatalogIndex;
use commerce_core::ir::{compile, SemanticLexicon};
use commerce_core::plan::{
    execute_planned, ExecutionOutcome, LexicalDelegate, LexicalHit, PlannerPolicy,
};
use round1_eval::catalog;
use round1_eval::classify::{self, AggregationRule, QueryClass};
use round1_eval::data::{self, EsciLabel, JudgedExample};

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

/// Identical to `planner_integration_eval.rs`'s `TantivyDelegate` (same
/// restrict_to-pushed-into-the-query fix) -- duplicated rather than shared
/// per this crate's existing one-binary-per-experiment convention.
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

#[derive(Debug, Clone, Copy, PartialEq)]
enum Mode {
    Baseline,
    AliasOnly,
    AliasFuzzy,
}

impl Mode {
    fn label(self) -> &'static str {
        match self {
            Mode::Baseline => "baseline (exact BrandId, Issue #9's compile_lexicon)",
            Mode::AliasOnly => "alias_only (tier 1: deterministic alias-group hard Constraint)",
            Mode::AliasFuzzy => "alias_fuzzy (tier 1 + tier 2: adds fuzzy soft Preference)",
        }
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
        .unwrap_or_else(|| PathBuf::from("dataset_cache/tantivy_alias_enforcement_index"));

    println!("loading + ingesting real catalog...");
    let products = data::load_catalog(&catalog_path);
    let ingested = catalog::build_catalog(&products);
    println!("{} real products ingested", ingested.catalog.products.len());

    println!("building commerce_core structural index...");
    let index = CatalogIndex::build(&ingested.catalog);

    println!("building Tantivy index (same schema/config as P2-E01/P2-E05)...");
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
    let judgments: Vec<JudgedExample> = data::load_queries(&queries_path);
    let mut judged_by_query: HashMap<u64, (String, HashMap<String, EsciLabel>)> = HashMap::new();
    let mut judgments_by_query: HashMap<u64, Vec<&JudgedExample>> = HashMap::new();
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

    // Bound the tier-2 fuzzy candidate pool to real query vocabulary
    // (single tokens through 3-word phrases from the real 22,458-query
    // corpus), not every one of the ~206K raw catalog brand strings --
    // most never appear in a real query, and fuzzy-matching all of them
    // against every trusted alias group would be needlessly expensive.
    // Same "bound by real query relevance, not raw catalog size"
    // discipline P2-E10's `build_query_relevant_brand_sample.py`
    // established for the model-assisted arm.
    let mut query_phrases: HashSet<String> = HashSet::new();
    for (query_text, _) in judged_by_query.values() {
        let tokens: Vec<String> = query_text
            .split_whitespace()
            .map(str::to_lowercase)
            .collect();
        for window in 1..=3usize.min(tokens.len().max(1)) {
            for w in tokens.windows(window) {
                query_phrases.insert(w.join(" "));
            }
        }
    }
    let fuzzy_candidates: Vec<String> = profile
        .brand_names()
        .filter(|name| query_phrases.contains(*name))
        .map(str::to_string)
        .collect();
    println!(
        "fuzzy candidate pool: {} real brand strings also appearing in real query text (of {} distinct real query phrases checked)",
        fuzzy_candidates.len(),
        query_phrases.len()
    );

    let selectivity_threshold = 0.05; // fixed per P2-E05's finding, see planner_integration_eval.rs
    let policy = PlannerPolicy {
        selectivity_threshold,
        delegate_oversample: 20,
    };

    for &min_enum_frequency in &[25usize, 100] {
        for mode in [Mode::Baseline, Mode::AliasOnly, Mode::AliasFuzzy] {
            let lexicon: SemanticLexicon = match mode {
                Mode::Baseline => compile_lexicon(&profile, min_enum_frequency),
                Mode::AliasOnly => {
                    compile_lexicon_with_alias_enforcement(&profile, min_enum_frequency, &[], 0)
                }
                Mode::AliasFuzzy => compile_lexicon_with_alias_enforcement(
                    &profile,
                    min_enum_frequency,
                    &fuzzy_candidates,
                    1,
                ),
            };

            // --- structural filter recall, P2-E07-E10's own measurement fn ---
            let mut compiled_by_query: HashMap<
                u64,
                (QueryClass, commerce_core::ir::CommerceQuery),
            > = HashMap::new();
            for (query_id, (query_text, _)) in &judged_by_query {
                let compiled = compile(query_text, &lexicon);
                let class = classify::classify(query_text, &compiled, &known_ids);
                compiled_by_query.insert(*query_id, (class, compiled));
            }
            let precision_report = classify::measure_precision(
                &ingested.catalog,
                &ingested.asin_to_product_id,
                &judgments_by_query,
                &compiled_by_query,
                AggregationRule::ExistingAnd,
            );

            // --- integrated end-to-end planner execution ---
            let mut outcome_counts: HashMap<&str, usize> = HashMap::new();
            let mut ndcgs = Vec::new();
            let mut recalls = Vec::new();
            let mut mrrs = Vec::new();
            let mut zero_result = 0usize;
            let mut evaluated = 0usize;
            let mut latency_samples = Vec::new();
            let mut hybrid_selectivity_sum = 0.0;
            let mut hybrid_selectivity_n = 0usize;

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
                if let Some(sel) = planned.selectivity {
                    if planned.outcome == ExecutionOutcome::Hybrid {
                        hybrid_selectivity_sum += sel;
                        hybrid_selectivity_n += 1;
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
                let mut ideal_gains: Vec<f64> =
                    judged.values().map(|&l| relevance_gain(l)).collect();
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
            let p50 = percentile_ms(&latency_samples, 0.5);
            println!();
            println!(
                "=== min_enum_frequency={min_enum_frequency}  mode={} ({:.1}s for {evaluated} queries) ===",
                mode.label(),
                sweep_start.elapsed().as_secs_f64()
            );
            println!("  outcome distribution: {outcome_counts:?}");
            println!(
                "  avg Hybrid selectivity (structural_candidates/catalog_size, lower = more candidate-set reduction): {:.4} (n={hybrid_selectivity_n})",
                if hybrid_selectivity_n > 0 {
                    hybrid_selectivity_sum / hybrid_selectivity_n as f64
                } else {
                    0.0
                }
            );
            println!(
                "  structural filter recall vs Exact+Substitute: {:.1}%  vs Exact only: {:.1}%  precision: {:.1}% (P2-E07-E10-comparable, {} queries measured)",
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
            println!(
                "  latency (n={}): p50={:.4}ms  p95={:.4}ms  p99={:.4}ms   QPS/core proxy (1000/p50): {:.0}",
                latency_samples.len(),
                p50,
                percentile_ms(&latency_samples, 0.95),
                percentile_ms(&latency_samples, 0.99),
                if p50 > 0.0 { 1000.0 / p50 } else { 0.0 }
            );
        }
    }

    println!();
    println!("(P2-E01 Tantivy-alone, full 22,458-query set: zero-result=0.6%, NDCG@10=0.3033, Recall@10=0.1801, MRR=0.4838, p50=1.09ms)");
    println!("(R1-E04 Solr baseline, 1,000-query sample: zero-result=0.2%, NDCG@10=0.3052, Recall@10=0.1811, MRR=0.4910, p50=1486us)");
    println!("(QPS/$ proxy: not computed -- no infrastructure cost basis exists in this repo; QPS/core above is the defensible proxy Issue #6 asks for absent one)");

    Ok(())
}
