use crate::{
    analyze_calibration, campaign_plan, AnalysisError, CampaignCycle, CampaignPhase,
    CampaignSeries, Engine, RawRecord, SessionMode, SessionSpec, RAW_SCHEMA_VERSION,
};

fn calibration_records() -> Vec<RawRecord> {
    campaign_plan(CampaignCycle::Run1)
        .sessions()
        .filter(|spec| spec.series().phase() == CampaignPhase::Calibration)
        .map(record_for)
        .collect()
}

fn record_for(spec: SessionSpec) -> RawRecord {
    let mode = spec.plan().mode();
    let (cpu_usage_usec, queries) = match mode {
        SessionMode::CalibrationFour => (400, 400),
        SessionMode::CalibrationFive => (500, 500),
        SessionMode::Warm | SessionMode::Cold => (1, 1),
    };
    assert!(matches!(
        spec.series(),
        CampaignSeries::Calibration { engine: _ }
    ));
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
        process_cpu_user_usec: None,
        process_cpu_system_usec: None,
        process_cpu_total_usec: None,
        process_cgroup_disagreement_pct: None,
        cgroup_memory_footprint_bytes: 0,
        cgroup_memory_current_median_bytes: 0,
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
        index_serialized_bytes: 0,
        latency_p50_us: 1.0,
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

#[derive(Clone, Copy)]
enum KeyMutation {
    Calibration,
    Engine,
    Dataset,
    QueryClass,
    Regime,
    Rep,
    EngineOrder,
}

fn mutate_key(record: &mut RawRecord, mutation: KeyMutation) {
    match mutation {
        KeyMutation::Calibration => record.calibration = false,
        KeyMutation::Engine => {
            record.engine = match record.engine.as_str() {
                "native" => Engine::Solr.as_str(),
                "solr" => Engine::Native.as_str(),
                _ => Engine::Native.as_str(),
            }
            .to_owned();
        }
        KeyMutation::Dataset => record.dataset = "esci_electronics".to_owned(),
        KeyMutation::QueryClass => record.query_class = "fast-path".to_owned(),
        KeyMutation::Regime => record.regime = SessionMode::Warm.as_str().to_owned(),
        KeyMutation::Rep => record.rep = 30,
        KeyMutation::EngineOrder => record.engine_order = 1 - record.engine_order,
    }
}

#[test]
fn calibration_pairs_by_typed_identity_when_records_are_reversed() {
    // Given
    let mut records = calibration_records();
    records.reverse();

    // When
    let calibrations = match analyze_calibration(CampaignCycle::Run1, &records) {
        Ok(calibrations) => calibrations,
        Err(error) => panic!("canonical reversed records must validate: {error}"),
    };

    // Then
    for check in [calibrations.native, calibrations.solr] {
        let check = match check {
            Some(check) => check,
            None => panic!("both engine calibrations must be present"),
        };
        assert_eq!(check.expected_ratio, 1.25);
        assert_eq!(check.observed.n_blocks, 30);
        assert_eq!(check.observed.point_ratio, 1.25);
        assert!(check.passed);
    }
}

#[test]
fn calibration_rejects_each_mutated_canonical_key_field() {
    for mutation in [
        KeyMutation::Calibration,
        KeyMutation::Engine,
        KeyMutation::Dataset,
        KeyMutation::QueryClass,
        KeyMutation::Regime,
        KeyMutation::Rep,
        KeyMutation::EngineOrder,
    ] {
        // Given
        let mut records = calibration_records();
        mutate_key(&mut records[0], mutation);

        // When
        let result = analyze_calibration(CampaignCycle::Run1, &records);

        // Then
        assert!(result.is_err());
    }
}

#[test]
fn calibration_rejects_missing_duplicate_and_malformed_identities() {
    // Given / When / Then: missing
    let mut missing = calibration_records();
    missing.pop();
    assert!(matches!(
        analyze_calibration(CampaignCycle::Run1, &missing),
        Err(AnalysisError::MissingIdentity { .. })
    ));

    // Given / When / Then: duplicate
    let mut duplicate = calibration_records();
    duplicate.push(duplicate[0].clone());
    assert!(matches!(
        analyze_calibration(CampaignCycle::Run1, &duplicate),
        Err(AnalysisError::DuplicateIdentity { .. })
    ));

    // Given / When / Then: malformed
    let mut malformed = calibration_records();
    malformed[0].engine = "not-an-engine".to_owned();
    assert!(matches!(
        analyze_calibration(CampaignCycle::Run1, &malformed),
        Err(AnalysisError::MalformedIdentity { .. })
    ));
}

#[test]
fn calibration_rejects_invalid_record_envelope() {
    let mutations: &[fn(&mut RawRecord)] = &[
        |record| record.schema_version += 1,
        |record| record.excluded = true,
        |record| record.exclusion_reason = Some("fixture exclusion".to_owned()),
        |record| record.experiment_id = "wrong-experiment".to_owned(),
        |record| record.run_id = "wrong-run".to_owned(),
    ];
    for mutate in mutations {
        // Given
        let mut records = calibration_records();
        mutate(&mut records[0]);

        // When
        let result = analyze_calibration(CampaignCycle::Run1, &records);

        // Then
        assert!(result.is_err());
    }
}
