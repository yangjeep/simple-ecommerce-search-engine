//! Issue #6 P1-D: for each of the 9 real-query structural-shape classes
//! (`round1_eval::query_taxonomy::QueryClass9`), measure the commerce-
//! native structural/hybrid execution path against two mature-baseline
//! proxies -- an embedded Tantivy standalone lexical search (the same
//! validated-equivalent-relevance engine P2-E01 established), and a
//! **fresh, same-run, same-environment Apache Solr 9.10.1 instance**
//! (`dataset_cache/solr/solr-9.10.1`, already indexed with the real
//! 1,215,854-product catalog from R1-E04 -- started fresh for this run,
//! not reused stale numbers) -- to determine which query classes, if any,
//! show a real physical advantage, and by how much.
//!
//! Two measurement phases per class, per Issue #6's rigor requirements:
//!
//! 1. **Single-pass, full-class-sample**: correctness (NDCG@10/Recall@10/
//!    MRR against real ESCI judgments), route/outcome, candidate counts.
//!    Deterministic (`BTreeMap`-ordered iteration throughout, fixing the
//!    HashMap-iteration-order floating-point noise P2-E12 found).
//! 2. **Repeated measurement, smaller per-class sample**: wall-clock
//!    latency via `bench_harness::measured_repeat` with warmup, methods
//!    interleaved via `bench_harness::round_robin_schedule` (not run
//!    back-to-back per method) to avoid drift bias, `Distribution` +
//!    `bootstrap_ci_diff_of_means` for the headline commerce-native-vs-
//!    baseline comparison.
//!
//! Fairness notes (see `docs/research/PAPER_NOTES.md` for the full
//! threats-to-validity writeup, and `case_insensitive_field_regex`'s doc
//! comment for a real bug this harness's first run found and fixed):
//! Solr's `brand`/`color` fields hold the *raw* per-record casing
//! (`scripts/round1/solr_index.py`, `solr.StrField`, no case-folding
//! analyzer), so this harness filters via a case-insensitive whole-field
//! regex query to match `commerce_core`'s own trim+lowercase brand/color
//! identity exactly -- not a single exact-case `brand:"Nike"` query,
//! which would be a *weaker*, unfairly-disadvantaged filter for Solr that
//! misses real casing variants `commerce_core` correctly merges. Solr's
//! `product_type`/`category`/price fields do not exist in this real
//! catalog (documented sentinel-value limitation carried since R1-E01) so
//! `SelectiveMultiAttribute`/`RangeStructural` classes are expected to be
//! empty or near-empty on real data -- reported honestly as such, not
//! fabricated.
//!
//! Usage: cargo run --release -p phase2-eval --bin p1d_physical_advantage_eval
//!        [catalog.jsonl] [queries.jsonl] [solr_base_url] [index_dir]

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::PathBuf;
use std::time::Instant;

use bench_harness::{round_robin_schedule, Distribution, RunManifest};
use commerce_core::cold_start::{compile_lexicon, CatalogProfile};
use commerce_core::domain::ProductId;
use commerce_core::index::CatalogIndex;
use commerce_core::ir::compile;
use commerce_core::plan::{
    execute_planned, ExecutionOutcome, LexicalDelegate, LexicalHit, PlannerPolicy,
};
use query_taxonomy::{classify9, QueryClass9};
use round1_eval::data::{self, EsciLabel};
use round1_eval::{catalog, query_taxonomy};

use tantivy::collector::{Count, TopDocs};
use tantivy::query::QueryParser;
use tantivy::schema::{Schema, Value, STORED, STRING, TEXT};
use tantivy::{doc, Index, IndexWriter, ReloadPolicy, TantivyDocument};

const K: usize = 10;
const CORRECTNESS_SAMPLE_PER_CLASS: usize = 200;
const LATENCY_SAMPLE_PER_CLASS: usize = 20;
const LATENCY_REPS_PER_METHOD: usize = 30; // decision-grade per Issue #6's rigor bar
const LATENCY_WARMUP: usize = 5;
const SEED: u64 = 7;

fn current_rss_kb() -> Option<u64> {
    let status = fs::read_to_string("/proc/self/status").ok()?;
    status
        .lines()
        .find_map(|line| line.strip_prefix("VmRSS:"))
        .and_then(|rest| rest.trim().trim_end_matches(" kB").trim().parse().ok())
}

fn process_rss_kb(pid: u32) -> Option<u64> {
    let status = fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    status
        .lines()
        .find_map(|line| line.strip_prefix("VmRSS:"))
        .and_then(|rest| rest.trim().trim_end_matches(" kB").trim().parse().ok())
}

fn relevance_gain(label: EsciLabel) -> f64 {
    match label {
        EsciLabel::Exact => 3.0,
        EsciLabel::Substitute => 2.0,
        EsciLabel::Complement => 1.0,
        EsciLabel::Irrelevant => 0.0,
    }
}

/// Deterministic (no HashMap iteration involved): `hits`/`judged` are both
/// already-ordered inputs (`Vec`/`BTreeMap`), so summation order is fixed
/// call to call -- the P2-E12 fix applied for real this time.
fn ndcg_recall_mrr(
    hits: &[String],
    judged: &BTreeMap<String, EsciLabel>,
    k: usize,
) -> (f64, f64, f64) {
    let relevant_total = judged.values().filter(|l| l.is_relevant()).count();
    if relevant_total == 0 {
        return (0.0, 0.0, 0.0);
    }
    let top: Vec<&str> = hits.iter().take(k).map(String::as_str).collect();
    let is_relevant_hit = |pid: &str| judged.get(pid).is_some_and(|l| l.is_relevant());

    let dcg: f64 = top
        .iter()
        .enumerate()
        .map(|(i, &pid)| {
            let gain = judged.get(pid).map(|&l| relevance_gain(l)).unwrap_or(0.0);
            gain / (i as f64 + 2.0).log2()
        })
        .sum();
    let mut ideal_gains: Vec<f64> = judged.values().map(|&l| relevance_gain(l)).collect();
    ideal_gains.sort_by(|a, b| b.total_cmp(a));
    let idcg: f64 = ideal_gains
        .iter()
        .take(k)
        .enumerate()
        .map(|(i, &g)| g / (i as f64 + 2.0).log2())
        .sum();
    let ndcg = if idcg > 0.0 { dcg / idcg } else { 0.0 };

    let hit_count = top.iter().filter(|&&pid| is_relevant_hit(pid)).count();
    let recall = hit_count as f64 / relevant_total as f64;

    let rr = top
        .iter()
        .position(|&pid| is_relevant_hit(pid))
        .map(|pos| 1.0 / (pos as f64 + 1.0))
        .unwrap_or(0.0);

    (ndcg, recall, rr)
}

fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Builds a Lucene `RegexpQuery` pattern that matches `s` case-
/// insensitively, anchored to the *entire* field value (Lucene's regex
/// queries on a `StrField`/`strings` field are implicitly whole-term-
/// anchored, confirmed directly against this run's own Solr instance:
/// `brand:/[Nn][Ii][Kk][Ee]/` returns exactly the ASCII-case variants of
/// "Nike," not substring matches like "Nike Inc").
///
/// **Why this exists at all** (P2-E13, `docs/experiments/PHASE2_LOG.md`):
/// this harness originally filtered on a field named `brand_lower`,
/// assumed to be a Solr-side lowercased copy field. Verified directly
/// against the running Solr core: `brand_lower` is a schema artifact with
/// **zero indexed values** across all 1,215,854 documents -- every query
/// against it returns 0 hits, which is exactly why every Solr number in
/// the first P1-D run that touched a brand/color filter was catastrophically
/// wrong (100% zero-result for pure structural classes). Solr's real
/// `brand`/`color` fields hold the *raw*, per-record casing
/// (`scripts/round1/solr_index.py`) with no case-folding analyzer
/// (`solr.StrField`, exact match only) -- a regex built from the already-
/// lowercased `commerce_core::domain::Brand::name` (itself the
/// trim+lowercase-normalized identity `round1_eval::catalog::build_catalog`
/// assigns a `BrandId` by) reproduces `commerce_core`'s own case-
/// insensitive brand-identity grouping exactly, which neither the empty
/// `brand_lower` field nor a single exact-case `brand:"Nike"` query would
/// (the latter would miss real "NIKE"/"nike" casing variants that
/// `commerce_core` correctly treats as the same brand).
fn case_insensitive_field_regex(s: &str) -> String {
    const REGEX_METACHARS: &str = "\\.?+*|{}[]()\"#@&<>~^$/";
    let mut out = String::with_capacity(s.len() * 4);
    for c in s.chars() {
        if c.is_ascii_alphabetic() {
            out.push('[');
            out.push(c.to_ascii_lowercase());
            out.push(c.to_ascii_uppercase());
            out.push(']');
        } else if REGEX_METACHARS.contains(c) {
            out.push('\\');
            out.push(c);
        } else {
            out.push(c);
        }
    }
    out
}

struct SolrResult {
    /// Server-side-only `QTime` from `responseHeader` (ms) -- excludes
    /// HTTP/JSON/network overhead. The separate repeated-measurement
    /// latency sub-experiment times the full round trip independently
    /// (via its own `Instant::now()` around each `solr_search` call), so
    /// this struct does not duplicate that -- it exists so the
    /// single-pass correctness loop can *also* report the compute-only
    /// proxy without a second HTTP round trip.
    qtime_ms: f64,
    num_found: usize,
    ids: Vec<String>,
}

fn solr_search(base_url: &str, q: &str, fq: &[String], rows: usize) -> Option<SolrResult> {
    let mut url = format!(
        "{base_url}/select?q={}&rows={rows}&fl=id",
        percent_encode(q)
    );
    for f in fq {
        url.push_str(&format!("&fq={}", percent_encode(f)));
    }
    let resp = ureq::get(&url)
        .timeout(std::time::Duration::from_secs(10))
        .call()
        .ok()?;
    let body: serde_json::Value = resp.into_json().ok()?;
    let qtime_ms = body["responseHeader"]["QTime"].as_f64().unwrap_or(0.0);
    let num_found = body["response"]["numFound"].as_u64().unwrap_or(0) as usize;
    let ids: Vec<String> = body["response"]["docs"]
        .as_array()?
        .iter()
        .filter_map(|d| d["id"].as_str().map(str::to_string))
        .collect();
    Some(SolrResult {
        qtime_ms,
        num_found,
        ids,
    })
}

/// Builds `(q, fq)` for a query, matching the *shape* a real commerce Solr
/// deployment would use for this class: structural signal (when this
/// dataset actually has it -- brand/color, both via a case-insensitive
/// whole-field regex against Solr's raw `brand`/`color` fields, see
/// `case_insensitive_field_regex`'s doc comment for why) goes to `fq`
/// (filter query, matching commerce-native's hard-constraint semantics),
/// free text goes to `q` via `edismax` over `all_text` (matching
/// Tantivy's own full-text scope).
fn solr_query_for(
    query_text: &str,
    residual_lexical: &[String],
    brand: Option<&str>,
    color: Option<&str>,
) -> (String, Vec<String>) {
    let mut fq = Vec::new();
    if let Some(b) = brand {
        fq.push(format!("brand:/{}/", case_insensitive_field_regex(b)));
    }
    if let Some(c) = color {
        fq.push(format!("color:/{}/", case_insensitive_field_regex(c)));
    }
    let text = if residual_lexical.is_empty() {
        query_text.to_string()
    } else {
        residual_lexical.join(" ")
    };
    let q = if text.trim().is_empty() {
        "*:*".to_string()
    } else {
        format!("{{!edismax qf=all_text}}{}", text)
    };
    (q, fq)
}

/// Issue #6 P1-D / P2-E16 (`docs/experiments/PHASE2_LOG.md`): the same
/// brand/color extraction the correctness loop already did inline,
/// factored out so the latency sub-experiment can build the *same*
/// `solr_query_for` request the correctness loop measures -- see
/// `solr_query_for`'s doc comment and P2-E16 for why using a different,
/// unfiltered raw-text query here was a real, severe measurement bug (it
/// silently hit Solr's unpopulated `_text_` default field, timing a
/// guaranteed-zero-hit lookup instead of the real edismax/`all_text` +
/// brand/color `fq` search every other measurement in this harness uses).
fn extract_brand_color(
    compiled: &commerce_core::ir::CommerceQuery,
    brand_name_by_id: &HashMap<commerce_core::domain::BrandId, String>,
) -> (Option<String>, Option<String>) {
    let brand = compiled.constraints.iter().find_map(|c| match c {
        commerce_core::ir::ResolvedConstraint::Structural(
            commerce_core::ir::StructuralConstraint::Brand(id),
        ) => brand_name_by_id.get(id).cloned(),
        commerce_core::ir::ResolvedConstraint::Structural(
            commerce_core::ir::StructuralConstraint::BrandAny(ids),
        ) => ids.first().and_then(|id| brand_name_by_id.get(id)).cloned(),
        _ => None,
    });
    let color = compiled.constraints.iter().find_map(|c| match c {
        commerce_core::ir::ResolvedConstraint::Attribute(
            commerce_core::domain::Constraint::Enum { attribute, value },
        ) if attribute == "color" => Some(value.clone()),
        _ => None,
    });
    (brand, color)
}

fn build_tantivy_index(
    products: &[data::RealProduct],
    index_dir: &PathBuf,
) -> tantivy::Result<Index> {
    if index_dir.exists() {
        fs::remove_dir_all(index_dir).expect("clear stale index dir");
    }
    fs::create_dir_all(index_dir).expect("create index dir");
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
        writer.add_document(doc!(id_field => product.id.clone(), text_field => all_text))?;
    }
    writer.commit()?;
    Ok(index)
}

struct TantivyDelegate<'a> {
    searcher: tantivy::Searcher,
    query_parser: QueryParser,
    id_field: tantivy::schema::Field,
    asin_to_product_id: &'a HashMap<String, ProductId>,
    product_id_to_asin: &'a HashMap<ProductId, String>,
}

impl TantivyDelegate<'_> {
    /// Real total-hit count via tantivy's `Count` collector, not just the
    /// top-K returned -- a fair candidate-count comparison against Solr's
    /// own `numFound`.
    fn search_with_count(
        &self,
        terms: &[String],
        restrict_to: Option<&BTreeSet<ProductId>>,
        limit: usize,
    ) -> (Vec<LexicalHit>, usize) {
        if terms.is_empty() {
            return (Vec::new(), 0);
        }
        let text = terms.join(" ");
        let (text_query, _errors) = self.query_parser.parse_query_lenient(&text);
        let query: Box<dyn tantivy::query::Query> = match restrict_to {
            None => text_query,
            Some(allowed) => {
                let filter_terms = allowed.iter().filter_map(|pid| {
                    self.product_id_to_asin
                        .get(pid)
                        .map(|asin| tantivy::Term::from_field_text(self.id_field, asin))
                });
                let filter_query = tantivy::query::TermSetQuery::new(filter_terms);
                Box::new(tantivy::query::BooleanQuery::new(vec![
                    (tantivy::query::Occur::Must, text_query),
                    (tantivy::query::Occur::Must, Box::new(filter_query)),
                ]))
            }
        };
        let (top_docs, count) = self
            .searcher
            .search(&query, &(TopDocs::with_limit(limit), Count))
            .unwrap_or_default();
        let hits = top_docs
            .into_iter()
            .filter_map(|(score, addr)| {
                let doc: TantivyDocument = self.searcher.doc(addr).ok()?;
                let asin = doc.get_first(self.id_field)?.as_str()?;
                let product = *self.asin_to_product_id.get(asin)?;
                Some(LexicalHit {
                    product,
                    score: score as f64,
                })
            })
            .collect();
        (hits, count)
    }
}

impl LexicalDelegate for TantivyDelegate<'_> {
    fn search(
        &self,
        terms: &[String],
        restrict_to: Option<&BTreeSet<ProductId>>,
        limit: usize,
    ) -> Vec<LexicalHit> {
        self.search_with_count(terms, restrict_to, limit).0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Method {
    CommerceNative,
    TantivyStandalone,
    Solr,
}

impl Method {
    fn label(self) -> &'static str {
        match self {
            Method::CommerceNative => "commerce_native",
            Method::TantivyStandalone => "tantivy_standalone",
            Method::Solr => "solr",
        }
    }
}

#[derive(Debug, Default, Clone)]
struct ClassCorrectness {
    n: usize,
    ndcg_sum: f64,
    recall_sum: f64,
    mrr_sum: f64,
    zero_result: usize,
    candidate_sum: u64,
    /// Solr-only: server-side `QTime` from `responseHeader` (excludes
    /// HTTP/JSON/network overhead) -- the fairer apples-to-apples
    /// compute-only proxy against commerce-native's/Tantivy's in-process
    /// wall-clock. `None` sum for the other two methods (always 0.0, not
    /// meaningful for them since they have no separate server-side leg).
    solr_qtime_sum: f64,
}

impl ClassCorrectness {
    fn record(
        &mut self,
        ndcg: f64,
        recall: f64,
        mrr: f64,
        hit_count: usize,
        candidates: usize,
        solr_qtime_ms: Option<f64>,
    ) {
        self.n += 1;
        self.ndcg_sum += ndcg;
        self.recall_sum += recall;
        self.mrr_sum += mrr;
        if hit_count == 0 {
            self.zero_result += 1;
        }
        self.candidate_sum += candidates as u64;
        if let Some(qt) = solr_qtime_ms {
            self.solr_qtime_sum += qt;
        }
    }

    fn print(&self, label: &str) {
        if self.n == 0 {
            println!("    {label:<20} n=0 (no queries in this class had this method's data)");
            return;
        }
        let qtime_note = if label == Method::Solr.label() {
            format!(
                " avg_server_qtime={:.2}ms",
                self.solr_qtime_sum / self.n as f64
            )
        } else {
            String::new()
        };
        println!(
            "    {label:<20} n={:<5} NDCG@10={:.4} Recall@10={:.4} MRR={:.4} zero_result={}/{} ({:.1}%) avg_candidates={:.0}{qtime_note}",
            self.n,
            self.ndcg_sum / self.n as f64,
            self.recall_sum / self.n as f64,
            self.mrr_sum / self.n as f64,
            self.zero_result,
            self.n,
            self.zero_result as f64 / self.n as f64 * 100.0,
            self.candidate_sum as f64 / self.n as f64
        );
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
    let solr_base_url = args
        .next()
        .unwrap_or_else(|| "http://localhost:8983/solr/commerce_bench".to_string());
    let index_dir = args
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("dataset_cache/tantivy_p1d_index"));

    println!("loading + ingesting real catalog...");
    let products = data::load_catalog(&catalog_path);
    let ingested = catalog::build_catalog(&products);
    println!("{} real products ingested", ingested.catalog.products.len());

    println!("building commerce_core structural index...");
    let index = CatalogIndex::build(&ingested.catalog);
    let rss_after_structural = current_rss_kb();

    println!("building Tantivy index...");
    let t0 = Instant::now();
    let tantivy_index = build_tantivy_index(&products, &index_dir)?;
    println!("Tantivy index built in {:.1}s", t0.elapsed().as_secs_f64());
    let rss_after_tantivy = current_rss_kb();
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

    println!("checking Solr ({solr_base_url})...");
    let solr_ping = solr_search(&solr_base_url, "*:*", &[], 0);
    let solr_available = solr_ping.is_some();
    match &solr_ping {
        Some(r) => println!("  Solr reachable: numFound={}", r.num_found),
        None => {
            println!("  Solr NOT reachable at {solr_base_url} -- Solr comparisons will report n=0")
        }
    }

    let profile = CatalogProfile::build(&ingested.catalog, &ingested.brands, &[], &[]);
    let brand_name_by_id: HashMap<commerce_core::domain::BrandId, String> = ingested
        .brands
        .iter()
        .map(|b| (b.id, b.name.clone()))
        .collect();
    let lexicon = compile_lexicon(&profile, 25);

    println!("loading real queries + judgments...");
    let judgments = data::load_queries(&queries_path);
    let mut judged_by_query: BTreeMap<u64, (String, BTreeMap<String, EsciLabel>)> = BTreeMap::new();
    for j in &judgments {
        judged_by_query
            .entry(j.query_id)
            .or_insert_with(|| (j.query.clone(), BTreeMap::new()))
            .1
            .insert(j.product_id.clone(), j.label);
    }
    println!(
        "{} distinct real judged queries loaded",
        judged_by_query.len()
    );

    // Classify every query, group by class -- deterministic (BTreeMap
    // input, sorted by query_id).
    let mut by_class: BTreeMap<QueryClass9, Vec<u64>> = BTreeMap::new();
    let mut compiled_cache: BTreeMap<u64, commerce_core::ir::CommerceQuery> = BTreeMap::new();
    for (&qid, (text, judged)) in &judged_by_query {
        if !judged.values().any(|l| l.is_relevant()) {
            continue; // no relevance ground truth for this query at all
        }
        let compiled = compile(text, &lexicon);
        let class = classify9(text, &compiled);
        compiled_cache.insert(qid, compiled);
        by_class.entry(class).or_default().push(qid);
    }

    let planner_policy = PlannerPolicy {
        selectivity_threshold: 0.05,
        delegate_oversample: 20,
    };

    let manifest = RunManifest::capture(
        "p1d_physical_advantage",
        "commerce_native_vs_tantivy_vs_solr",
        &catalog_path,
        &queries_path,
        serde_json::json!({
            "min_enum_frequency": 25,
            "selectivity_threshold": 0.05,
            "correctness_sample_per_class": CORRECTNESS_SAMPLE_PER_CLASS,
            "latency_sample_per_class": LATENCY_SAMPLE_PER_CLASS,
            "latency_reps_per_method": LATENCY_REPS_PER_METHOD,
            "solr_base_url": solr_base_url,
            "solr_available": solr_available,
        }),
        SEED,
        LATENCY_WARMUP,
        LATENCY_REPS_PER_METHOD,
    );
    manifest.print();
    if let Some(structural_kb) = rss_after_structural {
        println!(
            "  [manifest] commerce-native structural index RSS: {:.1} MB",
            structural_kb as f64 / 1024.0
        );
    }
    if let Some(tantivy_kb) = rss_after_tantivy {
        println!(
            "  [manifest] combined (structural + Tantivy) process RSS: {:.1} MB",
            tantivy_kb as f64 / 1024.0
        );
    }
    let solr_pid = fs::read_to_string("dataset_cache/solr/solr-9.10.1/bin/solr-8983.pid")
        .ok()
        .and_then(|s| s.trim().parse::<u32>().ok());
    if let Some(pid) = solr_pid {
        if let Some(kb) = process_rss_kb(pid) {
            println!(
                "  [manifest] Solr process RSS (pid {pid}): {:.1} MB",
                kb as f64 / 1024.0
            );
        }
    }
    let artifacts_dir = PathBuf::from("dataset_cache/p1d_artifacts");
    manifest
        .write_json(&artifacts_dir.join("manifest.json"))
        .ok();

    let mut weighted_ndcg: BTreeMap<Method, f64> = BTreeMap::new();
    let mut weighted_n: BTreeMap<Method, usize> = BTreeMap::new();
    let mut all_latency_samples: BTreeMap<Method, Vec<f64>> = BTreeMap::new();

    for &class in &QueryClass9::all() {
        let Some(qids) = by_class.get(&class) else {
            println!(
                "\n=== class={} === n=0 (no real queries classified into this class)",
                class.label()
            );
            continue;
        };
        let sample: Vec<u64> = qids
            .iter()
            .take(CORRECTNESS_SAMPLE_PER_CLASS)
            .copied()
            .collect();
        println!(
            "\n=== class={} === {} real queries in class, sampling {} for correctness",
            class.label(),
            qids.len(),
            sample.len()
        );

        let mut cn_correctness = ClassCorrectness::default();
        let mut ts_correctness = ClassCorrectness::default();
        let mut solr_correctness = ClassCorrectness::default();
        let mut outcome_counts: BTreeMap<&str, usize> = BTreeMap::new();

        for &qid in &sample {
            let (text, judged) = &judged_by_query[&qid];
            let compiled = &compiled_cache[&qid];

            // commerce-native
            let (planned, hits) = execute_planned(
                compiled,
                &ingested.catalog,
                &index,
                Some(&delegate),
                K,
                &planner_policy,
            );
            *outcome_counts
                .entry(match planned.outcome {
                    ExecutionOutcome::FastPath => "FastPath",
                    ExecutionOutcome::Hybrid => "Hybrid",
                    ExecutionOutcome::Punt => "Punt",
                })
                .or_insert(0) += 1;
            let cn_candidates = planned
                .selectivity
                .map(|s| (s * ingested.catalog.products.len() as f64).round() as usize)
                .unwrap_or(ingested.catalog.products.len());
            let cn_ids: Vec<String> = hits
                .iter()
                .filter_map(|h| product_id_to_asin.get(&h.product).cloned())
                .collect();
            let (ndcg, recall, mrr) = ndcg_recall_mrr(&cn_ids, judged, K);
            cn_correctness.record(ndcg, recall, mrr, cn_ids.len(), cn_candidates, None);

            // tantivy-standalone: whole raw query text, no structural
            // involvement at all -- "what if there were no commerce-
            // native layer, just an embedded lexical engine."
            let (ts_hits, ts_count) =
                delegate.search_with_count(std::slice::from_ref(text), None, K);
            let ts_ids: Vec<String> = ts_hits
                .iter()
                .filter_map(|h| product_id_to_asin.get(&h.product).cloned())
                .collect();
            let (ndcg, recall, mrr) = ndcg_recall_mrr(&ts_ids, judged, K);
            ts_correctness.record(ndcg, recall, mrr, ts_ids.len(), ts_count, None);

            // solr
            if solr_available {
                let (brand, color) = extract_brand_color(compiled, &brand_name_by_id);
                let (q, fq) = solr_query_for(
                    text,
                    &compiled.residual_lexical,
                    brand.as_deref(),
                    color.as_deref(),
                );
                if let Some(sr) = solr_search(&solr_base_url, &q, &fq, K) {
                    let (ndcg, recall, mrr) = ndcg_recall_mrr(&sr.ids, judged, K);
                    solr_correctness.record(
                        ndcg,
                        recall,
                        mrr,
                        sr.ids.len(),
                        sr.num_found,
                        Some(sr.qtime_ms),
                    );
                }
            }
        }

        println!("  outcome distribution (commerce-native): {outcome_counts:?}");
        cn_correctness.print(Method::CommerceNative.label());
        ts_correctness.print(Method::TantivyStandalone.label());
        if solr_available {
            solr_correctness.print(Method::Solr.label());
        } else {
            println!(
                "    {:<20} SKIPPED (Solr unreachable)",
                Method::Solr.label()
            );
        }

        if cn_correctness.n > 0 {
            *weighted_ndcg.entry(Method::CommerceNative).or_insert(0.0) += cn_correctness.ndcg_sum;
            *weighted_n.entry(Method::CommerceNative).or_insert(0) += cn_correctness.n;
        }
        if ts_correctness.n > 0 {
            *weighted_ndcg
                .entry(Method::TantivyStandalone)
                .or_insert(0.0) += ts_correctness.ndcg_sum;
            *weighted_n.entry(Method::TantivyStandalone).or_insert(0) += ts_correctness.n;
        }
        if solr_correctness.n > 0 {
            *weighted_ndcg.entry(Method::Solr).or_insert(0.0) += solr_correctness.ndcg_sum;
            *weighted_n.entry(Method::Solr).or_insert(0) += solr_correctness.n;
        }

        // --- repeated-measurement latency sub-experiment ---
        let latency_sample: Vec<u64> = sample
            .iter()
            .take(LATENCY_SAMPLE_PER_CLASS)
            .copied()
            .collect();
        if latency_sample.is_empty() {
            continue;
        }
        let methods: Vec<Method> = if solr_available {
            vec![
                Method::CommerceNative,
                Method::TantivyStandalone,
                Method::Solr,
            ]
        } else {
            vec![Method::CommerceNative, Method::TantivyStandalone]
        };
        let schedule = round_robin_schedule(methods.len(), LATENCY_REPS_PER_METHOD, SEED);
        // Issue #6 P1-D / P2-E16 (`docs/experiments/PHASE2_LOG.md`): build
        // the same `(q, fq)` the correctness loop measures for every query
        // in the latency sample *before* timing anything, exactly mirroring
        // how `compiled_cache` keeps commerce-native's query-compilation
        // cost out of its own timed block -- this stays a fair, symmetric
        // exclusion (query "understanding" cost excluded on both sides)
        // rather than a bug (as it was before P2-E16: the untimed
        // `solr_query_for` construction plus everything downstream was
        // skipped entirely, and a different, unfiltered raw-text query hit
        // Solr's unpopulated `_text_` default field instead).
        let solr_query_cache: BTreeMap<u64, (String, Vec<String>)> = latency_sample
            .iter()
            .map(|&qid| {
                let compiled = &compiled_cache[&qid];
                let (text, _) = &judged_by_query[&qid];
                let (brand, color) = extract_brand_color(compiled, &brand_name_by_id);
                let (q, fq) = solr_query_for(
                    text,
                    &compiled.residual_lexical,
                    brand.as_deref(),
                    color.as_deref(),
                );
                (qid, (q, fq))
            })
            .collect();
        // warmup: run each method a few times before measuring, per
        // bench-harness's warmup_then discipline.
        for _ in 0..LATENCY_WARMUP {
            for &qid in &latency_sample {
                let compiled = &compiled_cache[&qid];
                let (text, _) = &judged_by_query[&qid];
                let _ = execute_planned(
                    compiled,
                    &ingested.catalog,
                    &index,
                    Some(&delegate),
                    K,
                    &planner_policy,
                );
                let _ = delegate.search_with_count(std::slice::from_ref(text), None, K);
                if solr_available {
                    let (q, fq) = &solr_query_cache[&qid];
                    let _ = solr_search(&solr_base_url, q, fq, K);
                }
            }
        }
        let mut samples: BTreeMap<Method, Vec<f64>> = BTreeMap::new();
        for (i, &method_idx) in schedule.iter().enumerate() {
            let method = methods[method_idx];
            let qid = latency_sample[i % latency_sample.len()];
            let compiled = &compiled_cache[&qid];
            let (text, _) = &judged_by_query[&qid];
            let start = Instant::now();
            match method {
                Method::CommerceNative => {
                    let _ = execute_planned(
                        compiled,
                        &ingested.catalog,
                        &index,
                        Some(&delegate),
                        K,
                        &planner_policy,
                    );
                }
                Method::TantivyStandalone => {
                    let _ = delegate.search_with_count(std::slice::from_ref(text), None, K);
                }
                Method::Solr => {
                    let (q, fq) = &solr_query_cache[&qid];
                    let _ = solr_search(&solr_base_url, q, fq, K);
                }
            }
            let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
            samples.entry(method).or_default().push(elapsed_ms);
        }

        println!(
            "  --- latency, {} reps/method, n_queries={} ---",
            LATENCY_REPS_PER_METHOD,
            latency_sample.len()
        );
        for &method in &methods {
            if let Some(s) = samples.get(&method) {
                let d = Distribution::compute(s);
                d.print(method.label(), "ms");
                all_latency_samples
                    .entry(method)
                    .or_default()
                    .extend(s.iter().copied());
                bench_harness::append_raw_samples(
                    &artifacts_dir.join("raw_latency_samples.csv"),
                    &format!("p1d_{}", class.label()),
                    method.label(),
                    s,
                )
                .ok();
                bench_harness::append_summary_row(
                    &artifacts_dir.join("summary_latency.csv"),
                    &format!("p1d_{}", class.label()),
                    method.label(),
                    "latency_ms",
                    &d,
                )
                .ok();
            }
        }
        if let (Some(cn), Some(ts)) = (
            samples.get(&Method::CommerceNative),
            samples.get(&Method::TantivyStandalone),
        ) {
            let ci = bench_harness::bootstrap_ci_diff_of_means(ts, cn, 5000, 0.05, SEED);
            println!(
                "  bootstrap CI (commerce_native - tantivy_standalone), ms: diff={:.4} ci=[{:.4}, {:.4}] excludes_zero={}",
                ci.diff, ci.ci_low, ci.ci_high, ci.excludes_zero()
            );
        }
        if solr_available {
            if let (Some(cn), Some(sl)) = (
                samples.get(&Method::CommerceNative),
                samples.get(&Method::Solr),
            ) {
                let ci = bench_harness::bootstrap_ci_diff_of_means(sl, cn, 5000, 0.05, SEED);
                println!(
                    "  bootstrap CI (commerce_native - solr), ms: diff={:.4} ci=[{:.4}, {:.4}] excludes_zero={}",
                    ci.diff, ci.ci_low, ci.ci_high, ci.excludes_zero()
                );
            }
        }
    }

    println!("\n=== traffic-weighted aggregate (across all classified+sampled real queries) ===");
    for (&method, &n) in &weighted_n {
        let ndcg = weighted_ndcg[&method] / n as f64;
        println!(
            "  {:<20} n={n} weighted NDCG@10={:.4}",
            method.label(),
            ndcg
        );
    }
    for (&method, samples) in &all_latency_samples {
        let d = Distribution::compute(samples);
        d.print(&format!("{} (all classes pooled)", method.label()), "ms");
    }

    println!("\nartifacts written to {}", artifacts_dir.display());
    Ok(())
}
