use crate::{
    candidate_digest, AdmissionClass, AuditVerdict, CandidateAuditEvidence, CandidateAuditRecord,
    Dataset, Engine, FrozenQuery, RawRecord, SessionMode, SessionSpec, RAW_SCHEMA_VERSION,
};

const WANDS_AUDIT_RECORDS: usize = 480;
const ESCI_AUDIT_RECORDS: usize = 600;

#[derive(Debug, Clone)]
pub(super) struct CompleteCandidateAuditFixture {
    pub(super) wands_workload: Vec<FrozenQuery>,
    pub(super) wands_records: Vec<CandidateAuditRecord>,
    pub(super) esci_workload: Vec<FrozenQuery>,
    pub(super) esci_records: Vec<CandidateAuditRecord>,
}

impl CompleteCandidateAuditFixture {
    pub(super) fn complete() -> Self {
        let (wands_workload, wands_records) = workload_and_audits(Dataset::Wands);
        let (esci_workload, esci_records) = workload_and_audits(Dataset::EsciElectronics);
        Self {
            wands_workload,
            wands_records,
            esci_workload,
            esci_records,
        }
    }

    pub(super) fn evidence(&self) -> [CandidateAuditEvidence<'_>; 2] {
        [
            CandidateAuditEvidence {
                dataset: Dataset::Wands,
                workload: &self.wands_workload,
                records: &self.wands_records,
            },
            CandidateAuditEvidence {
                dataset: Dataset::EsciElectronics,
                workload: &self.esci_workload,
                records: &self.esci_records,
            },
        ]
    }
}

fn workload_and_audits(dataset: Dataset) -> (Vec<FrozenQuery>, Vec<CandidateAuditRecord>) {
    let record_count = match dataset {
        Dataset::Wands => WANDS_AUDIT_RECORDS,
        Dataset::EsciElectronics => ESCI_AUDIT_RECORDS,
    };
    (0..record_count)
        .map(|index| {
            let admission_class = match index % 3 {
                0 => AdmissionClass::FastPath,
                1 => AdmissionClass::Hybrid,
                2 => AdmissionClass::Punt,
                _ => unreachable!("remainder modulo three is always 0..=2"),
            };
            let query_id = format!("{}-query-{index:03}", dataset.as_str());
            let candidate_ids = vec![format!("{}-product-{index:03}", dataset.as_str())];
            let digest = candidate_digest(&candidate_ids);
            (
                FrozenQuery {
                    query_id: query_id.clone(),
                    text: format!("fixture query {index}"),
                    admission_class,
                    structural_constraint_count: usize::from(!matches!(
                        admission_class,
                        AdmissionClass::Punt
                    )),
                    has_residual_lexical: !matches!(admission_class, AdmissionClass::FastPath),
                    rows: 10,
                    native: None,
                    solr: None,
                },
                CandidateAuditRecord {
                    dataset: dataset.as_str().to_owned(),
                    query_id,
                    admission_class,
                    native_count: Some(candidate_ids.len()),
                    native_digest: Some(digest.clone()),
                    engine_count: Some(candidate_ids.len()),
                    engine_digest: Some(digest),
                    verdict: AuditVerdict::Match,
                    only_native: Vec::new(),
                    only_engine: Vec::new(),
                    failure_reason: None,
                },
            )
        })
        .unzip()
}

pub(super) fn set_native_process_total(record: &mut RawRecord, total_usec: u64) {
    record.process_cpu_user_usec = Some(total_usec);
    record.process_cpu_system_usec = Some(0);
    record.process_cpu_total_usec = Some(total_usec);
    record.process_cgroup_disagreement_pct = Some(
        total_usec.abs_diff(record.cpu_usage_usec) as f64 * 100.0 / record.cpu_usage_usec as f64,
    );
}

pub(super) fn calibration_records() -> Vec<RawRecord> {
    crate::campaign_plan(crate::CampaignCycle::Run1)
        .sessions()
        .filter(|spec| spec.series().phase() == crate::CampaignPhase::Calibration)
        .map(record_for)
        .collect()
}

pub(super) fn record_for(spec: SessionSpec) -> RawRecord {
    let mode = spec.plan().mode();
    let (cpu_usage_usec, queries) = match mode {
        SessionMode::CalibrationFour => (400, 400),
        SessionMode::CalibrationFive => (500, 500),
        SessionMode::Warm => (1_000, 10),
        SessionMode::Cold => (2_000, 10),
    };
    let process_cpu = (spec.engine() == Engine::Native).then_some(cpu_usage_usec);
    RawRecord {
        schema_version: RAW_SCHEMA_VERSION,
        experiment_id: "I61-E1".to_owned(),
        run_id: "seed-61".to_owned(),
        rep: spec.block_index().get(),
        engine_order: spec.slot().index(),
        calibration: mode.is_calibration(),
        engine: spec.engine().as_str().to_owned(),
        dataset: spec.series().dataset().as_str().to_owned(),
        query_class: spec.series().projection().as_str().to_owned(),
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
    }
}
