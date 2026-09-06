#[path = "support/i61_analyzer.rs"]
mod analyzer;
#[path = "support/i61_fixture/mod.rs"]
mod fixture;

use analyzer::analyze_completed;
use fixture::CompletedCycle;
use std::path::Path;

const RAW_NULLABLE_FIELDS: [&str; 7] = [
    "native_cgroup_host_pid",
    "native_pid_namespace",
    "process_cpu_user_usec",
    "process_cpu_system_usec",
    "process_cpu_total_usec",
    "process_cgroup_disagreement_pct",
    "exclusion_reason",
];

const AUDIT_NULLABLE_FIELDS: [&str; 5] = [
    "native_count",
    "native_digest",
    "engine_count",
    "engine_digest",
    "failure_reason",
];

const WORKLOADS: [(&str, &str); 2] = [
    (
        "benchmarks/workloads/i61_wands_480.jsonl",
        "checksum seal has wrong frozen hash for benchmarks/workloads/i61_wands_480.jsonl",
    ),
    (
        "benchmarks/workloads/i61_esci_electronics.jsonl",
        "checksum seal has wrong frozen hash for benchmarks/workloads/i61_esci_electronics.jsonl",
    ),
];

#[test]
fn post_run_rejects_every_omitted_raw_nullable_key() {
    // Given / When / Then
    for field in RAW_NULLABLE_FIELDS {
        assert_missing_key_rejected("raw.jsonl", field, |fixture| {
            fixture.input_path("raw.jsonl")
        });
    }
}

#[test]
fn post_run_rejects_every_omitted_candidate_audit_nullable_key() {
    // Given / When / Then
    for field in AUDIT_NULLABLE_FIELDS {
        assert_missing_key_rejected("candidate_audit_wands.jsonl", field, |fixture| {
            fixture.input_path("candidate_audit_wands.jsonl")
        });
    }
}

#[test]
fn sealed_cycles_reject_each_omitted_workload_engine_block() {
    for (relative, detail) in WORKLOADS {
        for field in ["native", "solr"] {
            let fixture = CompletedCycle::run1();
            remove_first_record_key(&fixture.root().join(relative), field);
            fixture.reseal();

            assert_analyzer_rejection(&fixture, detail);
        }
    }
}

#[test]
fn sealed_cycles_reject_each_null_workload_engine_block() {
    for (relative, detail) in WORKLOADS {
        for field in ["native", "solr"] {
            let fixture = CompletedCycle::run1();
            set_first_record_key_to_null(&fixture.root().join(relative), field);
            fixture.reseal();

            assert_analyzer_rejection(&fixture, detail);
        }
    }
}

fn assert_missing_key_rejected(
    diagnostic_file: &'static str,
    field: &str,
    path: impl FnOnce(&CompletedCycle) -> std::path::PathBuf,
) {
    let mut fixture = CompletedCycle::run1();
    fixture.rewrite_records(|_| {});
    remove_first_record_key(&path(&fixture), field);
    fixture.reseal();

    let output = analyze_completed(&fixture);

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert_eq!(
        output.stderr,
        format!("i61_analyze: {diagnostic_file} line 1 is missing field {field}\n").as_bytes()
    );
    assert!(!fixture.analysis_path().exists());
}

fn remove_first_record_key(path: &Path, field: &str) {
    rewrite_first_record(path, |record| {
        assert!(
            record
                .as_object_mut()
                .expect("record is an object")
                .remove(field)
                .is_some(),
            "field {field} exists"
        );
    });
}

fn set_first_record_key_to_null(path: &Path, field: &str) {
    rewrite_first_record(path, |record| {
        let slot = record
            .get_mut(field)
            .unwrap_or_else(|| panic!("field {field} exists"));
        *slot = serde_json::Value::Null;
    });
}

fn assert_analyzer_rejection(fixture: &CompletedCycle, detail: &str) {
    let output = analyze_completed(fixture);
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert_eq!(output.stderr, format!("i61_analyze: {detail}\n").as_bytes());
    assert!(!fixture.analysis_path().exists());
}

fn rewrite_first_record(path: &Path, mutate: impl FnOnce(&mut serde_json::Value)) {
    let text = std::fs::read_to_string(path).expect("fixture evidence is readable");
    let mut lines = text.lines();
    let first = lines.next().expect("fixture has a first record");
    let mut record: serde_json::Value = serde_json::from_str(first).expect("record is JSON");
    mutate(&mut record);
    let mut rewritten = serde_json::to_string(&record).expect("record serializes");
    rewritten.push('\n');
    for line in lines {
        rewritten.push_str(line);
        rewritten.push('\n');
    }
    std::fs::write(path, rewritten).expect("fixture evidence is rewritten");
}
