use commerce_core::index::CatalogIndex;
use commerce_core::ir::compile;
use issue61_eval::{
    load_dataset, native_candidate_ids, Dataset, LoadedDataset, ProcessCpuSnapshot,
};
use serde::Serialize;
use std::error::Error;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;

const DEFAULT_ROWS: usize = 10;
const MAX_ROWS: usize = 1000;

#[derive(Debug, PartialEq)]
struct SelectRequest {
    query: String,
    rows: usize,
}

#[derive(Serialize)]
struct ResponseHeader {
    status: u16,
}

#[derive(Serialize)]
struct Document<'a> {
    id: &'a str,
}

#[derive(Serialize)]
struct ResponseBody<'a> {
    #[serde(rename = "numFound")]
    num_found: usize,
    docs: Vec<Document<'a>>,
}

#[derive(Serialize)]
struct SuccessResponse<'a> {
    #[serde(rename = "responseHeader")]
    response_header: ResponseHeader,
    response: ResponseBody<'a>,
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

fn parse_select_request(line: &str) -> Result<SelectRequest, String> {
    let mut parts = line.split_whitespace();
    if parts.next() != Some("GET") || parts.next_back() != Some("HTTP/1.1") {
        return Err("expected GET request using HTTP/1.1".to_string());
    }
    let target = parts
        .next()
        .ok_or_else(|| "missing request target".to_string())?;
    let (path, query_string) = target.split_once('?').unwrap_or((target, ""));
    if path != "/select" {
        return Err(format!("unsupported path {path}"));
    }
    let mut query = None;
    let mut rows = DEFAULT_ROWS;
    for pair in query_string.split('&').filter(|pair| !pair.is_empty()) {
        let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
        match name {
            "q" => query = Some(percent_decode(value)?),
            "rows" => {
                rows = value
                    .parse::<usize>()
                    .map_err(|_| format!("invalid rows value {value:?}"))?
                    .min(MAX_ROWS);
            }
            _ => {}
        }
    }
    Ok(SelectRequest {
        query: query.ok_or_else(|| "missing q parameter".to_string())?,
        rows,
    })
}

fn render_success(num_found: usize, ids: &[String]) -> Result<String, String> {
    let response = SuccessResponse {
        response_header: ResponseHeader { status: 0 },
        response: ResponseBody {
            num_found,
            docs: ids.iter().map(|id| Document { id }).collect(),
        },
    };
    serde_json::to_string(&response).map_err(|error| format!("serialize response: {error}"))
}

fn render_error(message: &str) -> String {
    serde_json::json!({"responseHeader":{"status":500},"error":{"msg":message}}).to_string()
}

fn render_rusage(snapshot: &ProcessCpuSnapshot) -> Result<String, String> {
    serde_json::to_string(snapshot).map_err(|error| format!("serialize rusage response: {error}"))
}

fn search(
    request: &SelectRequest,
    data: &LoadedDataset,
    index: &CatalogIndex,
) -> Result<String, String> {
    let mut ids = native_candidate_ids(data, index, &request.query)?;
    let num_found = ids.len();
    ids.truncate(request.rows);
    render_success(num_found, &ids)
}

fn write_http(stream: &mut TcpStream, status: &str, body: &str) -> std::io::Result<()> {
    write!(stream, "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: keep-alive\r\n\r\n{body}", body.len())?;
    stream.flush()
}

fn serve_connection(
    mut stream: TcpStream,
    data: &LoadedDataset,
    index: &CatalogIndex,
) -> Result<(), Box<dyn Error>> {
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
        } else {
            match parse_select_request(request_line.trim_end())
                .and_then(|request| search(&request, data, index))
            {
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

fn selfcheck() -> Result<(), String> {
    let catalog = commerce_core::fixtures::variant_safety_catalog();
    let index = CatalogIndex::build(&catalog);
    let query = compile(
        "black nike waterproof running shoes size 8 under $150",
        &commerce_core::fixtures::shoe_lexicon(),
    );
    if index.execute(&query, &catalog).is_empty() {
        return Err("fixture compile+execute returned no hits".to_string());
    }
    println!("SELFCHECK_OK");
    Ok(())
}

fn required_arg(args: &[String], name: &str) -> Result<String, String> {
    args.windows(2)
        .find(|pair| pair[0] == name)
        .map(|pair| pair[1].clone())
        .ok_or_else(|| format!("missing {name}"))
}

fn run() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|arg| arg == "--selfcheck") {
        return selfcheck().map_err(Into::into);
    }
    let catalog_path = PathBuf::from(required_arg(&args, "--catalog")?);
    let dataset = Dataset::parse(&required_arg(&args, "--dataset")?)?;
    let port = required_arg(&args, "--port")?.parse::<u16>()?;
    let data = load_dataset(&catalog_path, dataset)?;
    let index = CatalogIndex::build(&data.catalog);
    println!(
        "NATIVE_READY docs={} index_bytes={}",
        data.catalog.products.len(),
        index.approximate_size_bytes()
    );
    let listener = TcpListener::bind(("0.0.0.0", port))?;
    for accepted in listener.incoming() {
        serve_connection(accepted?, &data, &index)?;
    }
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("i61_native_server: {error}");
        std::process::exit(1);
    }
}

#[cfg(test)]
#[path = "i61_native_server/tests.rs"]
mod tests;
