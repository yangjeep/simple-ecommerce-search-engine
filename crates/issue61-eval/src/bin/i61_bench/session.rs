use super::bench_request::{workload_pass, Config, QueryObservation};
use bench_harness::Distribution;
use issue61_eval::{
    read_proc_stat, steal_percent, CgroupReader, CgroupSnapshot, CpuTimes, FrozenQuery, RawRecord,
    SessionStep, RAW_SCHEMA_VERSION, WARM_SESSION_STEPS,
};
use std::error::Error;
use std::path::Path;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

pub(super) fn execute_warm_session<E>(
    mut execute: impl FnMut(SessionStep) -> Result<(), E>,
) -> Result<(), E> {
    for step in WARM_SESSION_STEPS {
        execute(step)?;
    }
    Ok(())
}

pub(super) fn measure_warm_session(
    agent: &ureq::Agent,
    config: &Config,
    workload: &[FrozenQuery],
) -> Result<RawRecord, Box<dyn Error>> {
    let cgroup = CgroupReader::at_dir(config.engine_cgroup.clone());
    let mut runner = WarmSessionRunner {
        agent,
        config,
        workload,
        cgroup: &cgroup,
        cpu_before: None,
        steal_before: None,
        wall_started: None,
        observations: Vec::with_capacity(workload.len() * 2),
        record: None,
    };
    execute_warm_session(|step| runner.execute(step))?;
    runner
        .record
        .ok_or_else(|| "warm session ended without closing counters".into())
}

struct WarmSessionRunner<'a> {
    agent: &'a ureq::Agent,
    config: &'a Config,
    workload: &'a [FrozenQuery],
    cgroup: &'a CgroupReader,
    cpu_before: Option<CgroupSnapshot>,
    steal_before: Option<CpuTimes>,
    wall_started: Option<Instant>,
    observations: Vec<QueryObservation>,
    record: Option<RawRecord>,
}

impl WarmSessionRunner<'_> {
    fn execute(&mut self, step: SessionStep) -> Result<(), Box<dyn Error>> {
        match step {
            SessionStep::WarmupPass => {
                workload_pass(
                    self.agent,
                    &self.config.engine_url,
                    self.workload,
                    self.config.engine,
                )?;
            }
            SessionStep::OpenCounters => {
                self.cpu_before = Some(self.cgroup.snapshot()?);
                self.steal_before = Some(read_proc_stat(Path::new("/proc"))?);
                self.wall_started = Some(Instant::now());
            }
            SessionStep::MeasuredPass => self.observations.extend(workload_pass(
                self.agent,
                &self.config.engine_url,
                self.workload,
                self.config.engine,
            )?),
            SessionStep::CloseCounters => self.close_counters()?,
        }
        Ok(())
    }

    fn close_counters(&mut self) -> Result<(), Box<dyn Error>> {
        let wall_elapsed_us = u64::try_from(
            self.wall_started
                .take()
                .ok_or("counters were not opened")?
                .elapsed()
                .as_micros(),
        )?;
        let steal_after = read_proc_stat(Path::new("/proc"))?;
        let cpu_after = self.cgroup.snapshot()?;
        let delta = cpu_after.delta_since(
            &self
                .cpu_before
                .take()
                .ok_or("CPU counters were not opened")?,
        )?;
        let steal_before = self
            .steal_before
            .take()
            .ok_or("steal counters were not opened")?;
        let latencies = self
            .observations
            .iter()
            .map(|observation| observation.latency_us)
            .collect::<Vec<_>>();
        let _num_found_by_query = self
            .observations
            .iter()
            .map(|observation| observation.num_found)
            .collect::<Vec<_>>();
        let distribution = Distribution::compute(&latencies);
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)?
            .as_secs()
            .to_string();
        self.record = Some(RawRecord {
            schema_version: RAW_SCHEMA_VERSION,
            experiment_id: "I61-E1".to_string(),
            run_id: format!("seed-{}", self.config.seed),
            rep: self.config.rep,
            engine_order: self.config.engine_order,
            calibration: false,
            engine: self.config.engine.as_str().to_string(),
            dataset: self.config.dataset.clone(),
            query_class: "all".to_string(),
            regime: self.config.session_mode.as_str().to_string(),
            queries: u64::try_from(self.observations.len())?,
            wall_elapsed_us,
            cpu_usage_usec: delta.usage_usec,
            cpu_user_usec: delta.user_usec,
            cpu_system_usec: delta.system_usec,
            cgroup_memory_footprint_bytes: cpu_after.memory_current_bytes,
            cgroup_memory_peak_bytes: cpu_after.memory_peak_bytes,
            index_serialized_bytes: 0,
            latency_p50_us: distribution.p50,
            latency_p95_us: distribution.p95,
            latency_p99_us: distribution.p99,
            latency_mean_us: distribution.mean,
            steal_pct: steal_percent(&steal_before, &steal_after),
            excluded: false,
            exclusion_reason: None,
            git_sha: std::env::var("GIT_SHA").unwrap_or_else(|_| "unknown".to_string()),
            host: std::env::var("HOSTNAME").unwrap_or_else(|_| "unknown".to_string()),
            timestamp_utc: timestamp,
        });
        Ok(())
    }
}
