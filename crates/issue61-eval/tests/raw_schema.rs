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
        engine: "native".to_owned(),
        dataset: "wands".to_owned(),
        query_class: "structural".to_owned(),
        regime: "warm".to_owned(),
        queries: 100,
        wall_elapsed_us: 2_000,
        cpu_usage_usec: 1_500,
        cpu_user_usec: 1_200,
        cpu_system_usec: 300,
        rss_current_bytes: 4_096,
        rss_peak_bytes: 8_192,
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
