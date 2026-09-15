//! One (engine, tier, run) measurement cycle for Issue #62 (Infra E2).
//!
//! Usage:
//!   i62_measure --engine <name> --tier <100k|500k|1m|3m|5m> --run <n>
//!               --repository-root <canonical-root> --out <result.json>

use issue61_eval::CgroupReader;
use issue62_eval::{
    parse_provision_ok, run_bounded, Engine, MeasurementResult, RunStatus, Tier, EXPERIMENT_ID,
    RAW_SCHEMA_VERSION,
};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

// Native's /ping only answers after the single-threaded server has finished
// loading the entire catalog into memory (established in #61) -- unlike the
// containerized competitors, whose provisioning-script readiness poll
// happens BEFORE indexing and is unrelated to their (separately-timed,
// BUILD_TIMEOUT-bounded) index build. So this deadline is native's
// equivalent of BUILD_TIMEOUT, not a generic "is the HTTP server up" check,
// and must share its budget. 120s was fine at 100k/500k but wrongly
// truncated a live, still-loading 1M-tier native process (verified via
// `docker stats`: 100% CPU, growing RSS, no crash) into a false
// InvalidEnvironmental failure -- root-caused live during the #62 campaign,
// 2026-09-14.
const READINESS_TIMEOUT: Duration = BUILD_TIMEOUT;
// Reduced from the originally-preregistered 3600s to 1200s after a
// ~30-minute silent hang was observed on this shared host under external
// memory pressure (see #62 decision doc's "host instability" section); that
// cut was a reactive defense against a *no-progress* failure signature
// (frozen container, no log growth). It then proved too tight for
// legitimately slow-but-steadily-progressing engines at the 1M tier:
// meilisearch/1m/run1 indexed all 1,031,856 docs successfully ("total
// submitted: 1031856") and was killed during trivial post-index cleanup
// (a bump to 1800s alone would have covered it); vespa/1m/run1 then timed
// out too, distinguished from meilisearch's near-miss by vespa's own
// consistently-measured feed rate (458-467 docs/sec across every completed
// run) implying ~37 minutes of feed alone at 1,031,856 docs, well past
// 1800s once container/convergence overhead is added. Two independent
// engines demonstrating genuine (non-stuck) need for more time at 1M is
// evidence the 1200s/1800s cuts were themselves miscalibrated, not that
// each engine individually needs a bespoke budget -- restored to the
// originally-preregistered 3600s. Silent no-progress hangs remain
// distinguishable from legitimate slow completion via provisioning-log
// inspection (progress lines / feed rate vs. dead silence), as done for
// both incidents above, so this does not reopen the original host-hang
// risk unaddressed. Applies uniformly to all engines/cells not yet
// measured; cannot retroactively change or invalidate any already-completed
// measurement.
const BUILD_TIMEOUT: Duration = Duration::from_secs(3600);
const DOCKER_TIMEOUT: Duration = Duration::from_secs(60);
const PROBE_QUERY_COUNT: u32 = 10;
const WARM_SETTLE: Duration = Duration::from_secs(2);

struct Config {
    engine: Engine,
    tier: Tier,
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
            "--tier" => tier = Some(value.parse::<Tier>()?),
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

/// Polls the container's cgroup memory (and, once found, snapshots cpu.stat
/// at first-seen and expects the caller to snapshot again at build-end) at a
/// coarse interval, tracking the observed peak. Also periodically `du -sb`s
/// the container's writable+volume footprint if `data_dir` is given, for the
/// (best-effort, "where stably measurable" per the preregistration) build
/// disk-amplification metric. Runs until `stop` is set.
struct BuildSampler {
    peak_memory_bytes: Arc<AtomicU64>,
    peak_disk_bytes: Arc<AtomicU64>,
    cpu_usec_at_start: Arc<AtomicU64>,
    stop: Arc<AtomicBool>,
    handle: thread::JoinHandle<()>,
}

impl BuildSampler {
    fn start(container_name: &'static str, data_dir: Option<&'static str>) -> Self {
        let peak_memory_bytes = Arc::new(AtomicU64::new(0));
        let peak_disk_bytes = Arc::new(AtomicU64::new(0));
        let cpu_usec_at_start = Arc::new(AtomicU64::new(u64::MAX));
        let stop = Arc::new(AtomicBool::new(false));
        let (peak_memory_bytes_t, peak_disk_bytes_t, cpu_usec_at_start_t, stop_t) = (
            Arc::clone(&peak_memory_bytes),
            Arc::clone(&peak_disk_bytes),
            Arc::clone(&cpu_usec_at_start),
            Arc::clone(&stop),
        );
        let handle = thread::spawn(move || {
            let mut last_disk_sample = Instant::now() - Duration::from_secs(30);
            while !stop_t.load(Ordering::Relaxed) {
                if let Some(pid) = container_pid(container_name) {
                    if let Ok(reader) =
                        CgroupReader::for_pid(pid, Path::new("/proc"), Path::new("/sys/fs/cgroup"))
                    {
                        if let Ok(current) = reader.read_memory_current() {
                            peak_memory_bytes_t.fetch_max(current, Ordering::Relaxed);
                        }
                        if let Ok(snapshot) = reader.snapshot() {
                            cpu_usec_at_start_t.fetch_min(snapshot.usage_usec, Ordering::Relaxed);
                        }
                    }
                    if let Some(dir) = data_dir {
                        if last_disk_sample.elapsed() >= Duration::from_secs(20) {
                            last_disk_sample = Instant::now();
                            if let Ok(output) = run_bounded(
                                &mut docker(&["exec", container_name, "du", "-sb", dir]),
                                Duration::from_secs(30),
                            ) {
                                if output.succeeded() {
                                    if let Some(bytes) = output
                                        .stdout
                                        .split_whitespace()
                                        .next()
                                        .and_then(|value| value.parse::<u64>().ok())
                                    {
                                        peak_disk_bytes_t.fetch_max(bytes, Ordering::Relaxed);
                                    }
                                }
                            }
                        }
                    }
                }
                thread::sleep(Duration::from_millis(500));
            }
        });
        Self {
            peak_memory_bytes,
            peak_disk_bytes,
            cpu_usec_at_start,
            stop,
            handle,
        }
    }

    fn finish(self) -> (u64, Option<u64>, Option<u64>) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = self.handle.join();
        let peak_memory = self.peak_memory_bytes.load(Ordering::Relaxed);
        let peak_disk = self.peak_disk_bytes.load(Ordering::Relaxed);
        let cpu_start = self.cpu_usec_at_start.load(Ordering::Relaxed);
        (
            peak_memory,
            (peak_disk > 0).then_some(peak_disk),
            (cpu_start != u64::MAX).then_some(cpu_start),
        )
    }
}

/// Data directory (inside the container) each engine writes its index to,
/// used for the build-time disk-amplification sampler. `None` where no
/// stable single directory covers it or the engine isn't yet implemented —
/// disk amplification is best-effort per the preregistration.
const fn data_dir_for(engine: Engine) -> Option<&'static str> {
    match engine {
        Engine::Native => None, // in-memory; no on-disk index during build to sample
        Engine::Solr => Some("/var/solr/data/i62_wands/data/index"),
        Engine::Elasticsearch => Some("/usr/share/elasticsearch/data"),
        Engine::Opensearch => Some("/usr/share/opensearch/data"),
        Engine::Typesense => Some("/data"),
        Engine::Meilisearch => Some("/meili_data"),
        Engine::Vespa | Engine::Havenask => None,
    }
}

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
    Some(repository_root.join("scripts/issue62").join(name))
}

/// Fixed, simple probe query per engine — light traffic to reach a warm
/// steady state, not a relevance check (that's out of scope for E2).
/// Returns `None` (probe skipped, non-fatal) for engines with no wired-up
/// query path yet.
fn probe_url(engine: Engine, port: u16) -> Option<String> {
    match engine {
        Engine::Native => Some(format!("http://127.0.0.1:{port}/select?q=chair&rows=10")),
        Engine::Solr => Some(format!(
            "http://127.0.0.1:{port}/solr/i62_wands/select?q=title:chair&rows=10"
        )),
        Engine::Elasticsearch | Engine::Opensearch => Some(format!(
            "http://127.0.0.1:{port}/i62_wands/_search?q=title:chair"
        )),
        Engine::Typesense => Some(format!(
            "http://127.0.0.1:{port}/collections/i62_wands/documents/search?q=chair&query_by=title"
        )),
        Engine::Meilisearch => Some(format!(
            "http://127.0.0.1:{port}/indexes/i62_wands/search?q=chair"
        )),
        Engine::Vespa | Engine::Havenask => None,
    }
}

/// Host port each engine's query API is reachable on, matching
/// `benchmarks/configs/issue62/container_limits.env`.
const fn query_port_for(engine: Engine) -> u16 {
    match engine {
        Engine::Native => 9901,
        Engine::Solr => 8984,
        Engine::Elasticsearch => 9201,
        Engine::Opensearch => 9202,
        Engine::Typesense => 8108,
        Engine::Meilisearch => 7700,
        Engine::Vespa => 8081,
        Engine::Havenask => 0,
    }
}

/// Measures cold RSS (immediately post-load, pre-query), fires the engine's
/// light probe traffic, settles briefly, then measures warm/steady RSS —
/// in that order, matching the #62 preregistration ("warm/steady-load RSS
/// after a short fixed post-load settle + light probe traffic").
struct RssAndProbes {
    cold_rss_bytes: Option<u64>,
    warm_rss_bytes: Option<u64>,
    probe_queries_ok: u32,
    probe_queries_failed: u32,
}

fn measure_rss_and_probe(engine: Engine, container_name: &str) -> RssAndProbes {
    let reader = container_pid(container_name).and_then(|pid| {
        CgroupReader::for_pid(pid, Path::new("/proc"), Path::new("/sys/fs/cgroup")).ok()
    });
    let cold_rss_bytes = reader
        .as_ref()
        .and_then(|reader| reader.read_memory_current().ok());

    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(10))
        .build();
    let mut probe_queries_ok = 0u32;
    let mut probe_queries_failed = 0u32;
    if let Some(url) = probe_url(engine, query_port_for(engine)) {
        for _ in 0..PROBE_QUERY_COUNT {
            match agent.get(&url).call() {
                Ok(_) => probe_queries_ok += 1,
                Err(_) => probe_queries_failed += 1,
            }
        }
    }
    thread::sleep(WARM_SETTLE);
    let warm_rss_bytes = reader
        .as_ref()
        .and_then(|reader| reader.read_memory_current().ok());

    RssAndProbes {
        cold_rss_bytes,
        warm_rss_bytes,
        probe_queries_ok,
        probe_queries_failed,
    }
}

fn launch_native(catalog_path: &Path, repository_root: &Path) -> Result<Instant, String> {
    let _ = docker(&["rm", "-f", Engine::Native.container_name()]).status();
    let binary = repository_root.join("target/release/i61_native_server");
    let dataset_dir = catalog_path
        .parent()
        .ok_or("catalog path has no parent directory")?;
    let catalog_name = catalog_path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("catalog path has no file name")?;
    let memory = read_env_var(repository_root, "I62_MEMORY").unwrap_or_else(|| "6g".to_owned());
    let memory_swap =
        read_env_var(repository_root, "I62_MEMORY_SWAP").unwrap_or_else(|| memory.clone());
    let started = Instant::now();
    let mut command = docker(&[
        "run",
        "-d",
        "--name",
        Engine::Native.container_name(),
        "--cpus=3",
        "--cpuset-cpus=0-2",
    ]);
    command
        .arg(format!("--memory={memory}"))
        .arg(format!("--memory-swap={memory_swap}"))
        .arg("-p")
        .arg("9901:9901");
    command
        .arg("-v")
        .arg(format!("{}:/opt/i61_native_server:ro", binary.display()))
        .arg("-v")
        .arg(format!("{}:/dataset:ro", dataset_dir.display()))
        .arg("debian@sha256:88200866dfff7ea7f5cbcb6ec7c8a701889efe6fe859fe64d6990e4b07ea4171")
        .arg("/opt/i61_native_server")
        .arg("--catalog")
        .arg(format!("/dataset/{catalog_name}"))
        .arg("--dataset")
        .arg("wands")
        .arg("--port")
        .arg("9901");
    let output = run_bounded(&mut command, DOCKER_TIMEOUT).map_err(|error| error.to_string())?;
    if output.timed_out() || !output.succeeded() {
        return Err(format!("native docker run failed: {}", output.stderr));
    }
    Ok(started)
}

fn wait_native_ready(deadline: Instant) -> Result<(u64, u64), String> {
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(5))
        .build();
    loop {
        if agent
            .get("http://127.0.0.1:9901/ping")
            .set("Connection", "close")
            .call()
            .is_ok()
        {
            break;
        }
        if Instant::now() >= deadline {
            return Err("native did not become ready".to_owned());
        }
        thread::sleep(Duration::from_millis(500));
    }
    let logs = run_bounded(
        &mut docker(&["logs", Engine::Native.container_name()]),
        DOCKER_TIMEOUT,
    )
    .map_err(|error| error.to_string())?;
    let combined = format!("{}\n{}", logs.stdout, logs.stderr);
    let line = combined
        .lines()
        .find(|line| line.contains("NATIVE_READY"))
        .ok_or("no NATIVE_READY line found")?;
    let docs = line
        .split_whitespace()
        .find_map(|token| token.strip_prefix("docs="))
        .and_then(|value| value.parse().ok())
        .ok_or("could not parse docs from NATIVE_READY")?;
    let index_bytes = line
        .split_whitespace()
        .find_map(|token| token.strip_prefix("index_bytes="))
        .and_then(|value| value.parse().ok())
        .ok_or("could not parse index_bytes from NATIVE_READY")?;
    Ok((docs, index_bytes))
}

/// Reads one `KEY=value` line from `benchmarks/configs/issue62/container_limits.env`.
///
/// The bash provisioning scripts `source` this file directly and always see
/// its live value; this orchestrator previously hardcoded `"16g"`/`8 GiB`
/// constants instead of reading it, which (a) gave native's own container an
/// unconditional 16g/16g limit even after the file was edited down to 6g/6g
/// for every competitor mid-campaign (an undisclosed, uncorrected
/// resource-ceiling asymmetry favoring native at the 1M tier, where native's
/// measured RSS of 8.8 GiB exceeds what competitors were capped to), and (b)
/// recorded stale `memory_limit`/`jvm_configured_heap_bytes` provenance in
/// every result file regardless of which limit was actually in force at
/// measurement time. Found via adversarial review during #62; fixed by
/// reading the file directly instead of hardcoding its values.
fn read_env_var(repository_root: &Path, key: &str) -> Option<String> {
    let path = repository_root.join("benchmarks/configs/issue62/container_limits.env");
    let contents = std::fs::read_to_string(path).ok()?;
    contents.lines().find_map(|line| {
        let line = line.trim();
        let (name, value) = line.split_once('=')?;
        (name.trim() == key).then(|| value.trim().to_owned())
    })
}

/// Parses a docker-style size string (`"6g"`, `"512m"`, `"1024k"`, or a bare
/// byte count) into bytes.
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

fn base_result(config: &Config) -> MeasurementResult {
    let engine = config.engine;
    let memory_limit =
        read_env_var(&config.repository_root, "I62_MEMORY").unwrap_or_else(|| "unknown".to_owned());
    let jvm_configured_heap_bytes = if engine.is_jvm() {
        read_env_var(&config.repository_root, "I62_JVM_HEAP").and_then(|v| parse_size_bytes(&v))
    } else {
        None
    };
    MeasurementResult {
        schema_version: RAW_SCHEMA_VERSION,
        experiment_id: EXPERIMENT_ID.to_owned(),
        engine: engine.as_str().to_owned(),
        tier: config.tier.as_str().to_owned(),
        run: config.run,
        expected_docs: config.tier.expected_docs(),
        docs: None,
        index_bytes: None,
        bytes_per_product: None,
        build_wall_ms: None,
        build_cpu_usec: None,
        peak_build_memory_bytes: None,
        peak_build_disk_bytes: None,
        cold_rss_bytes: None,
        warm_rss_bytes: None,
        probe_queries_ok: 0,
        probe_queries_failed: 0,
        jvm_configured_heap_bytes,
        container_name: engine.container_name().to_owned(),
        cpus: "3".to_owned(),
        cpuset: "0-2".to_owned(),
        memory_limit,
        git_sha: git_sha(&config.repository_root),
        hostname: hostname(),
        timestamp_utc: timestamp_utc(),
        status: RunStatus::HarnessFailure,
        failure_reason: None,
        provision_stdout_tail: None,
        provision_stderr_tail: None,
    }
}

fn run(config: &Config) -> MeasurementResult {
    let engine = config.engine;
    let tier = config.tier;
    let container_name = engine.container_name();
    let mut result = base_result(config);

    let catalog_path = config.repository_root.join(tier.catalog_relative_path());
    if !catalog_path.is_file() {
        result.failure_reason = Some(format!("catalog file missing: {}", catalog_path.display()));
        return result;
    }

    if engine == Engine::Native {
        return run_native(config, result, &catalog_path);
    }

    let Some(script) = provision_script_path(engine, &config.repository_root) else {
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

    let container_static: &'static str = Box::leak(container_name.to_owned().into_boxed_str());
    let data_dir = data_dir_for(engine);
    let sampler = BuildSampler::start(container_static, data_dir);
    let build_started = Instant::now();
    let mut command = Command::new("bash");
    command
        .arg(&script)
        .arg(&catalog_path)
        .arg(tier.expected_docs().to_string())
        .current_dir(&config.repository_root);
    let provision = run_bounded(&mut command, BUILD_TIMEOUT);
    let build_wall_ms = u64::try_from(build_started.elapsed().as_millis()).unwrap_or(u64::MAX);
    let (peak_memory, peak_disk, cpu_start) = sampler.finish();
    result.build_wall_ms = Some(build_wall_ms);
    result.peak_build_memory_bytes = Some(peak_memory);
    result.peak_build_disk_bytes = peak_disk;

    let provision = match provision {
        Ok(output) => output,
        Err(error) => {
            result.failure_reason = Some(format!("provisioning spawn failed: {error}"));
            return result;
        }
    };
    result.provision_stdout_tail = Some(issue62_eval::BoundedOutput::tail(&provision.stdout, 2000));
    result.provision_stderr_tail = Some(issue62_eval::BoundedOutput::tail(&provision.stderr, 2000));

    if provision.timed_out() {
        result.status = RunStatus::InvalidEnvironmental;
        result.failure_reason = Some("provisioning script timed out".to_owned());
        let _ = docker(&["rm", "-f", container_name]).status();
        return result;
    }
    if !provision.succeeded() {
        result.status = RunStatus::HarnessFailure;
        result.failure_reason = Some(format!(
            "provisioning script exited non-zero: {}",
            provision.stderr
        ));
        let _ = docker(&["rm", "-f", container_name]).status();
        return result;
    }
    let Some((_, docs, index_bytes)) = parse_provision_ok(&provision.stdout) else {
        result.status = RunStatus::HarnessFailure;
        result.failure_reason = Some("no PROVISION_OK line found".to_owned());
        let _ = docker(&["rm", "-f", container_name]).status();
        return result;
    };
    result.docs = Some(docs);
    result.index_bytes = Some(index_bytes);
    result.bytes_per_product = Some(index_bytes as f64 / docs.max(1) as f64);

    if let Some(pid) = container_pid(container_name) {
        if let Ok(reader) =
            CgroupReader::for_pid(pid, Path::new("/proc"), Path::new("/sys/fs/cgroup"))
        {
            if let (Some(cpu_start), Ok(snapshot)) = (cpu_start, reader.snapshot()) {
                result.build_cpu_usec = Some(snapshot.usage_usec.saturating_sub(cpu_start));
            }
        }
    }

    if docs != tier.expected_docs() {
        result.status = RunStatus::DocCountMismatch;
        result.failure_reason = Some(format!(
            "doc count mismatch: got {docs}, expected {}",
            tier.expected_docs()
        ));
        let _ = docker(&["rm", "-f", container_name]).status();
        return result;
    }

    let measured = measure_rss_and_probe(engine, container_name);
    result.cold_rss_bytes = measured.cold_rss_bytes;
    result.warm_rss_bytes = measured.warm_rss_bytes;
    result.probe_queries_ok = measured.probe_queries_ok;
    result.probe_queries_failed = measured.probe_queries_failed;

    let _ = docker(&["rm", "-f", container_name]).status();
    result.status = RunStatus::Ok;
    result
}

fn run_native(
    config: &Config,
    mut result: MeasurementResult,
    catalog_path: &Path,
) -> MeasurementResult {
    let build_started = match launch_native(catalog_path, &config.repository_root) {
        Ok(started) => started,
        Err(error) => {
            result.failure_reason = Some(format!("native launch failed: {error}"));
            let _ = docker(&["rm", "-f", Engine::Native.container_name()]).status();
            return result;
        }
    };
    let deadline = Instant::now() + READINESS_TIMEOUT;
    let (docs, index_bytes) = match wait_native_ready(deadline) {
        Ok(pair) => pair,
        Err(error) => {
            result.status = RunStatus::InvalidEnvironmental;
            result.failure_reason = Some(error);
            let _ = docker(&["rm", "-f", Engine::Native.container_name()]).status();
            return result;
        }
    };
    result.build_wall_ms =
        Some(u64::try_from(build_started.elapsed().as_millis()).unwrap_or(u64::MAX));
    result.docs = Some(docs);
    result.index_bytes = Some(index_bytes);
    result.bytes_per_product = Some(index_bytes as f64 / docs.max(1) as f64);

    if let Some(pid) = container_pid(Engine::Native.container_name()) {
        if let Ok(reader) =
            CgroupReader::for_pid(pid, Path::new("/proc"), Path::new("/sys/fs/cgroup"))
        {
            if let Ok(snapshot) = reader.snapshot() {
                result.peak_build_memory_bytes = Some(snapshot.memory_peak_bytes);
                result.build_cpu_usec = Some(snapshot.usage_usec);
            }
        }
    }

    if docs != config.tier.expected_docs() {
        result.status = RunStatus::DocCountMismatch;
        result.failure_reason = Some(format!(
            "doc count mismatch: got {docs}, expected {}",
            config.tier.expected_docs()
        ));
        let _ = docker(&["rm", "-f", Engine::Native.container_name()]).status();
        return result;
    }

    let measured = measure_rss_and_probe(Engine::Native, Engine::Native.container_name());
    result.cold_rss_bytes = measured.cold_rss_bytes;
    result.warm_rss_bytes = measured.warm_rss_bytes;
    result.probe_queries_ok = measured.probe_queries_ok;
    result.probe_queries_failed = measured.probe_queries_failed;

    let _ = docker(&["rm", "-f", Engine::Native.container_name()]).status();
    result.status = RunStatus::Ok;
    result
}

fn main() {
    let config = match parse_args(&std::env::args().collect::<Vec<_>>()) {
        Ok(config) => config,
        Err(error) => {
            eprintln!("i62_measure: {error}");
            std::process::exit(2);
        }
    };
    let result = run(&config);
    let json = serde_json::to_string_pretty(&result).expect("result serializes");
    if let Err(error) = std::fs::write(&config.out, json) {
        eprintln!(
            "i62_measure: failed to write {}: {error}",
            config.out.display()
        );
        std::process::exit(2);
    }
    if result.status == RunStatus::Ok {
        println!(
            "MEASURE_OK engine={} tier={} run={} docs={:?} index_bytes={:?}",
            result.engine, result.tier, result.run, result.docs, result.index_bytes
        );
    } else {
        eprintln!(
            "MEASURE_FAILED engine={} tier={} run={} status={:?} reason={:?}",
            result.engine, result.tier, result.run, result.status, result.failure_reason
        );
        std::process::exit(1);
    }
}
