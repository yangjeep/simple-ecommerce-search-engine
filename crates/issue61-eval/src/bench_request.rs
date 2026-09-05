use issue61_eval::{Engine, FrozenQuery, SessionMode};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Instant;

#[derive(Debug, PartialEq, Eq)]
pub(super) struct EngineRequest {
    pub(super) q: String,
    pub(super) fq: Vec<String>,
    pub(super) params: BTreeMap<String, String>,
}

pub(super) fn build_request(query: &FrozenQuery, engine: Engine) -> Result<EngineRequest, String> {
    match engine {
        Engine::Solr => query
            .solr
            .as_ref()
            .map(|request| EngineRequest {
                q: request.q.clone(),
                fq: request.fq.clone(),
                params: request.params.clone(),
            })
            .ok_or_else(|| format!("query {} is missing its solr request block", query.query_id)),
        Engine::Native => query
            .native
            .as_ref()
            .map(|request| EngineRequest {
                q: request.q.clone(),
                fq: Vec::new(),
                params: request.params.clone(),
            })
            .ok_or_else(|| {
                format!(
                    "query {} is missing its native request block",
                    query.query_id
                )
            }),
    }
}

#[derive(Deserialize)]
struct WireHeader {
    status: i64,
}

#[derive(Deserialize)]
struct WireDocument {
    id: serde_json::Value,
}

#[derive(Deserialize)]
struct WireBody {
    #[serde(rename = "numFound")]
    num_found: u64,
    docs: Vec<WireDocument>,
}

#[derive(Deserialize)]
struct WireResponse {
    #[serde(rename = "responseHeader")]
    header: WireHeader,
    response: Option<WireBody>,
}

pub(super) struct ValidatedResponse {
    pub(super) num_found: u64,
}

pub(super) fn validate_response(body: &str) -> Result<ValidatedResponse, String> {
    let response: WireResponse =
        serde_json::from_str(body).map_err(|error| format!("unparseable response: {error}"))?;
    if response.header.status != 0 {
        return Err(format!("responseHeader.status={}", response.header.status));
    }
    let payload = response
        .response
        .ok_or_else(|| "missing response".to_string())?;
    for (index, document) in payload.docs.iter().enumerate() {
        if document.id.as_str().is_none() {
            return Err(format!("document {index} has missing/non-string id"));
        }
    }
    Ok(ValidatedResponse {
        num_found: payload.num_found,
    })
}

pub(super) struct Config {
    pub(super) workload: PathBuf,
    pub(super) dataset: String,
    pub(super) engine: Engine,
    pub(super) session_mode: SessionMode,
    pub(super) engine_url: String,
    pub(super) engine_cgroup: PathBuf,
    pub(super) rep: usize,
    pub(super) engine_order: usize,
    pub(super) seed: u64,
    pub(super) output: PathBuf,
}

fn required_arg(args: &[String], name: &str) -> Result<String, String> {
    let mut values = args
        .windows(2)
        .filter(|pair| pair[0] == name)
        .map(|pair| &pair[1]);
    let value = values.next().ok_or_else(|| format!("missing {name}"))?;
    if values.next().is_some() {
        return Err(format!("duplicate {name}"));
    }
    Ok(value.clone())
}

pub(super) fn parse_config(args: &[String]) -> Result<Config, String> {
    let parse_usize = |name| {
        required_arg(args, name)?
            .parse()
            .map_err(|error| format!("invalid {name}: {error}"))
    };
    Ok(Config {
        workload: required_arg(args, "--workload")?.into(),
        dataset: required_arg(args, "--dataset")?,
        engine: required_arg(args, "--engine")?.parse()?,
        session_mode: required_arg(args, "--session-mode")?.parse()?,
        engine_url: required_arg(args, "--engine-url")?,
        engine_cgroup: required_arg(args, "--engine-cgroup")?.into(),
        rep: parse_usize("--rep")?,
        engine_order: parse_usize("--engine-order")?,
        seed: required_arg(args, "--seed")?
            .parse()
            .map_err(|error| format!("invalid --seed: {error}"))?,
        output: required_arg(args, "--out")?.into(),
    })
}

pub(super) struct QueryObservation {
    pub(super) latency_us: f64,
    pub(super) num_found: u64,
}

fn query_once(
    agent: &ureq::Agent,
    base_url: &str,
    query: &FrozenQuery,
    engine: Engine,
) -> Result<QueryObservation, String> {
    let url = format!("{}/select", base_url.trim_end_matches('/'));
    let request = build_request(query, engine)?;
    let started = Instant::now();
    let mut http_request = agent.get(&url).query("q", &request.q);
    for fq in &request.fq {
        http_request = http_request.query("fq", fq);
    }
    for (key, value) in &request.params {
        http_request = http_request.query(key, value);
    }
    let response = http_request
        .call()
        .map_err(|error| format!("HTTP request failed: {error}"))?;
    if response.status() != 200 {
        return Err(format!("HTTP status {}", response.status()));
    }
    let body = response
        .into_string()
        .map_err(|error| format!("read HTTP body: {error}"))?;
    let validated = validate_response(&body)?;
    Ok(QueryObservation {
        latency_us: started.elapsed().as_secs_f64() * 1_000_000.0,
        num_found: validated.num_found,
    })
}

pub(super) fn workload_pass(
    agent: &ureq::Agent,
    url: &str,
    workload: &[FrozenQuery],
    engine: Engine,
) -> Result<Vec<QueryObservation>, String> {
    workload
        .iter()
        .map(|query| query_once(agent, url, query, engine))
        .collect()
}
