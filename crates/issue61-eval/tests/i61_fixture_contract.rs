#[path = "support/i61_fixture/mod.rs"]
mod fixture;

use fixture::{campaign_records, CompletedCycle};
use issue61_eval::{sha256_hex, CampaignCycle};
use std::process::Command;

const STATIC_HASHES: [(&str, &str); 6] = [
    (
        "benchmarks/configs/issue61/solr_esci_electronics_config.json",
        "ae5c0e1c8de23a798a550b6042617f0bedd4b8e04a1ecbe5d8d9633df5bfff5b",
    ),
    (
        "benchmarks/configs/issue61/solr_esci_electronics_schema.json",
        "62266803df8715b3b09485fdc168b580c1ae337d1dd7b5b43ae1ce09e74eca2e",
    ),
    (
        "benchmarks/configs/issue61/solr_wands_config.json",
        "ae5c0e1c8de23a798a550b6042617f0bedd4b8e04a1ecbe5d8d9633df5bfff5b",
    ),
    (
        "benchmarks/configs/issue61/solr_wands_schema.json",
        "997e321ed081133b9a83fce5f35e42a75cfd3333bf91505f876501343600463a",
    ),
    (
        "benchmarks/workloads/i61_esci_electronics.jsonl",
        "531e39d0feda45591c0f3f17adfa25b1b52d73ff70994a3e31cad739364f050e",
    ),
    (
        "benchmarks/workloads/i61_wands_480.jsonl",
        "462b5bf8cae6e12fdcfa2cb5177a648d4de43aec0936c35e61aaffaacb0cad08",
    ),
];

#[test]
fn completed_cycle_fixture_has_frozen_counts_and_sorted_seal() {
    // Given / When
    let mut fixture = CompletedCycle::run1();
    fixture.rewrite_records(|_| {});
    let manifest = std::fs::read_to_string(fixture.manifest_path()).expect("manifest is readable");
    let paths = manifest
        .lines()
        .map(|line| line.split_once("  ").expect("two-space separator").1)
        .collect::<Vec<_>>();

    // Then
    assert_eq!(campaign_records(CampaignCycle::Run1).len(), 620);
    assert_eq!(
        issue61_eval::read_jsonl(&fixture.input_path("raw.jsonl"))
            .expect("raw fixture parses")
            .len(),
        620
    );
    assert_eq!(paths.len(), 12);
    assert!(paths.windows(2).all(|pair| pair[0] < pair[1]));
    assert_eq!(
        std::fs::read_to_string(fixture.input_path("index_artifacts.jsonl"))
            .expect("index evidence is readable")
            .lines()
            .count(),
        4
    );
    assert!(!fixture.analysis_path().exists());
    for (name, expected) in [
        (
            "candidate_audit_esci.jsonl",
            "51c4a47d91ff50af20e06e4da69e3496a49f01ae8ba12739506466f7e022a825",
        ),
        (
            "candidate_audit_wands.jsonl",
            "5354a3841f2681b0b9bb2f46418a5fa337e3b6c6e7629fdef86532b05c043780",
        ),
    ] {
        let bytes = std::fs::read(fixture.input_path(name)).expect("audit fixture is readable");
        assert_eq!(sha256_hex(&bytes), expected);
    }
}

#[test]
fn completed_cycle_fixture_copies_real_frozen_static_bytes() {
    // Given / When
    let fixture = CompletedCycle::run1();

    // Then
    for (relative, expected) in STATIC_HASHES {
        let bytes = std::fs::read(fixture.root().join(relative)).expect("static input is readable");
        assert_eq!(
            sha256_hex(&bytes),
            expected,
            "unexpected hash for {relative}"
        );
    }
}

#[test]
fn campaign_dry_run_bytes_remain_frozen() {
    // Given
    let fixture = CompletedCycle::run1();

    // When
    let output = Command::new(env!("CARGO_BIN_EXE_i61_campaign"))
        .args(["--dry-run", "--cycle", "run1"])
        .current_dir(fixture.root())
        .output()
        .expect("campaign binary executes");

    // Then
    assert!(output.status.success());
    assert_eq!(output.stdout, b"I61_CAMPAIGN_DRY_RUN cycle=run1 logical_pairs=310 sessions=620 warm_sessions=480 calibration_sessions=120 cold_sessions=20 stability_cells=48 exact_index_cells=4 external_commands=0 writes=0\n");
    assert!(output.stderr.is_empty());
}
