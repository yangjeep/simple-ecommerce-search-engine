//! Issue #35: the shared measurement procedure every unseen-vertical
//! slice binary runs. Extracted so a second/third vertical slice
//! (`docs/experiments/ISSUE35_ESCI_ELECTRONICS_PROTOCOL.md`'s own
//! stated Workstream D goal of testing "at least three materially
//! different verticals") reuses the identical measurement code -- only
//! the input data (a different real ESCI category slice) differs
//! between callers. Unlike per-experiment fixture-construction helpers
//! elsewhere in this project (e.g. `r1_full_gate_scale_rerun.rs`'s own
//! `scaled_catalog`, deliberately NOT shared because each caller needs
//! subtly different decoy semantics), this procedure is genuinely
//! identical across verticals: the whole point is applying the same
//! unmodified measurement to different data.

use std::collections::BTreeMap;

use commerce_core::cold_start::{compile_lexicon, CatalogProfile};
use commerce_core::domain::{BrandId, ProductId};
use commerce_core::index::CatalogIndex;
use commerce_core::ir::{compile, ResolvedConstraint, StructuralConstraint};
use commerce_core::plan::{execute_planned, ExecutionOutcome, LexicalDelegate, PlannerPolicy};
use phase9_eval::bitmap_delegate::{build_index, BitmapTantivyDelegate};

use crate::{build_catalog, label_gain, load_products, load_queries};

const K: usize = 10;
const MIN_ENUM_FREQUENCY: usize = 1;

fn ndcg_at_k_graded(ranked_ids: &[String], gains: &BTreeMap<String, f64>, k: usize) -> Option<f64> {
    let mut ideal: Vec<f64> = gains.values().copied().collect();
    ideal.sort_by(|a, b| b.total_cmp(a));
    let idcg: f64 = ideal
        .iter()
        .take(k)
        .enumerate()
        .map(|(i, g)| g / (i as f64 + 2.0).log2())
        .sum();
    if idcg <= 0.0 {
        return None;
    }
    let dcg: f64 = ranked_ids
        .iter()
        .take(k)
        .enumerate()
        .map(|(i, id)| gains.get(id).copied().unwrap_or(0.0) / (i as f64 + 2.0).log2())
        .sum();
    Some(dcg / idcg)
}

/// A Solr `/select` outcome, kept distinct from "the query legitimately
/// matched zero documents" (`Success(vec![])`, a real, scoreable
/// NDCG=0.0) so that a transport failure or an unparseable/erroring
/// response can never be silently folded into that same zero -- see
/// `docs/decisions/ISSUE35_SOLR_HARNESS_HARDENING_DECISION.md`. The
/// harness's job is to score *relevance*, not to launder infrastructure
/// failure into a relevance verdict against Solr.
#[derive(Debug)]
enum SolrLookup {
    /// The request round-tripped, the body parsed, and Solr itself
    /// reported success (`responseHeader.status == 0`). `Success(vec![])`
    /// is a legitimate zero-result query, not a failure.
    Success(Vec<String>),
    /// The HTTP request itself failed (connection refused, timeout, a
    /// non-2xx/3xx status ureq surfaces as `Err`) or, having round-tripped,
    /// Solr's own `responseHeader.status` was non-zero (a Solr-side query
    /// error, e.g. a malformed edismax query) -- from the harness's
    /// perspective both are "Solr did not answer this query," not
    /// "Solr answered with zero relevant documents."
    TransportError(String),
    /// The request round-tripped and returned a 2xx/3xx, but the body was
    /// not valid JSON, or valid JSON missing the expected
    /// `response.docs` array shape -- a benchmark-harness bug or a Solr
    /// response-format mismatch, not a relevance signal.
    ParseError(String),
}

fn solr_search(base_url: &str, q: &str, rows: usize) -> SolrLookup {
    let url = format!("{base_url}/select");
    let resp = ureq::post(&url)
        .timeout(std::time::Duration::from_secs(30))
        .send_form(&[
            ("q", q),
            ("defType", "edismax"),
            ("qf", "title description bullet_point"),
            ("rows", &rows.to_string()),
            ("fl", "id"),
        ]);
    let resp = match resp {
        Ok(resp) => resp,
        Err(e) => return SolrLookup::TransportError(format!("HTTP request failed: {e}")),
    };
    let body = match resp.into_json::<serde_json::Value>() {
        Ok(body) => body,
        Err(e) => return SolrLookup::ParseError(format!("response body was not valid JSON: {e}")),
    };
    if let Some(status) = body["responseHeader"]["status"].as_i64() {
        if status != 0 {
            return SolrLookup::TransportError(format!(
                "Solr responseHeader.status={status} (Solr-side query error): {body}"
            ));
        }
    }
    let Some(docs) = body["response"]["docs"].as_array() else {
        return SolrLookup::ParseError(format!(
            "response JSON parsed but had no response.docs array: {body}"
        ));
    };
    SolrLookup::Success(
        docs.iter()
            .filter_map(|d| d["id"].as_str().map(str::to_string))
            .collect(),
    )
}

fn mean(v: &[f64]) -> f64 {
    if v.is_empty() {
        0.0
    } else {
        v.iter().sum::<f64>() / v.len() as f64
    }
}

/// Runs the full Issue #35 unseen-vertical measurement
/// (`docs/experiments/ISSUE35_ESCI_ELECTRONICS_PROTOCOL.md`'s
/// methodology) against whatever products/queries files a caller
/// points it at, and against a real Solr core at `solr_base_url`.
/// `vertical_label` is printed only, for a human reading the output of
/// several different vertical runs.
pub fn run_vertical_eval(
    vertical_label: &str,
    products_path: &str,
    queries_path: &str,
    solr_base_url: &str,
) {
    println!(
        "=== Issue #35: unseen-vertical (real ESCI {vertical_label}) discovery + routing test ==="
    );

    let raw_products = load_products(products_path);
    let raw_queries = load_queries(queries_path);
    let ingested = build_catalog(&raw_products);
    println!(
        "catalog: {} products, {} distinct brands discovered (from real product_brand data), \
         0 product types/categories (none exist in this vertical's data -- left unregistered, \
         not fabricated)",
        ingested.catalog.products.len(),
        ingested.brands.len()
    );

    let index = CatalogIndex::build(&ingested.catalog);
    // No product types/categories registered -- exactly the "no
    // hand-authored vertical ontology" methodology constraint.
    let profile = CatalogProfile::build(&ingested.catalog, &ingested.brands, &[], &[]);
    let lexicon = compile_lexicon(&profile, MIN_ENUM_FREQUENCY);
    let built = build_index(&ingested.catalog).expect("in-memory tantivy index build");
    let delegate = BitmapTantivyDelegate::new(
        &built.index,
        vec![built.title_field, built.description_field],
    )
    .expect("tantivy delegate build");
    let policy = PlannerPolicy {
        selectivity_threshold: 0.05,
        delegate_oversample: 20,
    };

    let mut routing_counts: BTreeMap<&'static str, usize> = BTreeMap::new();
    let mut ambiguous_count = 0usize;
    let mut residual_count = 0usize;
    let mut brand_constrained_count = 0usize;
    let mut wrong_family_violations: Vec<String> = Vec::new();
    let mut native_ndcgs: Vec<f64> = Vec::new();
    let mut solr_ndcgs: Vec<f64> = Vec::new();
    let mut evaluated_queries = 0usize;
    // Preregistered rule (`docs/decisions/ISSUE35_SOLR_HARNESS_HARDENING_DECISION.md`):
    // this is a same-host, locally-controlled Solr core, not a flaky
    // remote dependency, so any transport/parse failure indicates a real
    // infrastructure problem, not expected variance. Such queries are
    // excluded from the native-vs-Solr comparison (never scored as
    // Solr NDCG=0.0) and recorded here; the run FAILS loudly rather than
    // silently reporting a partial, uncertified comparison.
    let mut solr_transport_errors = 0usize;
    let mut solr_parse_errors = 0usize;
    let mut solr_error_samples: Vec<String> = Vec::new();

    let title_by_product_id: BTreeMap<ProductId, &str> = ingested
        .catalog
        .products
        .iter()
        .map(|p| (p.id, p.title.as_str()))
        .collect();
    let brand_by_product_id: BTreeMap<ProductId, BrandId> = ingested
        .catalog
        .products
        .iter()
        .map(|p| (p.id, p.brand))
        .collect();

    for q in &raw_queries {
        let compiled = compile(&q.query, &lexicon);
        if !compiled.ambiguous.is_empty() {
            ambiguous_count += 1;
        }
        if !compiled.residual_lexical.is_empty() {
            residual_count += 1;
        }

        let brand_ids: Vec<BrandId> = compiled
            .constraints
            .iter()
            .filter_map(|c| match c {
                ResolvedConstraint::Structural(StructuralConstraint::Brand(id)) => Some(*id),
                _ => None,
            })
            .collect();
        if !brand_ids.is_empty() {
            brand_constrained_count += 1;
        }

        let (planned, hits) = execute_planned(
            &compiled,
            &ingested.catalog,
            &index,
            Some(&delegate as &dyn LexicalDelegate),
            K,
            &policy,
            None,
        );
        *routing_counts
            .entry(match planned.outcome {
                ExecutionOutcome::FastPath => "FastPath",
                ExecutionOutcome::Hybrid => "Hybrid",
                ExecutionOutcome::Punt => "Punt",
            })
            .or_insert(0) += 1;

        // Correctness hard gate: every hit for a Brand-constrained query
        // must carry that exact brand.
        for brand_id in &brand_ids {
            for hit in &hits {
                if brand_by_product_id.get(&hit.product) != Some(brand_id) {
                    wrong_family_violations.push(format!(
                        "query={:?} required_brand={:?} hit_product={:?} hit_brand={:?}",
                        q.query,
                        brand_id,
                        hit.product,
                        brand_by_product_id.get(&hit.product)
                    ));
                }
            }
        }

        // Relevance, using ASIN-keyed real judgments translated into
        // this catalog's own ProductId, then back to a string id for
        // the graded-NDCG helper (shared shape with Solr's own "id").
        let gains: BTreeMap<String, f64> = q
            .judgments
            .iter()
            .filter_map(|j| {
                ingested
                    .product_id_by_asin
                    .get(&j.product_id)
                    .map(|pid| (pid.0.to_string(), label_gain(&j.label)))
            })
            .collect();
        let native_ranked: Vec<String> = hits.iter().map(|h| h.product.0.to_string()).collect();
        if let Some(ndcg) = ndcg_at_k_graded(&native_ranked, &gains, K) {
            evaluated_queries += 1;

            // Solr comparison: translate Solr's real-ASIN hits back to
            // this catalog's ProductId space via the same map, so both
            // engines are scored against literally the same gains map.
            // native_ndcgs/solr_ndcgs are a PAIRED set: a query only
            // enters the native-vs-Solr comparison when Solr actually
            // answered it, so an infra failure can never masquerade as a
            // Solr relevance loss (see `solr_search`'s `SolrLookup`).
            match solr_search(solr_base_url, &q.query, K) {
                SolrLookup::Success(solr_hit_asins) => {
                    let solr_ranked: Vec<String> = solr_hit_asins
                        .iter()
                        .filter_map(|asin| ingested.product_id_by_asin.get(asin))
                        .map(|pid| pid.0.to_string())
                        .collect();
                    // A real, legitimate zero-result Solr query (valid
                    // response, empty/irrelevant docs) is scored 0.0 here
                    // -- that is a true relevance measurement, unlike the
                    // TransportError/ParseError arms below.
                    let solr_ndcg = ndcg_at_k_graded(&solr_ranked, &gains, K).unwrap_or(0.0);
                    native_ndcgs.push(ndcg);
                    solr_ndcgs.push(solr_ndcg);
                }
                SolrLookup::TransportError(detail) => {
                    solr_transport_errors += 1;
                    if solr_error_samples.len() < 10 {
                        solr_error_samples
                            .push(format!("query={:?} transport_error={detail}", q.query));
                    }
                }
                SolrLookup::ParseError(detail) => {
                    solr_parse_errors += 1;
                    if solr_error_samples.len() < 10 {
                        solr_error_samples
                            .push(format!("query={:?} parse_error={detail}", q.query));
                    }
                }
            }
        }
    }

    let solr_errors = solr_transport_errors + solr_parse_errors;
    if solr_errors > 0 {
        eprintln!(
            "\n=== SOLR HARNESS FAILURE: {solr_errors} of {evaluated_queries} evaluated queries \
             got no legitimate Solr answer ({solr_transport_errors} transport, {solr_parse_errors} \
             parse) -- excluded from the native-vs-Solr comparison below, NOT scored as Solr \
             NDCG=0.0 ==="
        );
        for sample in &solr_error_samples {
            eprintln!("  {sample}");
        }
        eprintln!(
            "Preregistered rule (docs/decisions/ISSUE35_SOLR_HARNESS_HARDENING_DECISION.md): \
             this Solr core is same-host and locally controlled, so any transport/parse failure \
             is a real infrastructure defect, not expected flakiness. Fix the infrastructure and \
             rerun -- the numbers below are NOT a certified comparison."
        );
        std::process::exit(1);
    }

    println!("\n=== discovery/routing (descriptive, no pass/fail threshold) ===");
    println!("routing distribution: {routing_counts:?}");
    println!(
        "queries with ambiguity: {ambiguous_count}/{}, queries with residual lexical text: \
         {residual_count}/{}, queries with a Brand structural constraint: {brand_constrained_count}/{}",
        raw_queries.len(),
        raw_queries.len(),
        raw_queries.len()
    );

    println!("\n=== correctness hard gate ===");
    if wrong_family_violations.is_empty() {
        println!(
            "PASS: zero wrong-family violations across {brand_constrained_count} Brand-constrained queries"
        );
    } else {
        println!(
            "FAIL: {} wrong-family violations found:",
            wrong_family_violations.len()
        );
        for v in wrong_family_violations.iter().take(10) {
            println!("  {v}");
        }
    }

    let native_mean = mean(&native_ndcgs);
    let solr_mean = mean(&solr_ndcgs);
    println!(
        "\n=== relevance (n={evaluated_queries} queries with >=1 non-Irrelevant judgment, \
         of {} total real queries in this slice) ===",
        raw_queries.len()
    );
    println!("native NDCG@10={native_mean:.4}  solr NDCG@10={solr_mean:.4}");
    let relative_gap = if solr_mean > 0.0 {
        100.0 * (native_mean - solr_mean) / solr_mean
    } else {
        0.0
    };
    println!("relative gap (native vs solr): {relative_gap:+.2}%");
    if relative_gap >= -15.0 {
        println!(
            "=== H0: native is within the preregistered <=15% relative gap -- the \
             delegate-fallback path carries real ranking quality on this unseen vertical ==="
        );
    } else {
        println!(
            "=== H1: native is materially worse than Solr (>15% relative gap) on this \
             unseen vertical ==="
        );
    }

    println!("\n=== qualitative sample (first 5 queries with a Brand constraint) ===");
    for q in raw_queries
        .iter()
        .filter(|q| {
            let compiled = compile(&q.query, &lexicon);
            compiled.constraints.iter().any(|c| {
                matches!(
                    c,
                    ResolvedConstraint::Structural(StructuralConstraint::Brand(_))
                )
            })
        })
        .take(5)
    {
        let compiled = compile(&q.query, &lexicon);
        let (_planned, hits) = execute_planned(
            &compiled,
            &ingested.catalog,
            &index,
            Some(&delegate as &dyn LexicalDelegate),
            K,
            &policy,
            None,
        );
        let titles: Vec<&str> = hits
            .iter()
            .take(3)
            .filter_map(|h| title_by_product_id.get(&h.product).copied())
            .collect();
        println!(
            "query={:?} constraints={:?} top-3 titles={:?}",
            q.query, compiled.constraints, titles
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{solr_search, SolrLookup};
    use std::io::{Read, Write};
    use std::net::TcpListener;

    /// Binds a listener, drops it immediately, and returns a base URL
    /// pointing at the now-closed port -- connecting to it deterministically
    /// yields ECONNREFUSED, simulating an unreachable Solr instance.
    fn closed_port_base_url() -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
        let addr = listener.local_addr().expect("local_addr");
        drop(listener);
        format!("http://{addr}/solr/closed_core")
    }

    /// Spawns a one-shot fake Solr that accepts exactly one connection and
    /// replies with a fixed raw HTTP response, then returns its base URL.
    fn fake_solr_base_url(status_line: &'static str, body: &'static str) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
        let addr = listener.local_addr().expect("local_addr");
        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buf = [0u8; 8192];
                let _ = stream.read(&mut buf);
                let response = format!(
                    "{status_line}\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.flush();
            }
        });
        format!("http://{addr}/solr/fake_core")
    }

    #[test]
    fn connection_refused_is_transport_error_not_empty_success() {
        let url = closed_port_base_url();
        match solr_search(&url, "widget", 10) {
            SolrLookup::TransportError(_) => {}
            other => panic!("expected TransportError, got {other:?}"),
        }
    }

    #[test]
    fn invalid_json_body_is_parse_error_not_empty_success() {
        let url = fake_solr_base_url("HTTP/1.1 200 OK", "this is not json {{{");
        match solr_search(&url, "widget", 10) {
            SolrLookup::ParseError(_) => {}
            other => panic!("expected ParseError, got {other:?}"),
        }
    }

    #[test]
    fn missing_response_docs_shape_is_parse_error_not_empty_success() {
        let url = fake_solr_base_url("HTTP/1.1 200 OK", r#"{"responseHeader":{"status":0}}"#);
        match solr_search(&url, "widget", 10) {
            SolrLookup::ParseError(_) => {}
            other => panic!("expected ParseError, got {other:?}"),
        }
    }

    #[test]
    fn nonzero_solr_status_is_transport_error_not_empty_success() {
        let url = fake_solr_base_url(
            "HTTP/1.1 200 OK",
            r#"{"responseHeader":{"status":400},"error":{"msg":"bad query"}}"#,
        );
        match solr_search(&url, "widget", 10) {
            SolrLookup::TransportError(_) => {}
            other => panic!("expected TransportError, got {other:?}"),
        }
    }

    #[test]
    fn valid_empty_docs_is_a_legitimate_success_not_an_error() {
        let url = fake_solr_base_url(
            "HTTP/1.1 200 OK",
            r#"{"responseHeader":{"status":0},"response":{"numFound":0,"start":0,"docs":[]}}"#,
        );
        match solr_search(&url, "widget", 10) {
            SolrLookup::Success(ids) => assert!(ids.is_empty()),
            other => panic!("expected Success(empty), got {other:?}"),
        }
    }

    #[test]
    fn valid_docs_round_trip_the_ids() {
        let url = fake_solr_base_url(
            "HTTP/1.1 200 OK",
            r#"{"responseHeader":{"status":0},"response":{"numFound":2,"start":0,"docs":[{"id":"B001"},{"id":"B002"}]}}"#,
        );
        match solr_search(&url, "widget", 10) {
            SolrLookup::Success(ids) => {
                assert_eq!(ids, vec!["B001".to_string(), "B002".to_string()])
            }
            other => panic!("expected Success, got {other:?}"),
        }
    }
}
