#[path = "support/i61_analyzer.rs"]
mod analyzer;
#[path = "support/i61_fixture/mod.rs"]
mod fixture;

use analyzer::analyze_completed;
use fixture::CompletedCycle;
use issue61_eval::sha256_hex;

#[test]
fn complete_report_has_frozen_digest_and_representative_ordered_values() {
    let fixture = CompletedCycle::run1();

    let output = analyze_completed(&fixture);

    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.is_empty());
    let bytes = std::fs::read(fixture.analysis_path()).expect("report is readable");
    assert_eq!(
        sha256_hex(&bytes),
        "905c23579724a7c3114f96e21c3cc866a921a0339a63d9235ba284a58483a9ce"
    );
    let report: serde_json::Value = serde_json::from_slice(&bytes).expect("report is JSON");
    let inputs = report["inputs"].as_array().expect("sealed inputs");
    assert_eq!(
        inputs[0]["path"],
        "artifacts/issue61/i61_e1_run1/candidate_audit_esci.jsonl"
    );
    assert_eq!(
        inputs[0]["sha256"].as_str().expect("input digest").len(),
        64
    );
    assert_eq!(report["decision"]["verdict"], "KEEP");
    assert_eq!(report["decision"]["exit_code"], 0);
    assert_eq!(report["global_gates"]["calibration_passed"], true);
    assert_eq!(report["global_gates"]["equivalence_passed"], true);
    assert_eq!(
        report["global_gates"]["process_cpu_reconciliation_passed"],
        true
    );
    assert_eq!(report["calibrations"]["arms"][0]["engine"], "native");
    assert_eq!(
        report["calibrations"]["arms"][0]["observed"]["point_ratio"],
        1.25
    );
    assert_eq!(report["candidate_audit"]["total_records"], 1080);
    assert_eq!(report["candidate_audit"]["equivalence_passed"], true);
    let warm = report["warm_cells"].as_array().expect("warm cells");
    assert_eq!(warm[0]["cell"], "native/wands/all/warm");
    assert_eq!(warm[0]["metric"], "cpu_us_per_query");
    assert_eq!(warm[0]["mean"], 100.0);
    assert_eq!(warm[1]["metric"], "latency_p50_us");
    assert_eq!(warm[1]["mean"], 200.0);
    assert_eq!(warm[2]["metric"], "cgroup_memory_current_median_bytes");
    assert_eq!(warm[2]["mean"], 300.0);
    assert_eq!(warm[24]["cell"], "solr/wands/all/warm");
    assert_eq!(warm[24]["mean"], 108.0);

    let exact = report["exact_index_cells"]
        .as_array()
        .expect("exact index cells");
    assert_eq!(exact[0]["cell"], "native/wands/exact-index");
    assert_eq!(exact[0]["mean"], 7777.0);
    assert_eq!(exact[1]["cell"], "native/esci_electronics/exact-index");
    assert_eq!(exact[1]["mean"], 7778.0);
    assert_eq!(exact[2]["cell"], "solr/wands/exact-index");
    assert_eq!(exact[2]["mean"], 8888.0);
    assert_eq!(exact[3]["cell"], "solr/esci_electronics/exact-index");
    assert_eq!(exact[3]["mean"], 8889.0);
}
