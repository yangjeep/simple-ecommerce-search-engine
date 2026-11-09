//! No-service paired public-data evaluation, not the full Issue #57 matrix.
//! Frozen before results: k=10, min enum frequency=1, policy=0.05/20,
//! residual_policy=None. No qrels enter compilation, retrieval, or tuning.
//! Run from repository root: public_ranked <wands|esci_electronics> <output-dir>.
//! The ESCI input is the existing electronics train slice, NOT full ESCI/test.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::Path;

use commerce_core::cold_start::{compile_lexicon, CatalogProfile};
use commerce_core::domain::{Catalog, ProductId};
use commerce_core::index::CatalogIndex;
use commerce_core::ir::compile;
use commerce_core::plan::{execute_planned, LexicalDelegate, LexicalHit, PlannerPolicy};
use issue57_eval::ndcg_recall_mrr;
use phase9_eval::bitmap_delegate::{build_index, BitmapTantivyDelegate};
use phase9_eval::wands_relevance::WandsLabel;
use serde_json::{json, Value};
use issue61_eval::sha256_hex;

const K: usize = 10;
type Result<T> = std::result::Result<T, Box<dyn Error>>;

struct Query {
    id: String,
    text: String,
    // Original dataset IDs, including judgments outside the catalog.
    gains: BTreeMap<String, f64>,
}

struct Dataset {
    catalog: Catalog,
    profile: CatalogProfile,
    external_ids: BTreeMap<ProductId, String>,
    queries: Vec<Query>,
    paths: Vec<String>,
    scope: &'static str,
    gain_scale: Value,
}

fn insert_gain(gains: &mut BTreeMap<String, f64>, id: &str, gain: f64) -> Result<()> {
    if gains.insert(id.to_owned(), gain).is_some() {
        return Err(format!("duplicate judgment for product {id}").into());
    }
    Ok(())
}

fn tsv(path: &str) -> Result<Vec<BTreeMap<String, String>>> {
    let output = std::process::Command::new("python3")
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/public_benchmarks/strict_tsv.py"))
        .arg(path)
        .output()?;
    if !output.status.success() {
        return Err(format!("TSV conversion failed: {}", String::from_utf8_lossy(&output.stderr)).into());
    }
    Ok(serde_json::from_slice(&output.stdout)?)
}

fn field<'a>(row: &'a BTreeMap<String, String>, key: &str) -> Result<&'a str> {
    row.get(key).map(String::as_str).ok_or_else(|| format!("missing TSV field {key}").into())
}

fn load_wands() -> Result<Dataset> {
    let paths: Vec<String> = ["catalog.jsonl", "query.csv", "label.csv"].iter()
        .map(|name| format!("dataset_cache/wands/{name}")).collect();
    let products = phase6a_eval::data::load_catalog(Path::new(&paths[0]));
    let ingested = phase6a_eval::catalog::build_catalog(&products);
    if ingested.wands_id_to_product_id.len() != products.len() {
        return Err("duplicate WANDS product IDs".into());
    }
    let profile = CatalogProfile::build(&ingested.catalog, &[], &ingested.product_types, &ingested.categories);
    let mut judgments: BTreeMap<String, BTreeMap<String, f64>> = BTreeMap::new();
    for row in tsv(&paths[2])? {
        let qid = field(&row, "query_id")?;
        let pid = field(&row, "product_id")?;
        let label = field(&row, "label")?;
        let gain = match WandsLabel::parse(label) {
            Some(WandsLabel::Exact) => 2.0,
            Some(WandsLabel::Partial) => 1.0,
            Some(WandsLabel::Irrelevant) => 0.0,
            None => return Err(format!("unknown WANDS label {label:?}").into()),
        };
        insert_gain(judgments.entry(qid.to_owned()).or_default(), pid, gain)?;
    }
    let mut queries = Vec::new();
    let mut seen = BTreeSet::new();
    for row in tsv(&paths[1])? {
        // Unlike the old positional loader, retain query.csv's actual key.
        let id = field(&row, "query_id")?.to_owned();
        if !seen.insert(id.clone()) {
            return Err(format!("duplicate WANDS query ID {id}").into());
        }
        queries.push(Query {
            gains: judgments.remove(&id).unwrap_or_default(),
            id,
            text: field(&row, "query")?.to_owned(),
        });
    }
    if !judgments.is_empty() {
        return Err("WANDS qrels reference query IDs missing from query.csv".into());
    }
    Ok(Dataset {
        catalog: ingested.catalog,
        profile,
        external_ids: ingested.wands_id_to_product_id.into_iter().map(|(id, pid)| (pid, id)).collect(),
        queries,
        paths,
        scope: "WANDS supplied catalog and all supplied queries; no train/test claim",
        gain_scale: json!({"Exact": 2.0, "Partial": 1.0, "Irrelevant": 0.0}),
    })
}

fn load_esci() -> Result<Dataset> {
    let paths = vec![
        "dataset_cache/esci_electronics/esci_electronics_products.jsonl".to_owned(),
        "dataset_cache/esci_electronics/esci_electronics_queries.jsonl".to_owned(),
    ];
    let products = issue35_eval::load_products(&paths[0]);
    let raw_queries = issue35_eval::load_queries(&paths[1]);
    let ingested = issue35_eval::build_catalog(&products);
    if ingested.product_id_by_asin.len() != products.len() {
        return Err("duplicate ESCI product IDs".into());
    }
    let profile = CatalogProfile::build(&ingested.catalog, &ingested.brands, &[], &[]);
    let mut queries = Vec::new();
    for (i, q) in raw_queries.into_iter().enumerate() {
        let mut gains = BTreeMap::new();
        for j in q.judgments {
            insert_gain(&mut gains, &j.product_id, issue35_eval::label_gain(&j.label))?;
        }
        queries.push(Query { id: format!("export_row_{}", i + 1), text: q.query, gains });
    }
    Ok(Dataset {
        catalog: ingested.catalog,
        profile,
        external_ids: ingested.product_id_by_asin.into_iter().map(|(id, pid)| (pid, id)).collect(),
        queries,
        paths,
        scope: "Existing pinned ESCI electronics TRAIN slice; NOT full ESCI, NOT held-out test; export lacks original query IDs",
        gain_scale: json!({"Exact": 1.0, "Substitute": 0.1, "Complement": 0.01, "Irrelevant": 0.0}),
    })
}

struct AuditedDelegate {
    inner: BitmapTantivyDelegate,
    diagnostics: RefCell<Vec<String>>,
}

impl LexicalDelegate for AuditedDelegate {
    fn search(&self, terms: &[String], restrict_to: Option<&BTreeSet<ProductId>>, limit: usize) -> Vec<LexicalHit> {
        let (hits, diagnostics) = self.inner.search_with_diagnostics(terms, restrict_to, limit);
        self.diagnostics.borrow_mut().extend(diagnostics);
        hits
    }
}

fn source(path: &str) -> Result<Value> {
    let bytes = fs::read(path)?;
    Ok(json!({"path": path, "bytes": bytes.len(), "sha256": sha256_hex(&bytes)}))
}

fn ranked_ids(ids: impl Iterator<Item = ProductId>, external: &BTreeMap<ProductId, String>) -> Result<Vec<String>> {
    let mut seen = BTreeSet::new();
    let mut ranked = Vec::new();
    for pid in ids {
        if !seen.insert(pid) {
            return Err(format!("duplicate ranked ProductId {}", pid.0).into());
        }
        ranked.push(external.get(&pid).ok_or_else(|| format!("unmapped ProductId {}", pid.0))?.clone());
    }
    Ok(ranked)
}

fn metric_json(score: (f64, f64, f64)) -> Value {
    json!({"ndcg_at_10": score.0, "recall_at_10": score.1, "mrr_at_10": score.2})
}

fn write_json(path: &Path, value: &Value) -> Result<()> {
    fs::write(path, serde_json::to_vec_pretty(value)?)?;
    Ok(())
}

fn run(dataset: &str, output: &Path) -> Result<()> {
    let data = match dataset {
        "wands" => load_wands()?,
        "esci_electronics" => load_esci()?,
        _ => return Err("dataset must be wands or esci_electronics".into()),
    };
    if data.catalog.products.is_empty() || data.queries.is_empty() {
        return Err("empty catalog or query set".into());
    }
    let sources = data.paths.iter().map(|p| source(p)).collect::<Result<Vec<_>>>()?;
    write_json(&output.join("manifest.json"), &json!({
        "protocol": "public_ranked_v1", "dataset": dataset, "scope": data.scope,
        "k": K, "min_enum_frequency": 1, "selectivity_threshold": 0.05,
        "delegate_oversample": 20, "residual_policy": null,
        "treatments": ["lexical_full_query_no_filter", "compiled_native_pipeline"],
        "gain_scale": data.gain_scale, "dcg": "linear gain / log2(rank + 1)",
        "relevant": "gain > 0; unjudged hits score zero and judged coverage is reported",
        "sources": sources, "catalog_products": data.catalog.products.len(), "queries": data.queries.len(),
        "code_revision": std::env::var("GITHUB_SHA").ok(),
        "limits": ["No external search services or model calls", "Not a whole-system or latency claim",
            "No qrel-based tuning; shared ingested catalog and shared Tantivy text index",
            "Missing-catalog judgments stay in the metric denominator",
            "Parser/search diagnostics invalidate BOTH paired scores; rows and raw rankings retained",
            "No-positive-judgment queries retained with null metrics, not pooled into score means",
            "No significance or held-out generalization claim"]
    }))?;
    let index = CatalogIndex::build(&data.catalog);
    let lexicon = compile_lexicon(&data.profile, 1);
    let built = build_index(&data.catalog)?;
    let delegate = AuditedDelegate {
        inner: BitmapTantivyDelegate::new(&built.index, vec![built.title_field, built.description_field])?,
        diagnostics: RefCell::new(Vec::new()),
    };
    let policy = PlannerPolicy { selectivity_threshold: 0.05, delegate_oversample: 20 };
    let known_ids: BTreeSet<&str> = data.external_ids.values().map(String::as_str).collect();
    let mut rows = BufWriter::new(File::create(output.join("queries.jsonl"))?);
    let mut sums = [[0.0; 3]; 2];
    let mut paired = 0usize;
    let mut diagnostic_queries = 0usize;
    let mut no_positive = 0usize;
    let mut changed = 0usize;
    for q in &data.queries {
        let baseline = delegate.search(std::slice::from_ref(&q.text), None, K);
        let baseline_errors = delegate.diagnostics.take();
        let compiled = compile(&q.text, &lexicon);
        let (plan, native) = execute_planned(&compiled, &data.catalog, &index, Some(&delegate), K, &policy, None);
        let native_errors = delegate.diagnostics.take();
        let baseline = ranked_ids(baseline.iter().map(|h| h.product), &data.external_ids)?;
        let native = ranked_ids(native.iter().map(|h| h.product), &data.external_ids)?;
        let has_positive = q.gains.values().any(|&g| g > 0.0);
        let has_errors = !baseline_errors.is_empty() || !native_errors.is_empty();
        diagnostic_queries += usize::from(has_errors);
        no_positive += usize::from(!has_positive);
        changed += usize::from(baseline != native);
        let status = if has_errors { "paired_diagnostic_failure" } else if !has_positive { "no_positive_qrels" } else { "scored" };
        let scores = if has_positive && !has_errors {
            paired += 1;
            let b = ndcg_recall_mrr(&baseline, &q.gains, K);
            let n = ndcg_recall_mrr(&native, &q.gains, K);
            for (total, score) in sums.iter_mut().zip([b, n]) {
                total[0] += score.0; total[1] += score.1; total[2] += score.2;
            }
            json!({"lexical": metric_json(b), "native": metric_json(n),
                "native_minus_lexical": metric_json((n.0-b.0, n.1-b.1, n.2-b.2))})
        } else { Value::Null };
        let judged_hits = |ranking: &[String]| ranking.iter().filter(|id| q.gains.contains_key(*id)).count();
        let row = json!({
            "query_id": q.id, "query": q.text, "status": status, "scores": scores,
            "qrels": q.gains, "qrels_outside_catalog": q.gains.keys().filter(|id| !known_ids.contains(id.as_str())).count(),
            "compiled_constraints": format!("{:?}", compiled.constraints),
            "compiled_ambiguities": format!("{:?}", compiled.ambiguous),
            "residual_lexical": compiled.residual_lexical, "plan": format!("{:?}", plan.outcome),
            "selectivity": plan.selectivity,
            "lexical_judged_hits": judged_hits(&baseline), "native_judged_hits": judged_hits(&native),
            "lexical_ranked_ids": baseline, "native_ranked_ids": native,
            "lexical_diagnostics": baseline_errors, "native_diagnostics": native_errors
        });
        serde_json::to_writer(&mut rows, &row)?;
        writeln!(rows)?;
        rows.flush()?;
    }
    let means = if paired == 0 { Value::Null } else {
        let mean = |s: [f64; 3]| metric_json((s[0]/paired as f64, s[1]/paired as f64, s[2]/paired as f64));
        json!({"lexical": mean(sums[0]), "native": mean(sums[1]),
            "native_minus_lexical": mean([sums[1][0]-sums[0][0], sums[1][1]-sums[0][1], sums[1][2]-sums[0][2]])})
    };
    write_json(&output.join("summary.json"), &json!({
        "dataset": dataset, "total_queries": data.queries.len(), "paired_scored_queries": paired,
        "diagnostic_queries": diagnostic_queries, "no_positive_qrel_queries": no_positive,
        "changed_rankings": changed, "macro_means": means,
        "complete_without_diagnostics": diagnostic_queries == 0,
        "claim": "descriptive paired retrieval comparison only; no system-level verdict"
    }))?;
    if diagnostic_queries > 0 || paired == 0 {
        return Err(format!("{diagnostic_queries} diagnostic queries; {paired} scoreable pairs; inspect retained artifacts").into());
    }
    Ok(())
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 {
        eprintln!("usage: public_ranked <wands|esci_electronics> <new-output-directory>");
        std::process::exit(2);
    }
    let output = Path::new(&args[2]);
    // Refuse to overwrite or append to any previous run's evidence.
    if let Err(error) = fs::create_dir(output) {
        eprintln!("cannot create new output directory: {error}");
        std::process::exit(1);
    }
    let outcome = std::panic::catch_unwind(|| run(&args[1], output));
    let error = match outcome {
        Ok(Ok(())) => return,
        Ok(Err(error)) => error.to_string(),
        Err(payload) => payload.downcast_ref::<String>().cloned()
            .or_else(|| payload.downcast_ref::<&str>().map(|s| (*s).to_owned()))
            .unwrap_or_else(|| "unrecognized panic payload".to_owned()),
    };
    // Never overwrite an existing run's evidence, including its failure record.
    if output.is_dir() {
        if let Ok(mut file) = File::options().write(true).create_new(true).open(output.join("failure.json")) {
            let _ = writeln!(file, "{}", json!({"error": error}));
        }
    }
    eprintln!("public_ranked failed: {error}");
    std::process::exit(1);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicate_qrels_are_errors() {
        let mut gains = BTreeMap::new();
        insert_gain(&mut gains, "p", 1.0).unwrap();
        assert!(insert_gain(&mut gains, "p", 0.0).is_err());
    }

    #[test]
    fn missing_catalog_positive_stays_in_denominator() {
        let gains = BTreeMap::from([("present".to_owned(), 1.0), ("missing".to_owned(), 1.0)]);
        let scores = ndcg_recall_mrr(&["present".to_owned()], &gains, K);
        assert_eq!(scores.1, 0.5);
        assert!(scores.0 < 1.0);
    }

    #[test]
    fn external_mapping_rejects_unknown_and_duplicate_hits() {
        let ids = BTreeMap::from([(ProductId(7), "asin".to_owned())]);
        assert_eq!(ranked_ids([ProductId(7)].into_iter(), &ids).unwrap(), vec!["asin"]);
        assert!(ranked_ids([ProductId(8)].into_iter(), &ids).is_err());
        assert!(ranked_ids([ProductId(7), ProductId(7)].into_iter(), &ids).is_err());
    }
}
