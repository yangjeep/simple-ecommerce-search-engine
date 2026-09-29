//! One clean native launch + measurement pass for Issue #79 (Infra E3b).
//!
//! Usage:
//!   e3b_native_measure --repository-root <root> --server-binary <path>
//!     --catalog <jsonl> --label <label> --run <n> --out <json>
//!     [--cells all|headline|calibration|none|<name,name>]
//!     [--modes none|<facet:sort,...>] [--facet-cell-modes <facet:sort,...>]
//!     [--sort-cell-modes <facet:sort,...>] [--server-args "<args>"]
//!
//! Issue #63: a mode may carry a third component, `facet:sort:cand`, which
//! adds `cand_mode=<cand>` (p0r|p1|p2|p2b) to the request.
//!
//! Launches the given native server binary inside #79's scope envelope
//! (the frozen #77 values from `benchmarks/configs/issue77/resource_envelope.env`:
//! CPU quota, CPU affinity, memory ceiling, swap 0), gates on #77's
//! same-product cross-variant fixture, then measures every (cell, mode)
//! with #77's per-cell loop (20 uncounted warmups, `Connection: close`,
//! serial requests) and #79's #74-aware batch rule: at least 200 measured
//! requests, extended until >= 2 s cumulative wall time (cap 5000), CPU/query
//! = cgroup `usage_usec` delta over the whole batch / batch size.
//! `--modes none` sends no mode parameters (the unchanged #77 N0 binary).
//! `--facet-cell-modes` / `--sort-cell-modes` override `--modes` for that
//! cell family (facet modes are meaningless on facet-free sort cells).
//! `--cells none` measures only load/build/RSS (memory-accounting launches).

use issue61_eval::CgroupReader;
use issue79_eval::cells::{
    all_cells, i64_cells, query_string_with_cand, reference_cells, Cell, Family, Role,
};
use issue79_eval::plp::Diag;
use issue79_eval::{EXPERIMENT_ID, RAW_SCHEMA_VERSION};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const WARMUP: usize = 20;
const MIN_MEASURED: usize = 200;
const MIN_WINDOW: Duration = Duration::from_secs(2);
const MAX_MEASURED: usize = 5000;
const UNIT: &str = "i77-native";

struct Args {
    root: PathBuf,
    server_binary: PathBuf,
    catalog: PathBuf,
    label: String,
    run: u32,
    out: PathBuf,
    cells: String,
    modes: Option<Vec<(String, String)>>,
    facet_cell_modes: Option<Vec<(String, String)>>,
    sort_cell_modes: Option<Vec<(String, String)>>,
    server_args: Vec<String>,
}

fn parse_args() -> Result<Args, String> {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    let mut map = BTreeMap::new();
    let mut iter = raw.iter();
    while let Some(flag) = iter.next() {
        let value = iter
            .next()
            .ok_or_else(|| format!("missing value for {flag}"))?;
        map.insert(flag.clone(), value.clone());
    }
    let get = |name: &str| map.get(name).cloned().ok_or(format!("missing {name}"));
    let parse_modes = |flag: &str| -> Result<Option<Vec<(String, String)>>, String> {
        match map.get(flag).map(String::as_str) {
            None | Some("none") => Ok(None),
            Some(list) => Ok(Some(
                list.split(',')
                    .map(|pair| {
                        pair.split_once(':')
                            .map(|(f, s)| (f.to_owned(), s.to_owned()))
                            .ok_or_else(|| format!("bad mode {pair:?}, want facet:sort"))
                    })
                    .collect::<Result<Vec<_>, _>>()?,
            )),
        }
    };
    let modes = parse_modes("--modes")?;
    let facet_cell_modes = parse_modes("--facet-cell-modes")?;
    let sort_cell_modes = parse_modes("--sort-cell-modes")?;
    Ok(Args {
        root: PathBuf::from(get("--repository-root")?),
        server_binary: PathBuf::from(get("--server-binary")?),
        catalog: PathBuf::from(get("--catalog")?),
        label: get("--label")?,
        run: get("--run")?.parse().map_err(|e| format!("--run: {e}"))?,
        out: PathBuf::from(get("--out")?),
        cells: map
            .get("--cells")
            .cloned()
            .unwrap_or_else(|| "all".to_owned()),
        modes,
        facet_cell_modes,
        sort_cell_modes,
        server_args: map
            .get("--server-args")
            .map(|s| s.split_whitespace().map(str::to_owned).collect())
            .unwrap_or_default(),
    })
}

fn read_env_var(root: &Path, key: &str) -> Option<String> {
    let path = root.join("benchmarks/configs/issue77/resource_envelope.env");
    let content = std::fs::read_to_string(path).ok()?;
    content.lines().find_map(|line| {
        let (k, v) = line.split_once('=')?;
        (k.trim() == key).then(|| v.trim().trim_matches('"').to_owned())
    })
}

fn session_env(command: &mut Command) {
    let runtime_dir =
        std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/run/user/1000".to_owned());
    command.env("XDG_RUNTIME_DIR", &runtime_dir);
    command.env(
        "DBUS_SESSION_BUS_ADDRESS",
        format!("unix:path={runtime_dir}/bus"),
    );
}

fn systemctl(args: &[&str]) -> Command {
    let mut command = Command::new("systemctl");
    command.arg("--user").args(args);
    session_env(&mut command);
    command
}

fn stop_unit() {
    let _ = systemctl(&["stop", &format!("{UNIT}.scope")]).output();
    let _ = systemctl(&["reset-failed", &format!("{UNIT}.scope")]).output();
}

fn unit_cgroup() -> Option<CgroupReader> {
    let output = systemctl(&[
        "show",
        "-p",
        "ControlGroup",
        "--value",
        &format!("{UNIT}.scope"),
    ])
    .output()
    .ok()?;
    let group = String::from_utf8(output.stdout).ok()?;
    let group = group.trim();
    (!group.is_empty())
        .then(|| CgroupReader::at_dir(PathBuf::from(format!("/sys/fs/cgroup{group}"))))
}

fn read_u64(dir: &Path, file: &str) -> Option<u64> {
    std::fs::read_to_string(dir.join(file))
        .ok()?
        .trim()
        .parse()
        .ok()
}

#[derive(Debug, Serialize, Deserialize, Default)]
struct CellResult {
    cell: String,
    role: String,
    family: String,
    facet_mode: Option<String>,
    sort_mode: Option<String>,
    /// Issue #63 `cand_mode` (absent = p0, #79's path).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    cand_mode: Option<String>,
    status: String,
    error: Option<String>,
    measured_queries: usize,
    p50_ms: f64,
    p95_ms: f64,
    p99_ms: f64,
    mean_wall_ms: f64,
    cpu_usec_per_query: Option<f64>,
    qps_per_core: Option<f64>,
    num_found: u64,
    facet_fields: usize,
    facets_fingerprint: u64,
    docs_fingerprint: u64,
    /// Mean of the server's in-process phase timers across the measured
    /// batch (decomposition only; CPU/query above is the headline).
    mean_candidates_us: Option<f64>,
    mean_facets_us: Option<f64>,
    mean_sort_us: Option<f64>,
    mean_total_us: Option<f64>,
    /// From the last response (deterministic per request).
    diag: Option<Diag>,
    memory_current_after_bytes: Option<u64>,
}

#[derive(Debug, Serialize, Deserialize)]
struct RunResult {
    schema_version: u32,
    experiment_id: String,
    label: String,
    run: u32,
    status: String,
    failure_reason: Option<String>,
    server_binary: String,
    server_binary_sha256: String,
    server_args: Vec<String>,
    catalog: String,
    runtime: String,
    cpus: String,
    cpuset: String,
    memory_limit: String,
    launch_to_ready_ms: f64,
    server_ready: Option<serde_json::Value>,
    fixture_correctness_all_passed: bool,
    rss_after_load_bytes: Option<u64>,
    peak_after_load_bytes: Option<u64>,
    rss_after_serving_bytes: Option<u64>,
    peak_after_serving_bytes: Option<u64>,
    cells: Vec<CellResult>,
    git_sha: String,
    hostname: String,
    timestamp_utc: u64,
}

fn command_stdout(program: &str, args: &[&str]) -> String {
    Command::new(program)
        .args(args)
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_default()
        .trim()
        .to_owned()
}

fn fingerprint(value: &serde_json::Value) -> u64 {
    // FNV-1a over the canonical JSON: a cheap cross-run/cross-mode equality
    // check (the correctness gate is the real oracle comparison).
    let text = value.to_string();
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in text.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

fn canonical(value: &serde_json::Value) -> serde_json::Value {
    // HashMap-serialized facet objects have arbitrary key order.
    match value {
        serde_json::Value::Object(map) => {
            let sorted: BTreeMap<_, _> =
                map.iter().map(|(k, v)| (k.clone(), canonical(v))).collect();
            serde_json::to_value(sorted).unwrap_or_default()
        }
        serde_json::Value::Array(items) => {
            serde_json::Value::Array(items.iter().map(canonical).collect())
        }
        other => other.clone(),
    }
}

fn get(agent: &ureq::Agent, url: &str) -> Result<String, String> {
    let mut last = String::new();
    for attempt in 0..3 {
        match agent.get(url).set("Connection", "close").call() {
            Ok(resp) => return resp.into_string().map_err(|e| e.to_string()),
            Err(ureq::Error::Status(code, resp)) => {
                return Err(format!(
                    "HTTP {code}: {}",
                    resp.into_string().unwrap_or_default()
                ))
            }
            Err(error) => {
                last = error.to_string();
                std::thread::sleep(Duration::from_millis(100 * (attempt + 1)));
            }
        }
    }
    Err(last)
}

fn measure_cell(
    agent: &ureq::Agent,
    base: &str,
    cell: &Cell,
    modes: Option<&(String, String)>,
    cgroup: Option<&CgroupReader>,
) -> CellResult {
    // `facet:sort:cand` arrives as ("facet", "sort:cand").
    let (sort_mode, cand_mode) = match modes.map(|m| m.1.as_str()) {
        Some(sort) => match sort.split_once(':') {
            Some((s, c)) => (Some(s), Some(c)),
            None => (Some(sort), None),
        },
        None => (None, None),
    };
    let mut result = CellResult {
        cell: cell.name.to_owned(),
        role: cell.role.as_str().to_owned(),
        family: cell.family.as_str().to_owned(),
        facet_mode: modes.map(|m| m.0.clone()),
        sort_mode: sort_mode.map(str::to_owned),
        cand_mode: cand_mode.map(str::to_owned),
        status: "ok".to_owned(),
        ..CellResult::default()
    };
    let url = format!(
        "{base}{}",
        query_string_with_cand(
            cell,
            modes.zip(sort_mode).map(|((f, _), s)| (f.as_str(), s)),
            cand_mode
        )
    );
    for _ in 0..WARMUP {
        if let Err(error) = get(agent, &url) {
            result.status = "error".to_owned();
            result.error = Some(error);
            return result;
        }
    }
    let cpu_before = cgroup.and_then(|r| r.snapshot().ok()).map(|s| s.usage_usec);
    let window_started = Instant::now();
    let mut walls = Vec::new();
    let mut sums = [0.0f64; 4];
    let mut diag_count = 0usize;
    let mut last: Option<serde_json::Value> = None;
    while walls.len() < MAX_MEASURED
        && (walls.len() < MIN_MEASURED || window_started.elapsed() < MIN_WINDOW)
    {
        let started = Instant::now();
        let body = match get(agent, &url) {
            Ok(body) => body,
            Err(error) => {
                result.status = "error".to_owned();
                result.error = Some(error);
                return result;
            }
        };
        walls.push(started.elapsed().as_secs_f64() * 1e3);
        let parsed: serde_json::Value = match serde_json::from_str(&body) {
            Ok(v) => v,
            Err(error) => {
                result.status = "error".to_owned();
                result.error = Some(error.to_string());
                return result;
            }
        };
        if let Some(diag) = parsed.get("diag") {
            diag_count += 1;
            for (slot, key) in ["candidates_us", "facets_us", "sort_us", "total_us"]
                .iter()
                .enumerate()
            {
                sums[slot] += diag[key].as_f64().unwrap_or(0.0);
            }
        }
        last = Some(parsed);
    }
    let cpu_after = cgroup.and_then(|r| r.snapshot().ok()).map(|s| s.usage_usec);
    let n = walls.len();
    result.measured_queries = n;
    result.cpu_usec_per_query = match (cpu_before, cpu_after) {
        (Some(b), Some(a)) if a >= b => Some((a - b) as f64 / n as f64),
        _ => None,
    };
    result.qps_per_core = result.cpu_usec_per_query.map(|c| 1e6 / c);
    walls.sort_by(f64::total_cmp);
    let pct = |p: f64| walls[(((p / 100.0) * (n - 1) as f64).round() as usize).min(n - 1)];
    result.p50_ms = pct(50.0);
    result.p95_ms = pct(95.0);
    result.p99_ms = pct(99.0);
    result.mean_wall_ms = walls.iter().sum::<f64>() / n as f64;
    if diag_count > 0 {
        let mean = |slot: usize| Some(sums[slot] / diag_count as f64);
        result.mean_candidates_us = mean(0);
        result.mean_facets_us = mean(1);
        result.mean_sort_us = mean(2);
        result.mean_total_us = mean(3);
    }
    if let Some(last) = &last {
        if let Err(error) = i63_dump(cell, modes, last) {
            result.status = "error".to_owned();
            result.error = Some(error);
        }
    }
    if let Some(last) = last {
        result.num_found = last["num_found"].as_u64().unwrap_or(0);
        result.facet_fields = last["facets"].as_object().map_or(0, |m| m.len());
        result.facets_fingerprint = fingerprint(&canonical(&last["facets"]));
        result.docs_fingerprint = fingerprint(&canonical(&last["docs"]));
        result.diag = last
            .get("diag")
            .and_then(|d| serde_json::from_value(d.clone()).ok());
    }
    result.memory_current_after_bytes = cgroup.and_then(|r| r.read_memory_current().ok());
    result
}

/// Issue #63: when `I63_DUMP_DIR` is set, the last measured response of each
/// (cell, mode) is written there (`num_found`, facet maps, hit keys) for the
/// equal-work check against the oracle. Written after the timed batch, so
/// it never enters a measurement window.
fn i63_dump(
    cell: &Cell,
    modes: Option<&(String, String)>,
    last: &serde_json::Value,
) -> Result<(), String> {
    let Ok(dir) = std::env::var("I63_DUMP_DIR") else {
        return Ok(());
    };
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let mode = modes.map_or_else(
        || "none".to_owned(),
        |(f, s)| format!("{f}-{}", s.replace(':', "-")),
    );
    let keys: Vec<String> = last["docs"]
        .as_array()
        .and_then(|a| a.first())
        .and_then(|d| d.as_object())
        .map(|o| o.keys().cloned().collect())
        .unwrap_or_default();
    let dump = serde_json::json!({
        "num_found": last["num_found"],
        "facets": canonical(&last["facets"]),
        "hit_keys": keys,
        "hit_count": last["docs"].as_array().map_or(0, Vec::len),
        "backend_requests": last["backend_requests"],
        "mode": mode,
    });
    let path = Path::new(&dir).join(format!("native__{}__{mode}.json", cell.name));
    std::fs::write(
        &path,
        serde_json::to_string_pretty(&dump).map_err(|e| e.to_string())?,
    )
    .map_err(|e| format!("{}: {e}", path.display()))
}

fn select_cells(spec: &str) -> Vec<Cell> {
    let cells = all_cells();
    match spec {
        "all" => cells,
        // Issue #63: #77's filter-depth cells (never part of "all").
        "reference" => reference_cells(),
        // Issue #64: the 44 facet-economics cells.
        "i64" => i64_cells(),
        "none" => Vec::new(),
        "headline" => cells
            .into_iter()
            .filter(|c| c.role == Role::Headline)
            .collect(),
        "calibration" => cells
            .into_iter()
            .filter(|c| c.role == Role::Calibration)
            .collect(),
        names => {
            let wanted: Vec<&str> = names.split(',').collect();
            cells
                .into_iter()
                .chain(reference_cells())
                .chain(i64_cells())
                .filter(|c| wanted.contains(&c.name))
                .collect()
        }
    }
}

fn run(args: &Args) -> RunResult {
    let cpus = read_env_var(&args.root, "I77_CPUS").unwrap_or_else(|| "3".to_owned());
    let cpuset = read_env_var(&args.root, "I77_CPUSET").unwrap_or_else(|| "0-2".to_owned());
    let memory = read_env_var(&args.root, "I77_MEMORY").unwrap_or_else(|| "6g".to_owned());
    let port = read_env_var(&args.root, "I77_NATIVE_PORT").unwrap_or_else(|| "9902".to_owned());
    let mut result = RunResult {
        schema_version: RAW_SCHEMA_VERSION,
        experiment_id: EXPERIMENT_ID.to_owned(),
        label: args.label.clone(),
        run: args.run,
        status: "harness_failure".to_owned(),
        failure_reason: None,
        server_binary: args.server_binary.display().to_string(),
        server_binary_sha256: command_stdout("sha256sum", &[&args.server_binary.to_string_lossy()])
            .split_whitespace()
            .next()
            .unwrap_or_default()
            .to_owned(),
        server_args: args.server_args.clone(),
        catalog: args.catalog.display().to_string(),
        runtime: "scope".to_owned(),
        cpus: cpus.clone(),
        cpuset: cpuset.clone(),
        memory_limit: memory.clone(),
        launch_to_ready_ms: 0.0,
        server_ready: None,
        fixture_correctness_all_passed: false,
        rss_after_load_bytes: None,
        peak_after_load_bytes: None,
        rss_after_serving_bytes: None,
        peak_after_serving_bytes: None,
        cells: Vec::new(),
        git_sha: command_stdout(
            "git",
            &["-C", &args.root.to_string_lossy(), "rev-parse", "HEAD"],
        ),
        hostname: command_stdout("hostname", &[]),
        timestamp_utc: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs()),
    };

    stop_unit();
    let log_path = std::env::temp_dir().join(format!("e3b_native_{}_{}.log", args.label, args.run));
    let log = match std::fs::File::create(&log_path) {
        Ok(f) => f,
        Err(error) => {
            result.failure_reason = Some(error.to_string());
            return result;
        }
    };
    let quota: f64 = cpus.parse::<f64>().unwrap_or(3.0) * 100.0;
    let mut command = Command::new("systemd-run");
    session_env(&mut command);
    command
        .args(["--user", "--scope", "--quiet", &format!("--unit={UNIT}")])
        .arg(format!("-pCPUQuota={quota:.0}%"))
        .arg(format!("-pMemoryMax={}", memory.to_uppercase()))
        .arg("-pMemorySwapMax=0")
        .args(["taskset", "-c", &cpuset])
        .arg(&args.server_binary)
        .arg("--catalog")
        .arg(&args.catalog)
        .args(["--dataset", "wands", "--port", &port])
        .args(&args.server_args)
        .stdout(log.try_clone().expect("clone log handle"))
        .stderr(log);
    let launched = Instant::now();
    if let Err(error) = command.spawn() {
        result.failure_reason = Some(format!("systemd-run: {error}"));
        return result;
    }
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(120))
        .build();
    let base = format!("http://127.0.0.1:{port}");
    let deadline = launched + Duration::from_secs(3600);
    loop {
        if get(&agent, &format!("{base}/ping")).is_ok() {
            break;
        }
        if Instant::now() > deadline {
            result.failure_reason = Some("server never became ready".to_owned());
            stop_unit();
            return result;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    result.launch_to_ready_ms = launched.elapsed().as_secs_f64() * 1e3;
    result.server_ready = std::fs::read_to_string(&log_path).ok().and_then(|text| {
        text.lines()
            .find_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
    });

    let cgroup = unit_cgroup();
    std::thread::sleep(Duration::from_secs(2));
    result.rss_after_load_bytes = cgroup.as_ref().and_then(|r| r.read_memory_current().ok());
    result.peak_after_load_bytes = cgroup
        .as_ref()
        .and_then(|r| read_u64(r.dir(), "memory.peak"));

    match get(&agent, &format!("{base}/correctness")).and_then(|body| {
        serde_json::from_str::<serde_json::Value>(&body).map_err(|e| e.to_string())
    }) {
        Ok(value) => {
            result.fixture_correctness_all_passed = value["all_passed"].as_bool() == Some(true)
        }
        Err(error) => {
            result.failure_reason = Some(format!("fixture correctness: {error}"));
            stop_unit();
            return result;
        }
    }
    if !result.fixture_correctness_all_passed {
        result.status = "correctness_fail".to_owned();
        stop_unit();
        return result;
    }

    let to_list = |modes: &Option<Vec<(String, String)>>| -> Vec<Option<(String, String)>> {
        match modes {
            None => vec![None],
            Some(modes) => modes.iter().cloned().map(Some).collect(),
        }
    };
    let default_list = to_list(&args.modes);
    let facet_list = args
        .facet_cell_modes
        .as_ref()
        .map_or_else(|| default_list.clone(), |_| to_list(&args.facet_cell_modes));
    let sort_list = args
        .sort_cell_modes
        .as_ref()
        .map_or_else(|| default_list.clone(), |_| to_list(&args.sort_cell_modes));
    for cell in select_cells(&args.cells) {
        let mode_list = match cell.family {
            Family::Facet => &facet_list,
            Family::Sort => &sort_list,
        };
        for modes in mode_list {
            let cell_result = measure_cell(&agent, &base, &cell, modes.as_ref(), cgroup.as_ref());
            eprintln!(
                "  {} {:?} cpu_us={:?} p50_ms={:.2} n={}",
                cell_result.cell,
                modes,
                cell_result.cpu_usec_per_query.map(|c| c.round()),
                cell_result.p50_ms,
                cell_result.measured_queries
            );
            result.cells.push(cell_result);
        }
    }
    result.rss_after_serving_bytes = cgroup.as_ref().and_then(|r| r.read_memory_current().ok());
    result.peak_after_serving_bytes = cgroup
        .as_ref()
        .and_then(|r| read_u64(r.dir(), "memory.peak"));
    stop_unit();
    result.status = if result.cells.iter().all(|c| c.status == "ok") {
        "ok".to_owned()
    } else {
        "cell_error".to_owned()
    };
    result
}

fn main() {
    let args = match parse_args() {
        Ok(args) => args,
        Err(error) => {
            eprintln!("e3b_native_measure: {error}");
            std::process::exit(2);
        }
    };
    let result = run(&args);
    if let Some(parent) = args.out.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    std::fs::write(
        &args.out,
        serde_json::to_string_pretty(&result).expect("serialize"),
    )
    .expect("write result");
    println!(
        "E3B_MEASURE_OK label={} run={} status={} cells={}",
        result.label,
        result.run,
        result.status,
        result.cells.len()
    );
}
