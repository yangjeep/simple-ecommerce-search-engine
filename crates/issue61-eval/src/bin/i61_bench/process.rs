use issue61_eval::{CgroupReader, Engine, NativePidIdentity, ProcessCpuDelta, ProcessCpuSnapshot};
use std::error::Error;
use std::path::Path;

pub(crate) fn process_snapshot_for<E>(
    engine: Engine,
    fetch: impl FnOnce() -> Result<ProcessCpuSnapshot, E>,
) -> Result<Option<ProcessCpuSnapshot>, E> {
    match engine {
        Engine::Native => fetch().map(Some),
        Engine::Solr => Ok(None),
    }
}

pub(super) fn fetch_process_snapshot(
    agent: &ureq::Agent,
    base_url: &str,
) -> Result<ProcessCpuSnapshot, String> {
    let url = format!("{}/rusage", base_url.trim_end_matches('/'));
    let response = agent
        .get(&url)
        .call()
        .map_err(|error| format!("rusage HTTP request failed: {error}"))?;
    if response.status() != 200 {
        return Err(format!("rusage HTTP status {}", response.status()));
    }
    response
        .into_json::<ProcessCpuSnapshot>()
        .map_err(|error| format!("parse rusage response: {error}"))
}

pub(super) fn validate_process_scope(
    cgroup: &CgroupReader,
    proc_root: &Path,
    process: Option<&ProcessCpuSnapshot>,
) -> Result<Option<NativePidIdentity>, Box<dyn Error>> {
    if let Some(process) = process {
        let cgroup_host_pid = cgroup.single_process_id()?;
        return NativePidIdentity::read(proc_root, cgroup_host_pid, process.pid)
            .map(Some)
            .map_err(Into::into);
    }
    Ok(None)
}

pub(super) fn reconcile_process_cpu(
    before: Option<ProcessCpuSnapshot>,
    after: Option<ProcessCpuSnapshot>,
    cgroup_usec: u64,
) -> Result<Option<(ProcessCpuDelta, f64)>, Box<dyn Error>> {
    match (before, after) {
        (Some(before), Some(after)) => {
            let delta = after.delta_since(&before)?;
            let disagreement = delta.reconcile_cgroup(cgroup_usec)?;
            Ok(Some((delta, disagreement)))
        }
        (None, None) => Ok(None),
        (Some(_), None) | (None, Some(_)) => {
            Err("native process snapshots must exist at both boundaries".into())
        }
    }
}
