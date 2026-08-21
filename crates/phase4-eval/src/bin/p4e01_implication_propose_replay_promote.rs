//! Issue #16 P4-E01: the offline propose -> replay -> promote pipeline
//! for learned Brand-implication rules, measured directly against Issue
//! #14's admission frontier (`docs/experiments/PHASE4_LOG.md`).
//!
//! **Propose**: scan every query Phase 3's three existing admission
//! mechanisms (`admit`, `admit_structurally_anchored_lexical`,
//! `admit_single_token_lexical`) currently reject, at a fixed, generous
//! cap held constant across baseline and treatment (so any admission
//! change is attributable only to the implication table, never to a cap
//! difference). Extract every 2-3-word window from each rejected query's
//! raw text, and for each *unique* phrase, compute
//! `cold_start::prefill::predict_brand_from_phrase` against a real
//! Tantivy title index over this same real catalog -- the exact,
//! already-validated (P1-C) zero-model-call signal this phase's own
//! prior-art survey identified as reusable. A phrase becomes a candidate
//! `ImplicationRule` (status `Candidate`) if its catalog purity/occurrence
//! clears `CANDIDATE_MIN_PURITY`/`CANDIDATE_MIN_OCCURRENCE` (a small
//! sensitivity sweep found purity>=0.8/occurrence>=10 recovers 5x the
//! real, zero-false-positive coverage of `prefill_eval.rs`'s own tighter
//! real-run thresholds, purity>=0.9/occurrence>=20, at a comparably tiny
//! isolated-degradation cost -- see `docs/experiments/PHASE4_LOG.md`
//! P4-E01 for both points), the phrase is not simply the predicted
//! brand's own name (P1-C's own "not a genuine inference" rule, reused
//! unchanged), and the predicted brand is not this catalog's own
//! missing-brand-field sentinel (`BrandId(0)`, `round1_eval::catalog`'s
//! `brand.unwrap_or(BrandId(0))`) -- a real adversarial finding this
//! binary's own first run surfaced (7/24 initially-promoted rules were
//! spurious "generic book/media phrase implies no-brand-data" matches
//! before this exclusion was added).
//!
//! **Replay**: for each candidate rule independently, apply it alone (a
//! solo `ImplicationTable`) to every rejected query whose raw text
//! contains its trigger, and check whether the enriched query is now
//! admitted at the *same* fixed caps. For every newly-admitted query,
//! execute it natively and score NDCG@10/Recall@10/MRR against the real
//! ESCI judgments, comparing to that query's own already-persisted real
//! Solr score (P3-E06's `whole_corpus_solr_ndcg.csv` -- reused, not
//! requeried).
//!
//! **Promote**: a rule promotes only if it recovers at least one query
//! and its own false-positive rate (native NDCG==0 while Solr found >=1
//! relevant result, the same definition P3-E05/E09 use) does not exceed
//! Phase 3's own worst KEPT mechanism's ceiling (15.35%, P3-E05 at
//! unlimited cap) -- the falsification threshold this phase's own
//! hypothesis stated before this binary was written.
//!
//! **Combined measurement**: apply every promoted rule together (so
//! `apply_implications`'s own cross-trigger abstention logic is
//! exercised, not just each rule in isolation) to the same rejected
//! population, and report whole-corpus coverage/relevance exactly like
//! every other Phase 3 admission-frontier experiment.
//!
//! Usage: cargo run --release -p phase4-eval --bin p4e01_implication_propose_replay_promote
//!        [catalog.jsonl] [queries.jsonl] [p3e06_whole_corpus_solr_csv] [tantivy_title_index_dir]

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
use std::time::Instant;

use commerce_core::admission::{
    admit, admit_single_token_lexical, admit_structurally_anchored_lexical, execute_admitted,
    execute_lexically_narrowed, AdmissionPolicy,
};
use commerce_core::cold_start::{compile_lexicon, predict_brand_from_phrase, CatalogProfile};
use commerce_core::control_plane::{apply_implications, ImplicationRule, ImplicationTable};
use commerce_core::domain::{BrandId, Catalog, ProductId};
use commerce_core::index::{CatalogIndex, RankedHit};
use commerce_core::ir::{compile, CommerceQuery, ResolvedConstraint, StructuralConstraint};
use round1_eval::catalog as catalog_ingest;
use round1_eval::data::{self, EsciLabel};
use round1_eval::relevance::ndcg_recall_mrr;

use tantivy::collector::TopDocs;
use tantivy::query::QueryParser;
use tantivy::schema::{Schema, Value, STORED, STRING, TEXT};
use tantivy::{doc, Index, IndexWriter, ReloadPolicy, TantivyDocument};

const K: usize = 10;
/// Fixed caps for the three baseline admission mechanisms, held constant
/// across baseline and treatment so any admission-decision change is
/// attributable only to the implication table under test.
///
/// **Not** the widest caps each mechanism's own sweep ever tested: an
/// earlier version of this experiment used (250, 250, 200_000) as a
/// "generous" baseline and found an apparently large 4.96% relative
/// whole-workload degradation -- almost entirely traceable to the
/// anchored-lexical mechanism's *own* isolated cost at cap=250 (2.91%
/// relative per P3-E05's own table), not to implications at all, since
/// that cap point already exceeds Issue #14's 2% budget on its own. These
/// values are instead P3-E16's own promoted `<=2.0%`-budget three-way
/// operating point (5.80% coverage / 1.98% relative degradation,
/// already-validated and in-budget) -- so this experiment measures
/// implications' *additional* marginal contribution on top of an
/// already-safe baseline, not conflated with an out-of-budget one.
const STRUCTURAL_CAP: usize = 2;
const ANCHORED_CAP: usize = 20;
const SINGLE_TOKEN_CAP: usize = 10;

/// Candidate-generation thresholds: `phase2_eval::prefill_eval`'s own
/// real-run `PrefillPolicy` high-confidence tier, reused unchanged for
/// continuity/comparability rather than re-derived here.
const CANDIDATE_MIN_PURITY: f64 = 0.8;
const CANDIDATE_MIN_OCCURRENCE: usize = 10;
const NGRAM_SIZES: [usize; 2] = [2, 3];
const SAMPLE_LIMIT: usize = 50;
const MAX_WINDOW_WORDS: usize = 3;

/// This phase's own stated falsification threshold
/// (`docs/experiments/PHASE4_LOG.md`): a promoted rule's own
/// false-positive rate must not categorically exceed Phase 3's own worst
/// KEPT mechanism (P3-E05's 15.35% at unlimited cap).
const MAX_FALSE_POSITIVE_RATE: f64 = 0.1535;

enum AdmittedVia {
    Structural,
    Narrowed(roaring::RoaringBitmap),
}

fn try_admit(query: &CommerceQuery, index: &CatalogIndex) -> Option<AdmittedVia> {
    let policy = AdmissionPolicy {
        max_candidates: STRUCTURAL_CAP,
    };
    if admit(query, index, &policy).is_admit() {
        return Some(AdmittedVia::Structural);
    }
    if let Some((bitmap, _)) = admit_structurally_anchored_lexical(query, index, ANCHORED_CAP) {
        return Some(AdmittedVia::Narrowed(bitmap));
    }
    if let Some((bitmap, _)) = admit_single_token_lexical(query, index, SINGLE_TOKEN_CAP) {
        return Some(AdmittedVia::Narrowed(bitmap));
    }
    None
}

fn execute_via(
    via: &AdmittedVia,
    index: &CatalogIndex,
    query: &CommerceQuery,
    catalog: &Catalog,
    k: usize,
) -> Vec<RankedHit> {
    match via {
        AdmittedVia::Structural => execute_admitted(index, query, catalog, k),
        AdmittedVia::Narrowed(bitmap) => {
            execute_lexically_narrowed(index, query, bitmap, catalog, k)
        }
    }
}

fn windows(raw_text: &str) -> Vec<String> {
    let tokens: Vec<String> = raw_text.split_whitespace().map(str::to_lowercase).collect();
    let mut out = Vec::new();
    for &n in &NGRAM_SIZES {
        if tokens.len() < n {
            continue;
        }
        for w in tokens.windows(n) {
            out.push(w.join(" "));
        }
    }
    out
}

/// `TitlePhraseIndex` backed by a Tantivy phrase query against a
/// title-only field, with a same-process cache -- identical shape to
/// `phase2_eval::prefill_eval`'s own `TantivyTitlePhraseIndex`.
struct TantivyTitlePhraseIndex<'a> {
    searcher: tantivy::Searcher,
    title_query_parser: QueryParser,
    id_field: tantivy::schema::Field,
    asin_to_product_id: &'a HashMap<String, ProductId>,
    cache: RefCell<HashMap<String, Vec<ProductId>>>,
}

impl commerce_core::cold_start::TitlePhraseIndex for TantivyTitlePhraseIndex<'_> {
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

fn build_title_tantivy_index(
    products: &[data::RealProduct],
    index_dir: &PathBuf,
) -> tantivy::Result<Index> {
    if index_dir.exists() {
        std::fs::remove_dir_all(index_dir).expect("clear stale index dir");
    }
    std::fs::create_dir_all(index_dir).expect("create index dir");

    let mut schema_builder = Schema::builder();
    let id_field = schema_builder.add_text_field("id", STRING | STORED);
    let title_field = schema_builder.add_text_field("title", TEXT);
    let schema = schema_builder.build();

    let index = Index::create_in_dir(index_dir, schema)?;
    let mut writer: IndexWriter = index.writer(512_000_000)?;
    for product in products {
        writer.add_document(doc!(
            id_field => product.id.clone(),
            title_field => product.title.clone(),
        ))?;
    }
    writer.commit()?;
    Ok(index)
}

struct SolrRow {
    ndcg: f64,
    recall: f64,
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
    let solr_csv_path = args.next().map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from("docs/research/artifacts/p3e06_run1/whole_corpus_solr_ndcg.csv")
    });
    let index_dir = args
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("dataset_cache/tantivy_p4e01_title_index"));

    println!("loading + ingesting real catalog...");
    let products = data::load_catalog(&catalog_path);
    let ingested = catalog_ingest::build_catalog(&products);
    println!("{} real products ingested", ingested.catalog.products.len());

    println!("building commerce_core structural index...");
    let index = CatalogIndex::build(&ingested.catalog);

    println!("building real title-only Tantivy index for candidate proposal...");
    let t0 = Instant::now();
    let tantivy_index = build_title_tantivy_index(&products, &index_dir)?;
    println!("  built in {:.1}s", t0.elapsed().as_secs_f64());
    let schema = tantivy_index.schema();
    let id_field = schema.get_field("id").unwrap();
    let title_field = schema.get_field("title").unwrap();
    let reader = tantivy_index
        .reader_builder()
        .reload_policy(ReloadPolicy::OnCommitWithDelay)
        .try_into()?;
    let phrase_index = TantivyTitlePhraseIndex {
        searcher: reader.searcher(),
        title_query_parser: QueryParser::for_index(&tantivy_index, vec![title_field]),
        id_field,
        asin_to_product_id: &ingested.asin_to_product_id,
        cache: RefCell::new(HashMap::new()),
    };

    let brand_name_by_id: HashMap<BrandId, String> = ingested
        .brands
        .iter()
        .map(|b| (b.id, b.name.clone()))
        .collect();
    let product_id_to_asin: HashMap<ProductId, String> = ingested
        .asin_to_product_id
        .iter()
        .map(|(asin, pid)| (*pid, asin.clone()))
        .collect();
    let profile = CatalogProfile::build(&ingested.catalog, &ingested.brands, &[], &[]);
    let lexicon = compile_lexicon(&profile, 25);

    println!("loading persisted whole-corpus Solr baseline from {solr_csv_path:?}...");
    let mut solr: HashMap<u64, SolrRow> = HashMap::new();
    for line in std::fs::read_to_string(&solr_csv_path)
        .unwrap_or_else(|e| panic!("failed to read {solr_csv_path:?}: {e}"))
        .lines()
        .skip(1)
    {
        if line.is_empty() {
            continue;
        }
        let mut cols = line.split(',');
        let qid: u64 = cols.next().unwrap().parse().unwrap();
        let ndcg: f64 = cols.next().unwrap().parse().unwrap();
        let recall: f64 = cols.next().unwrap().parse().unwrap();
        solr.insert(qid, SolrRow { ndcg, recall });
    }
    let total = solr.len();
    let solr_only_mean = solr.values().map(|r| r.ndcg).sum::<f64>() / total as f64;
    println!("  {total} queries loaded; whole-workload pure-Solr-only baseline NDCG@10: {solr_only_mean:.4}");

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

    println!("\ncompiling every query and computing baseline admission (fixed caps: structural<={STRUCTURAL_CAP}, anchored<={ANCHORED_CAP}, single_token<={SINGLE_TOKEN_CAP})...");
    let mut baseline_admitted_native_ndcg: HashMap<u64, f64> = HashMap::new();
    // qid -> (raw_text, compiled query) for every baseline-rejected query.
    let mut rejected: BTreeMap<u64, (String, CommerceQuery)> = BTreeMap::new();
    for (&qid, (raw_text, judged)) in &judged_by_query {
        if !judged.values().any(|l| l.is_relevant()) {
            continue;
        }
        if !solr.contains_key(&qid) {
            continue;
        }
        let compiled = compile(raw_text, &lexicon);
        match try_admit(&compiled, &index) {
            Some(via) => {
                let hits = execute_via(&via, &index, &compiled, &ingested.catalog, K);
                let ids: Vec<String> = hits
                    .iter()
                    .filter_map(|h| product_id_to_asin.get(&h.product).cloned())
                    .collect();
                let (ndcg, _, _) = ndcg_recall_mrr(&ids, judged, K);
                baseline_admitted_native_ndcg.insert(qid, ndcg);
            }
            None => {
                rejected.insert(qid, (raw_text.clone(), compiled));
            }
        }
    }
    println!(
        "  {} baseline-admitted, {} baseline-rejected (of {} judged-with-relevant, Solr-covered queries)",
        baseline_admitted_native_ndcg.len(),
        rejected.len(),
        baseline_admitted_native_ndcg.len() + rejected.len()
    );

    println!("\nPROPOSE: scanning baseline-rejected queries' raw text for 2-3-word windows, deduplicating by phrase...");
    let mut phrase_to_qids: HashMap<String, Vec<u64>> = HashMap::new();
    for (&qid, (raw_text, _)) in &rejected {
        for phrase in windows(raw_text) {
            phrase_to_qids.entry(phrase).or_default().push(qid);
        }
    }
    let mut unique_phrases: Vec<String> = phrase_to_qids.keys().cloned().collect();
    unique_phrases.sort();
    println!("  {} unique 2-3-word phrases found", unique_phrases.len());

    let mut candidates: Vec<ImplicationRule> = Vec::new();
    for phrase in &unique_phrases {
        let Some(prediction) = predict_brand_from_phrase(
            phrase,
            &ingested.catalog,
            &index,
            &phrase_index,
            SAMPLE_LIMIT,
        ) else {
            continue;
        };
        if prediction.purity < CANDIDATE_MIN_PURITY
            || prediction.occurrence < CANDIDATE_MIN_OCCURRENCE
        {
            continue;
        }
        // Real adversarial finding, caught by inspecting the first run's
        // own promoted-rule report before trusting it: `round1_eval::catalog`
        // maps any real product with no brand field at all to the sentinel
        // `BrandId(0)` (its own `build_catalog`, `brand.unwrap_or(BrandId(0))`).
        // Generic book/media phrases ("james patterson", "thriller series",
        // "romantic comedy", "kindle unlimited") are overwhelmingly common in
        // exactly this unbranded slice of the real catalog, so they scored a
        // spuriously high "purity" toward BrandId(0) -- 7 of this run's first
        // 24 promoted rules (29%) were this exact false pattern before this
        // exclusion was added. Asserting `Brand=BrandId(0)` is not a genuine
        // trigger-implies-brand fact at all: it means "this phrase correlates
        // with missing brand data," not "this phrase implies brand X." This
        // is the same real-catalog data-quality hazard P2-E15/P3-E02 already
        // found for diaper products' missing `size` attribute, recurring here
        // for a different field.
        if prediction.brand == BrandId(0) {
            continue;
        }
        if brand_name_by_id
            .get(&prediction.brand)
            .is_some_and(|name| name.eq_ignore_ascii_case(phrase))
        {
            continue; // not a genuine inference -- the phrase already IS the brand name
        }
        candidates.push(ImplicationRule::candidate(
            phrase,
            vec![ResolvedConstraint::Structural(StructuralConstraint::Brand(
                prediction.brand,
            ))],
            commerce_core::control_plane::RuleProvenance::Catalog,
            prediction.purity,
        ));
    }
    println!(
        "  {} candidate rules generated (purity>={CANDIDATE_MIN_PURITY}, occurrence>={CANDIDATE_MIN_OCCURRENCE})",
        candidates.len()
    );

    println!("\nREPLAY: validating each candidate rule independently against the queries it would match...");
    let mut promoted: Vec<ImplicationRule> = Vec::new();
    let mut rule_report = String::from(
        "trigger,brand_id,catalog_purity,matched_queries,newly_admitted,native_ndcg_mean,solr_ndcg_mean,false_positives,false_positive_rate,decision\n",
    );
    for candidate in &candidates {
        let solo_table = ImplicationTable::compile(1, [candidate.clone().promote()]);
        let matched_qids = phrase_to_qids
            .get(&candidate.trigger)
            .cloned()
            .unwrap_or_default();

        let mut newly_admitted = 0usize;
        let mut native_ndcg_sum = 0.0;
        let mut solr_ndcg_sum = 0.0;
        let mut false_positives = 0usize;

        for qid in &matched_qids {
            let (raw_text, compiled) = &rejected[qid];
            let mut enriched = compiled.clone();
            let applied =
                apply_implications(&mut enriched, raw_text, &solo_table, MAX_WINDOW_WORDS);
            if applied.is_empty() {
                continue; // e.g. an explicit brand constraint already present
            }
            let Some(via) = try_admit(&enriched, &index) else {
                continue;
            };
            let hits = execute_via(&via, &index, &enriched, &ingested.catalog, K);
            let ids: Vec<String> = hits
                .iter()
                .filter_map(|h| product_id_to_asin.get(&h.product).cloned())
                .collect();
            let (_, judged) = &judged_by_query[qid];
            let (native_ndcg, _, _) = ndcg_recall_mrr(&ids, judged, K);
            let solr_row = &solr[qid];

            newly_admitted += 1;
            native_ndcg_sum += native_ndcg;
            solr_ndcg_sum += solr_row.ndcg;
            if native_ndcg == 0.0 && solr_row.recall > 0.0 {
                false_positives += 1;
            }
        }

        let decision;
        if newly_admitted == 0 {
            decision = "REJECT (no coverage)";
        } else {
            let fp_rate = false_positives as f64 / newly_admitted as f64;
            if fp_rate > MAX_FALSE_POSITIVE_RATE {
                decision = "REJECT (false-positive rate too high)";
            } else {
                decision = "PROMOTE";
                promoted.push(candidate.clone().promote());
            }
        }
        rule_report.push_str(&format!(
            "{},{},{},{},{},{},{},{},{},{}\n",
            candidate.trigger,
            match &candidate.implies[0] {
                ResolvedConstraint::Structural(StructuralConstraint::Brand(id)) => id.0,
                _ => unreachable!(),
            },
            candidate.confidence,
            matched_qids.len(),
            newly_admitted,
            if newly_admitted > 0 {
                native_ndcg_sum / newly_admitted as f64
            } else {
                0.0
            },
            if newly_admitted > 0 {
                solr_ndcg_sum / newly_admitted as f64
            } else {
                0.0
            },
            false_positives,
            if newly_admitted > 0 {
                false_positives as f64 / newly_admitted as f64
            } else {
                0.0
            },
            decision,
        ));
    }
    println!(
        "  {}/{} candidate rules PROMOTED",
        promoted.len(),
        candidates.len()
    );

    println!("\nCOMBINED measurement: applying every promoted rule together (cross-trigger abstention exercised)...");
    let final_table = ImplicationTable::compile(1, promoted.clone());
    let mut implication_admitted_count = 0usize;
    let mut implication_native_ndcg_sum = 0.0;
    let mut implication_solr_ndcg_sum = 0.0;
    let mut implication_false_positives = 0usize;
    let mut implication_admitted_qids: HashSet<u64> = HashSet::new();
    let mut per_query_report = String::from("qid,applied_triggers,native_ndcg,solr_ndcg\n");

    for (&qid, (raw_text, compiled)) in &rejected {
        let mut enriched = compiled.clone();
        let applied = apply_implications(&mut enriched, raw_text, &final_table, MAX_WINDOW_WORDS);
        if applied.is_empty() {
            continue;
        }
        let Some(via) = try_admit(&enriched, &index) else {
            continue;
        };
        let hits = execute_via(&via, &index, &enriched, &ingested.catalog, K);
        let ids: Vec<String> = hits
            .iter()
            .filter_map(|h| product_id_to_asin.get(&h.product).cloned())
            .collect();
        let (_, judged) = &judged_by_query[&qid];
        let (native_ndcg, _, _) = ndcg_recall_mrr(&ids, judged, K);
        let solr_row = &solr[&qid];

        implication_admitted_count += 1;
        implication_admitted_qids.insert(qid);
        implication_native_ndcg_sum += native_ndcg;
        implication_solr_ndcg_sum += solr_row.ndcg;
        if native_ndcg == 0.0 && solr_row.recall > 0.0 {
            implication_false_positives += 1;
        }
        per_query_report.push_str(&format!(
            "{qid},{},{native_ndcg},{}\n",
            applied.join("|"),
            solr_row.ndcg
        ));
    }

    let coverage_pct = implication_admitted_count as f64 / total as f64 * 100.0;
    // Isolated-marginal-contribution whole-workload degradation, matching
    // every prior Phase 3 experiment's own methodology: implications only
    // ever touch baseline-rejected queries (disjoint from
    // baseline-admitted by construction, since propose/replay/promote
    // scanned only that set), so the combined whole-workload NDCG is
    // baseline-admitted's own native NDCG, plus implication-admitted's own
    // native NDCG, plus every remaining query's own persisted Solr NDCG.
    let baseline_admitted_sum: f64 = baseline_admitted_native_ndcg.values().sum();
    let rest_solr_sum: f64 = rejected
        .keys()
        .filter(|qid| !implication_admitted_qids.contains(qid))
        .map(|qid| solr[qid].ndcg)
        .sum();
    let whole_workload_ndcg =
        (baseline_admitted_sum + implication_native_ndcg_sum + rest_solr_sum) / total as f64;
    let whole_workload_degradation = solr_only_mean - whole_workload_ndcg;
    let relative_pct = whole_workload_degradation / solr_only_mean * 100.0;

    println!("\n=== P4-E01 combined result ===");
    println!(
        "  implications newly admitted: {implication_admitted_count}/{total} ({coverage_pct:.2}% of whole corpus)"
    );
    println!(
        "  native NDCG (implication-admitted, mean): {:.4}",
        if implication_admitted_count > 0 {
            implication_native_ndcg_sum / implication_admitted_count as f64
        } else {
            0.0
        }
    );
    println!(
        "  Solr NDCG (same admitted subset, mean):   {:.4}",
        if implication_admitted_count > 0 {
            implication_solr_ndcg_sum / implication_admitted_count as f64
        } else {
            0.0
        }
    );
    println!(
        "  false positives: {implication_false_positives}/{implication_admitted_count} ({:.2}%)",
        if implication_admitted_count > 0 {
            implication_false_positives as f64 / implication_admitted_count as f64 * 100.0
        } else {
            0.0
        }
    );
    println!("  whole-workload NDCG@10: {whole_workload_ndcg:.4} (pure-Solr-only baseline: {solr_only_mean:.4})");
    println!(
        "  whole-workload degradation: {whole_workload_degradation:.4} ({relative_pct:.2}% relative)"
    );
    for budget in [0.0, 0.5, 1.0, 2.0] {
        println!("  clears <={budget}% budget: {}", relative_pct <= budget);
    }

    let artifacts_dir = PathBuf::from("dataset_cache/p4e01_artifacts");
    std::fs::create_dir_all(&artifacts_dir).ok();
    std::fs::write(artifacts_dir.join("rule_report.csv"), &rule_report).ok();
    std::fs::write(
        artifacts_dir.join("per_query_report.csv"),
        &per_query_report,
    )
    .ok();
    println!("\nartifacts written to {}", artifacts_dir.display());

    Ok(())
}
