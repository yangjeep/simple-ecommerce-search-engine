//! One (engine, tier, run) measurement cycle for Issue #77 (Infra E3).
//!
//! Usage:
//!   i77_measure --engine <name> --tier <100k|500k|1m> --run <n>
//!               --repository-root <canonical-root> --out <result.json>
//!
//! Reuses #61/#62's generic measurement primitives (`issue61_eval::CgroupReader`,
//! `issue62_eval::run_bounded`) and #62's provision-then-measure lifecycle
//! shape. Provisioning (indexing Dataset A + Dataset B into a fresh
//! container) is delegated to `scripts/issue77/provision_*.sh` for
//! competitors, matching #62's contract; native is launched directly
//! in-process (no bash script), also matching #62.

use issue61_eval::CgroupReader;
use issue62_eval::run_bounded;
use issue77_eval::{Engine, RunStatus, EXPERIMENT_ID, RAW_SCHEMA_VERSION};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const BUILD_TIMEOUT: Duration = Duration::from_secs(3600);
const DOCKER_TIMEOUT: Duration = Duration::from_secs(60);
const WARMUP_QUERY_COUNT: usize = 20;
const MEASURED_QUERY_COUNT: usize = 200;
const THROUGHPUT_CONCURRENCY: usize = 8;
const THROUGHPUT_DURATION: Duration = Duration::from_secs(15);

struct Config {
    engine: Engine,
    tier: String,
    run: u32,
    repository_root: PathBuf,
    out: PathBuf,
}

fn parse_args(args: &[String]) -> Result<Config, String> {
    let mut engine = None;
    let mut tier = None;
    let mut run = None;
    let mut repository_root = None;
    let mut out = None;
    let mut iter = args.iter().skip(1);
    while let Some(flag) = iter.next() {
        let value = iter
            .next()
            .ok_or_else(|| format!("missing value for {flag}"))?;
        match flag.as_str() {
            "--engine" => engine = Some(value.parse::<Engine>()?),
            "--tier" => tier = Some(value.clone()),
            "--run" => run = Some(value.parse::<u32>().map_err(|error| error.to_string())?),
            "--repository-root" => repository_root = Some(PathBuf::from(value)),
            "--out" => out = Some(PathBuf::from(value)),
            other => return Err(format!("unknown flag {other}")),
        }
    }
    Ok(Config {
        engine: engine.ok_or("missing --engine")?,
        tier: tier.ok_or("missing --tier")?,
        run: run.ok_or("missing --run")?,
        repository_root: repository_root.ok_or("missing --repository-root")?,
        out: out.ok_or("missing --out")?,
    })
}

fn read_env_var(repository_root: &Path, key: &str) -> Option<String> {
    let path = repository_root.join("benchmarks/configs/issue77/resource_envelope.env");
    let contents = std::fs::read_to_string(path).ok()?;
    contents.lines().find_map(|line| {
        let line = line.trim();
        let (name, value) = line.split_once('=')?;
        let value = value.trim().trim_matches('"');
        (name.trim() == key).then(|| value.to_owned())
    })
}

fn catalog_path_for_tier(repository_root: &Path, tier: &str) -> Result<PathBuf, String> {
    let key = match tier {
        "100k" => "I77_CATALOG_100K_PATH",
        "500k" => "I77_CATALOG_500K_PATH",
        "1m" => "I77_CATALOG_1M_PATH",
        other => return Err(format!("unknown tier {other:?}")),
    };
    let relative = read_env_var(repository_root, key)
        .ok_or_else(|| format!("{key} missing from resource_envelope.env"))?;
    Ok(repository_root.join(relative))
}

fn docker(args: &[&str]) -> Command {
    let mut command = Command::new("docker");
    command.args(args);
    command
}

fn container_pid(name: &str) -> Option<u32> {
    let output = run_bounded(
        &mut docker(&["inspect", "--format", "{{.State.Pid}}", name]),
        DOCKER_TIMEOUT,
    )
    .ok()?;
    if !output.succeeded() {
        return None;
    }
    output.stdout.trim().parse().ok()
}

// --- Result schema ------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct WorkloadCellResult {
    name: String,
    p50_ms: f64,
    p95_ms: f64,
    p99_ms: f64,
    mean_wall_ms: f64,
    /// Aggregated over the full measured-query batch, never a single-query
    /// delta -- #74's disclosed low-CPU-session process-vs-cgroup
    /// reconciliation precision limitation applies exactly as it did in
    /// #62; batching the denominator across `measured_query_count` queries
    /// is how this harness avoids trusting a microsecond-scale single-query
    /// CPU delta.
    mean_cpu_usec_per_query: Option<f64>,
    mean_backend_requests: f64,
    sample_num_found: u64,
    facet_field_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct ThroughputResult {
    concurrency: usize,
    duration_secs: f64,
    total_requests: u64,
    requests_per_sec: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct MeasurementResult {
    schema_version: u32,
    experiment_id: String,
    engine: String,
    tier: String,
    run: u32,
    status: RunStatus,
    failure_reason: Option<String>,
    correctness_checks: Vec<issue77_eval::CorrectnessCheck>,
    correctness_all_passed: bool,
    workload_cells: Vec<WorkloadCellResult>,
    throughput: Option<ThroughputResult>,
    rss_before_load_bytes: Option<u64>,
    rss_during_serving_bytes: Option<u64>,
    peak_rss_during_serving_bytes: Option<u64>,
    container_name: String,
    cpus: String,
    cpuset: String,
    memory_limit: String,
    jvm_configured_heap_bytes: Option<u64>,
    git_sha: String,
    hostname: String,
    timestamp_utc: String,
}

fn git_sha(repository_root: &Path) -> String {
    Command::new("git")
        .arg("rev-parse")
        .arg("HEAD")
        .current_dir(repository_root)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|value| value.trim().to_owned())
        .unwrap_or_else(|| "unknown".to_owned())
}

fn hostname() -> String {
    std::fs::read_to_string("/proc/sys/kernel/hostname")
        .map(|value| value.trim().to_owned())
        .unwrap_or_else(|_| "unknown".to_owned())
}

fn timestamp_utc() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs().to_string())
        .unwrap_or_else(|_| "unknown".to_owned())
}

fn parse_size_bytes(value: &str) -> Option<u64> {
    let value = value.trim();
    let (digits, multiplier) = match value.chars().last()? {
        'g' | 'G' => (&value[..value.len() - 1], 1024 * 1024 * 1024),
        'm' | 'M' => (&value[..value.len() - 1], 1024 * 1024),
        'k' | 'K' => (&value[..value.len() - 1], 1024),
        _ => (value, 1),
    };
    digits.trim().parse::<u64>().ok().map(|n| n * multiplier)
}

fn base_result(config: &Config) -> MeasurementResult {
    let memory_limit =
        read_env_var(&config.repository_root, "I77_MEMORY").unwrap_or_else(|| "unknown".to_owned());
    let jvm_configured_heap_bytes = if config.engine.is_jvm() {
        read_env_var(&config.repository_root, "I77_JVM_HEAP").and_then(|v| parse_size_bytes(&v))
    } else {
        None
    };
    MeasurementResult {
        schema_version: RAW_SCHEMA_VERSION,
        experiment_id: EXPERIMENT_ID.to_owned(),
        engine: config.engine.as_str().to_owned(),
        tier: config.tier.clone(),
        run: config.run,
        status: RunStatus::HarnessFailure,
        failure_reason: None,
        correctness_checks: Vec::new(),
        correctness_all_passed: false,
        workload_cells: Vec::new(),
        throughput: None,
        rss_before_load_bytes: None,
        rss_during_serving_bytes: None,
        peak_rss_during_serving_bytes: None,
        container_name: config.engine.container_name().to_owned(),
        cpus: read_env_var(&config.repository_root, "I77_CPUS").unwrap_or_default(),
        cpuset: read_env_var(&config.repository_root, "I77_CPUSET").unwrap_or_default(),
        memory_limit,
        jvm_configured_heap_bytes,
        git_sha: git_sha(&config.repository_root),
        hostname: hostname(),
        timestamp_utc: timestamp_utc(),
    }
}

// --- Native lifecycle -----------------------------------------------------

fn launch_native(repository_root: &Path, catalog_path: &Path) -> Result<Instant, String> {
    let _ = docker(&["rm", "-f", Engine::Native.container_name()]).status();
    let binary = repository_root.join("target/release/i77_native_plp_server");
    let dataset_dir = catalog_path
        .parent()
        .ok_or("catalog path has no parent directory")?;
    let catalog_name = catalog_path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("catalog path has no file name")?;
    let memory = read_env_var(repository_root, "I77_MEMORY").unwrap_or_else(|| "6g".to_owned());
    let memory_swap =
        read_env_var(repository_root, "I77_MEMORY_SWAP").unwrap_or_else(|| memory.clone());
    let port =
        read_env_var(repository_root, "I77_NATIVE_PORT").unwrap_or_else(|| "9902".to_owned());
    // CPUs/cpuset must be read live from resource_envelope.env, not hardcoded --
    // an adversarial review of this round found this was hardcoded while every
    // competitor's provisioning script (and native's own memory args, right
    // below) sourced the env file, contradicting the frozen-envelope "no
    // hardcoded exemption anywhere" guarantee (same class of bug E2's review
    // found for memory). The values happened to already match, so this fix
    // changes no measured number, but the guarantee is now actually true.
    let cpus = read_env_var(repository_root, "I77_CPUS").unwrap_or_else(|| "3".to_owned());
    let cpuset = read_env_var(repository_root, "I77_CPUSET").unwrap_or_else(|| "0-2".to_owned());
    let started = Instant::now();
    let mut command = docker(&["run", "-d", "--name", Engine::Native.container_name()]);
    command
        .arg(format!("--cpus={cpus}"))
        .arg(format!("--cpuset-cpus={cpuset}"))
        .arg(format!("--memory={memory}"))
        .arg(format!("--memory-swap={memory_swap}"))
        .arg("-p")
        .arg(format!("{port}:{port}"));
    command
        .arg("-v")
        .arg(format!(
            "{}:/opt/i77_native_plp_server:ro",
            binary.display()
        ))
        .arg("-v")
        .arg(format!("{}:/dataset:ro", dataset_dir.display()))
        .arg("debian@sha256:88200866dfff7ea7f5cbcb6ec7c8a701889efe6fe859fe64d6990e4b07ea4171")
        .arg("/opt/i77_native_plp_server")
        .arg("--catalog")
        .arg(format!("/dataset/{catalog_name}"))
        .arg("--dataset")
        .arg("wands")
        .arg("--port")
        .arg(&port);
    let output = run_bounded(&mut command, DOCKER_TIMEOUT).map_err(|error| error.to_string())?;
    if output.timed_out() || !output.succeeded() {
        return Err(format!("native docker run failed: {}", output.stderr));
    }
    Ok(started)
}

fn wait_native_ready(port: &str, deadline: Instant) -> Result<(), String> {
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(5))
        .build();
    loop {
        if agent
            .get(&format!("http://127.0.0.1:{port}/ping"))
            .set("Connection", "close")
            .call()
            .is_ok()
        {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err("native did not become ready".to_owned());
        }
        std::thread::sleep(Duration::from_millis(500));
    }
}

// --- Native query adapter ---------------------------------------------------

#[derive(Debug, Clone)]
enum WorkloadKind {
    BasePlp {
        category: String,
    },
    /// Not category-scoped -- see resource_envelope.env's comment on why
    /// (the category chosen for Workload A has near-zero attribute
    /// coverage; filter/facet cells need real, verified-non-empty
    /// intersections instead).
    FilterDepth {
        filters: Vec<(String, String)>,
    },
    Facet {
        active_filter: Option<(String, String)>,
        facet_fields: Vec<String>,
    },
    NumericRangeSort {
        range: (String, String, f64),
        sort: (String, bool),
    },
}

#[derive(Debug, Clone)]
struct WorkloadCell {
    name: &'static str,
    kind: WorkloadKind,
    top_k: usize,
}

fn workload_matrix(repository_root: &Path) -> Vec<WorkloadCell> {
    let narrow = read_env_var(repository_root, "I77_CATEGORY_NARROW").unwrap_or_default();
    let medium = read_env_var(repository_root, "I77_CATEGORY_MEDIUM").unwrap_or_default();
    let broad = read_env_var(repository_root, "I77_CATEGORY_BROAD").unwrap_or_default();
    let depth1_color = read_env_var(repository_root, "I77_FILTER_DEPTH1_COLOR").unwrap_or_default();
    let depth3_style = read_env_var(repository_root, "I77_FILTER_DEPTH3_STYLE").unwrap_or_default();
    let depth3_primarymaterial =
        read_env_var(repository_root, "I77_FILTER_DEPTH3_PRIMARYMATERIAL").unwrap_or_default();
    let depth5_shape = read_env_var(repository_root, "I77_FILTER_DEPTH5_SHAPE").unwrap_or_default();
    let depth5_rating: f64 = read_env_var(repository_root, "I77_FILTER_DEPTH5_RATING_GTE")
        .and_then(|v| v.parse().ok())
        .unwrap_or(4.0);
    let facet_color_probe =
        read_env_var(repository_root, "I77_FACET_COLOR_PROBE").unwrap_or_default();

    vec![
        WorkloadCell {
            name: "base_plp_narrow",
            kind: WorkloadKind::BasePlp { category: narrow },
            top_k: 48,
        },
        WorkloadCell {
            name: "base_plp_medium",
            kind: WorkloadKind::BasePlp { category: medium },
            top_k: 48,
        },
        WorkloadCell {
            name: "base_plp_broad",
            kind: WorkloadKind::BasePlp { category: broad },
            top_k: 48,
        },
        // filter_depth_*/facet_*/numeric_range_sort are NOT category-scoped
        // -- see resource_envelope.env's comment: the "broad" category has
        // near-zero attribute coverage, and these values are instead
        // verified directly against the real WANDS corpus to have genuine,
        // non-zero, progressively-narrowing matches (1438 -> 188 -> 3 -> 1
        // in the unscaled 42,994-doc corpus, so 12x that at the 500k tier).
        WorkloadCell {
            name: "filter_depth_1",
            kind: WorkloadKind::FilterDepth {
                filters: vec![("color".to_owned(), depth1_color.clone())],
            },
            top_k: 48,
        },
        WorkloadCell {
            name: "filter_depth_3",
            kind: WorkloadKind::FilterDepth {
                filters: vec![
                    ("color".to_owned(), depth1_color.clone()),
                    ("style".to_owned(), depth3_style.clone()),
                    ("primarymaterial".to_owned(), depth3_primarymaterial.clone()),
                ],
            },
            top_k: 48,
        },
        WorkloadCell {
            name: "filter_depth_5",
            kind: WorkloadKind::FilterDepth {
                filters: vec![
                    ("color".to_owned(), depth1_color),
                    ("style".to_owned(), depth3_style),
                    ("primarymaterial".to_owned(), depth3_primarymaterial),
                    ("shape".to_owned(), depth5_shape),
                ],
            },
            top_k: 48,
        },
        WorkloadCell {
            name: "facet_low_cardinality_style",
            kind: WorkloadKind::Facet {
                active_filter: None,
                facet_fields: vec!["style".to_owned()],
            },
            top_k: 48,
        },
        WorkloadCell {
            name: "facet_medium_cardinality_primarymaterial",
            kind: WorkloadKind::Facet {
                active_filter: None,
                facet_fields: vec!["primarymaterial".to_owned()],
            },
            top_k: 48,
        },
        WorkloadCell {
            name: "facet_high_cardinality_color",
            kind: WorkloadKind::Facet {
                active_filter: None,
                facet_fields: vec!["color".to_owned()],
            },
            top_k: 48,
        },
        WorkloadCell {
            name: "facet_disjunctive_multi_dim",
            kind: WorkloadKind::Facet {
                active_filter: Some(("color".to_owned(), facet_color_probe)),
                facet_fields: vec![
                    "color".to_owned(),
                    "style".to_owned(),
                    "primarymaterial".to_owned(),
                    "material".to_owned(),
                    "shape".to_owned(),
                ],
            },
            top_k: 48,
        },
        WorkloadCell {
            name: "numeric_range_sort",
            kind: WorkloadKind::NumericRangeSort {
                range: ("average_rating".to_owned(), "gte".to_owned(), depth5_rating),
                sort: ("average_rating".to_owned(), true),
            },
            top_k: 48,
        },
    ]
}

fn native_query_string(port: &str, cell: &WorkloadCell) -> String {
    let mut params: Vec<String> = Vec::new();
    let mut category: Option<String> = None;
    match &cell.kind {
        WorkloadKind::BasePlp { category: c } => {
            category = Some(c.clone());
        }
        WorkloadKind::FilterDepth { filters } => {
            for (attr, val) in filters {
                params.push(format!("filter={attr}:{}", urlencode(val)));
            }
        }
        WorkloadKind::Facet {
            active_filter,
            facet_fields,
        } => {
            if let Some((attr, val)) = active_filter {
                params.push(format!("filter={attr}:{}", urlencode(val)));
            }
            params.push(format!("facets={}", facet_fields.join(",")));
        }
        WorkloadKind::NumericRangeSort { range, sort } => {
            params.push(format!("range={}:{}:{}", range.0, range.1, range.2));
            params.push(format!(
                "sort={}:{}",
                sort.0,
                if sort.1 { "desc" } else { "asc" }
            ));
        }
    }
    if let Some(c) = &category {
        params.insert(0, format!("category={}", urlencode(c)));
    }
    params.push(format!("topk={}", cell.top_k));
    format!("http://127.0.0.1:{}/plp?{}", port, params.join("&"))
}

fn urlencode(value: &str) -> String {
    let mut out = String::new();
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

#[derive(Debug, Deserialize)]
struct NativePlpResponse {
    num_found: u64,
    facets: BTreeMap<String, BTreeMap<String, u64>>,
    backend_requests: u32,
}

fn run_native_workload_cell(
    agent: &ureq::Agent,
    port: &str,
    cell: &WorkloadCell,
    cgroup: Option<&CgroupReader>,
) -> Result<WorkloadCellResult, String> {
    let url = native_query_string(port, cell);

    for _ in 0..WARMUP_QUERY_COUNT {
        retry_request(3, || {
            agent
                .get(&url)
                .set("Connection", "close")
                .call()
                .map_err(|error| error.to_string())
                .map(|_| ())
        })?;
    }

    let cpu_before = cgroup.and_then(|r| r.snapshot().ok()).map(|s| s.usage_usec);
    let mut wall_times_ms = Vec::with_capacity(MEASURED_QUERY_COUNT);
    let mut last_response: Option<NativePlpResponse> = None;
    let mut backend_requests_sum: f64 = 0.0;
    for _ in 0..MEASURED_QUERY_COUNT {
        // Retried, but only the successful attempt's wall-clock is
        // recorded -- see `retry_request`'s doc comment.
        let (parsed, elapsed_ms) = retry_request(3, || {
            let started = Instant::now();
            let body = agent
                .get(&url)
                .set("Connection", "close")
                .call()
                .map_err(|error| error.to_string())?
                .into_string()
                .map_err(|error| error.to_string())?;
            let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
            let parsed: NativePlpResponse =
                serde_json::from_str(&body).map_err(|error| error.to_string())?;
            Ok((parsed, elapsed_ms))
        })?;
        wall_times_ms.push(elapsed_ms);
        backend_requests_sum += f64::from(parsed.backend_requests);
        last_response = Some(parsed);
    }
    let cpu_after = cgroup.and_then(|r| r.snapshot().ok()).map(|s| s.usage_usec);
    let mean_cpu_usec_per_query = match (cpu_before, cpu_after) {
        (Some(before), Some(after)) if after >= before => {
            Some((after - before) as f64 / MEASURED_QUERY_COUNT as f64)
        }
        _ => None,
    };

    wall_times_ms.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let percentile = |p: f64| -> f64 {
        let idx = ((p / 100.0) * (wall_times_ms.len() - 1) as f64).round() as usize;
        wall_times_ms[idx.min(wall_times_ms.len() - 1)]
    };
    let mean_wall_ms = wall_times_ms.iter().sum::<f64>() / wall_times_ms.len() as f64;

    Ok(WorkloadCellResult {
        name: cell.name.to_owned(),
        p50_ms: percentile(50.0),
        p95_ms: percentile(95.0),
        p99_ms: percentile(99.0),
        mean_wall_ms,
        mean_cpu_usec_per_query,
        mean_backend_requests: backend_requests_sum / MEASURED_QUERY_COUNT as f64,
        sample_num_found: last_response.as_ref().map(|r| r.num_found).unwrap_or(0),
        facet_field_count: last_response.map(|r| r.facets.len() as u32).unwrap_or(0),
    })
}

fn run_native_correctness(
    agent: &ureq::Agent,
    port: &str,
) -> Result<(Vec<issue77_eval::CorrectnessCheck>, bool), String> {
    #[derive(Deserialize)]
    struct CorrectnessResponse {
        checks: Vec<issue77_eval::CorrectnessCheck>,
        all_passed: bool,
    }
    let body = retry_request(3, || {
        agent
            .get(&format!("http://127.0.0.1:{port}/correctness"))
            .set("Connection", "close")
            .call()
            .map_err(|error| error.to_string())?
            .into_string()
            .map_err(|error| error.to_string())
    })?;
    let parsed: CorrectnessResponse =
        serde_json::from_str(&body).map_err(|error| error.to_string())?;
    Ok((parsed.checks, parsed.all_passed))
}

/// Throughput methodology note (root-caused live during #77's first smoke
/// test, two layered bugs, both fixed):
/// 1. The server's `for accepted in listener.incoming() { serve_connection(accepted?, &state)? }`
///    propagated any single connection's I/O error (a broken pipe from one
///    concurrent client) all the way to `main()`, killing the entire
///    process -- one client's transport hiccup silently took the whole
///    server down for every other connection. Fixed in
///    `i77_native_plp_server.rs`'s `run()` to log and continue per
///    connection instead of propagating.
/// 2. Even after that fix, throughput stayed at 0: this native server is
///    genuinely single-threaded/one-connection-at-a-time by design
///    (matching #61's `i61_native_server` precedent), and the *other*
///    long-lived `ureq::Agent` this same function already uses for
///    correctness/workload-cell measurement kept ONE persistent
///    keep-alive connection open for the server's entire single accept
///    slot -- so the server's accept loop never returned to
///    `listener.incoming()` at all while that earlier connection stayed
///    open, and the throughput test's fresh connections queued forever.
///    Reproduced in isolation with a standalone debug binary (worked fine
///    with no prior connection) vs. the real call site (always 0) before
///    finding this. Fixed by applying `Connection: close` to *every*
///    native request this harness makes (correctness, workload-cell
///    warmup/measured queries, and throughput), not just the throughput
///    test -- applied identically to every engine this harness measures,
///    so no engine gets a connection-reuse advantage/disadvantage baked
///    into the methodology.
fn run_throughput(url: &str) -> ThroughputResult {
    run_throughput_with(|agent| agent.get(url).set("Connection", "close").call().is_ok())
}

/// Retries a request-building closure up to `attempts` times on any error,
/// with a short linear backoff. Disclosed, generic mitigation for a
/// reproducible-in-the-real-binary-but-not-in-isolation "Unexpected EOF" on
/// an early request after this process spawns a provisioning-script child
/// (see the removed `warm_connection` note below for the isolation attempts
/// that ruled out several more specific hypotheses without fully pinning
/// down the OS/ureq-level trigger). Applied uniformly to every engine's
/// correctness/workload HTTP calls, not engine-specific. Retries are cheap
/// (network-error-only, not retried on a successful-but-wrong response) and
/// excluded from every measured latency/CPU metric -- callers only invoke
/// this before the measured section begins, or wrap it themselves outside
/// their own timing window.
fn retry_request<T, F>(attempts: u32, mut f: F) -> Result<T, String>
where
    F: FnMut() -> Result<T, String>,
{
    let mut last_error = String::new();
    for attempt in 0..attempts {
        match f() {
            Ok(value) => return Ok(value),
            Err(error) => {
                last_error = error;
                if attempt + 1 < attempts {
                    std::thread::sleep(Duration::from_millis(300 * u64::from(attempt + 1)));
                }
            }
        }
    }
    Err(format!("failed after {attempts} attempts: {last_error}"))
}

// A `warm_connection` pre-flight GET was tried here and removed: root-caused
// live during #77's ES smoke testing that it was the actual bug, not a fix.
// A bare GET (no `Connection: close`) followed by the real request (which
// does set `Connection: close`) on the same freshly-constructed `ureq::Agent`
// reproducibly failed the *second* request with "Unexpected EOF" -- but
// isolated minimal repros showed the real request alone, as the very first
// call ever on a fresh `Agent`, always succeeded immediately, including
// right after this same process spawned and reaped a provisioning-script
// child (`launch_competitor`). Solr and native never needed a warmup step in
// the first place; it was added here for symmetry and turned out to be the
// only thing causing a failure. No replacement warmup call is needed.

/// Generalized over `run_throughput`: some engines (Solr's JSON Facet API)
/// need a POST with a JSON body to run the exact same query the latency
/// measurement used, not a bare GET -- reusing the GET-only helper for
/// those would silently throughput-test a different, wrong query. `build`
/// is called once per attempted request, once per thread's tight loop.
fn run_throughput_with<F>(build: F) -> ThroughputResult
where
    F: Fn(&ureq::Agent) -> bool + Sync,
{
    let total_requests = std::sync::atomic::AtomicU64::new(0);
    let stop_at = Instant::now() + THROUGHPUT_DURATION;
    let build = &build;
    let total_requests_ref = &total_requests;
    std::thread::scope(|scope| {
        for _ in 0..THROUGHPUT_CONCURRENCY {
            scope.spawn(move || {
                let total_requests = total_requests_ref;
                let agent = ureq::AgentBuilder::new()
                    .timeout(Duration::from_secs(10))
                    .build();
                while Instant::now() < stop_at {
                    if build(&agent) {
                        total_requests.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    }
                }
            });
        }
    });
    let total = total_requests.load(std::sync::atomic::Ordering::Relaxed);
    ThroughputResult {
        concurrency: THROUGHPUT_CONCURRENCY,
        duration_secs: THROUGHPUT_DURATION.as_secs_f64(),
        total_requests: total,
        requests_per_sec: total as f64 / THROUGHPUT_DURATION.as_secs_f64(),
    }
}

fn run_native(
    config: &Config,
    mut result: MeasurementResult,
    catalog_path: &Path,
) -> MeasurementResult {
    let port = read_env_var(&config.repository_root, "I77_NATIVE_PORT")
        .unwrap_or_else(|| "9902".to_owned());
    let started = match launch_native(&config.repository_root, catalog_path) {
        Ok(instant) => instant,
        Err(error) => {
            result.status = RunStatus::HarnessFailure;
            result.failure_reason = Some(error);
            return result;
        }
    };
    let deadline = started + BUILD_TIMEOUT;
    if let Err(error) = wait_native_ready(&port, deadline) {
        result.status = RunStatus::InvalidEnvironmental;
        result.failure_reason = Some(error);
        return result;
    }

    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(30))
        .build();
    match run_native_correctness(&agent, &port) {
        Ok((checks, all_passed)) => {
            result.correctness_checks = checks;
            result.correctness_all_passed = all_passed;
            if !all_passed {
                result.status = RunStatus::CorrectnessFail;
                result.failure_reason =
                    Some("one or more correctness oracle queries failed".to_owned());
                return result;
            }
        }
        Err(error) => {
            result.status = RunStatus::HarnessFailure;
            result.failure_reason = Some(format!("correctness check request failed: {error}"));
            return result;
        }
    }

    let cgroup = container_pid(Engine::Native.container_name()).and_then(|pid| {
        CgroupReader::for_pid(pid, Path::new("/proc"), Path::new("/sys/fs/cgroup")).ok()
    });
    result.rss_before_load_bytes = cgroup.as_ref().and_then(|r| r.read_memory_current().ok());

    let cells = workload_matrix(&config.repository_root);
    let mut peak_rss = result.rss_before_load_bytes.unwrap_or(0);
    for cell in &cells {
        match run_native_workload_cell(&agent, &port, cell, cgroup.as_ref()) {
            Ok(cell_result) => result.workload_cells.push(cell_result),
            Err(error) => {
                result.status = RunStatus::HarnessFailure;
                result.failure_reason =
                    Some(format!("workload cell {} failed: {error}", cell.name));
                return result;
            }
        }
        if let Some(rss) = cgroup.as_ref().and_then(|r| r.read_memory_current().ok()) {
            peak_rss = peak_rss.max(rss);
        }
    }
    result.rss_during_serving_bytes = cgroup.as_ref().and_then(|r| r.read_memory_current().ok());
    result.peak_rss_during_serving_bytes = Some(peak_rss);

    let throughput_cell = &cells[2]; // base_plp_broad, the highest-candidate-set base workload
    let throughput_url = native_query_string(&port, throughput_cell);
    result.throughput = Some(run_throughput(&throughput_url));

    result.status = RunStatus::Ok;
    result
}

// --- Generic competitor provisioning ---------------------------------------

fn provision_script_path(engine: Engine, repository_root: &Path) -> Option<PathBuf> {
    let name = match engine {
        Engine::Native => return None,
        Engine::Solr => "provision_solr.sh",
        Engine::Elasticsearch => "provision_elasticsearch.sh",
        Engine::Opensearch => "provision_opensearch.sh",
        Engine::Typesense => "provision_typesense.sh",
        Engine::Meilisearch => "provision_meilisearch.sh",
        Engine::Vespa => "provision_vespa.sh",
        Engine::Havenask => "provision_havenask.sh",
    };
    Some(repository_root.join("scripts/issue77").join(name))
}

fn expected_docs_for_tier(repository_root: &Path, tier: &str) -> Option<u64> {
    let key = match tier {
        "100k" => "I77_TIER_100K_DOCS",
        "500k" => "I77_TIER_500K_DOCS",
        "1m" => "I77_TIER_1M_DOCS",
        _ => return None,
    };
    read_env_var(repository_root, key).and_then(|v| v.parse().ok())
}

/// Runs `scripts/issue77/provision_<engine>.sh <catalog_path> <expected_docs>`,
/// matching #62's exact provisioning contract: the script's last stdout
/// line is `PROVISION_OK container=<name> docs=<n> index_bytes=<n>`, and
/// the script leaves the container running for post-provisioning
/// measurement (correctness + workload queries) rather than tearing it
/// down itself.
fn launch_competitor(config: &Config, script: &Path, catalog_path: &Path) -> Result<u64, String> {
    let expected_docs = expected_docs_for_tier(&config.repository_root, &config.tier)
        .ok_or_else(|| format!("no expected doc count configured for tier {}", config.tier))?;
    let mut command = Command::new("bash");
    command
        .arg(script)
        .arg(catalog_path)
        .arg(expected_docs.to_string())
        .current_dir(&config.repository_root);
    let output = run_bounded(&mut command, BUILD_TIMEOUT).map_err(|error| error.to_string())?;
    if output.timed_out() || !output.succeeded() {
        return Err(format!(
            "provisioning script exited non-zero: {}",
            output.stderr.trim()
        ));
    }
    let index_bytes = output
        .stdout
        .lines()
        .rev()
        .find_map(|line| line.strip_prefix("PROVISION_OK "))
        .and_then(|rest| {
            rest.split_whitespace()
                .find_map(|token| token.strip_prefix("index_bytes="))
        })
        .and_then(|value| value.parse::<u64>().ok())
        .ok_or_else(|| "no PROVISION_OK line with index_bytes found".to_owned())?;
    Ok(index_bytes)
}

// --- Solr adapter ------------------------------------------------------------

fn solr_correctness(
    agent: &ureq::Agent,
    port: &str,
) -> Result<(Vec<issue77_eval::CorrectnessCheck>, bool), String> {
    let base = format!("http://127.0.0.1:{port}/solr/i77_fixture");
    let mut checks = Vec::new();
    let mut all_passed = true;
    for query in issue77_eval::fixture::oracle_queries() {
        let mut url = format!("{base}/select?q=*:*&rows=10&wt=json&fl=product_id");
        for (field, value) in query.filters {
            url.push_str(&format!(
                "&fq={}",
                urlencode(&format!("{field}:\"{value}\""))
            ));
        }
        let body = retry_request(3, || {
            agent
                .get(&url)
                .set("Connection", "close")
                .call()
                .map_err(|error| error.to_string())?
                .into_string()
                .map_err(|error| error.to_string())
        })?;
        let parsed: serde_json::Value =
            serde_json::from_str(&body).map_err(|error| error.to_string())?;
        let docs = parsed["response"]["docs"]
            .as_array()
            .ok_or("missing response.docs")?;
        let mut actual: Vec<String> = docs
            .iter()
            .filter_map(|d| d["product_id"].as_str().map(|s| s.to_owned()))
            .collect();
        actual.sort();
        actual.dedup();
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
    Ok((checks, all_passed))
}

/// Builds a Solr `/select` JSON-Facet-API request body for one workload
/// cell. Disjunctive faceting: each active filter is tagged
/// `{!tag=<attribute>}`, and the facet for that SAME attribute excludes its
/// own tag via `domain.excludeTags` -- every other facet sees the filter
/// normally (Solr's standard disjunctive-facet idiom). One request answers
/// base retrieval + every requested facet, matching native's
/// backend_requests=1.
fn solr_query_body(cell: &WorkloadCell) -> (String, serde_json::Value) {
    let mut filters: Vec<String> = Vec::new();
    let mut facet_json = serde_json::Map::new();
    let mut sort: Option<String> = None;
    let mut category: Option<String> = None;
    match &cell.kind {
        WorkloadKind::BasePlp { category: c } => category = Some(c.clone()),
        WorkloadKind::FilterDepth { filters: f } => {
            for (attr, val) in f {
                filters.push(format!("{attr}:\"{val}\""));
            }
        }
        WorkloadKind::Facet {
            active_filter,
            facet_fields,
        } => {
            if let Some((attr, val)) = active_filter {
                filters.push(format!("{{!tag={attr}}}{attr}:\"{val}\""));
            }
            for field in facet_fields {
                let mut facet_def =
                    serde_json::json!({"type": "terms", "field": field, "limit": 200});
                if active_filter.as_ref().is_some_and(|(a, _)| a == field) {
                    facet_def["domain"] = serde_json::json!({"excludeTags": [field]});
                }
                facet_json.insert(field.clone(), facet_def);
            }
        }
        WorkloadKind::NumericRangeSort { range, sort: s } => {
            let op = match range.1.as_str() {
                "gte" => format!("[{} TO *]", range.2),
                "lte" => format!("[* TO {}]", range.2),
                "gt" => format!("{{{} TO *}}", range.2),
                "lt" => format!("{{* TO {}}}", range.2),
                _ => format!("{}", range.2),
            };
            filters.push(format!("{}:{}", range.0, op));
            sort = Some(format!("{} {}", s.0, if s.1 { "desc" } else { "asc" }));
        }
    }
    if let Some(c) = &category {
        filters.insert(0, format!("category_leaf:\"{c}\""));
    }
    let mut body = serde_json::json!({
        "query": "*:*",
        "filter": filters,
        "limit": cell.top_k,
        "fields": "id",
    });
    if !facet_json.is_empty() {
        body["facet"] = serde_json::Value::Object(facet_json);
    }
    if let Some(s) = sort {
        body["sort"] = serde_json::Value::String(s);
    }
    (
        "http://127.0.0.1:{PORT}/solr/i77_wands/select".to_owned(),
        body,
    )
}

fn run_solr_workload_cell(
    agent: &ureq::Agent,
    port: &str,
    cell: &WorkloadCell,
    cgroup: Option<&CgroupReader>,
) -> Result<WorkloadCellResult, String> {
    let (url_template, body) = solr_query_body(cell);
    let url = url_template.replace("{PORT}", port);

    for _ in 0..WARMUP_QUERY_COUNT {
        retry_request(3, || {
            agent
                .post(&url)
                .set("Connection", "close")
                .send_json(body.clone())
                .map_err(|error| error.to_string())
                .map(|_| ())
        })?;
    }

    let cpu_before = cgroup.and_then(|r| r.snapshot().ok()).map(|s| s.usage_usec);
    let mut wall_times_ms = Vec::with_capacity(MEASURED_QUERY_COUNT);
    let mut last_num_found = 0u64;
    let mut last_facet_count = 0u32;
    for _ in 0..MEASURED_QUERY_COUNT {
        // Retried, but only the successful attempt's wall-clock is
        // recorded -- see `retry_request`'s doc comment.
        let (parsed, elapsed_ms) = retry_request(3, || {
            let started = Instant::now();
            let resp = agent
                .post(&url)
                .set("Connection", "close")
                .send_json(body.clone())
                .map_err(|error| error.to_string())?;
            let parsed: serde_json::Value = resp.into_json().map_err(|error| error.to_string())?;
            Ok((parsed, started.elapsed().as_secs_f64() * 1000.0))
        })?;
        wall_times_ms.push(elapsed_ms);
        last_num_found = parsed["response"]["numFound"].as_u64().unwrap_or(0);
        last_facet_count = parsed["facets"]
            .as_object()
            .map(|m| m.keys().filter(|k| *k != "count").count() as u32)
            .unwrap_or(0);
    }
    let cpu_after = cgroup.and_then(|r| r.snapshot().ok()).map(|s| s.usage_usec);
    let mean_cpu_usec_per_query = match (cpu_before, cpu_after) {
        (Some(before), Some(after)) if after >= before => {
            Some((after - before) as f64 / MEASURED_QUERY_COUNT as f64)
        }
        _ => None,
    };

    wall_times_ms.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let percentile = |p: f64| -> f64 {
        let idx = ((p / 100.0) * (wall_times_ms.len() - 1) as f64).round() as usize;
        wall_times_ms[idx.min(wall_times_ms.len() - 1)]
    };
    let mean_wall_ms = wall_times_ms.iter().sum::<f64>() / wall_times_ms.len() as f64;

    Ok(WorkloadCellResult {
        name: cell.name.to_owned(),
        p50_ms: percentile(50.0),
        p95_ms: percentile(95.0),
        p99_ms: percentile(99.0),
        mean_wall_ms,
        mean_cpu_usec_per_query,
        mean_backend_requests: 1.0,
        sample_num_found: last_num_found,
        facet_field_count: last_facet_count,
    })
}

fn run_solr(
    config: &Config,
    mut result: MeasurementResult,
    catalog_path: &Path,
    script: &Path,
) -> MeasurementResult {
    let port =
        read_env_var(&config.repository_root, "I77_SOLR_PORT").unwrap_or_else(|| "8985".to_owned());
    match launch_competitor(config, script, catalog_path) {
        Ok(_) => {}
        Err(error) => {
            result.status = RunStatus::HarnessFailure;
            result.failure_reason = Some(error);
            return result;
        }
    }

    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(30))
        .build();
    match solr_correctness(&agent, &port) {
        Ok((checks, all_passed)) => {
            result.correctness_checks = checks;
            result.correctness_all_passed = all_passed;
            if !all_passed {
                result.status = RunStatus::CorrectnessFail;
                result.failure_reason =
                    Some("one or more correctness oracle queries failed".to_owned());
                return result;
            }
        }
        Err(error) => {
            result.status = RunStatus::HarnessFailure;
            result.failure_reason = Some(format!("correctness check request failed: {error}"));
            return result;
        }
    }

    let cgroup = container_pid(Engine::Solr.container_name()).and_then(|pid| {
        CgroupReader::for_pid(pid, Path::new("/proc"), Path::new("/sys/fs/cgroup")).ok()
    });
    result.rss_before_load_bytes = cgroup.as_ref().and_then(|r| r.read_memory_current().ok());

    let cells = workload_matrix(&config.repository_root);
    let mut peak_rss = result.rss_before_load_bytes.unwrap_or(0);
    for cell in &cells {
        match run_solr_workload_cell(&agent, &port, cell, cgroup.as_ref()) {
            Ok(cell_result) => result.workload_cells.push(cell_result),
            Err(error) => {
                result.status = RunStatus::HarnessFailure;
                result.failure_reason =
                    Some(format!("workload cell {} failed: {error}", cell.name));
                return result;
            }
        }
        if let Some(rss) = cgroup.as_ref().and_then(|r| r.read_memory_current().ok()) {
            peak_rss = peak_rss.max(rss);
        }
    }
    result.rss_during_serving_bytes = cgroup.as_ref().and_then(|r| r.read_memory_current().ok());
    result.peak_rss_during_serving_bytes = Some(peak_rss);

    let throughput_cell = &cells[2];
    let (url_template, body) = solr_query_body(throughput_cell);
    let throughput_url = url_template.replace("{PORT}", &port);
    result.throughput = Some(run_throughput_with(|agent| {
        agent
            .post(&throughput_url)
            .set("Connection", "close")
            .send_json(body.clone())
            .is_ok()
    }));

    result.status = RunStatus::Ok;
    result
}

// --- Elasticsearch / OpenSearch adapter (shared: compatible REST API) ------

fn es_correctness(
    agent: &ureq::Agent,
    port: &str,
) -> Result<(Vec<issue77_eval::CorrectnessCheck>, bool), String> {
    let mut checks = Vec::new();
    let mut all_passed = true;
    for query in issue77_eval::fixture::oracle_queries() {
        let must: Vec<serde_json::Value> = query
            .filters
            .iter()
            .map(|(field, value)| {
                if *field == "available" {
                    serde_json::json!({"term": {"available": *value == "true"}})
                } else {
                    serde_json::json!({"term": {*field: *value}})
                }
            })
            .collect();
        let body = serde_json::json!({
            "query": {"bool": {"filter": must}},
            "size": 10,
            "_source": ["product_id"]
        });
        let resp: serde_json::Value = retry_request(3, || {
            agent
                .post(&format!("http://127.0.0.1:{port}/i77_fixture/_search"))
                .set("Connection", "close")
                .send_json(body.clone())
                .map_err(|error| error.to_string())?
                .into_json()
                .map_err(|error| error.to_string())
        })?;
        let hits = resp["hits"]["hits"].as_array().ok_or("missing hits.hits")?;
        let mut actual: Vec<String> = hits
            .iter()
            .filter_map(|h| h["_source"]["product_id"].as_str().map(|s| s.to_owned()))
            .collect();
        actual.sort();
        actual.dedup();
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
    Ok((checks, all_passed))
}

/// Builds an Elasticsearch/OpenSearch `_search` request body for one
/// workload cell. Disjunctive faceting: base structural filters (category,
/// numeric range) go in `query.bool.filter` (restricts both hits and
/// aggregations); attribute-equality filters go in `post_filter` (restricts
/// hits only, so aggregations see the UNfiltered candidate set by default);
/// each facet is wrapped in its own `filter` aggregation that reapplies
/// every *other* active attribute filter except its own -- the standard
/// ES/OpenSearch disjunctive-facet idiom. One request answers base
/// retrieval + every requested facet (backend_requests=1).
fn es_query_body(cell: &WorkloadCell) -> serde_json::Value {
    let mut base_filter: Vec<serde_json::Value> = Vec::new();
    let mut attr_filters: Vec<(String, String)> = Vec::new();
    let mut aggs = serde_json::Map::new();
    let mut sort: Option<serde_json::Value> = None;
    match &cell.kind {
        WorkloadKind::BasePlp { category } => {
            base_filter.push(serde_json::json!({"term": {"category_leaf": category}}));
        }
        WorkloadKind::FilterDepth { filters } => {
            attr_filters = filters.clone();
        }
        WorkloadKind::Facet {
            active_filter,
            facet_fields,
        } => {
            if let Some(f) = active_filter {
                attr_filters.push(f.clone());
            }
            for field in facet_fields {
                let other_filters: Vec<serde_json::Value> = attr_filters
                    .iter()
                    .filter(|(a, _)| a != field)
                    .map(|(a, v)| serde_json::json!({"term": {a.clone(): v.clone()}}))
                    .collect();
                aggs.insert(
                    field.clone(),
                    serde_json::json!({
                        "filter": {"bool": {"filter": other_filters}},
                        "aggs": {"values": {"terms": {"field": field, "size": 200}}}
                    }),
                );
            }
        }
        WorkloadKind::NumericRangeSort { range, sort: s } => {
            let op = range.1.as_str();
            base_filter.push(serde_json::json!({"range": {range.0.clone(): {op: range.2}}}));
            sort = Some(serde_json::json!([{s.0.clone(): if s.1 {"desc"} else {"asc"}}]));
        }
    }
    let post_filter: Vec<serde_json::Value> = attr_filters
        .iter()
        .map(|(a, v)| serde_json::json!({"term": {a.clone(): v.clone()}}))
        .collect();
    let mut body = serde_json::json!({
        "query": {"bool": {"filter": base_filter}},
        "size": cell.top_k,
        "_source": ["id"],
        // ES caps hits.total.value at 10000 by default (its own
        // `track_total_hits` behavior) -- found live comparing ES's
        // facet_high_cardinality_color result (10000) against native/Solr's
        // (128982, the real count) on identical data. Without this, ES's
        // reported num_found is silently wrong for any query matching more
        // than 10000 docs, which several workload cells do at this tier.
        "track_total_hits": true,
    });
    if !post_filter.is_empty() {
        body["post_filter"] = serde_json::json!({"bool": {"filter": post_filter}});
    }
    if !aggs.is_empty() {
        body["aggs"] = serde_json::Value::Object(aggs);
    }
    if let Some(s) = sort {
        body["sort"] = s;
    }
    body
}

fn run_es_workload_cell(
    agent: &ureq::Agent,
    url: &str,
    cell: &WorkloadCell,
    cgroup: Option<&CgroupReader>,
) -> Result<WorkloadCellResult, String> {
    let body = es_query_body(cell);

    for _ in 0..WARMUP_QUERY_COUNT {
        // Every warmup call is retried (cheap; warmup is uncounted anyway):
        // the disclosed early-connection flakiness (see `retry_request`'s
        // doc comment) was observed recurring on the first request of a
        // *new, distinct query/URL* on an `Agent` that had already
        // succeeded on other requests, not only ever on a truly first-ever
        // call -- so every warmup's first attempt gets the same protection.
        retry_request(3, || {
            agent
                .post(url)
                .set("Connection", "close")
                .send_json(body.clone())
                .map_err(|error| error.to_string())
                .map(|_| ())
        })?;
    }

    let cpu_before = cgroup.and_then(|r| r.snapshot().ok()).map(|s| s.usage_usec);
    let mut wall_times_ms = Vec::with_capacity(MEASURED_QUERY_COUNT);
    let mut last_num_found = 0u64;
    let mut last_facet_count = 0u32;
    for _ in 0..MEASURED_QUERY_COUNT {
        // A failed attempt is retried (up to 2 extra tries) but NOT timed --
        // only the wall-clock duration of the attempt that actually
        // succeeds is recorded, so a retry never inflates a reported
        // latency sample. See `retry_request`'s doc comment for why this
        // network flakiness is retried at all rather than treated as fatal.
        let (resp, elapsed_ms) = retry_request(3, || {
            let started = Instant::now();
            let resp: serde_json::Value = agent
                .post(url)
                .set("Connection", "close")
                .send_json(body.clone())
                .map_err(|error| error.to_string())?
                .into_json()
                .map_err(|error| error.to_string())?;
            Ok((resp, started.elapsed().as_secs_f64() * 1000.0))
        })?;
        wall_times_ms.push(elapsed_ms);
        last_num_found = resp["hits"]["total"]["value"].as_u64().unwrap_or(0);
        last_facet_count = resp["aggregations"]
            .as_object()
            .map(|m| m.len() as u32)
            .unwrap_or(0);
    }
    let cpu_after = cgroup.and_then(|r| r.snapshot().ok()).map(|s| s.usage_usec);
    let mean_cpu_usec_per_query = match (cpu_before, cpu_after) {
        (Some(before), Some(after)) if after >= before => {
            Some((after - before) as f64 / MEASURED_QUERY_COUNT as f64)
        }
        _ => None,
    };

    wall_times_ms.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let percentile = |p: f64| -> f64 {
        let idx = ((p / 100.0) * (wall_times_ms.len() - 1) as f64).round() as usize;
        wall_times_ms[idx.min(wall_times_ms.len() - 1)]
    };
    let mean_wall_ms = wall_times_ms.iter().sum::<f64>() / wall_times_ms.len() as f64;

    Ok(WorkloadCellResult {
        name: cell.name.to_owned(),
        p50_ms: percentile(50.0),
        p95_ms: percentile(95.0),
        p99_ms: percentile(99.0),
        mean_wall_ms,
        mean_cpu_usec_per_query,
        mean_backend_requests: 1.0,
        sample_num_found: last_num_found,
        facet_field_count: last_facet_count,
    })
}

fn run_es_like(
    config: &Config,
    mut result: MeasurementResult,
    catalog_path: &Path,
    script: &Path,
    engine: Engine,
    port_key: &str,
) -> MeasurementResult {
    let port = read_env_var(&config.repository_root, port_key).unwrap_or_else(|| "9200".to_owned());
    match launch_competitor(config, script, catalog_path) {
        Ok(_) => {}
        Err(error) => {
            result.status = RunStatus::HarnessFailure;
            result.failure_reason = Some(error);
            return result;
        }
    }

    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(30))
        .build();
    match es_correctness(&agent, &port) {
        Ok((checks, all_passed)) => {
            result.correctness_checks = checks;
            result.correctness_all_passed = all_passed;
            if !all_passed {
                result.status = RunStatus::CorrectnessFail;
                result.failure_reason =
                    Some("one or more correctness oracle queries failed".to_owned());
                return result;
            }
        }
        Err(error) => {
            result.status = RunStatus::HarnessFailure;
            result.failure_reason = Some(format!("correctness check request failed: {error}"));
            return result;
        }
    }

    let cgroup = container_pid(engine.container_name()).and_then(|pid| {
        CgroupReader::for_pid(pid, Path::new("/proc"), Path::new("/sys/fs/cgroup")).ok()
    });
    result.rss_before_load_bytes = cgroup.as_ref().and_then(|r| r.read_memory_current().ok());

    let search_url = format!("http://127.0.0.1:{port}/i77_wands/_search");
    let cells = workload_matrix(&config.repository_root);
    let mut peak_rss = result.rss_before_load_bytes.unwrap_or(0);
    for cell in &cells {
        match run_es_workload_cell(&agent, &search_url, cell, cgroup.as_ref()) {
            Ok(cell_result) => result.workload_cells.push(cell_result),
            Err(error) => {
                result.status = RunStatus::HarnessFailure;
                result.failure_reason =
                    Some(format!("workload cell {} failed: {error}", cell.name));
                return result;
            }
        }
        if let Some(rss) = cgroup.as_ref().and_then(|r| r.read_memory_current().ok()) {
            peak_rss = peak_rss.max(rss);
        }
    }
    result.rss_during_serving_bytes = cgroup.as_ref().and_then(|r| r.read_memory_current().ok());
    result.peak_rss_during_serving_bytes = Some(peak_rss);

    let throughput_body = es_query_body(&cells[2]);
    result.throughput = Some(run_throughput_with(|agent| {
        agent
            .post(&search_url)
            .set("Connection", "close")
            .send_json(throughput_body.clone())
            .is_ok()
    }));

    result.status = RunStatus::Ok;
    result
}

// --- Typesense adapter -------------------------------------------------------

fn typesense_correctness(
    agent: &ureq::Agent,
    port: &str,
    api_key: &str,
) -> Result<(Vec<issue77_eval::CorrectnessCheck>, bool), String> {
    let mut checks = Vec::new();
    let mut all_passed = true;
    for query in issue77_eval::fixture::oracle_queries() {
        let filter_by = query
            .filters
            .iter()
            .map(|(field, value)| format!("{field}:={value}"))
            .collect::<Vec<_>>()
            .join(" && ");
        let url = format!(
            "http://127.0.0.1:{port}/collections/i77_fixture/documents/search?q=*&query_by=product_id&filter_by={}&per_page=10",
            urlencode(&filter_by)
        );
        let resp: serde_json::Value = retry_request(3, || {
            agent
                .get(&url)
                .set("X-TYPESENSE-API-KEY", api_key)
                .set("Connection", "close")
                .call()
                .map_err(|error| error.to_string())?
                .into_json()
                .map_err(|error| error.to_string())
        })?;
        let hits = resp["hits"].as_array().ok_or("missing hits")?;
        let mut actual: Vec<String> = hits
            .iter()
            .filter_map(|h| h["document"]["product_id"].as_str().map(|s| s.to_owned()))
            .collect();
        actual.sort();
        actual.dedup();
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
    Ok((checks, all_passed))
}

/// Typesense has no server-side facet-domain-exclusion mechanism (unlike
/// Solr's `excludeTags`/ES's `post_filter`+per-agg `filter`) for the ONE
/// facet field whose own filter is currently active -- that field genuinely
/// needs its own request with its own filter excluded. Every OTHER
/// requested facet field shares the same filter set as the base hits
/// request (no exclusion needed) and is combined into that one base request
/// via `facet_by=a,b,c`, matching how a maximally-efficient Typesense client
/// would actually query. An earlier version of this function issued one
/// extra request per facet field unconditionally, even when no exclusion
/// was needed -- an adversarial review of this round found that inflated
/// competitor CPU/latency cost and understated native's reported
/// faceting-slowdown multiplier; fixed here. `backend_requests` (1, or 2
/// when one field needs its own excluded request) reports the true minimum
/// count, per the preregistered "sum of all N calls, never just the
/// cheapest one" rule -- N is now the honestly-minimal N, not an inflated one.
fn typesense_urls(cell: &WorkloadCell, port: &str) -> Vec<(String, &'static str)> {
    let base = format!("http://127.0.0.1:{port}/collections/i77_wands/documents/search");
    let mut filters: Vec<(String, String)> = Vec::new();
    let mut facet_fields: Vec<String> = Vec::new();
    let mut active_filter: Option<(String, String)> = None;
    let mut category: Option<String> = None;
    let mut sort_param: Option<String> = None;
    match &cell.kind {
        WorkloadKind::BasePlp { category: c } => category = Some(c.clone()),
        WorkloadKind::FilterDepth { filters: f } => filters = f.clone(),
        WorkloadKind::Facet {
            active_filter: af,
            facet_fields: ff,
        } => {
            active_filter = af.clone();
            facet_fields = ff.clone();
            if let Some(f) = af {
                filters.push(f.clone());
            }
        }
        WorkloadKind::NumericRangeSort { range, sort: s } => {
            let op = match range.1.as_str() {
                "gte" => ">=",
                "lte" => "<=",
                "gt" => ">",
                "lt" => "<",
                other => other,
            };
            filters.push((range.0.clone(), format!("{op}{}", range.2)));
            sort_param = Some(format!("{}:{}", s.0, if s.1 { "desc" } else { "asc" }));
        }
    }
    let render_filter_by = |exclude: Option<&str>| -> String {
        let mut parts: Vec<String> = Vec::new();
        if let Some(c) = &category {
            parts.push(format!("category_leaf:={c}"));
        }
        for (attr, val) in &filters {
            if Some(attr.as_str()) == exclude {
                continue;
            }
            if val.starts_with(|c: char| "><=".contains(c)) {
                parts.push(format!("{attr}:{val}"));
            } else {
                parts.push(format!("{attr}:={val}"));
            }
        }
        parts.join(" && ")
    };

    let excluded_field = active_filter
        .as_ref()
        .map(|(a, _)| a.as_str())
        .filter(|field| facet_fields.iter().any(|f| f == field));
    let combined_facets: Vec<&String> = facet_fields
        .iter()
        .filter(|f| Some(f.as_str()) != excluded_field)
        .collect();

    let mut urls = Vec::new();
    // Base request: returns hits (topk docs), num_found, plus every facet
    // field that doesn't need its own filter excluded (combined via
    // facet_by=a,b,c -- one request serves hits and N-1 of the N facets).
    let mut base_url = format!(
        "{base}?q=*&query_by=title&filter_by={}&per_page={}",
        urlencode(&render_filter_by(None)),
        cell.top_k
    );
    if !combined_facets.is_empty() {
        let fields = combined_facets
            .iter()
            .map(|f| f.as_str())
            .collect::<Vec<_>>()
            .join(",");
        base_url.push_str(&format!(
            "&facet_by={}&max_facet_values=200",
            urlencode(&fields)
        ));
    }
    if let Some(s) = &sort_param {
        base_url.push_str(&format!("&sort_by={}", urlencode(s)));
    }
    urls.push((base_url, "base"));
    // Only the one field whose own filter is currently active needs its own
    // separate, exclusion-applied request.
    if let Some(field) = excluded_field {
        let facet_url = format!(
            "{base}?q=*&query_by=title&filter_by={}&facet_by={field}&max_facet_values=200&per_page=0",
            urlencode(&render_filter_by(Some(field)))
        );
        urls.push((facet_url, "facet"));
    }
    urls
}

fn run_typesense_workload_cell(
    agent: &ureq::Agent,
    port: &str,
    api_key: &str,
    cell: &WorkloadCell,
    cgroup: Option<&CgroupReader>,
) -> Result<WorkloadCellResult, String> {
    let urls = typesense_urls(cell, port);

    for _ in 0..WARMUP_QUERY_COUNT {
        for (url, _kind) in &urls {
            retry_request(3, || {
                agent
                    .get(url)
                    .set("X-TYPESENSE-API-KEY", api_key)
                    .set("Connection", "close")
                    .call()
                    .map_err(|error| error.to_string())
                    .map(|_| ())
            })?;
        }
    }

    let cpu_before = cgroup.and_then(|r| r.snapshot().ok()).map(|s| s.usage_usec);
    let mut wall_times_ms = Vec::with_capacity(MEASURED_QUERY_COUNT);
    let mut last_num_found = 0u64;
    for _ in 0..MEASURED_QUERY_COUNT {
        let (num_found, elapsed_ms) = retry_request(3, || {
            let started = Instant::now();
            let mut num_found = 0u64;
            for (url, kind) in &urls {
                let resp: serde_json::Value = agent
                    .get(url)
                    .set("X-TYPESENSE-API-KEY", api_key)
                    .set("Connection", "close")
                    .call()
                    .map_err(|error| error.to_string())?
                    .into_json()
                    .map_err(|error| error.to_string())?;
                if *kind == "base" {
                    num_found = resp["found"].as_u64().unwrap_or(0);
                }
            }
            Ok((num_found, started.elapsed().as_secs_f64() * 1000.0))
        })?;
        wall_times_ms.push(elapsed_ms);
        last_num_found = num_found;
    }
    let cpu_after = cgroup.and_then(|r| r.snapshot().ok()).map(|s| s.usage_usec);
    let mean_cpu_usec_per_query = match (cpu_before, cpu_after) {
        (Some(before), Some(after)) if after >= before => {
            Some((after - before) as f64 / MEASURED_QUERY_COUNT as f64)
        }
        _ => None,
    };

    wall_times_ms.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let percentile = |p: f64| -> f64 {
        let idx = ((p / 100.0) * (wall_times_ms.len() - 1) as f64).round() as usize;
        wall_times_ms[idx.min(wall_times_ms.len() - 1)]
    };
    let mean_wall_ms = wall_times_ms.iter().sum::<f64>() / wall_times_ms.len() as f64;
    // facet_field_count is the number of facet FIELDS computed, not the
    // number of backend requests -- since the fix above combines multiple
    // facet fields into one request where no exclusion is needed, these two
    // counts are no longer always equal.
    let facet_field_count = match &cell.kind {
        WorkloadKind::Facet { facet_fields, .. } => facet_fields.len() as u32,
        _ => 0,
    };

    Ok(WorkloadCellResult {
        name: cell.name.to_owned(),
        p50_ms: percentile(50.0),
        p95_ms: percentile(95.0),
        p99_ms: percentile(99.0),
        mean_wall_ms,
        mean_cpu_usec_per_query,
        mean_backend_requests: urls.len() as f64,
        sample_num_found: last_num_found,
        facet_field_count,
    })
}

fn run_typesense(
    config: &Config,
    mut result: MeasurementResult,
    catalog_path: &Path,
    script: &Path,
) -> MeasurementResult {
    let port = read_env_var(&config.repository_root, "I77_TYPESENSE_PORT")
        .unwrap_or_else(|| "8109".to_owned());
    let api_key = read_env_var(&config.repository_root, "I77_TYPESENSE_API_KEY")
        .unwrap_or_else(|| "i77-benchmark-key".to_owned());
    match launch_competitor(config, script, catalog_path) {
        Ok(_) => {}
        Err(error) => {
            result.status = RunStatus::HarnessFailure;
            result.failure_reason = Some(error);
            return result;
        }
    }

    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(30))
        .build();
    match typesense_correctness(&agent, &port, &api_key) {
        Ok((checks, all_passed)) => {
            result.correctness_checks = checks;
            result.correctness_all_passed = all_passed;
            if !all_passed {
                result.status = RunStatus::CorrectnessFail;
                result.failure_reason =
                    Some("one or more correctness oracle queries failed".to_owned());
                return result;
            }
        }
        Err(error) => {
            result.status = RunStatus::HarnessFailure;
            result.failure_reason = Some(format!("correctness check request failed: {error}"));
            return result;
        }
    }

    let cgroup = container_pid(Engine::Typesense.container_name()).and_then(|pid| {
        CgroupReader::for_pid(pid, Path::new("/proc"), Path::new("/sys/fs/cgroup")).ok()
    });
    result.rss_before_load_bytes = cgroup.as_ref().and_then(|r| r.read_memory_current().ok());

    let cells = workload_matrix(&config.repository_root);
    let mut peak_rss = result.rss_before_load_bytes.unwrap_or(0);
    for cell in &cells {
        match run_typesense_workload_cell(&agent, &port, &api_key, cell, cgroup.as_ref()) {
            Ok(cell_result) => result.workload_cells.push(cell_result),
            Err(error) => {
                result.status = RunStatus::HarnessFailure;
                result.failure_reason =
                    Some(format!("workload cell {} failed: {error}", cell.name));
                return result;
            }
        }
        if let Some(rss) = cgroup.as_ref().and_then(|r| r.read_memory_current().ok()) {
            peak_rss = peak_rss.max(rss);
        }
    }
    result.rss_during_serving_bytes = cgroup.as_ref().and_then(|r| r.read_memory_current().ok());
    result.peak_rss_during_serving_bytes = Some(peak_rss);

    let throughput_urls = typesense_urls(&cells[2], &port);
    let throughput_url = throughput_urls[0].0.clone();
    result.throughput = Some(run_throughput_with(|agent| {
        agent
            .get(&throughput_url)
            .set("X-TYPESENSE-API-KEY", &api_key)
            .set("Connection", "close")
            .call()
            .is_ok()
    }));

    result.status = RunStatus::Ok;
    result
}

// --- Meilisearch adapter -----------------------------------------------------

fn meili_correctness(
    agent: &ureq::Agent,
    port: &str,
) -> Result<(Vec<issue77_eval::CorrectnessCheck>, bool), String> {
    let mut checks = Vec::new();
    let mut all_passed = true;
    for query in issue77_eval::fixture::oracle_queries() {
        let filter = query
            .filters
            .iter()
            .map(|(field, value)| {
                if *field == "available" {
                    format!("{field} = {value}")
                } else {
                    format!("{field} = '{value}'")
                }
            })
            .collect::<Vec<_>>()
            .join(" AND ");
        let body = serde_json::json!({"filter": filter, "limit": 10});
        let resp: serde_json::Value = retry_request(3, || {
            agent
                .post(&format!(
                    "http://127.0.0.1:{port}/indexes/i77_fixture/search"
                ))
                .set("Connection", "close")
                .send_json(body.clone())
                .map_err(|error| error.to_string())?
                .into_json()
                .map_err(|error| error.to_string())
        })?;
        let hits = resp["hits"].as_array().ok_or("missing hits")?;
        let mut actual: Vec<String> = hits
            .iter()
            .filter_map(|h| h["product_id"].as_str().map(|s| s.to_owned()))
            .collect();
        actual.sort();
        actual.dedup();
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
    Ok((checks, all_passed))
}

/// Meilisearch's `facets` search param returns `facetDistribution` computed
/// over the filtered result set, with no built-in mechanism to exclude a
/// facet's own active filter (no ES `post_filter`/Solr `excludeTags`
/// equivalent) for the ONE field whose own filter is active -- same real
/// limitation as Typesense, handled the same way (see `typesense_urls`'s
/// doc comment): every facet field that does NOT need its own filter
/// excluded is combined into the base request's `facets` array (Meilisearch
/// accepts multiple facet fields in one `facets: [...]` list), and only the
/// field needing exclusion gets its own separate request. `backend_requests`
/// (1, or 2 when one field needs exclusion) reports the true minimum.
fn meili_bodies(cell: &WorkloadCell) -> Vec<(serde_json::Value, &'static str)> {
    let mut filters: Vec<(String, String)> = Vec::new();
    let mut facet_fields: Vec<String> = Vec::new();
    let mut active_filter: Option<(String, String)> = None;
    let mut category: Option<String> = None;
    let mut sort: Option<Vec<String>> = None;
    match &cell.kind {
        WorkloadKind::BasePlp { category: c } => category = Some(c.clone()),
        WorkloadKind::FilterDepth { filters: f } => filters = f.clone(),
        WorkloadKind::Facet {
            active_filter: af,
            facet_fields: ff,
        } => {
            active_filter = af.clone();
            facet_fields = ff.clone();
            if let Some(f) = af {
                filters.push(f.clone());
            }
        }
        WorkloadKind::NumericRangeSort { range, sort: s } => {
            let op = match range.1.as_str() {
                "gte" => ">=",
                "lte" => "<=",
                "gt" => ">",
                "lt" => "<",
                other => other,
            };
            filters.push((range.0.clone(), format!("{op} {}", range.2)));
            sort = Some(vec![format!(
                "{}:{}",
                s.0,
                if s.1 { "desc" } else { "asc" }
            )]);
        }
    }
    let render_filter = |exclude: Option<&str>| -> String {
        let mut parts: Vec<String> = Vec::new();
        if let Some(c) = &category {
            parts.push(format!("category_leaf = '{c}'"));
        }
        for (attr, val) in &filters {
            if Some(attr.as_str()) == exclude {
                continue;
            }
            if val.starts_with(|c: char| "><=".contains(c)) {
                parts.push(format!("{attr} {val}"));
            } else {
                parts.push(format!("{attr} = '{val}'"));
            }
        }
        parts.join(" AND ")
    };

    let excluded_field = active_filter
        .as_ref()
        .map(|(a, _)| a.as_str())
        .filter(|field| facet_fields.iter().any(|f| f == field));
    let combined_facets: Vec<&String> = facet_fields
        .iter()
        .filter(|f| Some(f.as_str()) != excluded_field)
        .collect();

    let mut bodies = Vec::new();
    let mut base = serde_json::json!({
        "filter": render_filter(None),
        "limit": cell.top_k,
    });
    if !combined_facets.is_empty() {
        base["facets"] = serde_json::json!(combined_facets);
    }
    if let Some(s) = &sort {
        base["sort"] = serde_json::json!(s);
    }
    bodies.push((base, "base"));
    if let Some(field) = excluded_field {
        let facet_body = serde_json::json!({
            "filter": render_filter(Some(field)),
            "facets": [field],
            "limit": 0,
        });
        bodies.push((facet_body, "facet"));
    }
    bodies
}

fn run_meili_workload_cell(
    agent: &ureq::Agent,
    port: &str,
    cell: &WorkloadCell,
    cgroup: Option<&CgroupReader>,
) -> Result<WorkloadCellResult, String> {
    let bodies = meili_bodies(cell);
    let url = format!("http://127.0.0.1:{port}/indexes/i77_wands/search");

    for _ in 0..WARMUP_QUERY_COUNT {
        for (body, _kind) in &bodies {
            retry_request(3, || {
                agent
                    .post(&url)
                    .set("Connection", "close")
                    .send_json(body.clone())
                    .map_err(|error| error.to_string())
                    .map(|_| ())
            })?;
        }
    }

    let cpu_before = cgroup.and_then(|r| r.snapshot().ok()).map(|s| s.usage_usec);
    let mut wall_times_ms = Vec::with_capacity(MEASURED_QUERY_COUNT);
    let mut last_num_found = 0u64;
    for _ in 0..MEASURED_QUERY_COUNT {
        let (num_found, elapsed_ms) = retry_request(3, || {
            let started = Instant::now();
            let mut num_found = 0u64;
            for (body, kind) in &bodies {
                let resp: serde_json::Value = agent
                    .post(&url)
                    .set("Connection", "close")
                    .send_json(body.clone())
                    .map_err(|error| error.to_string())?
                    .into_json()
                    .map_err(|error| error.to_string())?;
                if *kind == "base" {
                    num_found = resp["estimatedTotalHits"].as_u64().unwrap_or(0);
                }
            }
            Ok((num_found, started.elapsed().as_secs_f64() * 1000.0))
        })?;
        wall_times_ms.push(elapsed_ms);
        last_num_found = num_found;
    }
    let cpu_after = cgroup.and_then(|r| r.snapshot().ok()).map(|s| s.usage_usec);
    let mean_cpu_usec_per_query = match (cpu_before, cpu_after) {
        (Some(before), Some(after)) if after >= before => {
            Some((after - before) as f64 / MEASURED_QUERY_COUNT as f64)
        }
        _ => None,
    };

    wall_times_ms.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let percentile = |p: f64| -> f64 {
        let idx = ((p / 100.0) * (wall_times_ms.len() - 1) as f64).round() as usize;
        wall_times_ms[idx.min(wall_times_ms.len() - 1)]
    };
    let mean_wall_ms = wall_times_ms.iter().sum::<f64>() / wall_times_ms.len() as f64;
    let facet_field_count = match &cell.kind {
        WorkloadKind::Facet { facet_fields, .. } => facet_fields.len() as u32,
        _ => 0,
    };

    Ok(WorkloadCellResult {
        name: cell.name.to_owned(),
        p50_ms: percentile(50.0),
        p95_ms: percentile(95.0),
        p99_ms: percentile(99.0),
        mean_wall_ms,
        mean_cpu_usec_per_query,
        mean_backend_requests: bodies.len() as f64,
        sample_num_found: last_num_found,
        facet_field_count,
    })
}

fn run_meilisearch(
    config: &Config,
    mut result: MeasurementResult,
    catalog_path: &Path,
    script: &Path,
) -> MeasurementResult {
    let port = read_env_var(&config.repository_root, "I77_MEILISEARCH_PORT")
        .unwrap_or_else(|| "7701".to_owned());
    match launch_competitor(config, script, catalog_path) {
        Ok(_) => {}
        Err(error) => {
            result.status = RunStatus::HarnessFailure;
            result.failure_reason = Some(error);
            return result;
        }
    }

    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(30))
        .build();
    match meili_correctness(&agent, &port) {
        Ok((checks, all_passed)) => {
            result.correctness_checks = checks;
            result.correctness_all_passed = all_passed;
            if !all_passed {
                result.status = RunStatus::CorrectnessFail;
                result.failure_reason =
                    Some("one or more correctness oracle queries failed".to_owned());
                return result;
            }
        }
        Err(error) => {
            result.status = RunStatus::HarnessFailure;
            result.failure_reason = Some(format!("correctness check request failed: {error}"));
            return result;
        }
    }

    let cgroup = container_pid(Engine::Meilisearch.container_name()).and_then(|pid| {
        CgroupReader::for_pid(pid, Path::new("/proc"), Path::new("/sys/fs/cgroup")).ok()
    });
    result.rss_before_load_bytes = cgroup.as_ref().and_then(|r| r.read_memory_current().ok());

    let cells = workload_matrix(&config.repository_root);
    let mut peak_rss = result.rss_before_load_bytes.unwrap_or(0);
    for cell in &cells {
        match run_meili_workload_cell(&agent, &port, cell, cgroup.as_ref()) {
            Ok(cell_result) => result.workload_cells.push(cell_result),
            Err(error) => {
                result.status = RunStatus::HarnessFailure;
                result.failure_reason =
                    Some(format!("workload cell {} failed: {error}", cell.name));
                return result;
            }
        }
        if let Some(rss) = cgroup.as_ref().and_then(|r| r.read_memory_current().ok()) {
            peak_rss = peak_rss.max(rss);
        }
    }
    result.rss_during_serving_bytes = cgroup.as_ref().and_then(|r| r.read_memory_current().ok());
    result.peak_rss_during_serving_bytes = Some(peak_rss);

    let throughput_bodies = meili_bodies(&cells[2]);
    let throughput_body = throughput_bodies[0].0.clone();
    let throughput_url = format!("http://127.0.0.1:{port}/indexes/i77_wands/search");
    result.throughput = Some(run_throughput_with(|agent| {
        agent
            .post(&throughput_url)
            .set("Connection", "close")
            .send_json(throughput_body.clone())
            .is_ok()
    }));

    result.status = RunStatus::Ok;
    result
}

// --- Vespa adapter -----------------------------------------------------------

fn vespa_yql_escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn vespa_correctness(
    agent: &ureq::Agent,
    query_url: &str,
) -> Result<(Vec<issue77_eval::CorrectnessCheck>, bool), String> {
    let mut checks = Vec::new();
    let mut all_passed = true;
    for query in issue77_eval::fixture::oracle_queries() {
        let conditions: Vec<String> = query
            .filters
            .iter()
            .map(|(field, value)| {
                if *field == "available" {
                    format!("{field} = {value}")
                } else {
                    format!("{field} contains \"{}\"", vespa_yql_escape(value))
                }
            })
            .collect();
        let where_clause = if conditions.is_empty() {
            "true".to_owned()
        } else {
            conditions.join(" and ")
        };
        let yql = format!("select * from fixture where {where_clause}");
        let url = format!("{query_url}/search/?yql={}&hits=10", urlencode(&yql));
        let resp: serde_json::Value = retry_request(3, || {
            agent
                .get(&url)
                .set("Connection", "close")
                .call()
                .map_err(|error| error.to_string())?
                .into_json()
                .map_err(|error| error.to_string())
        })?;
        let empty = Vec::new();
        let children = resp["root"]["children"].as_array().unwrap_or(&empty);
        let mut actual: Vec<String> = children
            .iter()
            .filter_map(|c| c["fields"]["product_id"].as_str().map(|s| s.to_owned()))
            .collect();
        actual.sort();
        actual.dedup();
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
    Ok((checks, all_passed))
}

/// Vespa's grouping API (`| all(group(field) each(output(count())))`) has no
/// built-in mechanism to exclude a facet's own active filter from its own
/// domain (no Solr `excludeTags`/ES `post_filter` equivalent available
/// through a single request) for the ONE field whose own filter is active --
/// same real limitation as Typesense/Meilisearch, handled the same way (see
/// `typesense_urls`'s doc comment): every facet field that does NOT need its
/// own filter excluded is combined into the base request via multiple
/// sibling `all(group(...) each(output(count())))` blocks (Vespa supports
/// several independent groupings in one query), and only the field needing
/// exclusion gets its own separate request. `backend_requests` (1, or 2 when
/// one field needs exclusion) reports the true minimum.
fn vespa_yqls(cell: &WorkloadCell) -> Vec<(String, &'static str)> {
    let mut filters: Vec<(String, String)> = Vec::new();
    let mut facet_fields: Vec<String> = Vec::new();
    let mut active_filter: Option<(String, String)> = None;
    let mut category: Option<String> = None;
    let mut sort_param: Option<(String, bool)> = None;
    match &cell.kind {
        WorkloadKind::BasePlp { category: c } => category = Some(c.clone()),
        WorkloadKind::FilterDepth { filters: f } => filters = f.clone(),
        WorkloadKind::Facet {
            active_filter: af,
            facet_fields: ff,
        } => {
            active_filter = af.clone();
            facet_fields = ff.clone();
            if let Some(f) = af {
                filters.push(f.clone());
            }
        }
        WorkloadKind::NumericRangeSort { range, sort: s } => {
            let op = match range.1.as_str() {
                "gte" => ">=",
                "lte" => "<=",
                "gt" => ">",
                "lt" => "<",
                other => other,
            };
            filters.push((range.0.clone(), format!("{op}{}", range.2)));
            sort_param = Some(s.clone());
        }
    }
    let render_where = |exclude: Option<&str>| -> String {
        let mut parts: Vec<String> = Vec::new();
        if let Some(c) = &category {
            parts.push(format!(
                "category_leaf contains \"{}\"",
                vespa_yql_escape(c)
            ));
        }
        for (attr, val) in &filters {
            if Some(attr.as_str()) == exclude {
                continue;
            }
            if let Some(rest) = val
                .strip_prefix(">=")
                .or_else(|| val.strip_prefix("<="))
                .or_else(|| val.strip_prefix('>'))
                .or_else(|| val.strip_prefix('<'))
            {
                let op = &val[..val.len() - rest.len()];
                parts.push(format!("{attr} {op} {rest}"));
            } else {
                parts.push(format!("{attr} contains \"{}\"", vespa_yql_escape(val)));
            }
        }
        if parts.is_empty() {
            "true".to_owned()
        } else {
            parts.join(" and ")
        }
    };

    let excluded_field = active_filter
        .as_ref()
        .map(|(a, _)| a.as_str())
        .filter(|field| facet_fields.iter().any(|f| f == field));
    let combined_facets: Vec<&String> = facet_fields
        .iter()
        .filter(|f| Some(f.as_str()) != excluded_field)
        .collect();

    let mut yqls = Vec::new();
    // `order by` is part of the where-statement and must precede the `|
    // grouping` pipe stage in YQL grammar; the current workload matrix never
    // combines sort and facets in the same cell (NumericRangeSort sets
    // sort_param with empty facet_fields, Facet sets facet_fields with no
    // sort_param), but this order is kept grammar-correct regardless.
    let mut base_yql = format!("select * from wands where {}", render_where(None));
    if let Some((field, desc)) = &sort_param {
        base_yql.push_str(&format!(
            " order by {field} {}",
            if *desc { "desc" } else { "asc" }
        ));
    }
    if !combined_facets.is_empty() {
        // Multiple sibling top-level groupings in one Vespa query must be
        // wrapped as SPACE-separated (not comma-separated -- tried first,
        // HTTP 400 "was expecting ... <SPACE> ... \"all\"") children of one
        // outer all(...). Verified live against a running container:
        // `all(all(group(style) each(output(count()))) all(group(x)...))`
        // returns totalCount matching every other engine's num_found for
        // this cell.
        let groupings = combined_facets
            .iter()
            .map(|f| format!("all(group({f}) each(output(count())))"))
            .collect::<Vec<_>>()
            .join(" ");
        base_yql.push_str(&format!(" | all({groupings})"));
    }
    let hits = cell.top_k;
    yqls.push((format!("{base_yql}&hits={hits}"), "base"));
    if let Some(field) = excluded_field {
        let facet_yql = format!(
            "select * from wands where {} | all(group({field}) each(output(count())))&hits=0",
            render_where(Some(field))
        );
        yqls.push((facet_yql, "facet"));
    }
    yqls
}

fn run_vespa_workload_cell(
    agent: &ureq::Agent,
    query_url: &str,
    cell: &WorkloadCell,
    cgroup: Option<&CgroupReader>,
) -> Result<WorkloadCellResult, String> {
    let requests = vespa_yqls(cell);
    let make_url = |yql_and_hits: &str| -> String {
        let (yql, hits) = yql_and_hits
            .split_once("&hits=")
            .unwrap_or((yql_and_hits, "0"));
        format!("{query_url}/search/?yql={}&hits={hits}", urlencode(yql))
    };

    for _ in 0..WARMUP_QUERY_COUNT {
        for (yql_and_hits, _kind) in &requests {
            let url = make_url(yql_and_hits);
            retry_request(3, || {
                agent
                    .get(&url)
                    .set("Connection", "close")
                    .call()
                    .map_err(|error| error.to_string())
                    .map(|_| ())
            })?;
        }
    }

    let cpu_before = cgroup.and_then(|r| r.snapshot().ok()).map(|s| s.usage_usec);
    let mut wall_times_ms = Vec::with_capacity(MEASURED_QUERY_COUNT);
    let mut last_num_found = 0u64;
    for _ in 0..MEASURED_QUERY_COUNT {
        let (num_found, elapsed_ms) = retry_request(3, || {
            let started = Instant::now();
            let mut num_found = 0u64;
            for (yql_and_hits, kind) in &requests {
                let url = make_url(yql_and_hits);
                let resp: serde_json::Value = agent
                    .get(&url)
                    .set("Connection", "close")
                    .call()
                    .map_err(|error| error.to_string())?
                    .into_json()
                    .map_err(|error| error.to_string())?;
                if *kind == "base" {
                    num_found = resp["root"]["fields"]["totalCount"].as_u64().unwrap_or(0);
                }
            }
            Ok((num_found, started.elapsed().as_secs_f64() * 1000.0))
        })?;
        wall_times_ms.push(elapsed_ms);
        last_num_found = num_found;
    }
    let cpu_after = cgroup.and_then(|r| r.snapshot().ok()).map(|s| s.usage_usec);
    let mean_cpu_usec_per_query = match (cpu_before, cpu_after) {
        (Some(before), Some(after)) if after >= before => {
            Some((after - before) as f64 / MEASURED_QUERY_COUNT as f64)
        }
        _ => None,
    };

    wall_times_ms.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let percentile = |p: f64| -> f64 {
        let idx = ((p / 100.0) * (wall_times_ms.len() - 1) as f64).round() as usize;
        wall_times_ms[idx.min(wall_times_ms.len() - 1)]
    };
    let mean_wall_ms = wall_times_ms.iter().sum::<f64>() / wall_times_ms.len() as f64;
    let facet_field_count = match &cell.kind {
        WorkloadKind::Facet { facet_fields, .. } => facet_fields.len() as u32,
        _ => 0,
    };

    Ok(WorkloadCellResult {
        name: cell.name.to_owned(),
        p50_ms: percentile(50.0),
        p95_ms: percentile(95.0),
        p99_ms: percentile(99.0),
        mean_wall_ms,
        mean_cpu_usec_per_query,
        mean_backend_requests: requests.len() as f64,
        sample_num_found: last_num_found,
        facet_field_count,
    })
}

fn run_vespa(
    config: &Config,
    mut result: MeasurementResult,
    catalog_path: &Path,
    script: &Path,
) -> MeasurementResult {
    let query_port = read_env_var(&config.repository_root, "I77_VESPA_QUERY_PORT")
        .unwrap_or_else(|| "8082".to_owned());
    let query_url = format!("http://127.0.0.1:{query_port}");
    match launch_competitor(config, script, catalog_path) {
        Ok(_) => {}
        Err(error) => {
            result.status = RunStatus::HarnessFailure;
            result.failure_reason = Some(error);
            return result;
        }
    }

    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(30))
        .build();
    match vespa_correctness(&agent, &query_url) {
        Ok((checks, all_passed)) => {
            result.correctness_checks = checks;
            result.correctness_all_passed = all_passed;
            if !all_passed {
                result.status = RunStatus::CorrectnessFail;
                result.failure_reason =
                    Some("one or more correctness oracle queries failed".to_owned());
                return result;
            }
        }
        Err(error) => {
            result.status = RunStatus::HarnessFailure;
            result.failure_reason = Some(format!("correctness check request failed: {error}"));
            return result;
        }
    }

    let cgroup = container_pid(Engine::Vespa.container_name()).and_then(|pid| {
        CgroupReader::for_pid(pid, Path::new("/proc"), Path::new("/sys/fs/cgroup")).ok()
    });
    result.rss_before_load_bytes = cgroup.as_ref().and_then(|r| r.read_memory_current().ok());

    let cells = workload_matrix(&config.repository_root);
    let mut peak_rss = result.rss_before_load_bytes.unwrap_or(0);
    for cell in &cells {
        match run_vespa_workload_cell(&agent, &query_url, cell, cgroup.as_ref()) {
            Ok(cell_result) => result.workload_cells.push(cell_result),
            Err(error) => {
                result.status = RunStatus::HarnessFailure;
                result.failure_reason =
                    Some(format!("workload cell {} failed: {error}", cell.name));
                return result;
            }
        }
        if let Some(rss) = cgroup.as_ref().and_then(|r| r.read_memory_current().ok()) {
            peak_rss = peak_rss.max(rss);
        }
    }
    result.rss_during_serving_bytes = cgroup.as_ref().and_then(|r| r.read_memory_current().ok());
    result.peak_rss_during_serving_bytes = Some(peak_rss);

    let throughput_requests = vespa_yqls(&cells[2]);
    let (throughput_yql, throughput_hits) = throughput_requests[0]
        .0
        .split_once("&hits=")
        .unwrap_or((throughput_requests[0].0.as_str(), "0"));
    let throughput_url = format!(
        "{query_url}/search/?yql={}&hits={throughput_hits}",
        urlencode(throughput_yql)
    );
    result.throughput = Some(run_throughput_with(|agent| {
        agent
            .get(&throughput_url)
            .set("Connection", "close")
            .call()
            .is_ok()
    }));

    result.status = RunStatus::Ok;
    result
}

fn run(config: &Config) -> MeasurementResult {
    let mut result = base_result(config);
    let catalog_path = match catalog_path_for_tier(&config.repository_root, &config.tier) {
        Ok(path) => path,
        Err(error) => {
            result.failure_reason = Some(error);
            return result;
        }
    };
    if !catalog_path.is_file() {
        result.failure_reason = Some(format!("catalog file missing: {}", catalog_path.display()));
        return result;
    }

    if config.engine == Engine::Native {
        return run_native(config, result, &catalog_path);
    }

    let Some(script) = provision_script_path(config.engine, &config.repository_root) else {
        result.status = RunStatus::EngineExcluded;
        result.failure_reason = Some("no provisioning path for this engine".to_owned());
        return result;
    };
    if !script.is_file() {
        result.status = RunStatus::EngineExcluded;
        result.failure_reason = Some(format!(
            "no provisioning script yet at {}",
            script.display()
        ));
        return result;
    }

    match config.engine {
        Engine::Solr => run_solr(config, result, &catalog_path, &script),
        Engine::Elasticsearch => run_es_like(
            config,
            result,
            &catalog_path,
            &script,
            Engine::Elasticsearch,
            "I77_ES_PORT",
        ),
        Engine::Opensearch => run_es_like(
            config,
            result,
            &catalog_path,
            &script,
            Engine::Opensearch,
            "I77_OPENSEARCH_PORT",
        ),
        Engine::Typesense => run_typesense(config, result, &catalog_path, &script),
        Engine::Meilisearch => run_meilisearch(config, result, &catalog_path, &script),
        Engine::Vespa => run_vespa(config, result, &catalog_path, &script),
        _ => {
            result.status = RunStatus::EngineExcluded;
            result.failure_reason = Some(format!(
                "no query-adapter implemented yet for {}",
                config.engine
            ));
            result
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let config = match parse_args(&args) {
        Ok(config) => config,
        Err(error) => {
            eprintln!("i77_measure: {error}");
            std::process::exit(2);
        }
    };
    let result = run(&config);
    let json = serde_json::to_string_pretty(&result).expect("serialize result");
    if let Some(parent) = config.out.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    std::fs::write(&config.out, &json).expect("write result file");
    println!(
        "MEASURE_OK engine={} tier={} run={} status={:?} correctness={}",
        config.engine, config.tier, config.run, result.status, result.correctness_all_passed
    );
}
