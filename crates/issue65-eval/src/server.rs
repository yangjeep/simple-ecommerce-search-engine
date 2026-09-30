//! Issue #65 N1: the minimal concurrent native serving harness (amendment 1
//! section 4).
//!
//! - One immutable [`State`], shared as `Arc<State>`. Nothing in it is
//!   mutated after startup, and the per-request executor (`plp::execute`)
//!   allocates its own scratch (candidate bitmaps, counters), so requests
//!   share no mutable state.
//! - One thread per connection (so keep-alive clients do not serialize), but
//!   native CPU work is gated by a fixed pool of `W` execution slots (a
//!   counting semaphore): at most `W` requests execute at once.
//! - The H1 router is this server's front door: `/solr/...` requests are
//!   forwarded to the Solr delegate over a per-connection keep-alive client
//!   and never hold an execution slot.
//!
//! Deliberately not a production server: no async runtime, no connection
//! pool subsystem, no HA.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use commerce_core::domain::{Catalog, CategoryId, ProductId};
use commerce_core::index::CatalogIndex;
use issue79_eval::plp::{execute, parse_plp_request, PlpContext, SortStructures};

/// Everything a request reads. Built once; immutable afterwards.
pub struct State {
    pub catalog: Catalog,
    pub index: CatalogIndex,
    pub category_id_by_leaf: HashMap<String, CategoryId>,
    pub source_id_by_product: HashMap<ProductId, String>,
    /// Exact lookup (class A): external product id -> product.
    pub product_by_source_id: HashMap<String, ProductId>,
    pub structures: SortStructures,
    pub tau_f: Option<f64>,
    pub rho_s: Option<f64>,
    /// H1 delegate base URL (e.g. `http://127.0.0.1:8985`), if routing.
    pub solr_url: Option<String>,
    /// #77's cross-variant fixture check (binary only).
    pub correctness: Option<Box<dyn Fn() -> String + Send + Sync>>,
}

/// A fixed pool of execution slots.
pub struct Slots {
    free: Mutex<usize>,
    available: Condvar,
}

impl Slots {
    #[must_use]
    pub fn new(count: usize) -> Self {
        Slots {
            free: Mutex::new(count.max(1)),
            available: Condvar::new(),
        }
    }

    fn run<R>(&self, work: impl FnOnce() -> R) -> R {
        {
            let mut free = self.free.lock().expect("slots lock");
            while *free == 0 {
                free = self.available.wait(free).expect("slots wait");
            }
            *free -= 1;
        }
        let result = work();
        *self.free.lock().expect("slots lock") += 1;
        self.available.notify_one();
        result
    }
}

pub struct Request {
    pub method: String,
    pub target: String,
    pub body: Vec<u8>,
    pub close: bool,
}

/// Reads one HTTP/1.1 request; `Ok(None)` at a clean end of stream.
pub fn read_request(reader: &mut BufReader<TcpStream>) -> std::io::Result<Option<Request>> {
    let mut line = String::new();
    if reader.read_line(&mut line)? == 0 {
        return Ok(None);
    }
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or("").to_owned();
    let target = parts.next().unwrap_or("").to_owned();
    let mut close = false;
    let mut length = 0usize;
    loop {
        let mut header = String::new();
        if reader.read_line(&mut header)? == 0 || header == "\r\n" || header == "\n" {
            break;
        }
        let (name, value) = header.split_once(':').unwrap_or((header.as_str(), ""));
        let (name, value) = (name.trim(), value.trim());
        if name.eq_ignore_ascii_case("connection") && value.eq_ignore_ascii_case("close") {
            close = true;
        } else if name.eq_ignore_ascii_case("content-length") {
            length = value.parse().unwrap_or(0);
        }
    }
    let mut body = vec![0u8; length];
    reader.read_exact(&mut body)?;
    Ok(Some(Request {
        method,
        target,
        body,
        close,
    }))
}

fn write_http(
    stream: &mut TcpStream,
    status: &str,
    body: &[u8],
    close: bool,
) -> std::io::Result<()> {
    write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: {}\r\n\r\n",
        body.len(),
        if close { "close" } else { "keep-alive" }
    )?;
    stream.write_all(body)?;
    stream.flush()
}

fn error_json(message: &str) -> Vec<u8> {
    serde_json::json!({ "error": message })
        .to_string()
        .into_bytes()
}

/// Class A: exact product lookup by external id.
pub fn handle_lookup(state: &State, target: &str) -> Result<String, String> {
    let query = target.split_once('?').map_or("", |(_, q)| q);
    let id = query
        .split('&')
        .find_map(|pair| pair.strip_prefix("id="))
        .ok_or("lookup requires id=")?;
    let id = issue79_eval::plp::percent_decode(id)?;
    let docs: Vec<serde_json::Value> = state
        .product_by_source_id
        .get(&id)
        .and_then(|pid| state.index.lookup_product(&state.catalog, *pid))
        .map(|product| {
            vec![serde_json::json!({
                "id": state.source_id_by_product.get(&product.id).cloned().unwrap_or_default()
            })]
        })
        .unwrap_or_default();
    Ok(serde_json::json!({ "num_found": docs.len(), "docs": docs }).to_string())
}

/// Classes B-E: the unchanged #79/#63/#64 `/plp` executor.
pub fn handle_plp(state: &State, target: &str) -> Result<String, String> {
    let req = parse_plp_request(target)?;
    let ctx = PlpContext {
        catalog: &state.catalog,
        index: &state.index,
        category_id_by_leaf: &state.category_id_by_leaf,
        source_id_by_product: &state.source_id_by_product,
        structures: &state.structures,
        tau_f: state.tau_f,
        rho_s: state.rho_s,
    };
    let response = execute(&ctx, &req)?;
    serde_json::to_string(&response).map_err(|e| e.to_string())
}

fn proxy(agent: &ureq::Agent, solr: &str, req: &Request) -> (String, Vec<u8>) {
    let url = format!("{solr}{}", req.target);
    let result = if req.method == "POST" {
        agent
            .post(&url)
            .set("Content-Type", "application/json")
            .send_bytes(&req.body)
    } else {
        agent.get(&url).call()
    };
    match result {
        Ok(resp) => {
            let status = format!("{} {}", resp.status(), resp.status_text());
            let mut body = Vec::new();
            match resp.into_reader().read_to_end(&mut body) {
                Ok(_) => (status, body),
                Err(e) => ("502 Bad Gateway".to_owned(), error_json(&e.to_string())),
            }
        }
        Err(ureq::Error::Status(code, resp)) => (
            format!("{code} Upstream"),
            resp.into_string().unwrap_or_default().into_bytes(),
        ),
        Err(e) => ("502 Bad Gateway".to_owned(), error_json(&e.to_string())),
    }
}

/// Serves one connection until the client closes it (or asks to).
pub fn serve_connection(stream: TcpStream, state: &State, slots: &Slots) -> std::io::Result<()> {
    let _ = stream.set_nodelay(true);
    let mut writer = stream.try_clone()?;
    let mut reader = BufReader::new(stream);
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(10))
        .build();
    while let Some(req) = read_request(&mut reader)? {
        let path = req.target.split('?').next().unwrap_or("");
        let (status, body) = if path.starts_with("/solr/") {
            match &state.solr_url {
                Some(solr) => proxy(&agent, solr, &req),
                None => ("404 Not Found".to_owned(), error_json("no Solr delegate")),
            }
        } else if path == "/noop" {
            ("200 OK".to_owned(), br#"{"ok":true}"#.to_vec())
        } else if path == "/correctness" {
            let body = state
                .correctness
                .as_ref()
                .map_or_else(|| r#"{"all_passed":true,"checks":[]}"#.to_owned(), |f| f());
            ("200 OK".to_owned(), body.into_bytes())
        } else {
            let result = slots.run(|| match path {
                "/lookup" => handle_lookup(state, &req.target),
                "/plp" => handle_plp(state, &req.target),
                other => Err(format!("unsupported path {other}")),
            });
            match result {
                Ok(body) => ("200 OK".to_owned(), body.into_bytes()),
                Err(e) => ("500 Internal Server Error".to_owned(), error_json(&e)),
            }
        };
        write_http(&mut writer, &status, &body, req.close)?;
        if req.close {
            break;
        }
    }
    Ok(())
}

/// Accept loop: one thread per connection, `workers` execution slots.
pub fn serve(listener: TcpListener, state: Arc<State>, workers: usize) {
    let slots = Arc::new(Slots::new(workers));
    for accepted in listener.incoming() {
        match accepted {
            Ok(stream) => {
                let state = Arc::clone(&state);
                let slots = Arc::clone(&slots);
                std::thread::spawn(move || {
                    if let Err(e) = serve_connection(stream, &state, &slots) {
                        if e.kind() != std::io::ErrorKind::UnexpectedEof {
                            eprintln!("i65_server: connection error: {e}");
                        }
                    }
                });
            }
            Err(e) => eprintln!("i65_server: accept error: {e}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn slots_bound_concurrency() {
        let slots = Arc::new(Slots::new(2));
        let running = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let handles: Vec<_> = (0..16)
            .map(|_| {
                let (slots, running, peak) =
                    (Arc::clone(&slots), Arc::clone(&running), Arc::clone(&peak));
                std::thread::spawn(move || {
                    slots.run(|| {
                        let now = running.fetch_add(1, Ordering::SeqCst) + 1;
                        peak.fetch_max(now, Ordering::SeqCst);
                        std::thread::sleep(Duration::from_millis(5));
                        running.fetch_sub(1, Ordering::SeqCst);
                    });
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
        assert_eq!(peak.load(Ordering::SeqCst), 2);
    }
}
