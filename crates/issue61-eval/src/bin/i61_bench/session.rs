use super::bench_request::{workload_pass, Config, QueryObservation};
use bench_harness::Distribution;
use issue61_eval::{
    measure_timer_floor, read_proc_stat, steal_percent, validate_measurement_window, CgroupReader,
    CgroupSnapshot, CpuTimes, MemorySampler, ProjectedWorkload, RawRecord, SessionPlan,
    SessionStep, TimerFloor, RAW_SCHEMA_VERSION,
};
use std::error::Error;
use std::path::Path;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

pub(super) fn execute_session<E>(
    plan: SessionPlan,
    mut execute: impl FnMut(SessionStep) -> Result<(), E>,
) -> Result<(), E> {
    for step in plan.steps() {
        execute(step)?;
    }
    Ok(())
}

pub(super) fn measure_session(
    agent: &ureq::Agent,
    config: &Config,
    workload: ProjectedWorkload<'_>,
) -> Result<RawRecord, Box<dyn Error>> {
    let cgroup = CgroupReader::at_dir(config.engine_cgroup.clone());
    let timer_floor = measure_timer_floor(10_000);
    let mut runner = SessionRunner {
        agent,
        config,
        workload,
        cgroup: &cgroup,
        timer_floor,
        cpu_before: None,
        memory_sampler: None,
        steal_before: None,
        wall_started: None,
        observations: Vec::with_capacity(workload.query_count() * config.plan.pass_counts().1),
        record: None,
    };
    execute_session(config.plan, |step| runner.execute(step))?;
    runner
        .record
        .ok_or_else(|| "session ended without closing counters".into())
}

struct SessionRunner<'a> {
    agent: &'a ureq::Agent,
    config: &'a Config,
    workload: ProjectedWorkload<'a>,
    cgroup: &'a CgroupReader,
    timer_floor: TimerFloor,
    cpu_before: Option<CgroupSnapshot>,
    memory_sampler: Option<MemorySampler>,
    steal_before: Option<CpuTimes>,
    wall_started: Option<Instant>,
    observations: Vec<QueryObservation>,
    record: Option<RawRecord>,
}

impl SessionRunner<'_> {
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
                self.memory_sampler = Some(MemorySampler::start(self.cgroup.clone())?);
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
        let wall_elapsed = self
            .wall_started
            .take()
            .ok_or("counters were not opened")?
            .elapsed();
        let wall_elapsed_us = u64::try_from(wall_elapsed.as_micros())?;
        let wall_elapsed_ns = u64::try_from(wall_elapsed.as_nanos())?;
        let memory_samples = self
            .memory_sampler
            .take()
            .ok_or("memory sampler was not opened")?
            .finish()?;
        let steal_after = read_proc_stat(Path::new("/proc"))?;
        let cpu_after = self.cgroup.snapshot()?;
        let delta = cpu_after.delta_since(
            &self
                .cpu_before
                .take()
                .ok_or("CPU counters were not opened")?,
        )?;
        validate_measurement_window(wall_elapsed_ns, delta.usage_usec, &self.timer_floor)?;
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
            run_id: format!("seed-{}", self.config.seed.get()),
            rep: self.config.block.get(),
            engine_order: self.config.order.index(),
            calibration: self.config.plan.mode().is_calibration(),
            engine: self.config.engine.as_str().to_string(),
            dataset: self.config.dataset.as_str().to_string(),
            query_class: self.config.projection.as_str().to_string(),
            regime: self.config.plan.mode().as_str().to_string(),
            queries: u64::try_from(self.observations.len())?,
            wall_elapsed_us,
            timer_floor_clock_resolution_ns: self.timer_floor.clock_resolution_ns,
            timer_floor_instant_now_overhead_ns: self.timer_floor.instant_now_overhead_ns,
            timer_floor_effective_ns: self.timer_floor.effective_ns(),
            cpu_usage_usec: delta.usage_usec,
            cpu_user_usec: delta.user_usec,
            cpu_system_usec: delta.system_usec,
            cpu_nr_periods: delta.nr_periods,
            cpu_nr_throttled: delta.nr_throttled,
            cpu_throttled_usec: delta.throttled_usec,
            cpu_pressure_some_usec: delta.cpu_pressure_some_usec,
            cpu_pressure_full_usec: delta.cpu_pressure_full_usec,
            cgroup_memory_footprint_bytes: cpu_after.memory_current_bytes,
            cgroup_memory_current_median_bytes: memory_samples.median_bytes,
            cgroup_memory_current_max_bytes: memory_samples.max_bytes,
            cgroup_memory_peak_bytes: cpu_after.memory_peak_bytes,
            memory_anon_bytes: cpu_after.memory_anon_bytes,
            memory_file_bytes: cpu_after.memory_file_bytes,
            memory_kernel_bytes: cpu_after.memory_kernel_bytes,
            memory_sock_bytes: cpu_after.memory_sock_bytes,
            memory_swap_current_bytes: cpu_after.memory_swap_current_bytes,
            memory_swap_peak_bytes: cpu_after.memory_swap_peak_bytes,
            memory_events_low: delta.memory_events.low,
            memory_events_high: delta.memory_events.high,
            memory_events_max: delta.memory_events.max,
            memory_events_oom: delta.memory_events.oom,
            memory_events_oom_kill: delta.memory_events.oom_kill,
            memory_events_oom_group_kill: delta.memory_events.oom_group_kill,
            memory_swap_events_high: delta.memory_swap_events.high,
            memory_swap_events_max: delta.memory_swap_events.max,
            memory_swap_events_fail: delta.memory_swap_events.fail,
            cpuset_cpus_effective: cpu_after.cpuset_cpus_effective,
            cpu_max: cpu_after.cpu_max,
            memory_max: cpu_after.memory_max,
            memory_swap_max: cpu_after.memory_swap_max,
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
