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
}

/// Prints the human-readable table, writes `results.csv` under
/// `dataset_cache/issue57_<dataset>_full_matrix/`, and exits the process
/// with status 1 if any row's `counts_match` is false or `mismatches` is
/// non-empty -- the correctness gate every dataset binary shares.
pub fn report(dataset: &str, rows: &[Row], mismatches: &[String]) {
    println!("\n=== Issue #57 {dataset} full-matrix result ===");
    let mut csv = String::from(
        "class,key,native_count,engine_counts,counts_match,engine,mean_ms,p50_ms,p99_ms\n",
    );
    for r in rows {
        println!(
            "{:<45} {:<60} native={:>8} match={}",
            r.class, r.key, r.native_count, r.counts_match
        );
        for (engine, mean, p50, p99) in &r.timings_ms {
            println!("    {engine:<15} mean={mean:>9.4}ms p50={p50:>9.4}ms p99={p99:>9.4}ms");
            csv.push_str(&format!(
                "{},{},{},{:?},{},{},{},{},{}\n",
                r.class,
                r.key.replace(',', ";"),
                r.native_count,
                r.counts,
                r.counts_match,
                engine,
                mean,
                p50,
                p99
            ));
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
