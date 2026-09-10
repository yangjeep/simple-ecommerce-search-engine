use super::{run1, FakePort};
use crate::lifecycle::{IndexError, LifecycleError, Operation};
use crate::post_run::parse_index_artifacts_for_test;
use serde_json::Value;

#[test]
fn index_evidence_has_exact_schema_v1_identity_and_positive_safe_bytes() {
    // Given
    let mut fake = FakePort::default();

    // When
    run1(&mut fake).expect("campaign succeeds");

    // Then
    let records = fake.index_lines().map(parse_json).collect::<Vec<_>>();
    assert_eq!(records.len(), 4);
    for record in records {
        assert_eq!(record["schema_version"], 1);
        assert_eq!(record["experiment_id"], "I61-E1");
        assert_eq!(record["cycle"], "run1");
        let bytes = record["index_serialized_bytes"]
            .as_u64()
            .expect("serialized bytes is unsigned");
        assert!((1..=1_u64 << 53).contains(&bytes));
    }
}

#[test]
fn lifecycle_index_evidence_omits_native_snapshots_and_is_post_run_compatible() {
    // Given
    let mut fake = FakePort::default();
    run1(&mut fake).expect("campaign succeeds");
    let jsonl = fake.index_lines().collect::<String>();

    // When
    let records = jsonl.lines().map(parse_json).collect::<Vec<_>>();
    let parsed = parse_index_artifacts_for_test(jsonl.as_bytes(), crate::CampaignCycle::Run1);

    // Then
    for native in &records[..2] {
        assert!(native.get("schema_snapshot").is_none());
        assert!(native.get("config_snapshot").is_none());
    }
    for solr in &records[2..] {
        assert!(solr["schema_snapshot"].is_object());
        assert!(solr["config_snapshot"].is_object());
    }
    assert_eq!(parsed.expect("lifecycle evidence must parse").len(), 4);
}

#[test]
fn index_rejects_every_schema_v1_invariant_independently() {
    // Given / When / Then
    for corruption in [
        IndexError::WrongSchema,
        IndexError::WrongExperiment,
        IndexError::WrongCycle,
        IndexError::WrongSerializedBytes,
    ] {
        let mut fake = FakePort::default();
        fake.corrupt_index(corruption);
        assert_eq!(
            run1(&mut fake).expect_err("invalid index identity must stop"),
            LifecycleError::Index(corruption)
        );
    }
}

#[test]
fn seal_contains_exact_sorted_revision9_paths_and_static_hashes() {
    // Given
    let mut fake = FakePort::default();

    // When
    run1(&mut fake).expect("campaign succeeds");

    // Then
    let lines = fake.seal_lines().collect::<Vec<_>>();
    assert_eq!(lines.len(), 12);
    for line in &lines {
        assert_eq!(line.matches("  ").count(), 1);
        let (hash, _) = line.split_once("  ").expect("two-space separator");
        assert_eq!(hash.len(), 64);
        assert!(hash
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)));
    }
    let paths = lines
        .iter()
        .map(|line| line.split_once("  ").expect("two-space separator").1)
        .collect::<Vec<_>>();
    let mut sorted = paths.clone();
    sorted.sort_unstable();
    assert_eq!(paths, sorted);
    assert!(lines.contains(&concat!(
        "ae5c0e1c8de23a798a550b6042617f0bedd4b8e04a1ecbe5d8d9633df5bfff5b",
        "  benchmarks/configs/issue61/solr_esci_electronics_config.json"
    )));
    assert!(lines.contains(&concat!(
        "62266803df8715b3b09485fdc168b580c1ae337d1dd7b5b43ae1ce09e74eca2e",
        "  benchmarks/configs/issue61/solr_esci_electronics_schema.json"
    )));
    assert!(lines.contains(&concat!(
        "ae5c0e1c8de23a798a550b6042617f0bedd4b8e04a1ecbe5d8d9633df5bfff5b",
        "  benchmarks/configs/issue61/solr_wands_config.json"
    )));
    assert!(lines.contains(&concat!(
        "997e321ed081133b9a83fce5f35e42a75cfd3333bf91505f876501343600463a",
        "  benchmarks/configs/issue61/solr_wands_schema.json"
    )));
    assert!(lines.contains(&concat!(
        "531e39d0feda45591c0f3f17adfa25b1b52d73ff70994a3e31cad739364f050e",
        "  benchmarks/workloads/i61_esci_electronics.jsonl"
    )));
    assert!(lines.contains(&concat!(
        "462b5bf8cae6e12fdcfa2cb5177a648d4de43aec0936c35e61aaffaacb0cad08",
        "  benchmarks/workloads/i61_wands_480.jsonl"
    )));
}

#[test]
fn seal_computes_local_hashes_and_keeps_pinned_hashes_without_placeholders() {
    // Given
    let mut fake = FakePort::default();

    // When
    run1(&mut fake).expect("campaign succeeds");

    // Then
    assert_eq!(
        fake.computed_hash_paths(),
        [
            "artifacts/issue61/i61_e1_run1/candidate_audit_esci.jsonl",
            "artifacts/issue61/i61_e1_run1/candidate_audit_wands.jsonl",
            "artifacts/issue61/i61_e1_run1/commands.log",
            "artifacts/issue61/i61_e1_run1/events.jsonl",
            "artifacts/issue61/i61_e1_run1/index_artifacts.jsonl",
            "artifacts/issue61/i61_e1_run1/raw.jsonl",
        ]
    );
    assert_eq!(
        fake.finalization_operations()
            .iter()
            .filter(|operation| **operation == Operation::ComputeHash)
            .count(),
        6
    );
    for line in fake.seal_lines().take(6) {
        let (hash, _) = line.split_once("  ").expect("two-space separator");
        assert_ne!(hash, "0".repeat(64));
    }
}

fn parse_json(line: &str) -> Value {
    serde_json::from_str(line).expect("index evidence is JSON")
}
