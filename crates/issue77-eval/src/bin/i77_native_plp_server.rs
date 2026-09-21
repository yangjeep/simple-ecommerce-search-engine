//! Issue #77 (Infra E3) native PLP/faceting HTTP server.
//!
//! A new binary, deliberately separate from #61's frozen `i61_native_server`
//! (which only exposes `/select?q=...`, a keyword-search contract #61/#73
//! already froze). `commerce_core`'s underlying library
//! (`CatalogIndex::indexed_candidates`/`facet_counts`/generic
//! `Constraint::Enum`/`Numeric`) already has everything #77's workload
//! needs -- this binary is a thin HTTP surface over that existing capability,
//! not a new query engine.
//!
//! Loads two independent catalogs at startup:
//! - Dataset A (WANDS-scale, via `--catalog`/`--dataset`, same loader #61/#62
//!   already use) -- served at `/plp`, the PLP/facet/filter/sort workload.
//! - Dataset B (the small deterministic multi-variant fixture,
//!   `issue77_eval::fixture`) -- served at `/correctness`, gating
//!   product/variant correctness. Never mixed with Dataset A.

use commerce_core::domain::{CategoryId, Constraint};
use commerce_core::index::CatalogIndex;
use commerce_core::ir::{ResolvedConstraint, StructuralConstraint};
use comparator_eval::translate::StructuralNames;
use issue61_eval::{load_dataset, Dataset, LoadedDataset, ProcessCpuSnapshot};
use issue77_eval::fixture::{build_fixture_catalog, oracle_queries};
use serde::Serialize;
use std::collections::HashMap;
use std::error::Error;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;

const MAX_ROWS: usize = 200;
const DEFAULT_ROWS: usize = 48;

struct ServerState {
    data: LoadedDataset,
    index: CatalogIndex,
    category_id_by_leaf: HashMap<String, CategoryId>,
    fixture_catalog: commerce_core::domain::Catalog,
    fixture_index: CatalogIndex,
    fixture_label_by_product: HashMap<u64, String>,
}

fn percent_decode(value: &str) -> Result<String, String> {
    let mut output = Vec::with_capacity(value.len());
    let bytes = value.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'+' => output.push(b' '),
            b'%' if index + 2 < bytes.len() => {
                let encoded = std::str::from_utf8(&bytes[index + 1..index + 3])
                    .map_err(|error| format!("invalid percent escape: {error}"))?;
                output.push(
                    u8::from_str_radix(encoded, 16)
                        .map_err(|_| format!("invalid percent escape %{encoded}"))?,
                );
                index += 2;
            }
            b'%' => return Err("truncated percent escape".to_string()),
            byte => output.push(byte),
        }
        index += 1;
    }
    String::from_utf8(output).map_err(|error| format!("query is not UTF-8: {error}"))
}

fn write_http(stream: &mut TcpStream, status: &str, body: &str) -> std::io::Result<()> {
    write!(stream, "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: keep-alive\r\n\r\n{body}", body.len())?;
    stream.flush()
}

fn render_error(message: &str) -> String {
    serde_json::json!({"error": message}).to_string()
}

// --- /correctness -----------------------------------------------------------

#[derive(Serialize)]
struct CorrectnessResponse {
    checks: Vec<issue77_eval::CorrectnessCheck>,
    all_passed: bool,
}

fn handle_correctness(state: &ServerState) -> String {
    let mut checks = Vec::new();
    let mut all_passed = true;
    for query in oracle_queries() {
        let constraints: Vec<ResolvedConstraint> = query
            .filters
            .iter()
            .map(|(field, value)| {
                ResolvedConstraint::Attribute(if *field == "available" {
                    Constraint::Boolean {
                        attribute: (*field).to_owned(),
                        value: *value == "true",
                    }
                } else {
                    Constraint::Enum {
                        attribute: (*field).to_owned(),
                        value: (*value).to_owned(),
                    }
                })
            })
            .collect();
        let candidates = state.fixture_index.indexed_candidates(&constraints);
        let product_ids = state.fixture_index.candidate_product_ids(&candidates);
        let mut actual: Vec<String> = product_ids
            .iter()
            .filter_map(|pid| state.fixture_label_by_product.get(&pid.0).cloned())
            .collect();
        actual.sort();
        let mut expected: Vec<String> = query
            .expected_product_ids
            .iter()
            .map(|s| (*s).to_owned())
            .collect();
        expected.sort();
        let passed = actual == expected;
        all_passed &= passed;
        checks.push(issue77_eval::CorrectnessCheck {
            query_name: query.name.to_owned(),
            expected_product_ids: expected,
            actual_product_ids: actual,
            passed,
        });
    }
    serde_json::to_string(&CorrectnessResponse { checks, all_passed })
        .unwrap_or_else(|error| render_error(&error.to_string()))
}

// --- /plp --------------------------------------------------------------------

struct PlpRequest {
    category: Option<String>,
    filters: Vec<(String, String)>,
    ranges: Vec<(String, String, f64)>,
    facets: Vec<String>,
    sort: Option<(String, bool)>, // (attribute, descending)
    top_k: usize,
    offset: usize,
}

fn parse_plp_request(target: &str) -> Result<PlpRequest, String> {
    let (path, query_string) = target.split_once('?').unwrap_or((target, ""));
    if path != "/plp" {
        return Err(format!("unsupported path {path}"));
    }
    let mut req = PlpRequest {
        category: None,
        filters: Vec::new(),
        ranges: Vec::new(),
        facets: Vec::new(),
        sort: None,
        top_k: DEFAULT_ROWS,
        offset: 0,
    };
    for pair in query_string.split('&').filter(|p| !p.is_empty()) {
        let (name, raw_value) = pair.split_once('=').unwrap_or((pair, ""));
        let value = percent_decode(raw_value)?;
        match name {
            "category" => req.category = Some(value),
            "filter" => {
                let (attr, val) = value
                    .split_once(':')
                    .ok_or_else(|| format!("malformed filter {value:?}, want attr:value"))?;
                req.filters.push((attr.to_owned(), val.to_owned()));
            }
            "range" => {
                let mut parts = value.splitn(3, ':');
                let attr = parts.next().ok_or("malformed range")?;
                let op = parts.next().ok_or("malformed range")?;
                let val: f64 = parts
                    .next()
                    .ok_or("malformed range")?
                    .parse()
                    .map_err(|_| "malformed range value".to_string())?;
                req.ranges.push((attr.to_owned(), op.to_owned(), val));
            }
            "facets" => req.facets = value.split(',').map(|s| s.to_owned()).collect(),
            "sort" => {
                let (attr, dir) = value.split_once(':').unwrap_or((value.as_str(), "desc"));
                req.sort = Some((attr.to_owned(), dir != "asc"));
            }
            "topk" => {
                req.top_k = value
                    .parse::<usize>()
                    .map_err(|_| "invalid topk".to_string())?
                    .min(MAX_ROWS);
            }
            "offset" => {
                req.offset = value
                    .parse::<usize>()
                    .map_err(|_| "invalid offset".to_string())?;
            }
            _ => {}
        }
    }
    Ok(req)
}

fn numeric_op(op: &str) -> Result<commerce_core::domain::NumericOp, String> {
    use commerce_core::domain::NumericOp;
    match op {
        "eq" => Ok(NumericOp::Eq),
        "lt" => Ok(NumericOp::Lt),
        "lte" => Ok(NumericOp::Lte),
        "gt" => Ok(NumericOp::Gt),
        "gte" => Ok(NumericOp::Gte),
        other => Err(format!("unknown numeric op {other:?}")),
    }
}

/// Builds the resolved constraint list for a request, optionally excluding
/// one attribute's own filter -- the mechanism disjunctive faceting needs:
/// a facet's own reported counts must reflect every *other* active filter
/// but not its own current selection.
fn build_constraints(
    state: &ServerState,
    req: &PlpRequest,
    exclude_attribute: Option<&str>,
) -> Result<Vec<ResolvedConstraint>, String> {
    let mut constraints = Vec::new();
    if let Some(leaf) = &req.category {
        let id = state
            .category_id_by_leaf
            .get(leaf)
            .copied()
            .ok_or_else(|| format!("unknown category_leaf {leaf:?}"))?;
        constraints.push(ResolvedConstraint::Structural(
            StructuralConstraint::Category(id),
        ));
    }
    for (attr, val) in &req.filters {
        if Some(attr.as_str()) == exclude_attribute {
            continue;
        }
        constraints.push(ResolvedConstraint::Attribute(Constraint::Enum {
            attribute: attr.clone(),
            value: val.clone(),
        }));
    }
    for (attr, op, val) in &req.ranges {
        constraints.push(ResolvedConstraint::Attribute(Constraint::Numeric {
            attribute: attr.clone(),
            op: numeric_op(op)?,
            value: *val,
        }));
    }
    Ok(constraints)
}

#[derive(Serialize)]
struct PlpDoc {
    id: String,
    sort_value: Option<f64>,
}

#[derive(Serialize)]
struct PlpResponse {
    num_found: usize,
    docs: Vec<PlpDoc>,
    facets: HashMap<String, std::collections::BTreeMap<String, u64>>,
    /// Native answers one logical PLP request (base retrieval + every
    /// requested disjunctive facet) in exactly one in-process call -- there
    /// is no physical sub-request concept here, unlike competitors that may
    /// need N HTTP calls for N facets. Always 1 for native.
    backend_requests: u32,
}

fn doc_id_for(state: &ServerState, product_id: commerce_core::domain::ProductId) -> String {
    state
        .data
        .source_id_by_product
        .get(&product_id)
        .cloned()
        .unwrap_or_else(|| product_id.0.to_string())
}

fn handle_plp(state: &ServerState, req: &PlpRequest) -> Result<String, String> {
    let base_constraints = build_constraints(state, req, None)?;
    let candidates = state.index.indexed_candidates(&base_constraints);
    let num_found = candidates.len() as usize;

    let mut facets = HashMap::new();
    for facet_attr in &req.facets {
        let constraints_without_this_facet =
            build_constraints(state, req, Some(facet_attr.as_str()))?;
        let candidates_without_this_facet = state
            .index
            .indexed_candidates(&constraints_without_this_facet);
        let counts = state
            .index
            .facet_counts(facet_attr, &candidates_without_this_facet);
        facets.insert(facet_attr.clone(), counts);
    }

    let mut scored: Vec<(commerce_core::domain::ProductId, Option<f64>)> = Vec::new();
    for ord in candidates.iter() {
        let Some(variant_id) = state.index.variant_id_at(ord) else {
            continue;
        };
        let Some((product, variant)) = state.index.lookup_variant(&state.data.catalog, variant_id)
        else {
            continue;
        };
        let sort_value = req.sort.as_ref().and_then(|(attr, _)| {
            let attrs = commerce_core::domain::effective_attributes(product, variant);
            match attrs.get(attr) {
                Some(commerce_core::domain::AttributeValue::Numeric(v)) => Some(*v),
                _ => None,
            }
        });
        scored.push((product.id, sort_value));
    }
    if let Some((_, descending)) = req.sort {
        scored.sort_by(|a, b| {
            let ordering = a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal);
            if descending {
                ordering.reverse()
            } else {
                ordering
            }
        });
    }
    let docs: Vec<PlpDoc> = scored
        .into_iter()
        .skip(req.offset)
        .take(req.top_k)
        .map(|(product_id, sort_value)| PlpDoc {
            id: doc_id_for(state, product_id),
            sort_value,
        })
        .collect();

    serde_json::to_string(&PlpResponse {
        num_found,
        docs,
        facets,
        backend_requests: 1,
    })
    .map_err(|error| error.to_string())
}

fn render_rusage(snapshot: &ProcessCpuSnapshot) -> Result<String, String> {
    serde_json::to_string(snapshot).map_err(|error| format!("serialize rusage response: {error}"))
}

fn serve_connection(mut stream: TcpStream, state: &ServerState) -> Result<(), Box<dyn Error>> {
    let mut reader = BufReader::new(stream.try_clone()?);
    loop {
        let mut request_line = String::new();
        if reader.read_line(&mut request_line)? == 0 {
            return Ok(());
        }
        let mut close = false;
        loop {
            let mut header = String::new();
            reader.read_line(&mut header)?;
            if header == "\r\n" || header.is_empty() {
                break;
            }
            if header.trim().eq_ignore_ascii_case("connection: close") {
                close = true;
            }
        }
        let target = request_line.split_whitespace().nth(1).unwrap_or("");
        if target == "/ping" {
            write_http(&mut stream, "200 OK", r#"{"status":"ok"}"#)?;
        } else if target == "/rusage" {
            match ProcessCpuSnapshot::capture_self()
                .map_err(|error| error.to_string())
                .and_then(|snapshot| render_rusage(&snapshot))
            {
                Ok(body) => write_http(&mut stream, "200 OK", &body)?,
                Err(error) => write_http(
                    &mut stream,
                    "500 Internal Server Error",
                    &render_error(&error),
                )?,
            }
        } else if target == "/correctness" {
            write_http(&mut stream, "200 OK", &handle_correctness(state))?;
        } else {
            match parse_plp_request(target).and_then(|req| handle_plp(state, &req)) {
                Ok(body) => write_http(&mut stream, "200 OK", &body)?,
                Err(error) => write_http(
                    &mut stream,
                    "500 Internal Server Error",
                    &render_error(&error),
                )?,
            }
        }
        if close {
            return Ok(());
        }
    }
}

fn required_arg(args: &[String], name: &str) -> Result<String, String> {
    args.windows(2)
        .find(|pair| pair[0] == name)
        .map(|pair| pair[1].clone())
        .ok_or_else(|| format!("missing {name}"))
}

fn build_state(catalog_path: PathBuf, dataset: Dataset) -> Result<ServerState, Box<dyn Error>> {
    let data = load_dataset(&catalog_path, dataset)?;
    let index = CatalogIndex::build(&data.catalog);
    let category_id_by_leaf: HashMap<String, CategoryId> = data
        .catalog
        .products
        .iter()
        .filter_map(|p| {
            data.category_name(p.category)
                .map(|name| (name.to_owned(), p.category))
        })
        .collect();

    let fixture_catalog = build_fixture_catalog();
    let fixture_index = CatalogIndex::build(&fixture_catalog);
    let fixture_label_by_product: HashMap<u64, String> = fixture_catalog
        .products
        .iter()
        .zip(["A", "B", "C"])
        .map(|(p, label)| (p.id.0, label.to_owned()))
        .collect();

    Ok(ServerState {
        data,
        index,
        category_id_by_leaf,
        fixture_catalog,
        fixture_index,
        fixture_label_by_product,
    })
}

fn run() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = std::env::args().collect();
    let catalog_path = PathBuf::from(required_arg(&args, "--catalog")?);
    let dataset = Dataset::parse(&required_arg(&args, "--dataset")?)?;
    let port = required_arg(&args, "--port")?.parse::<u16>()?;
    let state = build_state(catalog_path, dataset)?;
    println!(
        "NATIVE_PLP_READY docs={} index_bytes={} fixture_docs={}",
        state.data.catalog.products.len(),
        state.index.approximate_size_bytes(),
        state
            .fixture_catalog
            .products
            .iter()
            .map(|p| p.variants.len())
            .sum::<usize>()
    );
    let listener = TcpListener::bind(("0.0.0.0", port))?;
    // Deliberately does NOT propagate a per-connection I/O error (e.g. a
    // broken pipe from one client closing early) up to `main()` -- found
    // live during #77's throughput smoke test: a concurrent-client
    // scenario tripped exactly this, and because the old code used `?`
    // here, ONE client's transport error killed the entire server process,
    // silently failing every other in-flight and future connection (a
    // throughput measurement showed 0 total requests as a result). This
    // server remains genuinely single-threaded/one-connection-at-a-time by
    // design, matching #61's `i61_native_server` precedent -- that
    // characteristic is unchanged and is itself a real, disclosed
    // measurement in #77's throughput results, not something this fix
    // papers over. The fix only stops one connection's own error from
    // taking down every other connection's ability to be served next.
    for accepted in listener.incoming() {
        match accepted {
            Ok(stream) => {
                if let Err(error) = serve_connection(stream, &state) {
                    eprintln!("i77_native_plp_server: connection error: {error}");
                }
            }
            Err(error) => {
                eprintln!("i77_native_plp_server: accept error: {error}");
            }
        }
    }
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("i77_native_plp_server: {error}");
        std::process::exit(1);
    }
}
