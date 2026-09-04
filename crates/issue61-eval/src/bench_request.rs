use issue61_eval::FrozenQuery;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Instant;

#[derive(Debug, Clone, Copy)]
pub(super) enum RequestEngine {
    Solr,
    Native,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct EngineRequest {
    pub(super) q: String,
    pub(super) fq: Vec<String>,
    pub(super) params: BTreeMap<String, String>,
}

pub(super) fn build_request(
    query: &FrozenQuery,
    engine: RequestEngine,
) -> Result<EngineRequest, String> {
    match engine {
        RequestEngine::Solr => query
            .solr
            .as_ref()
            .map(|request| EngineRequest {
                q: request.q.clone(),
                fq: request.fq.clone(),
                params: request.params.clone(),
            })
            .ok_or_else(|| format!("query {} is missing its solr request block", query.query_id)),
        RequestEngine::Native => query
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
    pub(super) regime: String,
    pub(super) baseline_url: String,
    pub(super) baseline_cgroup: PathBuf,
    pub(super) treatment_url: String,
    pub(super) treatment_cgroup: PathBuf,
    pub(super) blocks: usize,
    pub(super) warmup_passes: usize,
    pub(super) seed: u64,
    pub(super) output: PathBuf,
    pub(super) calibration_passes: usize,
}

fn required_arg(args: &[String], name: &str) -> Result<String, String> {
    args.windows(2)
        .find(|pair| pair[0] == name)
        .map(|pair| pair[1].clone())
        .ok_or_else(|| format!("missing {name}"))
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
        regime: required_arg(args, "--regime")?,
        baseline_url: required_arg(args, "--baseline-url")?,
        baseline_cgroup: required_arg(args, "--baseline-cgroup")?.into(),
        treatment_url: required_arg(args, "--treatment-url")?,
        treatment_cgroup: required_arg(args, "--treatment-cgroup")?.into(),
        blocks: parse_usize("--blocks")?,
        warmup_passes: parse_usize("--warmup-passes")?,
        seed: required_arg(args, "--seed")?
            .parse()
            .map_err(|error| format!("invalid --seed: {error}"))?,
        output: required_arg(args, "--out")?.into(),
        calibration_passes: args
            .windows(2)
            .find(|pair| pair[0] == "--calibration-passes")
            .map(|pair| {
                pair[1]
                    .parse()
                    .map_err(|error| format!("invalid --calibration-passes: {error}"))
            })
            .transpose()?
            .unwrap_or(5),
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
    engine: RequestEngine,
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
    engine: RequestEngine,
) -> Result<Vec<QueryObservation>, String> {
    workload
        .iter()
        .map(|query| query_once(agent, url, query, engine))
        .collect()
}

pub(super) fn repeated_passes(
    agent: &ureq::Agent,
    url: &str,
    workload: &[FrozenQuery],
    passes: usize,
    engine: RequestEngine,
) -> Result<Vec<f64>, String> {
    let mut samples = Vec::with_capacity(workload.len() * passes);
    for _ in 0..passes {
        samples.extend(
            workload_pass(agent, url, workload, engine)?
                .into_iter()
                .map(|observation| observation.latency_us),
        );
    }
    Ok(samples)
}
