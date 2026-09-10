use super::super::*;

fn valid_args() -> Vec<String> {
    [
        "i61_bench",
        "--workload",
        "workload.jsonl",
        "--dataset",
        "wands",
        "--query-class",
        "all",
        "--engine",
        "solr",
        "--session-mode",
        "warm",
        "--engine-url",
        "http://solr:8983",
        "--engine-cgroup",
        "/sys/fs/cgroup/solr",
        "--block",
        "4",
        "--engine-order",
        "1",
        "--seed",
        "61",
        "--out",
        "raw.jsonl",
    ]
    .into_iter()
    .map(str::to_string)
    .collect()
}

fn replace_value(args: &mut [String], flag: &str, value: &str) {
    let index = args
        .iter()
        .position(|arg| arg == flag)
        .expect("fixture flag exists");
    args[index + 1] = value.to_string();
}

#[test]
fn config_rejects_non_frozen_seed_block_and_order() {
    for (flag, value) in [("--seed", "62"), ("--block", "30"), ("--engine-order", "2")] {
        // Given
        let mut args = valid_args();
        replace_value(&mut args, flag, value);

        // When / Then
        assert!(parse_config(&args).is_err(), "accepted {flag}={value}");
    }
}

#[test]
fn config_rejects_empty_dataset_and_query_class() {
    for flag in ["--dataset", "--query-class"] {
        // Given
        let mut args = valid_args();
        replace_value(&mut args, flag, "");

        // When / Then
        assert!(parse_config(&args).is_err(), "accepted empty {flag}");
    }
}

#[test]
fn config_rejects_engine_that_does_not_match_its_frozen_block_slot() {
    // Given
    let mut args = valid_args();
    replace_value(&mut args, "--engine", "native");

    // When / Then
    assert!(parse_config(&args).is_err());
}

#[test]
fn config_rejects_arbitrary_pass_counts_and_calibration_outside_wands() {
    // Given
    let mut pass_args = valid_args();
    pass_args.extend(["--measured-passes".to_string(), "9".to_string()]);
    let mut calibration_args = valid_args();
    replace_value(&mut calibration_args, "--dataset", "esci_electronics");
    replace_value(&mut calibration_args, "--session-mode", "calibration-five");

    // When / Then
    assert!(parse_config(&pass_args).is_err());
    assert!(parse_config(&calibration_args).is_err());
}
