use crate::lifecycle::model::{RawRecord, SlotContext};
use crate::{Engine, SessionMode, RAW_SCHEMA_VERSION};

pub(super) fn record_for(
    slot: SlotContext,
    calibration_passes: bool,
    duplicate_calibration_identity: bool,
) -> RawRecord {
    let spec = slot.block.campaign_series;
    let mode = slot.session.plan().mode();
    let (cpu_usage_usec, queries) = match mode {
        SessionMode::CalibrationFour => (400, 400),
        SessionMode::CalibrationFive if calibration_passes => (500, 500),
        SessionMode::CalibrationFive => (400, 500),
        SessionMode::Warm => (1_000, 10),
        SessionMode::Cold => (2_000, 10),
    };
    let engine = slot.session.engine();
    let process_cpu = (engine == Engine::Native).then_some(cpu_usage_usec);
    let mut record = RawRecord {
        schema_version: RAW_SCHEMA_VERSION,
        experiment_id: "I61-E1".to_owned(),
        run_id: "seed-61".to_owned(),
        rep: slot.block.block_index,
        engine_order: usize::from(slot.slot.get()),
        calibration: mode.is_calibration(),
        engine: engine.as_str().to_owned(),
        dataset: spec.dataset().as_str().to_owned(),
        query_class: spec.projection().as_str().to_owned(),
        regime: mode.as_str().to_owned(),
        queries,
        wall_elapsed_us: 1,
        timer_floor_clock_resolution_ns: 1.0,
        timer_floor_instant_now_overhead_ns: 1.0,
        timer_floor_effective_ns: 1.0,
        cpu_usage_usec,
        cpu_user_usec: cpu_usage_usec,
        cpu_system_usec: 0,
        cpu_nr_periods: 0,
        cpu_nr_throttled: 0,
        cpu_throttled_usec: 0,
        cpu_pressure_some_usec: 0,
        cpu_pressure_full_usec: 0,
        native_cgroup_host_pid: None,
        native_pid_namespace: None,
        process_cpu_user_usec: process_cpu,
        process_cpu_system_usec: process_cpu.map(|_| 0),
        process_cpu_total_usec: process_cpu,
        process_cgroup_disagreement_pct: process_cpu.map(|_| 0.0),
        cgroup_memory_footprint_bytes: 9_999,
        cgroup_memory_current_median_bytes: 300,
        cgroup_memory_current_max_bytes: 0,
        cgroup_memory_peak_bytes: 0,
        memory_anon_bytes: 0,
        memory_file_bytes: 0,
        memory_kernel_bytes: 0,
        memory_sock_bytes: 0,
        memory_swap_current_bytes: 0,
        memory_swap_peak_bytes: 0,
        memory_events_low: 0,
        memory_events_high: 0,
        memory_events_max: 0,
        memory_events_oom: 0,
        memory_events_oom_kill: 0,
        memory_events_oom_group_kill: 0,
        memory_swap_events_high: 0,
        memory_swap_events_max: 0,
        memory_swap_events_fail: 0,
        cpuset_cpus_effective: "0".to_owned(),
        cpu_max: "max 100000".to_owned(),
        memory_max: "max".to_owned(),
        memory_swap_max: "max".to_owned(),
        index_serialized_bytes: 7_777,
        latency_p50_us: 20.0,
        latency_p95_us: 1.0,
        latency_p99_us: 1.0,
        latency_mean_us: 1.0,
        steal_pct: 0.0,
        excluded: false,
        exclusion_reason: None,
        git_sha: "fixture".to_owned(),
        host: "fixture".to_owned(),
        timestamp_utc: "2026-01-01T00:00:00Z".to_owned(),
    };
    let duplicates_second_slot = match spec {
        crate::CampaignSeries::Calibration {
            engine: Engine::Native,
        } => slot.block.block_index == 0 && slot.slot.get() == 0,
        crate::CampaignSeries::Calibration {
            engine: Engine::Solr,
        }
        | crate::CampaignSeries::Warm { .. }
        | crate::CampaignSeries::Cold { .. } => false,
    };
    if duplicate_calibration_identity && duplicates_second_slot {
        record.engine_order = 1;
        record.regime = match mode {
            SessionMode::CalibrationFour => SessionMode::CalibrationFive,
            SessionMode::CalibrationFive => SessionMode::CalibrationFour,
            SessionMode::Warm | SessionMode::Cold => mode,
        }
        .as_str()
        .to_owned();
    }
    record
}
