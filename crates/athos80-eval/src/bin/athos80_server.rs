//! Loopback-only JSON experiment harness, deliberately NOT production HTTP.
use athos80_eval::{CatalogInput, Engine, Request, Target, CONTRACT};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::os::fd::AsRawFd;
use std::sync::Arc;
use std::time::{Duration, Instant};
const MAX_BODY: usize = 8 * 1024 * 1024;
const MAX_HEADERS: usize = 16 * 1024;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let catalog_path = args.next().ok_or("usage: athos80_server CATALOG.json [--addr 127.0.0.1:18780] [--workers N] [--bm25] [--trace]")?;
    let mut addr: SocketAddr = "127.0.0.1:18780".parse()?;
    let mut workers: usize = 1;
    let mut bm25 = false;
    let mut trace = false;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--addr" => addr = args.next().ok_or("missing --addr")?.parse()?,
            "--workers" => workers = args.next().ok_or("missing --workers")?.parse()?,
            "--bm25" => bm25 = true,
            "--trace" => trace = true,
            _ => return Err(format!("unknown argument {arg}").into()),
        }
    }
    if !addr.ip().is_loopback() {
        return Err("experiment server only binds loopback".into());
    }
    if !(1..=64).contains(&workers) {
        return Err("workers must be in [1,64]".into());
    }
    let input: CatalogInput = serde_json::from_reader(std::fs::File::open(catalog_path)?)?;
    let engine = if bm25 {
        #[cfg(feature = "bm25")]
        {
            Engine::new_with_bm25(input)?
        }
        #[cfg(not(feature = "bm25"))]
        {
            return Err("--bm25 requires build --features bm25".into());
        }
    } else {
        Engine::new(input)?
    };
    let engine = Arc::new(engine);
    let listener = TcpListener::bind(addr)?;
    // POSIX listen() on an already listening socket changes its bounded backlog.
    // No unbounded user-space connection queue exists: fixed workers accept directly.
    if unsafe { libc::listen(listener.as_raw_fd(), 128) } != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    eprintln!(
        "{}",
        json!({"listening":listener.local_addr()?.to_string(),"contract":CONTRACT,"bm25_built":bm25,"trace_enabled":trace,"workers":workers,"build_micros":engine.build_micros})
    );
    let mut handles = Vec::new();
    for _ in 0..workers {
        let listener = listener.try_clone()?;
        let engine = Arc::clone(&engine);
        handles.push(std::thread::spawn(move || {
            for stream in listener.incoming() {
                match stream {
                    Ok(mut stream) => {
                        let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
                        let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));
                        let (status, value) = handle(&mut stream, &engine, trace, bm25, workers);
                        let _ = respond(&mut stream, status, &value);
                    }
                    Err(error) => eprintln!("accept: {error}"),
                }
            }
        }));
    }
    for handle in handles {
        handle.join().map_err(|_| "server worker panicked")?;
    }
    Ok(())
}
fn handle(
    stream: &mut TcpStream,
    engine: &Engine,
    trace: bool,
    bm25: bool,
    workers: usize,
) -> (u16, Value) {
    let (method, path, body) = match read_request(stream) {
        Ok(v) => v,
        Err(e) => return (400, json!({"error":e})),
    };
    if method == "GET" && path == "/health" {
        return (
            200,
            json!({"contract":CONTRACT,"bm25_built":bm25,"trace_enabled":trace,"workers":workers,"build_micros":engine.build_micros,"production":false}),
        );
    }
    if method != "POST" {
        return (405, json!({"error":"use POST /search or GET /health"}));
    }
    if path != "/search" && !(trace && path == "/debug/lexical") {
        return (404, json!({"error":"unknown endpoint"}));
    }
    let request: Request = match serde_json::from_slice(&body) {
        Ok(r) => r,
        Err(e) => return (400, json!({"error":format!("request JSON: {e}")})),
    };
    if let Err(error) = validate_http_request(&request) {
        return (400, json!({"error":error}));
    }
    if path == "/debug/lexical" {
        match engine.lexical_trace(&request) {
            Ok(hits) => (
                200,
                json!({"contract":CONTRACT,"scope":"all lexical variants before filters/hides","hits":hits}),
            ),
            Err(e) => (400, json!({"error":e.to_string()})),
        }
    } else {
        match engine.search(&request) {
            Ok(response) => (
                200,
                serde_json::to_value(response).expect("finite validated response"),
            ),
            Err(e) => (400, json!({"error":e.to_string()})),
        }
    }
}
fn read_request(stream: &mut TcpStream) -> Result<(String, String, Vec<u8>), String> {
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut bytes = Vec::new();
    let mut chunk = [0u8; 4096];
    let header_end = loop {
        let n = read_until_deadline(stream, &mut chunk, deadline)?;
        if n == 0 {
            return Err("connection closed before headers".into());
        }
        bytes.extend_from_slice(&chunk[..n]);
        if let Some(pos) = bytes.windows(4).position(|v| v == b"\r\n\r\n") {
            if pos > MAX_HEADERS {
                return Err("headers too large".into());
            }
            break pos + 4;
        }
        if bytes.len() > MAX_HEADERS {
            return Err("headers too large".into());
        }
    };
    let headers = std::str::from_utf8(&bytes[..header_end]).map_err(|_| "headers must be UTF-8")?;
    let mut lines = headers.split("\r\n");
    let mut first = lines
        .next()
        .ok_or("missing request line")?
        .split_whitespace();
    let method = first.next().ok_or("missing method")?.to_string();
    let path = first.next().ok_or("missing path")?.to_string();
    if first.next() != Some("HTTP/1.1") || first.next().is_some() {
        return Err("HTTP/1.1 required".into());
    }
    let mut length = None;
    for line in lines.filter(|l| !l.is_empty()) {
        let (key, value) = line.split_once(':').ok_or("invalid header")?;
        if key.eq_ignore_ascii_case("transfer-encoding") {
            return Err("chunked/transfer encoding unsupported".into());
        }
        if key.eq_ignore_ascii_case("content-length") {
            if length.is_some() {
                return Err("duplicate content-length".into());
            }
            length = Some(
                value
                    .trim()
                    .parse::<usize>()
                    .map_err(|_| "invalid content-length")?,
            );
        }
    }
    if method == "POST" && length.is_none() {
        return Err("POST requires content-length".into());
    }
    let length = length.unwrap_or(0);
    if length > MAX_BODY {
        return Err("request exceeds 8 MiB limit".into());
    }
    while bytes.len() < header_end + length {
        let n = read_until_deadline(stream, &mut chunk, deadline)?;
        if n == 0 {
            return Err("connection closed before body".into());
        }
        bytes.extend_from_slice(&chunk[..n]);
    }
    Ok((
        method,
        path,
        bytes[header_end..header_end + length].to_vec(),
    ))
}
fn respond(stream: &mut TcpStream, status: u16, value: &Value) -> std::io::Result<()> {
    let body = serde_json::to_vec(value)?;
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        405 => "Method Not Allowed",
        _ => "Error",
    };
    write!(stream,"HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",body.len())?;
    stream.write_all(&body)
}

fn read_until_deadline(
    stream: &mut TcpStream,
    chunk: &mut [u8],
    deadline: Instant,
) -> Result<usize, String> {
    let remaining = deadline
        .checked_duration_since(Instant::now())
        .ok_or("absolute request read deadline exceeded")?;
    stream
        .set_read_timeout(Some(remaining))
        .map_err(|e| e.to_string())?;
    stream.read(chunk).map_err(|e| {
        if matches!(
            e.kind(),
            std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
        ) {
            "absolute request read deadline exceeded".into()
        } else {
            e.to_string()
        }
    })
}
fn validate_http_request(r: &Request) -> Result<(), String> {
    if r.q.len() > 4096
        || r.filters.len() > 64
        || r.rules.len() > 64
        || r.personal_signals.len() > 64
        || r.facets.len() > 16
        || r.limit > 1000
        || r.context.tags.len() > 64
    {
        return Err("HTTP semantic limits: q<=4096 bytes, filters/rules/signals/tags<=64, facets<=16, limit<=1000".into());
    }
    let target_ok = |t: &Target| match t {
        Target::Products { ids } | Target::Variants { ids } => ids.len() <= 4096,
        Target::Filters { filters } => filters.len() <= 32,
        Target::All => true,
    };
    if r.rules
        .iter()
        .any(|v| v.conditions.len() > 16 || !target_ok(&v.target))
        || r.personal_signals.iter().any(|v| !target_ok(&v.target))
    {
        return Err(
            "HTTP semantic limits: conditions/rule<=16, IDs/target<=4096, filters/target<=32"
                .into(),
        );
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn caps_reject_not_truncate() {
        assert!(validate_http_request(&Request::default()).is_ok());
        assert!(validate_http_request(&Request {
            limit: 1001,
            ..Request::default()
        })
        .is_err());
        assert!(validate_http_request(&Request {
            q: "x".repeat(4097),
            ..Request::default()
        })
        .is_err());
    }
}
