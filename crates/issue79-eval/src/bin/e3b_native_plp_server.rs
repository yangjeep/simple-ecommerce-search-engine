//! Issue #79 (Infra E3b) native PLP server: #77's `i77_native_plp_server`
//! HTTP surface and single-connection serving loop, unchanged (the
//! throughput/concurrency confound is explicitly out of scope), serving
//! every E3b variant through `issue79_eval::plp::execute` selected per
//! request by `facet_mode`/`sort_mode`.
//!
//! Optional physical structures are built only when requested at startup,
//! so each launch configuration's RSS and build time can be measured:
//!   --sort-columns <f1,f2|none>   S1 dense value columns
//!   --presence <f1,f2|none>       S2 presence bitmaps
//!   --tau-f <x> / --rho-s <x>     F3/S3 constants (after calibration only)

use commerce_core::domain::CategoryId;
use commerce_core::index::CatalogIndex;
use comparator_eval::translate::StructuralNames;
use issue61_eval::{load_dataset, Dataset, ProcessCpuSnapshot};
use issue77_eval::fixture::{build_fixture_catalog, oracle_queries};
use issue79_eval::plp::{execute, parse_plp_request, PlpContext, SortStructures};
use serde::Serialize;
use std::collections::HashMap;
use std::error::Error;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::time::Instant;

struct ServerState {
    data: issue61_eval::LoadedDataset,
    index: CatalogIndex,
    category_id_by_leaf: HashMap<String, CategoryId>,
    structures: SortStructures,
    tau_f: Option<f64>,
    rho_s: Option<f64>,
    fixture_index: CatalogIndex,
    fixture_label_by_product: HashMap<u64, String>,
}

fn write_http(stream: &mut TcpStream, status: &str, body: &str) -> std::io::Result<()> {
    write!(stream, "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: keep-alive\r\n\r\n{body}", body.len())?;
    stream.flush()
}

fn render_error(message: &str) -> String {
    serde_json::json!({"error": message}).to_string()
}

#[derive(Serialize)]
struct CorrectnessResponse {
    checks: Vec<issue77_eval::CorrectnessCheck>,
    all_passed: bool,
}

/// #77's same-product cross-variant fixture check, unchanged: it goes
/// through `indexed_candidates`, which E3b does not modify.
fn handle_correctness(state: &ServerState) -> String {
    use commerce_core::domain::Constraint;
    use commerce_core::ir::ResolvedConstraint;
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

fn handle_plp(state: &ServerState, target: &str) -> Result<String, String> {
    let req = parse_plp_request(target)?;
    let ctx = PlpContext {
        catalog: &state.data.catalog,
        index: &state.index,
        category_id_by_leaf: &state.category_id_by_leaf,
        source_id_by_product: &state.data.source_id_by_product,
        structures: &state.structures,
        tau_f: state.tau_f,
        rho_s: state.rho_s,
    };
    let response = execute(&ctx, &req)?;
    serde_json::to_string(&response).map_err(|error| error.to_string())
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
                .and_then(|s| serde_json::to_string(&s).map_err(|error| error.to_string()))
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
            match handle_plp(state, target) {
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

fn arg(args: &[String], name: &str) -> Option<String> {
    args.windows(2)
        .find(|pair| pair[0] == name)
        .map(|pair| pair[1].clone())
}

fn field_list(value: Option<String>) -> Vec<String> {
    match value.as_deref() {
        None | Some("none") | Some("") => Vec::new(),
        Some(list) => list.split(',').map(str::to_owned).collect(),
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = std::env::args().collect();
    let catalog_path = PathBuf::from(arg(&args, "--catalog").ok_or("missing --catalog")?);
    let dataset = Dataset::parse(&arg(&args, "--dataset").ok_or("missing --dataset")?)?;
    let port = arg(&args, "--port")
        .ok_or("missing --port")?
        .parse::<u16>()?;
    let sort_columns = field_list(arg(&args, "--sort-columns"));
    let presence = field_list(arg(&args, "--presence"));
    let tau_f = arg(&args, "--tau-f")
        .map(|v| v.parse::<f64>())
        .transpose()?;
    let rho_s = arg(&args, "--rho-s")
        .map(|v| v.parse::<f64>())
        .transpose()?;

    let load_started = Instant::now();
    let data = load_dataset(&catalog_path, dataset)?;
    let load_ms = load_started.elapsed().as_secs_f64() * 1e3;

    let index_started = Instant::now();
    let index = CatalogIndex::build(&data.catalog);
    let index_build_ms = index_started.elapsed().as_secs_f64() * 1e3;

    let columns_started = Instant::now();
    let column_refs: Vec<&str> = sort_columns.iter().map(String::as_str).collect();
    let mut structures = SortStructures::build(&index, &column_refs, &[]);
    let sort_columns_build_ms = columns_started.elapsed().as_secs_f64() * 1e3;
    let presence_started = Instant::now();
    let presence_refs: Vec<&str> = presence.iter().map(String::as_str).collect();
    structures.presence = SortStructures::build(&index, &[], &presence_refs).presence;
    let presence_build_ms = presence_started.elapsed().as_secs_f64() * 1e3;
    let (columns_bytes, presence_bytes) = structures.owned_bytes();

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

    // One machine-readable line; the driver parses it for build/structure
    // accounting (deterministic on-heap estimates, never disk footprint).
    println!(
        "{}",
        serde_json::json!({
            "e3b_ready": true,
            "docs": data.catalog.products.len(),
            "load_ms": load_ms,
            "index_build_ms": index_build_ms,
            "sort_columns_build_ms": sort_columns_build_ms,
            "presence_build_ms": presence_build_ms,
            "index_approx_bytes": index.approximate_size_bytes(),
            "ordinal_facet_approx_bytes": index.approximate_ordinal_facet_bytes(),
            "sort_columns_fields": sort_columns,
            "sort_columns_bytes": columns_bytes,
            "presence_fields": presence,
            "presence_bytes": presence_bytes,
            "tau_f": tau_f,
            "rho_s": rho_s,
        })
    );
    std::io::stdout().flush()?;

    let state = ServerState {
        data,
        index,
        category_id_by_leaf,
        structures,
        tau_f,
        rho_s,
        fixture_index,
        fixture_label_by_product,
    };
    let listener = TcpListener::bind(("0.0.0.0", port))?;
    // Same per-connection error isolation as #77's server; still
    // single-connection-at-a-time by design (out of scope for #79).
    for accepted in listener.incoming() {
        match accepted {
            Ok(stream) => {
                if let Err(error) = serve_connection(stream, &state) {
                    eprintln!("e3b_native_plp_server: connection error: {error}");
                }
            }
            Err(error) => eprintln!("e3b_native_plp_server: accept error: {error}"),
        }
    }
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("e3b_native_plp_server: {error}");
        std::process::exit(1);
    }
}
