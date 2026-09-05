use issue61_eval::{read_jsonl, write_jsonl, RawError, RawRecord, RAW_SCHEMA_VERSION};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let id = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!("issue61-raw-{}-{id}", std::process::id()));
        std::fs::create_dir_all(&root).expect("fixture root should be created");
        Self { root }
    }

    fn path(&self) -> PathBuf {
        self.root.join("raw.jsonl")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn record() -> RawRecord {
    RawRecord {
        schema_version: RAW_SCHEMA_VERSION,
        experiment_id: "I61-E1".to_owned(),
        run_id: "run-1".to_owned(),
        rep: 3,
        engine_order: 1,
        calibration: false,
        engine: "native".to_owned(),
        dataset: "wands".to_owned(),
        query_class: "structural".to_owned(),
        regime: "warm".to_owned(),
        queries: 100,
        wall_elapsed_us: 2_000,
        timer_floor_clock_resolution_ns: 10.0,
        timer_floor_instant_now_overhead_ns: 20.0,
        timer_floor_effective_ns: 20.0,
        cpu_usage_usec: 1_500,
        cpu_user_usec: 1_200,
        cpu_system_usec: 300,
        cpu_nr_periods: 10,
        cpu_nr_throttled: 2,
        cpu_throttled_usec: 25,
        cpu_pressure_some_usec: 7,
        cpu_pressure_full_usec: 3,
        cgroup_memory_footprint_bytes: 4_096,
        cgroup_memory_current_median_bytes: 4_000,
        cgroup_memory_current_max_bytes: 4_500,
        cgroup_memory_peak_bytes: 8_192,
        memory_anon_bytes: 2_000,
        memory_file_bytes: 1_000,
        memory_kernel_bytes: 500,
        memory_sock_bytes: 64,
        memory_swap_current_bytes: 128,
        memory_swap_peak_bytes: 256,
        memory_events_low: 1,
        memory_events_high: 2,
        memory_events_max: 3,
        memory_events_oom: 4,
        memory_events_oom_kill: 5,
        memory_events_oom_group_kill: 6,
        memory_swap_events_high: 7,
        memory_swap_events_max: 8,
        memory_swap_events_fail: 9,
        cpuset_cpus_effective: "0-2".to_owned(),
        cpu_max: "300000 100000".to_owned(),
        memory_max: "6442450944".to_owned(),
        memory_swap_max: "1024".to_owned(),
        index_serialized_bytes: 16_384,
        latency_p50_us: 10.0,
        latency_p95_us: 20.0,
        latency_p99_us: 30.0,
        latency_mean_us: 12.5,
        steal_pct: 0.25,
        excluded: false,
        exclusion_reason: None,
        git_sha: "abc123".to_owned(),
        host: "fixture-host".to_owned(),
        timestamp_utc: "2026-09-04T00:00:00Z".to_owned(),
    }
}

#[test]
fn raw_record_roundtrip_preserves_all_fields_and_schema_version() {
    let fixture = Fixture::new();
    let expected = record();
    write_jsonl(&fixture.path(), std::slice::from_ref(&expected))
        .expect("record should be written");

    let actual = read_jsonl(&fixture.path()).expect("record should be read");

    assert_eq!(actual, vec![expected]);
    assert_eq!(actual[0].schema_version, RAW_SCHEMA_VERSION);
}

#[test]
fn reading_a_future_schema_version_is_rejected_not_silently_accepted() {
    let fixture = Fixture::new();
    let mut future = record();
    future.schema_version = RAW_SCHEMA_VERSION + 1;
    write_jsonl(&fixture.path(), &[future]).expect("future record should be written");

    let error = read_jsonl(&fixture.path()).expect_err("future schema must be rejected");

    assert!(matches!(
        error,
        RawError::UnsupportedSchemaVersion {
            found,
            expected: RAW_SCHEMA_VERSION
        } if found == RAW_SCHEMA_VERSION + 1
    ));
}

#[test]
fn non_v3_schema_is_rejected_before_record_shape_is_parsed() {
    // Given
    let fixture = Fixture::new();
    std::fs::write(fixture.path(), "{\"schema_version\":2}\n")
        .expect("legacy-shaped fixture should be written");

    // When
    let error = read_jsonl(&fixture.path()).expect_err("v2 schema must be rejected first");

    // Then
    assert!(matches!(
        error,
        RawError::UnsupportedSchemaVersion {
            found: 2,
            expected: RAW_SCHEMA_VERSION
        }
    ));
}

#[test]
fn cpu_us_per_query_is_none_for_zero_queries() {
    let mut raw = record();
    raw.queries = 0;

    assert_eq!(raw.cpu_us_per_query(), None);
}

#[test]
fn write_jsonl_appends_rather_than_truncating() {
    let fixture = Fixture::new();
    let raw = record();
    write_jsonl(&fixture.path(), std::slice::from_ref(&raw)).expect("first append should work");
    write_jsonl(&fixture.path(), std::slice::from_ref(&raw)).expect("second append should work");

    let records = read_jsonl(&fixture.path()).expect("appended records should parse");

    assert_eq!(records, vec![raw.clone(), raw]);
}

#[test]
fn blank_lines_are_skipped() {
    let fixture = Fixture::new();
    let serialized = serde_json::to_string(&record()).expect("record should serialize");
    std::fs::write(fixture.path(), format!("\n{serialized}\n   \n"))
        .expect("fixture should be written");

    let records = read_jsonl(&fixture.path()).expect("blank lines should be ignored");

    assert_eq!(records, vec![record()]);
}
