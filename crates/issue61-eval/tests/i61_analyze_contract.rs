#[path = "support/i61_analyzer.rs"]
mod analyzer;
#[path = "support/i61_fixture/mod.rs"]
mod fixture;

use analyzer::analyze_completed;
use fixture::CompletedCycle;
use issue61_eval::{Dataset, Engine, SessionMode, WorkloadProjection};
use std::process::Output;

fn assert_failure(output: &Output, fixture: &CompletedCycle, detail: &str) {
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert_eq!(output.stderr, format!("i61_analyze: {detail}\n").as_bytes());
    assert!(!fixture.analysis_path().exists());
}

fn report(fixture: &CompletedCycle) -> serde_json::Value {
    serde_json::from_slice(&std::fs::read(fixture.analysis_path()).expect("report is readable"))
        .expect("report is JSON")
}

#[test]
fn checksum_verification_precedes_semantic_parse() {
    // Given
    let fixture = CompletedCycle::run1();
    std::fs::write(fixture.input_path("raw.jsonl"), b"not JSON\n")
        .expect("raw evidence is corrupted after sealing");

    // When
    let output = analyze_completed(&fixture);

    // Then
    assert_failure(
        &output,
        &fixture,
        "checksum mismatch for artifacts/issue61/i61_e1_run1/raw.jsonl",
    );
}

#[test]
fn malformed_unsorted_extra_and_self_referential_manifests_are_rejected() {
    // Given / When / Then
    for (change, detail) in [
        (
            malformed_manifest as fn(&str) -> String,
            "malformed checksum seal",
        ),
        (
            unsorted_manifest,
            "checksum seal paths are not bytewise sorted",
        ),
        (
            extra_manifest,
            "checksum seal contains unexpected path unexpected.txt",
        ),
        (
            self_referential_manifest,
            "checksum seal must not reference itself",
        ),
    ] {
        let fixture = CompletedCycle::run1();
        let path = fixture.manifest_path();
        let original = std::fs::read_to_string(&path).expect("manifest is readable");
        std::fs::write(path, change(&original)).expect("invalid manifest is written");
        let output = analyze_completed(&fixture);
        assert_failure(&output, &fixture, detail);
    }
}

#[test]
fn output_collision_preserves_existing_bytes() {
    // Given
    let fixture = CompletedCycle::run1();
    let sentinel = b"existing report must survive\n";
    std::fs::write(fixture.analysis_path(), sentinel).expect("collision is created");

    // When
    let output = analyze_completed(&fixture);

    // Then
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert_eq!(
        output.stderr,
        b"i61_analyze: analysis.json already exists\n"
    );
    assert_eq!(
        std::fs::read(fixture.analysis_path()).expect("collision remains"),
        sentinel
    );
}

#[test]
fn complete_keep_cycle_has_frozen_summary_shape_and_deterministic_bytes() {
    // Given
    let first = CompletedCycle::run1();
    let second = CompletedCycle::run1();

    // When
    let first_output = analyze_completed(&first);
    let second_output = analyze_completed(&second);

    // Then
    for output in [first_output, second_output] {
        assert_eq!(output.status.code(), Some(0));
        assert_eq!(output.stdout, b"i61_analyze: cycle=run1 verdict=KEEP passing_dataset=null blocked_dataset=null exit_code=0\n");
        assert!(output.stderr.is_empty());
    }
    let first_bytes = std::fs::read(first.analysis_path()).expect("first report is readable");
    let second_bytes = std::fs::read(second.analysis_path()).expect("second report is readable");
    assert_eq!(first_bytes, second_bytes);
    assert!(first_bytes.ends_with(b"\n"));
    assert!(!first_bytes.ends_with(b"\n\n"));
    let text = std::str::from_utf8(&first_bytes).expect("report is UTF-8");
    let keys = text
        .lines()
        .filter_map(|line| {
            line.strip_prefix("  \"")?
                .split_once('\"')
                .map(|item| item.0)
        })
        .collect::<Vec<_>>();
    assert_eq!(
        keys,
        [
            "schema_version",
            "experiment_id",
            "cycle",
            "inputs",
            "decision",
            "global_gates",
            "calibrations",
            "candidate_audit",
            "process_cpu",
            "warm_cells",
            "exact_index_cells",
            "cold",
        ]
    );
    let parsed = report(&first);
    assert_eq!(parsed["schema_version"], 1);
    assert_eq!(parsed["experiment_id"], "I61-E1");
    assert_eq!(parsed["cycle"], "run1");
    assert_eq!(parsed["inputs"].as_array().expect("inputs array").len(), 12);
    assert_eq!(
        parsed["warm_cells"].as_array().expect("warm cells").len(),
        48
    );
    assert_eq!(
        parsed["exact_index_cells"]
            .as_array()
            .expect("index cells")
            .len(),
        4
    );
    assert_eq!(parsed["cold"]["session_count"], 20);
}

#[test]
fn refine_and_fix_measurement_publish_reports_with_exit_one() {
    // Given / When / Then
    for (fixture, expected_stdout, expected_verdict) in
        [refine_fixture(), fix_measurement_fixture()]
    {
        let output = analyze_completed(&fixture);
        assert_eq!(output.status.code(), Some(1));
        assert_eq!(output.stdout, expected_stdout.as_bytes());
        assert!(output.stderr.is_empty());
        assert_eq!(report(&fixture)["decision"]["verdict"], expected_verdict);
    }
}

#[test]
fn parse_schema_and_analysis_errors_exit_two_without_report() {
    // Given / When / Then
    for (fixture, expected_detail) in [parse_failure(), schema_failure(), analysis_failure()] {
        let output = analyze_completed(&fixture);
        assert_failure(&output, &fixture, expected_detail);
    }
}

fn malformed_manifest(manifest: &str) -> String {
    manifest.replacen(&manifest[..64], "NOT-A-DIGEST", 1)
}

fn unsorted_manifest(manifest: &str) -> String {
    let mut lines = manifest.lines().collect::<Vec<_>>();
    lines.swap(0, 1);
    lines.join("\n") + "\n"
}

fn extra_manifest(manifest: &str) -> String {
    format!("{manifest}{}  unexpected.txt\n", "0".repeat(64))
}

fn self_referential_manifest(manifest: &str) -> String {
    let self_reference = format!(
        "{}  artifacts/issue61/i61_e1_run1/checksums.sha256",
        "0".repeat(64)
    );
    let mut lines = manifest
        .lines()
        .chain([self_reference.as_str()])
        .collect::<Vec<_>>();
    lines.sort_unstable();
    lines.join("\n") + "\n"
}

fn refine_fixture() -> (CompletedCycle, String, &'static str) {
    let mut fixture = CompletedCycle::run1();
    fixture.rewrite_records(|records| {
        records
            .iter_mut()
            .find(|record| {
                record.engine == Engine::Native.as_str()
                    && record.dataset == Dataset::Wands.as_str()
                    && record.regime == SessionMode::Warm.as_str()
                    && record.query_class == WorkloadProjection::All.as_str()
            })
            .expect("WANDS warm cell exists")
            .latency_p50_us = 2_000.0;
    });
    (
        fixture,
        "i61_analyze: cycle=run1 verdict=REFINE passing_dataset=esci_electronics blocked_dataset=wands exit_code=1\n".to_owned(),
        "REFINE",
    )
}

fn fix_measurement_fixture() -> (CompletedCycle, String, &'static str) {
    let mut fixture = CompletedCycle::run1();
    fixture.rewrite_records(|records| {
        for record in records {
            if record.engine == Engine::Native.as_str()
                && record.regime == SessionMode::CalibrationFive.as_str()
            {
                record.cpu_usage_usec = 400;
                record.process_cpu_user_usec = Some(400);
                record.process_cpu_total_usec = Some(400);
            }
        }
    });
    (
        fixture,
        "i61_analyze: cycle=run1 verdict=FIX MEASUREMENT passing_dataset=null blocked_dataset=null exit_code=1\n".to_owned(),
        "FIX MEASUREMENT",
    )
}

fn parse_failure() -> (CompletedCycle, &'static str) {
    let fixture = CompletedCycle::run1();
    std::fs::write(fixture.input_path("raw.jsonl"), b"not JSON\n")
        .expect("malformed raw evidence is written");
    fixture.reseal();
    (fixture, "raw.jsonl contains invalid JSON at line 1")
}

fn schema_failure() -> (CompletedCycle, &'static str) {
    let fixture = CompletedCycle::run1();
    let path = fixture.input_path("index_artifacts.jsonl");
    let bytes = std::fs::read_to_string(&path).expect("index evidence is readable");
    std::fs::write(path, bytes.replacen("{", "{\"unknown\":true,", 1))
        .expect("invalid index schema is written");
    fixture.reseal();
    (
        fixture,
        "index_artifacts.jsonl line 1 contains unknown field unknown",
    )
}

fn analysis_failure() -> (CompletedCycle, &'static str) {
    let mut fixture = CompletedCycle::run1();
    fixture.rewrite_records(|records| records[0].experiment_id = "wrong".to_owned());
    (fixture, "record 0 does not belong to I61-E1")
}
