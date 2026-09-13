//! Revision 11/12 live execution port: the concrete, non-test `LifecyclePort`
//! implementation that drives real Docker containers and real `i61_bench`
//! subprocesses through the already-frozen lifecycle core.

use super::live::NativeLaunchContract;
use super::model::{
    BlockContext, CommandInput, CommandRequest, CyclePath, DatasetIdentity, EngineIdentity, Event,
    EventRecord, EvidenceFile, ExecutionEvidence, IndexArtifact, IndexCell, LifecycleError,
    LoggedText, OutputVisibility, PreparedCommand, Projection, RawRecord, ResolvedSealManifest,
    Sequence, SeriesIdentity, SlotContext,
};
use super::port::{InitializationFailure, LifecyclePort, SlotExecution};
use crate::post_run::run_completed_analysis;
use crate::{
    audit_candidate_sets, campaign_plan, fetch_complete_solr, frozen_native_query, load_dataset,
    load_workload, native_candidate_ids, sha256_hex, CampaignCycle, CampaignSeries,
    CompletedAnalysis, Dataset, Engine,
};
use commerce_core::index::CatalogIndex;
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const SOLR_CONTAINER: &str = "i61-solr";
const SOLR_PORT: u16 = 8983;
const DOCKER_TIMEOUT: Duration = Duration::from_secs(60);
const TEARDOWN_TIMEOUT: Duration = Duration::from_secs(30);
const SOLR_PROVISION_TIMEOUT: Duration = Duration::from_secs(300);
const SLOT_TIMEOUT: Duration = Duration::from_secs(180);
const STEAL_CPUSET: &str = "0-2";
/// `container_limits.env`'s `I61_DRIVER_CPU`: the engine's cpuset (0-2) is
/// reserved so the driver's own HTTP/serialization work never competes with
/// the engine it is measuring. `i61_bench` is pinned here with `taskset`.
const DRIVER_CPU: &str = "3";

/// Result of a bounded subprocess invocation: whichever of exit/timeout the
/// process actually reached, plus whatever it wrote before that point.
struct BoundedOutput {
    status: Option<std::process::ExitStatus>,
    stdout: String,
    stderr: String,
}

impl BoundedOutput {
    fn timed_out(&self) -> bool {
        self.status.is_none()
    }

    fn succeeded(&self) -> bool {
        self.status.is_some_and(|status| status.success())
    }
}

/// Kills an entire process group (negative PID) with SIGKILL via the
/// external `kill` utility, so a timed-out shell wrapper (e.g.
/// `provision_solr.sh`) cannot leave an orphaned grandchild (a stuck `curl`
/// or `docker exec`) holding the piped stdout/stderr open forever.
fn kill_process_group(pid: u32) {
    let _ = Command::new("kill")
        .arg("-KILL")
        .arg("--")
        .arg(format!("-{pid}"))
        .status();
}

fn run_bounded(command: &mut Command, timeout: Duration) -> io::Result<BoundedOutput> {
    use std::os::unix::process::CommandExt;
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    // Puts the child in its own new process group (group id == child pid) so
    // a timeout can kill the whole tree, not just the direct child.
    unsafe {
        command.pre_exec(|| {
            nix::unistd::setpgid(nix::unistd::Pid::from_raw(0), nix::unistd::Pid::from_raw(0))
                .map_err(|errno| io::Error::from_raw_os_error(errno as i32))
        });
    }
    let mut child = command.spawn()?;
    let pid = child.id();
    let mut stdout_pipe = child.stdout.take().expect("stdout is piped");
    let mut stderr_pipe = child.stderr.take().expect("stderr is piped");
    let stdout_thread = thread::spawn(move || {
        let mut buffer = String::new();
        let _ = stdout_pipe.read_to_string(&mut buffer);
        buffer
    });
    let stderr_thread = thread::spawn(move || {
        let mut buffer = String::new();
        let _ = stderr_pipe.read_to_string(&mut buffer);
        buffer
    });
    let deadline = Instant::now() + timeout;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break Some(status);
        }
        if Instant::now() >= deadline {
            kill_process_group(pid);
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        thread::sleep(Duration::from_millis(20));
    };
    let stdout = stdout_thread.join().unwrap_or_default();
    let stderr = stderr_thread.join().unwrap_or_default();
    Ok(BoundedOutput {
        status,
        stdout,
        stderr,
    })
}

fn docker(args: &[&str]) -> Command {
    let mut command = Command::new("docker");
    command.args(args);
    command
}

fn run_docker(args: &[&str], timeout: Duration) -> Result<BoundedOutput, io::Error> {
    run_bounded(&mut docker(args), timeout)
}

/// Removes a container by name, treating "already gone" as success.
/// Returns whether the container is now confirmed absent.
fn remove_container(name: &str) -> bool {
    match run_docker(&["rm", "-f", name], TEARDOWN_TIMEOUT) {
        Ok(output) => output.succeeded() || output.stderr.contains("No such container"),
        Err(_) => false,
    }
}

/// Parses the `NATIVE_READY docs=<n> index_bytes=<n>` line the native server
/// prints at startup (see `bin/i61_native_server.rs`). Returns `(docs,
/// index_bytes)`.
fn parse_native_ready_line(text: &str) -> Option<(u64, u64)> {
    let line = text.lines().find(|line| line.contains("NATIVE_READY"))?;
    let docs = extract_kv(line, "docs=")?;
    let index_bytes = extract_kv(line, "index_bytes=")?;
    Some((docs, index_bytes))
}

/// Parses the `PROVISION_OK core=<core> docs=<n> index_bytes=<n>` line
/// `scripts/issue61/provision_solr.sh` prints on success.
fn parse_provision_ok_line(text: &str) -> Option<(String, u64, u64)> {
    let line = text.lines().find(|line| line.contains("PROVISION_OK"))?;
    let core = line
        .split_whitespace()
        .find_map(|token| token.strip_prefix("core="))?
        .to_owned();
    let docs = extract_kv(line, "docs=")?;
    let index_bytes = extract_kv(line, "index_bytes=")?;
    Some((core, docs, index_bytes))
}

fn extract_kv(line: &str, key: &str) -> Option<u64> {
    line.split_whitespace()
        .find_map(|token| token.strip_prefix(key))
        .and_then(|value| value.parse().ok())
}

/// Host memory/CPU snapshot for run provenance (non-sealed, supplementary
/// infra log only — never part of the frozen measurement contract). Records
/// what the host actually looked like at the start of this cycle, so a
/// memory-pressure event during a run is reproducible/explicable later.
fn host_provenance_line() -> String {
    let meminfo = std::fs::read_to_string("/proc/meminfo").unwrap_or_default();
    let field = |key: &str| -> String {
        meminfo
            .lines()
            .find(|line| line.starts_with(key))
            .and_then(|line| line.split_whitespace().nth(1))
            .map_or_else(|| "unknown".to_owned(), ToOwned::to_owned)
    };
    let cpus = std::thread::available_parallelism().map_or(0, std::num::NonZeroUsize::get);
    format!(
        "host provenance: cpus={cpus} mem_total_kb={} mem_available_kb={} swap_total_kb={}",
        field("MemTotal:"),
        field("MemAvailable:"),
        field("SwapTotal:"),
    )
}

const fn expected_document_count(dataset: Dataset) -> u64 {
    match dataset {
        Dataset::Wands => 42_994,
        Dataset::EsciElectronics => 2_075,
    }
}

fn catalog_path(repository_root: &Path, dataset: Dataset) -> PathBuf {
    match dataset {
        Dataset::Wands => repository_root.join("dataset_cache/wands/catalog.jsonl"),
        Dataset::EsciElectronics => {
            repository_root.join("dataset_cache/esci_electronics/esci_electronics_products.jsonl")
        }
    }
}

fn workload_path(repository_root: &Path, dataset: Dataset) -> PathBuf {
    match dataset {
        Dataset::Wands => repository_root.join("benchmarks/workloads/i61_wands_480.jsonl"),
        Dataset::EsciElectronics => {
            repository_root.join("benchmarks/workloads/i61_esci_electronics.jsonl")
        }
    }
}

fn evidence_file_name(file: EvidenceFile) -> &'static str {
    match file {
        EvidenceFile::CandidateAuditEsci => "candidate_audit_esci.jsonl",
        EvidenceFile::CandidateAuditWands => "candidate_audit_wands.jsonl",
        EvidenceFile::Commands => "commands.log",
        EvidenceFile::Events => "events.jsonl",
        EvidenceFile::IndexArtifacts => "index_artifacts.jsonl",
        EvidenceFile::Raw => "raw.jsonl",
    }
}

/// Deterministic, globally-unique scratch output path for one `i61_bench`
/// invocation. Lives outside the sealed cycle directory so it never affects
/// the frozen seven-regular-file invariant.
fn scratch_path(repository_root: &Path, cycle: CampaignCycle, slot: SlotContext) -> PathBuf {
    let series_tag = match slot.block.series {
        SeriesIdentity::Calibration => "calibration",
        SeriesIdentity::Warm => "warm",
        SeriesIdentity::Cold => "cold",
    };
    let projection_tag = slot
        .block
        .projection
        .map_or("na", |projection| match projection {
            Projection::All => "all",
            Projection::FastPath => "fast-path",
            Projection::Hybrid => "hybrid",
            Projection::Punt => "punt",
        });
    repository_root
        .join("target/i61_live_scratch")
        .join(format!(
            "{}_{}_{}_{}_{}_{}_{}.jsonl",
            cycle.as_str(),
            series_tag,
            slot.block.dataset.as_str(),
            projection_tag,
            slot.block.block_index,
            slot.session.engine().as_str(),
            slot.slot.get(),
        ))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EngineNeed {
    NativeOnly,
    SolrOnly,
    Both,
}

fn need_for(series: CampaignSeries) -> (Dataset, EngineNeed) {
    match series {
        CampaignSeries::Calibration { engine } => (
            Dataset::Wands,
            match engine {
                Engine::Native => EngineNeed::NativeOnly,
                Engine::Solr => EngineNeed::SolrOnly,
            },
        ),
        CampaignSeries::Warm { dataset, .. } | CampaignSeries::Cold { dataset } => {
            (dataset, EngineNeed::Both)
        }
    }
}

struct NativeHandle {
    dataset: Dataset,
    endpoint: String,
    cgroup_dir: PathBuf,
}

struct SolrHandle {
    dataset: Dataset,
    core_url: String,
    cgroup_dir: PathBuf,
}

enum Environment {
    None,
    Native(NativeHandle),
    Solr(SolrHandle),
    Both {
        native: NativeHandle,
        solr: SolrHandle,
    },
}

impl Environment {
    fn matches(&self, dataset: Dataset, need: EngineNeed) -> bool {
        match (self, need) {
            (Self::Native(handle), EngineNeed::NativeOnly) => handle.dataset == dataset,
            (Self::Solr(handle), EngineNeed::SolrOnly) => handle.dataset == dataset,
            (Self::Both { native, solr }, EngineNeed::Both) => {
                native.dataset == dataset && solr.dataset == dataset
            }
            _ => false,
        }
    }

    fn native_handle(&self) -> Option<&NativeHandle> {
        match self {
            Self::Native(handle) | Self::Both { native: handle, .. } => Some(handle),
            Self::Solr(_) | Self::None => None,
        }
    }

    fn solr_handle(&self) -> Option<&SolrHandle> {
        match self {
            Self::Solr(handle) | Self::Both { solr: handle, .. } => Some(handle),
            Self::Native(_) | Self::None => None,
        }
    }
}

pub(crate) struct LivePort {
    repository_root: PathBuf,
    cycle: CampaignCycle,
    cycle_dir: Option<PathBuf>,
    candidate_audit_esci: Option<File>,
    candidate_audit_wands: Option<File>,
    commands: Option<File>,
    events: Option<File>,
    index_artifacts: Option<File>,
    raw: Option<File>,
    seal_file: Option<File>,
    regular_files: usize,
    environment: Environment,
    equivalence_passed: bool,
    native_index_bytes_wands: Option<u64>,
    native_index_bytes_esci: Option<u64>,
    solr_index_bytes_wands: Option<u64>,
    solr_index_bytes_esci: Option<u64>,
    agent: ureq::Agent,
    git_sha: String,
    hostname: String,
    analysis: Option<CompletedAnalysis>,
    infra_log: Option<File>,
    last_error: Option<String>,
}

impl LivePort {
    pub(crate) fn new(repository_root: PathBuf, cycle: CampaignCycle) -> Self {
        let git_sha = Command::new("git")
            .arg("rev-parse")
            .arg("HEAD")
            .current_dir(&repository_root)
            .output()
            .ok()
            .filter(|output| output.status.success())
            .and_then(|output| String::from_utf8(output.stdout).ok())
            .map(|value| value.trim().to_owned())
            .unwrap_or_else(|| "unknown".to_owned());
        let hostname = std::fs::read_to_string("/proc/sys/kernel/hostname")
            .map(|value| value.trim().to_owned())
            .unwrap_or_else(|_| "unknown".to_owned());
        let infra_log_dir = repository_root.join("artifacts/issue61");
        let _ = std::fs::create_dir_all(&infra_log_dir);
        let infra_log_path =
            infra_log_dir.join(format!("i61_e1_{}_live_infra.log", cycle.as_str()));
        let infra_log = OpenOptions::new()
            .create(true)
            .append(true)
            .open(infra_log_path)
            .ok();
        let mut port = Self {
            repository_root,
            cycle,
            cycle_dir: None,
            candidate_audit_esci: None,
            candidate_audit_wands: None,
            commands: None,
            events: None,
            index_artifacts: None,
            raw: None,
            seal_file: None,
            regular_files: 0,
            environment: Environment::None,
            equivalence_passed: true,
            native_index_bytes_wands: None,
            native_index_bytes_esci: None,
            solr_index_bytes_wands: None,
            solr_index_bytes_esci: None,
            agent: ureq::AgentBuilder::new()
                .timeout(Duration::from_secs(30))
                .build(),
            git_sha,
            hostname,
            analysis: None,
            infra_log,
            last_error: None,
        };
        port.log(&host_provenance_line());
        port
    }

    pub(crate) fn analysis(&self) -> Option<&CompletedAnalysis> {
        self.analysis.as_ref()
    }

    /// The most recent recorded failure detail, if any. Surfaced by
    /// `run_live` on a hard error so a real run's diagnostics don't collapse
    /// to the single generic `environment_launch_failed` tag when the
    /// supplementary infra log itself couldn't be opened or written.
    pub(crate) fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }

    fn log(&mut self, message: &str) {
        if let Some(file) = self.infra_log.as_mut() {
            let _ = writeln!(file, "{message}");
        }
    }

    /// Like [`Self::log`], but also keeps the detail in memory so it survives
    /// even when the infra log file couldn't be opened or a write failed.
    fn record_error(&mut self, message: String) {
        self.log(&message);
        self.last_error = Some(message);
    }

    fn file_slot_mut(&mut self, file: EvidenceFile) -> &mut Option<File> {
        match file {
            EvidenceFile::CandidateAuditEsci => &mut self.candidate_audit_esci,
            EvidenceFile::CandidateAuditWands => &mut self.candidate_audit_wands,
            EvidenceFile::Commands => &mut self.commands,
            EvidenceFile::Events => &mut self.events,
            EvidenceFile::IndexArtifacts => &mut self.index_artifacts,
            EvidenceFile::Raw => &mut self.raw,
        }
    }

    fn file_ref(&self, file: EvidenceFile) -> Option<&File> {
        match file {
            EvidenceFile::CandidateAuditEsci => self.candidate_audit_esci.as_ref(),
            EvidenceFile::CandidateAuditWands => self.candidate_audit_wands.as_ref(),
            EvidenceFile::Commands => self.commands.as_ref(),
            EvidenceFile::Events => self.events.as_ref(),
            EvidenceFile::IndexArtifacts => self.index_artifacts.as_ref(),
            EvidenceFile::Raw => self.raw.as_ref(),
        }
    }

    fn write_evidence(&mut self, file: EvidenceFile, bytes: &[u8]) -> io::Result<()> {
        self.file_slot_mut(file)
            .as_mut()
            .ok_or_else(|| io::Error::other("evidence file is not open"))?
            .write_all(bytes)
    }

    fn ensure_environment(
        &mut self,
        dataset: Dataset,
        need: EngineNeed,
    ) -> Result<(), LifecycleError> {
        if self.environment.matches(dataset, need) {
            return Ok(());
        }
        if !self.teardown_current() {
            self.record_error("environment teardown failed before relaunch".to_owned());
            return Err(LifecycleError::EnvironmentLaunch);
        }
        self.environment = match need {
            EngineNeed::NativeOnly => Environment::Native(self.launch_native(dataset)?),
            EngineNeed::SolrOnly => Environment::Solr(self.launch_solr(dataset)?),
            EngineNeed::Both => {
                let native = self.launch_native(dataset)?;
                let solr = self.launch_solr(dataset)?;
                Environment::Both { native, solr }
            }
        };
        Ok(())
    }

    /// Tears down whatever is currently live. Returns whether every
    /// container involved is now confirmed absent (protocol §25.2/§25.3:
    /// a teardown failure must be observable, never silently swallowed).
    fn teardown_current(&mut self) -> bool {
        match std::mem::replace(&mut self.environment, Environment::None) {
            Environment::None => true,
            Environment::Native(_) => {
                self.log("teardown native");
                let ok = remove_container(
                    NativeLaunchContract::for_dataset(&self.repository_root, Dataset::Wands)
                        .container_name(),
                );
                if !ok {
                    self.record_error("native container teardown failed".to_owned());
                }
                ok
            }
            Environment::Solr(_) => {
                self.log("teardown solr");
                let ok = remove_container(SOLR_CONTAINER);
                if !ok {
                    self.record_error("solr container teardown failed".to_owned());
                }
                ok
            }
            Environment::Both { .. } => {
                self.log("teardown native+solr");
                let native_ok = remove_container(
                    NativeLaunchContract::for_dataset(&self.repository_root, Dataset::Wands)
                        .container_name(),
                );
                let solr_ok = remove_container(SOLR_CONTAINER);
                if !native_ok {
                    self.record_error("native container teardown failed".to_owned());
                }
                if !solr_ok {
                    self.record_error("solr container teardown failed".to_owned());
                }
                native_ok && solr_ok
            }
        }
    }

    /// Fires a real `/select` query (not just `/ping`) a few times with a
    /// short per-attempt timeout, to confirm the server can actually answer
    /// before any real session traffic is sent its way.
    fn native_self_check(&self, endpoint: &str) -> bool {
        let short_agent = ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(5))
            .build();
        let url = format!("{endpoint}/select?q=chair&rows=1");
        for attempt in 0..3 {
            if short_agent
                .get(&url)
                .set("Connection", "close")
                .call()
                .is_ok()
            {
                return true;
            }
            if attempt < 2 {
                thread::sleep(Duration::from_millis(300));
            }
        }
        false
    }

    fn launch_native(&mut self, dataset: Dataset) -> Result<NativeHandle, LifecycleError> {
        let contract = NativeLaunchContract::for_dataset(&self.repository_root, dataset);
        let _ = remove_container(contract.container_name());
        let (binary_host, binary_container, binary_ro) = contract.binary_mount();
        let (dataset_host, dataset_container, dataset_ro) = contract.dataset_mount();
        let (host_port, container_port) = contract.port_binding();
        let mut args: Vec<String> = vec![
            "run".into(),
            "-d".into(),
            "--name".into(),
            contract.container_name().into(),
            "--cpus".into(),
            contract.cpus().into(),
            "--cpuset-cpus".into(),
            contract.cpuset_cpus().into(),
            "--memory".into(),
            contract.memory().into(),
            "--memory-swap".into(),
            contract.memory_swap().into(),
            "-p".into(),
            format!("{host_port}:{container_port}"),
            "-v".into(),
            format!(
                "{}:{}:{}",
                binary_host.display(),
                binary_container.display(),
                if binary_ro { "ro" } else { "rw" }
            ),
            "-v".into(),
            format!(
                "{}:{}:{}",
                dataset_host.display(),
                dataset_container.display(),
                if dataset_ro { "ro" } else { "rw" }
            ),
            contract.image().into(),
        ];
        args.extend(contract.argv().iter().map(ToString::to_string));
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        let run_output =
            run_docker(&refs, DOCKER_TIMEOUT).map_err(|_| LifecycleError::EnvironmentLaunch)?;
        if run_output.timed_out() || !run_output.succeeded() {
            self.record_error(format!(
                "native launch failed dataset={} stderr={}",
                dataset.as_str(),
                run_output.stderr
            ));
            return Err(LifecycleError::EnvironmentLaunch);
        }
        let deadline = Instant::now() + contract.readiness_timeout();
        let ping_url = format!("{}{}", contract.endpoint(), contract.readiness_path());
        loop {
            if self
                .agent
                .get(&ping_url)
                .set("Connection", "close")
                .call()
                .is_ok()
            {
                break;
            }
            if Instant::now() >= deadline {
                self.record_error(format!(
                    "native readiness timed out dataset={}",
                    dataset.as_str()
                ));
                return Err(LifecycleError::EnvironmentLaunch);
            }
            thread::sleep(contract.readiness_interval());
        }
        // `/ping` succeeding only proves the listener is bound; the
        // NATIVE_READY line is printed just before that in program order and
        // Rust's stdout is line-buffered, but tolerate a few retries anyway
        // rather than trust that ordering exactly.
        let mut ready_docs = None;
        for attempt in 0..5 {
            let logs = run_docker(&["logs", contract.container_name()], DOCKER_TIMEOUT)
                .map_err(|_| LifecycleError::EnvironmentLaunch)?;
            if let Some(parsed) = parse_native_ready_line(&logs.stdout)
                .or_else(|| parse_native_ready_line(&logs.stderr))
            {
                ready_docs = Some(parsed);
                break;
            }
            if attempt < 4 {
                thread::sleep(Duration::from_millis(200));
            }
        }
        let Some((docs, _)) = ready_docs else {
            self.record_error(format!(
                "native NATIVE_READY line never appeared dataset={}",
                dataset.as_str()
            ));
            return Err(LifecycleError::EnvironmentLaunch);
        };
        if docs != expected_document_count(dataset) {
            self.record_error(format!(
                "native document count mismatch dataset={} docs={docs}",
                dataset.as_str()
            ));
            return Err(LifecycleError::EnvironmentLaunch);
        }
        // `/ping` succeeding only proves the listener accepted a connection;
        // it does not prove a real `/select` round-trip will complete
        // promptly right after a rapid teardown-then-relaunch transition
        // (observed once in practice as a client-side read timeout on the
        // very first warm query). Mirrors the self-check the protected
        // reference launcher already performed before going live.
        if !self.native_self_check(contract.endpoint()) {
            self.record_error(format!(
                "native self-check query never answered dataset={}",
                dataset.as_str()
            ));
            return Err(LifecycleError::EnvironmentLaunch);
        }
        let pid_output = run_docker(
            &[
                "inspect",
                "--format",
                "{{.State.Pid}}",
                contract.container_name(),
            ],
            DOCKER_TIMEOUT,
        )
        .map_err(|_| LifecycleError::EnvironmentLaunch)?;
        let pid: u32 = pid_output
            .stdout
            .trim()
            .parse()
            .map_err(|_| LifecycleError::EnvironmentLaunch)?;
        let cgroup = contract
            .cgroup_reader_for_pid(pid, Path::new("/proc"), Path::new("/sys/fs/cgroup"))
            .map_err(|_| LifecycleError::EnvironmentLaunch)?;
        self.log(&format!(
            "native ready dataset={} pid={pid}",
            dataset.as_str()
        ));
        Ok(NativeHandle {
            dataset,
            endpoint: contract.endpoint().to_owned(),
            cgroup_dir: cgroup.dir().to_path_buf(),
        })
    }

    fn launch_solr(&mut self, dataset: Dataset) -> Result<SolrHandle, LifecycleError> {
        let script = self
            .repository_root
            .join("scripts/issue61/provision_solr.sh");
        let mut command = Command::new("bash");
        command
            .arg(&script)
            .arg(dataset.as_str())
            .current_dir(&self.repository_root);
        let output = run_bounded(&mut command, SOLR_PROVISION_TIMEOUT)
            .map_err(|_| LifecycleError::EnvironmentLaunch)?;
        if output.timed_out() || !output.succeeded() {
            self.record_error(format!(
                "provision_solr.sh failed dataset={} stdout={} stderr={}",
                dataset.as_str(),
                output.stdout,
                output.stderr
            ));
            return Err(LifecycleError::EnvironmentLaunch);
        }
        let Some((core, docs, index_bytes)) = parse_provision_ok_line(&output.stdout) else {
            self.record_error(format!(
                "provision_solr.sh produced no PROVISION_OK line dataset={} stdout={} stderr={}",
                dataset.as_str(),
                output.stdout,
                output.stderr
            ));
            return Err(LifecycleError::EnvironmentLaunch);
        };
        if docs != expected_document_count(dataset) || core != format!("i61_{}", dataset.as_str()) {
            self.record_error(format!(
                "solr provisioning mismatch dataset={} core={core} docs={docs}",
                dataset.as_str()
            ));
            return Err(LifecycleError::EnvironmentLaunch);
        }
        match dataset {
            Dataset::Wands => self.solr_index_bytes_wands = Some(index_bytes),
            Dataset::EsciElectronics => self.solr_index_bytes_esci = Some(index_bytes),
        }
        let pid_output = run_docker(
            &["inspect", "--format", "{{.State.Pid}}", SOLR_CONTAINER],
            DOCKER_TIMEOUT,
        )
        .map_err(|_| LifecycleError::EnvironmentLaunch)?;
        let pid: u32 = pid_output
            .stdout
            .trim()
            .parse()
            .map_err(|_| LifecycleError::EnvironmentLaunch)?;
        let cgroup =
            crate::CgroupReader::for_pid(pid, Path::new("/proc"), Path::new("/sys/fs/cgroup"))
                .map_err(|_| LifecycleError::EnvironmentLaunch)?;
        self.log(&format!(
            "solr ready dataset={} core={core} pid={pid}",
            dataset.as_str()
        ));
        Ok(SolrHandle {
            dataset,
            core_url: format!("http://127.0.0.1:{SOLR_PORT}/solr/{core}"),
            cgroup_dir: cgroup.dir().to_path_buf(),
        })
    }

    fn run_equivalence_audit_for(&mut self, dataset: Dataset) -> Result<(), LifecycleError> {
        self.ensure_environment(dataset, EngineNeed::SolrOnly)
            .map_err(|_| LifecycleError::EquivalenceAudit)?;
        let catalog = catalog_path(&self.repository_root, dataset);
        let data = load_dataset(&catalog, dataset).map_err(|_| LifecycleError::EquivalenceAudit)?;
        let index = CatalogIndex::build(&data.catalog);
        let native_bytes = u64::try_from(index.approximate_size_bytes()).unwrap_or(u64::MAX);
        match dataset {
            Dataset::Wands => self.native_index_bytes_wands = Some(native_bytes),
            Dataset::EsciElectronics => self.native_index_bytes_esci = Some(native_bytes),
        }
        let workload = load_workload(&workload_path(&self.repository_root, dataset))
            .map_err(|_| LifecycleError::EquivalenceAudit)?;
        let core_url = self
            .environment
            .solr_handle()
            .expect("solr environment ensured before audit")
            .core_url
            .clone();
        let file = match dataset {
            Dataset::Wands => EvidenceFile::CandidateAuditWands,
            Dataset::EsciElectronics => EvidenceFile::CandidateAuditEsci,
        };
        for query in &workload {
            let native = frozen_native_query(query)
                .and_then(|text| native_candidate_ids(&data, &index, text));
            let engine = fetch_complete_solr(&self.agent, &core_url, query);
            let record = audit_candidate_sets(dataset.as_str(), query, native, engine);
            if record.verdict != crate::AuditVerdict::Match {
                self.equivalence_passed = false;
            }
            let mut line =
                serde_json::to_string(&record).map_err(|_| LifecycleError::EquivalenceAudit)?;
            line.push('\n');
            self.write_evidence(file, line.as_bytes())
                .map_err(|_| LifecycleError::EquivalenceAudit)?;
        }
        Ok(())
    }
}

impl Drop for LivePort {
    fn drop(&mut self) {
        if !remove_container(
            NativeLaunchContract::for_dataset(&self.repository_root, Dataset::Wands)
                .container_name(),
        ) {
            self.record_error("drop: native container teardown failed".to_owned());
        }
        if !remove_container(SOLR_CONTAINER) {
            self.record_error("drop: solr container teardown failed".to_owned());
        }
    }
}

impl LifecyclePort for LivePort {
    fn validate_static(&mut self) -> Result<(), LifecycleError> {
        super::live::validate_repository_root(&self.repository_root)
            .map(|_| ())
            .map_err(|_| LifecycleError::StaticValidation)
    }

    fn repository_root(&self) -> &Path {
        &self.repository_root
    }

    fn initialize(&mut self, path: &CyclePath) -> Result<(), InitializationFailure> {
        let dir = path.as_path();
        if dir.exists() || std::fs::create_dir(dir).is_err() {
            return Err(InitializationFailure {
                error: LifecycleError::Initialization,
                events_opened: false,
            });
        }
        self.cycle_dir = Some(dir.to_path_buf());
        for file in EvidenceFile::ALL {
            let file_path = dir.join(evidence_file_name(file));
            match OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&file_path)
            {
                Ok(handle) => {
                    *self.file_slot_mut(file) = Some(handle);
                    self.regular_files += 1;
                }
                Err(_) => {
                    return Err(InitializationFailure {
                        error: LifecycleError::Initialization,
                        events_opened: self.evidence_is_open(EvidenceFile::Events),
                    });
                }
            }
        }
        Ok(())
    }

    fn evidence_is_open(&self, file: EvidenceFile) -> bool {
        self.file_ref(file).is_some()
    }

    fn append_event(
        &mut self,
        cycle: CampaignCycle,
        sequence: Sequence,
        event: Event,
    ) -> Result<(), LifecycleError> {
        let mut line = serde_json::to_string(&EventRecord::new(cycle, sequence, event))
            .map_err(|_| LifecycleError::Serialization)?;
        line.push('\n');
        self.write_evidence(EvidenceFile::Events, line.as_bytes())
            .map_err(|_| LifecycleError::EventEvidence)
    }

    fn append_command(&mut self, _sequence: Sequence, line: String) -> Result<(), LifecycleError> {
        self.write_evidence(EvidenceFile::Commands, line.as_bytes())
            .map_err(|_| LifecycleError::CommandEvidence)
    }

    fn audit_equivalence(&mut self) -> Result<(), LifecycleError> {
        self.equivalence_passed = true;
        for dataset in [Dataset::Wands, Dataset::EsciElectronics] {
            self.run_equivalence_audit_for(dataset)?;
        }
        Ok(())
    }

    fn capture_index(
        &mut self,
        cycle: CampaignCycle,
    ) -> Result<Vec<IndexArtifact>, LifecycleError> {
        let native_wands = self
            .native_index_bytes_wands
            .ok_or(LifecycleError::IndexCapture)?;
        let native_esci = self
            .native_index_bytes_esci
            .ok_or(LifecycleError::IndexCapture)?;
        let solr_wands = self
            .solr_index_bytes_wands
            .ok_or(LifecycleError::IndexCapture)?;
        let solr_esci = self
            .solr_index_bytes_esci
            .ok_or(LifecycleError::IndexCapture)?;
        let cells = vec![
            IndexCell::native(cycle, DatasetIdentity::Wands, native_wands),
            IndexCell::native(cycle, DatasetIdentity::EsciElectronics, native_esci),
            IndexCell::solr(cycle, DatasetIdentity::Wands, solr_wands),
            IndexCell::solr(cycle, DatasetIdentity::EsciElectronics, solr_esci),
        ];
        for cell in &cells {
            let mut line = serde_json::to_string(cell).map_err(|_| LifecycleError::IndexCapture)?;
            line.push('\n');
            self.write_evidence(EvidenceFile::IndexArtifacts, line.as_bytes())
                .map_err(|_| LifecycleError::IndexCapture)?;
        }
        Ok(cells.into_iter().map(IndexArtifact).collect())
    }

    fn evaluate_equivalence_gate(&mut self) -> bool {
        self.equivalence_passed
    }

    fn prescreen(&mut self, block: BlockContext) -> Result<bool, LifecycleError> {
        let (dataset, need) = need_for(block.campaign_series);
        self.ensure_environment(dataset, need)?;
        let cpus =
            crate::CpuSet::parse(STEAL_CPUSET).map_err(|_| LifecycleError::EnvironmentLaunch)?;
        let result = crate::run_assigned_cpu_steal_probe(Path::new("/proc"), &cpus)
            .map_err(|_| LifecycleError::EnvironmentLaunch)?;
        Ok(!result.rejected)
    }

    fn command_request(&self, slot: SlotContext) -> CommandRequest {
        let dataset = slot.block.campaign_series.dataset();
        let (engine_url, engine_cgroup) = match slot.engine {
            EngineIdentity::Native => {
                let handle = self
                    .environment
                    .native_handle()
                    .expect("native environment ensured before command_request");
                (handle.endpoint.clone(), handle.cgroup_dir.clone())
            }
            EngineIdentity::Solr => {
                let handle = self
                    .environment
                    .solr_handle()
                    .expect("solr environment ensured before command_request");
                (handle.core_url.clone(), handle.cgroup_dir.clone())
            }
        };
        let scratch = scratch_path(&self.repository_root, self.cycle, slot);
        let workload = workload_path(&self.repository_root, dataset);
        let public = CommandInput::Public;
        let args = vec![
            public("--workload".to_owned()),
            public(workload.display().to_string()),
            public("--dataset".to_owned()),
            public(dataset.as_str().to_owned()),
            public("--query-class".to_owned()),
            public(slot.block.campaign_series.projection().as_str().to_owned()),
            public("--engine".to_owned()),
            public(slot.session.engine().as_str().to_owned()),
            public("--session-mode".to_owned()),
            public(slot.session.plan().mode().as_str().to_owned()),
            public("--engine-url".to_owned()),
            public(engine_url),
            public("--engine-cgroup".to_owned()),
            public(engine_cgroup.display().to_string()),
            public("--block".to_owned()),
            public(slot.session.block_index().get().to_string()),
            public("--engine-order".to_owned()),
            public(slot.session.slot().index().to_string()),
            public("--seed".to_owned()),
            public("61".to_owned()),
            public("--out".to_owned()),
            public(scratch.display().to_string()),
        ];
        CommandRequest {
            executable: self
                .repository_root
                .join("target/release/i61_bench")
                .display()
                .to_string(),
            args,
            env: vec![
                super::model::EnvironmentInput::public("GIT_SHA", &self.git_sha),
                super::model::EnvironmentInput::public("HOSTNAME", &self.hostname),
            ],
            environment_allowlist: vec!["GIT_SHA".to_owned(), "HOSTNAME".to_owned()],
            stdout_visibility: OutputVisibility::Public,
            stderr_visibility: OutputVisibility::Public,
        }
    }

    fn execute_slot(&mut self, slot: SlotContext, command: &PreparedCommand) -> SlotExecution {
        let scratch = scratch_path(&self.repository_root, self.cycle, slot);
        if let Some(parent) = scratch.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let mut cmd = Command::new("taskset");
        cmd.arg("-c").arg(DRIVER_CPU).arg(command.executable());
        cmd.args(command.direct_args());
        cmd.env_clear();
        for (name, value) in command.direct_environment() {
            cmd.env(name, value);
        }
        cmd.current_dir(&self.repository_root);
        match run_bounded(&mut cmd, SLOT_TIMEOUT) {
            Ok(output) if output.timed_out() => SlotExecution::Failed {
                evidence: ExecutionEvidence {
                    outcome: super::model::CommandOutcome::TimedOut {
                        timeout_ms: u64::try_from(SLOT_TIMEOUT.as_millis()).unwrap_or(u64::MAX),
                    },
                    stdout: output.stdout,
                    stderr: output.stderr,
                },
                error: LifecycleError::SlotFailed,
            },
            Ok(output) => {
                let status = output.status.expect("non-timed-out run has a status");
                let outcome = exit_outcome(status);
                let succeeded = outcome.succeeded();
                let evidence = ExecutionEvidence {
                    outcome,
                    stdout: output.stdout,
                    stderr: output.stderr,
                };
                if succeeded {
                    match crate::read_jsonl(&scratch) {
                        Ok(mut records) if records.len() == 1 => {
                            let _ = std::fs::remove_file(&scratch);
                            SlotExecution::Completed {
                                evidence,
                                record: Box::new(records.remove(0)),
                            }
                        }
                        _ => SlotExecution::Failed {
                            evidence,
                            error: LifecycleError::SlotFailed,
                        },
                    }
                } else {
                    SlotExecution::Failed {
                        evidence,
                        error: LifecycleError::SlotFailed,
                    }
                }
            }
            Err(error) => SlotExecution::Failed {
                evidence: ExecutionEvidence {
                    outcome: super::model::CommandOutcome::SpawnFailed {
                        reason: LoggedText::Public(error.to_string()),
                    },
                    stdout: String::new(),
                    stderr: String::new(),
                },
                error: LifecycleError::SlotFailed,
            },
        }
    }

    fn teardown(&mut self, _slot: SlotContext) -> Result<(), LifecycleError> {
        Ok(())
    }

    fn teardown_environment(&mut self) -> Result<(), LifecycleError> {
        if self.teardown_current() {
            Ok(())
        } else {
            Err(LifecycleError::TeardownFailed)
        }
    }

    fn append_raw(&mut self, records: [RawRecord; 2]) -> Result<(), LifecycleError> {
        for record in &records {
            let mut line = serde_json::to_string(record).map_err(|_| LifecycleError::SlotFailed)?;
            line.push('\n');
            self.write_evidence(EvidenceFile::Raw, line.as_bytes())
                .map_err(|_| LifecycleError::SlotFailed)?;
        }
        Ok(())
    }

    fn calibration_analyzed(&mut self, records: usize) {
        self.log(&format!("calibration analyzed records={records}"));
    }

    fn flush(&mut self, file: EvidenceFile) -> Result<(), LifecycleError> {
        match self.file_slot_mut(file) {
            Some(handle) => handle.flush().map_err(|_| LifecycleError::Flush(file)),
            None => Ok(()),
        }
    }

    fn sync(&mut self, file: EvidenceFile) -> Result<(), LifecycleError> {
        match self.file_ref(file) {
            Some(handle) => handle.sync_all().map_err(|_| LifecycleError::Sync(file)),
            None => Ok(()),
        }
    }

    fn close(&mut self, file: EvidenceFile) -> Result<(), LifecycleError> {
        *self.file_slot_mut(file) = None;
        Ok(())
    }

    fn compute_hash(&mut self, path: &str) -> Result<String, LifecycleError> {
        let bytes = std::fs::read(self.repository_root.join(path))
            .map_err(|_| LifecycleError::SealWriteAll)?;
        Ok(sha256_hex(&bytes))
    }

    fn create_seal(&mut self) -> Result<(), LifecycleError> {
        let path = self
            .cycle_dir
            .as_ref()
            .expect("cycle directory is set before finalization")
            .join("checksums.sha256");
        let handle = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(|_| LifecycleError::SealCreateNew)?;
        self.seal_file = Some(handle);
        self.regular_files += 1;
        Ok(())
    }

    fn write_seal(&mut self, manifest: &ResolvedSealManifest) -> Result<(), LifecycleError> {
        let mut content = String::new();
        for entry in &manifest.entries {
            content.push_str(&format!("{}  {}\n", entry.hash, entry.path));
        }
        self.seal_file
            .as_mut()
            .ok_or(LifecycleError::SealWriteAll)?
            .write_all(content.as_bytes())
            .map_err(|_| LifecycleError::SealWriteAll)
    }

    fn flush_seal(&mut self) -> Result<(), LifecycleError> {
        self.seal_file
            .as_mut()
            .ok_or(LifecycleError::SealFlush)?
            .flush()
            .map_err(|_| LifecycleError::SealFlush)
    }

    fn sync_seal(&mut self) -> Result<(), LifecycleError> {
        self.seal_file
            .as_ref()
            .ok_or(LifecycleError::SealSync)?
            .sync_all()
            .map_err(|_| LifecycleError::SealSync)
    }

    fn close_seal(&mut self) -> Result<(), LifecycleError> {
        self.seal_file = None;
        Ok(())
    }

    fn verify_seal(&mut self) -> Result<bool, LifecycleError> {
        let path = self
            .cycle_dir
            .as_ref()
            .expect("cycle directory is set before finalization")
            .join("checksums.sha256");
        let content = std::fs::read_to_string(path).map_err(|_| LifecycleError::InvalidSeal)?;
        for line in content.lines() {
            let Some((hash, relative)) = line.split_once("  ") else {
                return Ok(false);
            };
            let bytes = std::fs::read(self.repository_root.join(relative))
                .map_err(|_| LifecycleError::InvalidSeal)?;
            if sha256_hex(&bytes) != hash {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn regular_file_count(&self) -> usize {
        self.regular_files
    }

    fn invoke_analyzer(&mut self) -> Result<(), LifecycleError> {
        match run_completed_analysis(&self.repository_root, self.cycle) {
            Ok(analysis) => {
                self.analysis = Some(analysis);
                Ok(())
            }
            Err(error) => {
                self.log(&format!("analyzer failed: {error}"));
                Err(LifecycleError::AnalyzerFailed)
            }
        }
    }
}

fn exit_outcome(status: std::process::ExitStatus) -> super::model::CommandOutcome {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(signal) = status.signal() {
            return super::model::CommandOutcome::Signaled { signal };
        }
    }
    super::model::CommandOutcome::Exited {
        exit_code: status.code().unwrap_or(-1),
    }
}

/// Constructs the plan for `cycle`, drives it through the real
/// [`LifecyclePort`] implementation, and returns `(exit_code, message)`
/// exactly per protocol Revision 11 §25.5's CLI result classes.
pub fn run_live(repository_root: PathBuf, cycle: CampaignCycle) -> (i32, String) {
    let plan = campaign_plan(cycle);
    let mut port = LivePort::new(repository_root, cycle);
    match super::core::run(&plan, &mut port) {
        Ok(super::model::Terminal::Completed) => {
            let analysis = port
                .analysis()
                .expect("analyzer runs before a successful Terminal::Completed");
            (
                i32::from(analysis.exit_code()),
                analysis.summary().to_owned(),
            )
        }
        Ok(super::model::Terminal::GateFailed(phase)) => (
            1,
            format!(
                "i61_campaign: cycle={} gate_failed={phase:?}\n",
                cycle.as_str()
            ),
        ),
        Err(error) => {
            let mut message = format!("i61_campaign: {}\n", format_lifecycle_error(&error));
            if let Some(detail) = port.last_error() {
                message.push_str(&format!("  detail: {detail}\n"));
            }
            (2, message)
        }
    }
}

fn format_lifecycle_error(error: &LifecycleError) -> String {
    match error.classified_reason() {
        LoggedText::Public(reason) | LoggedText::Redacted(reason) => reason,
    }
}

#[cfg(test)]
#[path = "live_port/tests.rs"]
mod tests;
