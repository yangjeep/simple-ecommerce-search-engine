//! Issue #57 frozen full-matrix benchmark: shared engine-transport,
//! timing, and reporting helpers reused by every per-dataset binary
//! under `src/bin/` (`wands_full_matrix.rs`, `esci_*_full_matrix.rs`,
//! `magento_full_matrix.rs`). Kept in one place rather than duplicated
//! per binary so a fix (e.g. the ES `track_total_hits` cap and the
//! Havenask empty-string/tie-break facet bugs both found and fixed while
//! building the WANDS cell -- see `docs/experiments/FULL_MATRIX_PROTOCOL.md`
//! §12) applies to every dataset automatically instead of needing to be
//! rediscovered and reapplied per binary.
//!
//! Every per-engine function returns `Result<_, String>` rather than
//! panicking, and every timed count/facet call site in a `src/bin/`
//! binary is expected to `.expect(...)` it immediately inside the timed
//! closure -- a transport/parse failure aborts the run loudly (matching
//! `comparator-eval`'s "never silently drop a failed comparator query"
//! discipline) rather than being folded into a timing sample.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Instant;

pub const REPS: usize = 30;
pub const WARMUP: usize = 5;

// ---------- Revision 2 gap closure: randomized/counterbalanced engine
// order (Issue #57 adversarial review, gap 2) ----------
//
// Revision 1 always benchmarked engines in the fixed order
// native->solr->es->opensearch->havenask, every cell, every dataset --
// the adversarial review's single most important flagged confound
// (Havenask always queried last, after four already-resident engines).
// Rather than a hand-picked "run it twice" fix, every cell below is
// given its own deterministic-but-distinct execution order, derived by
// hashing that cell's own (dataset, class, key) identity -- across the
// dozens of cells in the full matrix this counterbalances engine
// identity against queue position (each engine lands in each of the 5
// positions roughly 1/5 of the time), while remaining fully
// reproducible from the committed seed inputs (no external RNG crate
// dependency, no unseeded randomness).

/// FNV-1a, a well-known non-cryptographic hash -- adequate for seeding a
/// per-cell execution-order permutation, not for anything security-
/// sensitive.
pub fn cell_seed(parts: &[&str]) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for part in parts {
        for byte in part.as_bytes() {
            hash ^= *byte as u64;
            hash = hash.wrapping_mul(0x100000001b3);
        }
        hash ^= 0xff;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

/// splitmix64, seeded by `cell_seed` -- deterministic, reproducible,
/// no external crate.
fn splitmix64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E3779B97F4A7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
    z ^ (z >> 31)
}

/// Fisher-Yates over `0..n`, seeded by `seed`.
pub fn shuffled_order(seed: u64, n: usize) -> Vec<usize> {
    let mut state = seed;
    let mut order: Vec<usize> = (0..n).collect();
    for i in (1..n).rev() {
        let j = (splitmix64(&mut state) as usize) % (i + 1);
        order.swap(i, j);
    }
    order
}

/// Runs each named engine closure's full `time_reps` (warmup + REPS)
/// block, in an order permuted by `seed` rather than the closures'
/// declaration order -- returning results keyed by engine name (so
/// call sites are order-agnostic) plus the actual execution order used
/// (recorded into `Row::engine_order` for audit). Every closure still
/// runs its own full isolated timed block (Issue #57 "isolate engines
/// so one engine's process/cache state does not contaminate another"),
/// unlike a per-repetition interleave -- only which engine's block goes
/// first/second/... changes.
pub fn run_shuffled<T>(
    seed: u64,
    mut engines: Vec<(&'static str, Box<dyn FnMut() -> T + '_>)>,
) -> (BTreeMap<&'static str, (Vec<u128>, T)>, Vec<String>) {
    let order = shuffled_order(seed, engines.len());
    let mut results = BTreeMap::new();
    let mut order_labels = Vec::new();
    for &i in &order {
        let (name, f) = &mut engines[i];
        order_labels.push(name.to_string());
        results.insert(*name, time_reps(f));
    }
    (results, order_labels)
}

pub fn percentile_ms(sorted_ns: &[u128], p: f64) -> f64 {
    if sorted_ns.is_empty() {
        return 0.0;
    }
    let idx = ((sorted_ns.len() as f64 - 1.0) * p).round() as usize;
    sorted_ns[idx] as f64 / 1_000_000.0
}

pub fn stats_ms(mut samples_ns: Vec<u128>) -> (f64, f64, f64) {
    samples_ns.sort_unstable();
    let mean = samples_ns.iter().sum::<u128>() as f64 / samples_ns.len() as f64 / 1_000_000.0;
    (
        mean,
        percentile_ms(&samples_ns, 0.5),
        percentile_ms(&samples_ns, 0.99),
    )
}

pub fn time_reps<T, F: FnMut() -> T>(mut f: F) -> (Vec<u128>, T) {
    for _ in 0..WARMUP {
        f();
    }
    let mut samples = Vec::with_capacity(REPS);
    let mut last = None;
    for _ in 0..REPS {
        let start = Instant::now();
        let result = f();
        samples.push(start.elapsed().as_nanos());
        last = Some(result);
    }
    (samples, last.unwrap())
}

pub fn escape_solr_phrase(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

pub fn escape_sql_literal(s: &str) -> String {
    s.replace('\'', "''")
}

// ---------- per-engine count/facet/text-search calls ----------

pub fn solr_count(base_url: &str, fq: &[String]) -> Result<u64, String> {
    let mut req = ureq::get(&format!("{base_url}/select"))
        .query("q", "*:*")
        .query("rows", "0");
    for f in fq {
        req = req.query("fq", f);
    }
    let body: serde_json::Value = req
        .call()
        .map_err(|e| format!("solr transport: {e}"))?
        .into_json()
        .map_err(|e| format!("solr parse: {e}"))?;
    body["response"]["numFound"]
        .as_u64()
        .ok_or_else(|| format!("solr: no numFound in {body}"))
}

pub fn solr_facet(
    base_url: &str,
    fq: &[String],
    field: &str,
    limit: u64,
) -> Result<BTreeMap<String, u64>, String> {
    let facet_spec = format!(r#"{{"vals":{{"type":"terms","field":"{field}","limit":{limit}}}}}"#);
    let mut req = ureq::get(&format!("{base_url}/select"))
        .query("q", "*:*")
        .query("rows", "0")
        .query("json.facet", &facet_spec);
    for f in fq {
        req = req.query("fq", f);
    }
    let body: serde_json::Value = req
        .call()
        .map_err(|e| format!("solr transport: {e}"))?
        .into_json()
        .map_err(|e| format!("solr parse: {e}"))?;
    let mut out = BTreeMap::new();
    if let Some(buckets) = body["facets"]["vals"]["buckets"].as_array() {
        for b in buckets {
            out.insert(
                b["val"].as_str().unwrap_or_default().to_string(),
                b["count"].as_u64().unwrap_or(0),
            );
        }
    }
    Ok(out)
}

pub fn solr_text_count(base_url: &str, q: &str, qf: &str) -> Result<u64, String> {
    let body: serde_json::Value = ureq::get(&format!("{base_url}/select"))
        .query("q", q)
        .query("defType", "edismax")
        .query("qf", qf)
        .query("rows", "0")
        .call()
        .map_err(|e| format!("solr transport: {e}"))?
        .into_json()
        .map_err(|e| format!("solr parse: {e}"))?;
    body["response"]["numFound"]
        .as_u64()
        .ok_or_else(|| format!("solr: no numFound in {body}"))
}

pub fn es_count(base_url: &str, index: &str, filter: &[serde_json::Value]) -> Result<u64, String> {
    let body = serde_json::json!({"query": {"bool": {"filter": filter}}, "size": 0, "track_total_hits": true});
    let resp: serde_json::Value = ureq::post(&format!("{base_url}/{index}/_search"))
        .send_json(body)
        .map_err(|e| format!("es transport: {e}"))?
        .into_json()
        .map_err(|e| format!("es parse: {e}"))?;
    resp["hits"]["total"]["value"]
        .as_u64()
        .ok_or_else(|| format!("es: no hits.total.value in {resp}"))
}

pub fn es_facet(
    base_url: &str,
    index: &str,
    filter: &[serde_json::Value],
    field: &str,
    limit: u64,
) -> Result<BTreeMap<String, u64>, String> {
    let body = serde_json::json!({
        "query": {"bool": {"filter": filter}},
        "size": 0,
        "track_total_hits": true,
        "aggs": {"vals": {"terms": {"field": field, "size": limit}}}
    });
    let resp: serde_json::Value = ureq::post(&format!("{base_url}/{index}/_search"))
        .send_json(body)
        .map_err(|e| format!("es transport: {e}"))?
        .into_json()
        .map_err(|e| format!("es parse: {e}"))?;
    let mut out = BTreeMap::new();
    if let Some(buckets) = resp["aggregations"]["vals"]["buckets"].as_array() {
        for b in buckets {
            out.insert(
                b["key"].as_str().unwrap_or_default().to_string(),
                b["doc_count"].as_u64().unwrap_or(0),
            );
        }
    }
    Ok(out)
}

pub fn es_text_count(base_url: &str, index: &str, q: &str, fields: &[&str]) -> Result<u64, String> {
    let body = serde_json::json!({
        "query": {"multi_match": {"query": q, "fields": fields}},
        "size": 0,
        "track_total_hits": true
    });
    let resp: serde_json::Value = ureq::post(&format!("{base_url}/{index}/_search"))
        .send_json(body)
        .map_err(|e| format!("es transport: {e}"))?
        .into_json()
        .map_err(|e| format!("es parse: {e}"))?;
    resp["hits"]["total"]["value"]
        .as_u64()
        .ok_or_else(|| format!("es: no hits.total.value in {resp}"))
}

pub fn havenask_query(base_url: &str, sql: &str) -> Result<serde_json::Value, String> {
    let full = format!("{sql}&&kvpair=databaseName:database;formatType:json");
    let resp: serde_json::Value = ureq::post(&format!("{base_url}/QrsService/searchSql"))
        .send_json(serde_json::json!({"assemblyQuery": full}))
        .map_err(|e| format!("havenask transport: {e}"))?
        .into_json()
        .map_err(|e| format!("havenask parse: {e}"))?;
    let error_info = resp["error_info"].as_str().unwrap_or_default();
    if !error_info.contains("ERROR_NONE") {
        return Err(format!("havenask query error: {error_info}: sql={full}"));
    }
    let sql_result_str = resp["sql_result"]
        .as_str()
        .ok_or_else(|| format!("havenask: no sql_result in {resp}"))?;
    serde_json::from_str(sql_result_str)
        .map_err(|e| format!("havenask nested parse: {e}: {sql_result_str}"))
}

pub fn havenask_count(base_url: &str, table: &str, where_clause: &str) -> Result<u64, String> {
    let sql = format!("select count(*) from {table}{where_clause}");
    let inner = havenask_query(base_url, &sql)?;
    // Confirmed-live Havenask SQL quirk (found building the Magento
    // cell's Q8 trap checks -- most of which expect a genuine zero
    // match): a `COUNT(*)` over a `WHERE` clause matching zero rows
    // returns an EMPTY `data` array, not the standard-SQL single row
    // `[[0]]` every other engine here returns. Both shapes mean "zero",
    // so both are treated identically rather than one surfacing as a
    // parse failure.
    if let Some(data) = inner["data"].as_array() {
        if data.is_empty() {
            return Ok(0);
        }
    }
    inner["data"][0][0]
        .as_u64()
        .ok_or_else(|| format!("havenask: no count in {inner}"))
}

/// See this crate's top-level doc comment: excludes Havenask's
/// default-`''`-for-unset-STRING-columns from the facet (matching
/// Solr/ES/OpenSearch's "missing field is excluded" semantics) and adds
/// an explicit alphabetical secondary sort key so a `LIMIT`-truncated
/// top-N among tied counts is deterministic and comparable across
/// engines -- both confirmed-live, disclosed findings from the WANDS
/// cell, not assumptions.
pub fn havenask_facet(
    base_url: &str,
    table: &str,
    where_clause: &str,
    field: &str,
    limit: u64,
) -> Result<BTreeMap<String, u64>, String> {
    let extended_where = if where_clause.is_empty() {
        format!(" where {field} <> ''")
    } else {
        format!("{where_clause} and {field} <> ''")
    };
    let sql = format!(
        "select {field}, count(*) as c from {table}{extended_where} group by {field} order by c desc, {field} asc limit {limit}"
    );
    let inner = havenask_query(base_url, &sql)?;
    let mut out = BTreeMap::new();
    if let Some(rows) = inner["data"].as_array() {
        for row in rows {
            let val = row[0].as_str().unwrap_or_default().to_string();
            let count = row[1].as_u64().unwrap_or(0);
            if !val.is_empty() {
                out.insert(val, count);
            }
        }
    }
    Ok(out)
}

pub fn havenask_text_count(
    base_url: &str,
    table: &str,
    index_name: &str,
    q: &str,
) -> Result<u64, String> {
    let escaped = escape_sql_literal(q);
    let where_clause = format!(" where MATCHINDEX('{index_name}', '{escaped}')");
    havenask_count(base_url, table, &where_clause)
}

// ---------- shared report row / printer / CSV writer ----------

pub struct Row {
    pub class: String,
    pub key: String,
    pub native_count: u64,
    pub counts: Vec<(String, u64)>,
    pub counts_match: bool,
    /// (engine, mean_ms, p50_ms, p99_ms)
    pub timings_ms: Vec<(String, f64, f64, f64)>,
    /// The actual per-cell engine execution order used (Revision 2 gap
    /// closure: randomized/counterbalanced order, see `run_shuffled`).
    /// Empty for a row that has no timed engine comparison (e.g. Q2b).
    pub engine_order: Vec<String>,
}

/// Prints the human-readable table, writes `results.csv` under
/// `dataset_cache/issue57_<dataset>_full_matrix/`, and exits the process
/// with status 1 if any row's `counts_match` is false or `mismatches` is
/// non-empty -- the correctness gate every dataset binary shares.
pub fn report(dataset: &str, rows: &[Row], mismatches: &[String]) {
    println!("\n=== Issue #57 {dataset} full-matrix result ===");
    let mut csv = String::from(
        "class,key,native_count,engine_counts,counts_match,engine,mean_ms,p50_ms,p99_ms,engine_order\n",
    );
    for r in rows {
        println!(
            "{:<45} {:<60} native={:>8} match={}",
            r.class, r.key, r.native_count, r.counts_match
        );
        if !r.engine_order.is_empty() {
            println!("    execution order: {}", r.engine_order.join(" -> "));
        }
        for (engine, mean, p50, p99) in &r.timings_ms {
            println!("    {engine:<15} mean={mean:>9.4}ms p50={p50:>9.4}ms p99={p99:>9.4}ms");
            csv.push_str(&format!(
                "{},{},{},{:?},{},{},{},{},{},{}\n",
                r.class,
                r.key.replace(',', ";"),
                r.native_count,
                r.counts,
                r.counts_match,
                engine,
                mean,
                p50,
                p99,
                r.engine_order.join("->")
            ));
        }
    }

    // Revision 2 gap closure: the ordering-confound check itself --
    // mean latency by queue *position* (1st..5th queried), per engine,
    // across every row that recorded an execution order. If Havenask
    // (or any engine) is systematically slower only when queried late,
    // that would show up here as position correlating with latency
    // *within* an engine; a real per-engine performance difference
    // shows up as a latency gap that persists across positions instead.
    let mut by_engine_position: BTreeMap<(String, usize), Vec<f64>> = BTreeMap::new();
    for r in rows {
        if r.engine_order.is_empty() {
            continue;
        }
        let position_of: BTreeMap<&str, usize> = r
            .engine_order
            .iter()
            .enumerate()
            .map(|(i, name)| (name.as_str(), i + 1))
            .collect();
        for (engine, mean, _p50, _p99) in &r.timings_ms {
            if let Some(&pos) = position_of.get(engine.as_str()) {
                by_engine_position
                    .entry((engine.clone(), pos))
                    .or_default()
                    .push(*mean);
            }
        }
    }
    if !by_engine_position.is_empty() {
        println!("\n=== engine-order confound check: mean latency (ms) by queue position ===");
        let engines: Vec<String> = {
            let mut names: Vec<String> = by_engine_position.keys().map(|(e, _)| e.clone()).collect();
            names.sort();
            names.dedup();
            names
        };
        for engine in &engines {
            let mut cells = Vec::new();
            for pos in 1..=5 {
                if let Some(samples) = by_engine_position.get(&(engine.clone(), pos)) {
                    let mean_of_means = samples.iter().sum::<f64>() / samples.len() as f64;
                    cells.push(format!("pos{pos}(n={})={mean_of_means:.4}", samples.len()));
                }
            }
            println!("  {engine:<15} {}", cells.join("  "));
        }
    }

    let all_match = rows.iter().all(|r| r.counts_match) && mismatches.is_empty();
    println!(
        "\ncorrectness: {}/{} rows had matching counts{}",
        rows.iter().filter(|r| r.counts_match).count(),
        rows.len(),
        if all_match {
            " (ALL MATCH)"
        } else {
            " -- SEE MISMATCHES BELOW"
        }
    );
    for m in mismatches {
        println!("  {m}");
    }

    let artifacts_dir = PathBuf::from(format!("dataset_cache/issue57_{dataset}_full_matrix"));
    std::fs::create_dir_all(&artifacts_dir).ok();
    std::fs::write(artifacts_dir.join("results.csv"), &csv).ok();
    println!("\nartifacts written to {}", artifacts_dir.display());

    if !all_match {
        std::process::exit(1);
    }
}

// ---------- Revision 2 gap closure: relevance-quality metrics
// (NDCG@k/Recall@k/MRR@k), Issue #57 adversarial review gap 1 ----------
//
// Revision 1 measured zero relevance-quality metrics for any of the
// four external engines. `ndcg_recall_mrr` below is a single graded-
// gain implementation usable against both WANDS's 3-grade label scale
// (`phase9_eval::wands_relevance::WandsLabel::gain`) and ESCI's 4-grade
// scale (`issue35_eval::label_gain`) -- both callers already reduce
// their own dataset-specific label to an `f64` gain before calling in,
// so one scorer serves both, exactly like `round1_eval::relevance`/
// `phase9_eval::wands_relevance` already established for their own
// single-engine (Solr-only) precedents. `gain > 0.0` is "relevant" for
// Recall/MRR, matching both datasets' own convention (WANDS's
// `WandsLabel::is_relevant`, ESCI's implicit "Irrelevant=0.0").

/// Returns `(ndcg_at_k, recall_at_k, mrr_at_k)`. `(0.0, 0.0, 0.0)` when
/// `gains` has no relevant (gain > 0) entries at all -- a query with no
/// non-Irrelevant judgment carries no scoreable signal, matching
/// `phase9_eval::wands_relevance::ndcg_recall_mrr`'s own precedent
/// rather than dividing by zero or fabricating a score.
pub fn ndcg_recall_mrr(ranked_ids: &[String], gains: &BTreeMap<String, f64>, k: usize) -> (f64, f64, f64) {
    let relevant_total = gains.values().filter(|&&g| g > 0.0).count();
    if relevant_total == 0 {
        return (0.0, 0.0, 0.0);
    }
    let top: Vec<&str> = ranked_ids.iter().take(k).map(String::as_str).collect();

    let dcg: f64 = top
        .iter()
        .enumerate()
        .map(|(i, id)| gains.get(*id).copied().unwrap_or(0.0) / (i as f64 + 2.0).log2())
        .sum();
    let mut ideal: Vec<f64> = gains.values().copied().collect();
    ideal.sort_by(|a, b| b.total_cmp(a));
    let idcg: f64 = ideal
        .iter()
        .take(k)
        .enumerate()
        .map(|(i, g)| g / (i as f64 + 2.0).log2())
        .sum();
    let ndcg = if idcg > 0.0 { dcg / idcg } else { 0.0 };

    let hits = top
        .iter()
        .filter(|id| gains.get(**id).copied().unwrap_or(0.0) > 0.0)
        .count();
    let recall = hits as f64 / relevant_total as f64;

    let rr = top
        .iter()
        .position(|id| gains.get(*id).copied().unwrap_or(0.0) > 0.0)
        .map(|pos| 1.0 / (pos as f64 + 1.0))
        .unwrap_or(0.0);

    (ndcg, recall, rr)
}

// ---------- ranked (not just count) retrieval per engine, needed for
// relevance scoring above ----------

/// Solr edismax search returning ranked `id`s (Solr's own relevance
/// order, not re-sorted here) -- same `fq`-fairness discipline as
/// `solr_count`/`issue35_eval::eval::solr_search`: structural
/// constraints the caller already enforces natively must be passed as
/// `fq` so Solr answers the identically-scoped question, not a broader
/// one.
pub fn solr_search_ids(
    base_url: &str,
    q: &str,
    fq: &[String],
    qf: &str,
    rows: usize,
) -> Result<Vec<String>, String> {
    let rows_str = rows.to_string();
    let mut req = ureq::get(&format!("{base_url}/select"))
        .query("q", q)
        .query("defType", "edismax")
        .query("qf", qf)
        .query("rows", &rows_str)
        .query("fl", "id");
    for f in fq {
        req = req.query("fq", f);
    }
    let body: serde_json::Value = req
        .call()
        .map_err(|e| format!("solr transport: {e}"))?
        .into_json()
        .map_err(|e| format!("solr parse: {e}"))?;
    let docs = body["response"]["docs"]
        .as_array()
        .ok_or_else(|| format!("solr: no response.docs in {body}"))?;
    Ok(docs
        .iter()
        .filter_map(|d| d["id"].as_str().map(str::to_string))
        .collect())
}

/// Elasticsearch/OpenSearch `multi_match` search returning ranked
/// `_id`s in the engine's own `_score`-descending order (the default
/// `_search` order, not re-sorted here).
pub fn es_search_ids(
    base_url: &str,
    index: &str,
    q: &str,
    fields: &[&str],
    filter: &[serde_json::Value],
    rows: usize,
) -> Result<Vec<String>, String> {
    let body = serde_json::json!({
        "query": {"bool": {
            "must": {"multi_match": {"query": q, "fields": fields}},
            "filter": filter,
        }},
        "size": rows,
        "_source": false,
    });
    let resp: serde_json::Value = ureq::post(&format!("{base_url}/{index}/_search"))
        .send_json(body)
        .map_err(|e| format!("es transport: {e}"))?
        .into_json()
        .map_err(|e| format!("es parse: {e}"))?;
    let hits = resp["hits"]["hits"]
        .as_array()
        .ok_or_else(|| format!("es: no hits.hits in {resp}"))?;
    Ok(hits
        .iter()
        .filter_map(|h| h["_id"].as_str().map(str::to_string))
        .collect())
}

/// Havenask SQL `MATCHINDEX` search returning ids in the order Havenask
/// itself returns them.
///
/// **Disclosed capability gap, not a silent omission**: this SQL/QRS
/// endpoint (the same one used for every count/facet query elsewhere in
/// this crate) exposes no documented relevance-score column or
/// `ORDER BY <score>` clause in this deployment's schema (`direct`
/// table type, no custom ranking profile) -- Issue #57 §"Fairness
/// contract" is explicit that when "one engine genuinely cannot express
/// a semantic requirement equivalently, report that as a capability
/// result rather than forcing an invalid... comparison." Rows are
/// therefore returned in Havenask's own default result order (an
/// index/docid order, not a verified relevance order), and every NDCG/
/// Recall/MRR row computed from this function's output is labeled
/// `havenask_unranked_capability_gap` rather than compared head-to-head
/// against the other four engines' genuinely relevance-ranked results.
pub fn havenask_search_ids(
    base_url: &str,
    table: &str,
    index_name: &str,
    id_field: &str,
    q: &str,
    extra_where: &[String],
    rows: usize,
) -> Result<Vec<String>, String> {
    let escaped = escape_sql_literal(q);
    let mut clause = format!("MATCHINDEX('{index_name}', '{escaped}')");
    for w in extra_where {
        clause.push_str(" and ");
        clause.push_str(w);
    }
    let sql = format!("select {id_field} from {table} where {clause} limit {rows}");
    let inner = havenask_query(base_url, &sql)?;
    let data = inner["data"]
        .as_array()
        .ok_or_else(|| format!("havenask: no data in {inner}"))?;
    Ok(data
        .iter()
        .filter_map(|row| {
            row[0]
                .as_str()
                .map(str::to_string)
                .or_else(|| row[0].as_u64().map(|n| n.to_string()))
        })
        .collect())
}

// ---------- Revision 2 gap closure: index size / build time / startup
// time / memory instrumentation, Issue #57 adversarial review gap
// ("no index/build-time matrix") ----------

/// Recursively sums file sizes under `path` (best-effort: a permission
/// error or race on an individual entry is skipped, not fatal -- this
/// is a diagnostic instrumentation figure, not a correctness gate).
pub fn dir_size_bytes(path: &std::path::Path) -> u64 {
    let mut total = 0u64;
    let Ok(entries) = std::fs::read_dir(path) else {
        return 0;
    };
    for entry in entries.flatten() {
        let p = entry.path();
        if let Ok(meta) = entry.metadata() {
            if meta.is_dir() {
                total += dir_size_bytes(&p);
            } else {
                total += meta.len();
            }
        }
    }
    total
}

/// Current resident-set size (RSS, kilobytes) of `pid`, read from
/// `/proc/<pid>/status` -- Linux-specific (this project's own
/// documented host/runtime environment, `FULL_MATRIX_PROTOCOL.md` §4),
/// not portable, and that is an accepted, disclosed scope limit rather
/// than a silent Linux-only assumption.
pub fn process_rss_kb(pid: u32) -> Option<u64> {
    let status = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    status.lines().find_map(|line| {
        line.strip_prefix("VmRSS:")
            .and_then(|rest| rest.trim().split_whitespace().next())
            .and_then(|n| n.parse::<u64>().ok())
    })
}

/// One (dataset, engine) cell's ingestion/footprint economics --
/// `FULL_MATRIX_PROTOCOL.md` §11's required measurement, not
/// systematically instrumented in Revision 1 (adversarial review,
/// confirmed limitation).
#[derive(Debug, Clone)]
pub struct EngineFootprint {
    pub engine: String,
    pub build_ms: f64,
    pub index_bytes: u64,
    pub startup_ms: f64,
    pub peak_rss_kb: u64,
}

pub fn write_footprint_csv(dataset: &str, footprints: &[EngineFootprint]) {
    println!("\n=== Issue #57 {dataset} index/build/startup/memory footprint ===");
    let mut csv = String::from("dataset,engine,build_ms,index_bytes,startup_ms,peak_rss_kb\n");
    for f in footprints {
        println!(
            "  {:<15} build={:>10.1}ms index={:>12} bytes startup={:>10.1}ms peak_rss={:>10} kB",
            f.engine, f.build_ms, f.index_bytes, f.startup_ms, f.peak_rss_kb
        );
        csv.push_str(&format!(
            "{},{},{},{},{},{}\n",
            dataset, f.engine, f.build_ms, f.index_bytes, f.startup_ms, f.peak_rss_kb
        ));
    }
    let artifacts_dir = PathBuf::from(format!("dataset_cache/issue57_{dataset}_full_matrix"));
    std::fs::create_dir_all(&artifacts_dir).ok();
    std::fs::write(artifacts_dir.join("footprint.csv"), &csv).ok();
}

/// One dataset's relevance-metric row for one engine, aggregated over
/// every judged query that had at least one non-Irrelevant judgment
/// (matching Issue #35's own `evaluated_queries` convention).
#[derive(Debug, Clone)]
pub struct RelevanceRow {
    pub engine: String,
    pub n_queries: usize,
    pub ndcg_at_10: f64,
    pub recall_at_10: f64,
    pub mrr_at_10: f64,
}

pub fn report_relevance(dataset: &str, rows: &[RelevanceRow]) {
    println!("\n=== Issue #57 {dataset} relevance (NDCG@10/Recall@10/MRR@10) ===");
    let mut csv = String::from("dataset,engine,n_queries,ndcg_at_10,recall_at_10,mrr_at_10\n");
    for r in rows {
        println!(
            "  {:<15} n={:<6} NDCG@10={:.4}  Recall@10={:.4}  MRR@10={:.4}",
            r.engine, r.n_queries, r.ndcg_at_10, r.recall_at_10, r.mrr_at_10
        );
        csv.push_str(&format!(
            "{},{},{},{},{},{}\n",
            dataset, r.engine, r.n_queries, r.ndcg_at_10, r.recall_at_10, r.mrr_at_10
        ));
    }
    let artifacts_dir = PathBuf::from(format!("dataset_cache/issue57_{dataset}_full_matrix"));
    std::fs::create_dir_all(&artifacts_dir).ok();
    std::fs::write(artifacts_dir.join("relevance.csv"), &csv).ok();
}
