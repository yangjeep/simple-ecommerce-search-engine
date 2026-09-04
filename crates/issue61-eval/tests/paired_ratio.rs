use issue61_eval::{check_calibration, paired_ratio, PairedBlock, RatioVerdict, ALPHA, MIN_BLOCKS};

fn constant_blocks(ratio: f64) -> Vec<PairedBlock> {
    (0..MIN_BLOCKS)
        .map(|block| PairedBlock {
            block,
            baseline: 100.0,
            treatment: 100.0 * ratio,
        })
        .collect()
}

fn alternating_blocks(first: f64, second: f64) -> Vec<PairedBlock> {
    (0..MIN_BLOCKS)
        .map(|block| PairedBlock {
            block,
            baseline: 100.0,
            treatment: 100.0 * if block % 2 == 0 { first } else { second },
        })
        .collect()
}

#[test]
fn a_true_25_percent_saving_with_tight_blocks_is_materially_better() {
    // Given: thirty paired blocks with a treatment/baseline ratio of 0.75.
    let blocks = constant_blocks(0.75);

    // When: uncertainty is estimated directly on paired log ratios.
    let result = paired_ratio(&blocks, ALPHA).expect("positive paired ratios should be defined");

    // Then: the upper bound clears the inclusive 25% materiality bar.
    assert_eq!(result.verdict, RatioVerdict::MateriallyBetter);
    assert!(result.ci_high <= 0.75);
}

#[test]
fn a_point_estimate_of_exactly_25_percent_with_a_wide_interval_is_inconclusive() {
    // Given: ratios whose geometric mean is 0.75 but whose block spread is wide.
    let blocks = alternating_blocks(0.5, 1.125);

    // When: the paired log-ratio interval is computed.
    let result = paired_ratio(&blocks, ALPHA).expect("positive paired ratios should be defined");

    // Then: a 25% point estimate cannot pass while its upper bound misses the bar.
    assert!((result.point_ratio - 0.75).abs() < 1e-12);
    assert!(result.ci_low < 0.75);
    assert!(result.ci_high > 0.75);
    assert_eq!(result.verdict, RatioVerdict::Inconclusive);
}

#[test]
fn a_ratio_above_one_is_not_better() {
    // Given: treatment is consistently ten percent more expensive.
    let blocks = constant_blocks(1.10);

    // When: the paired ratio is evaluated.
    let result = paired_ratio(&blocks, ALPHA).expect("positive paired ratios should be defined");

    // Then: the result is decisively not materially better.
    assert_eq!(result.verdict, RatioVerdict::NotBetter);
}

#[test]
fn ratio_against_a_zero_baseline_is_none_not_infinity() {
    // Given: one block has an undefined zero baseline.
    let blocks = vec![PairedBlock {
        block: 0,
        baseline: 0.0,
        treatment: 1.0,
    }];

    // When: the ratio analysis is requested.
    let result = paired_ratio(&blocks, ALPHA);

    // Then: invalid accounting cannot become an infinite or zero ratio.
    assert!(result.is_none());
}

#[test]
fn calibration_that_recovers_the_expected_ratio_passes() {
    // Given: a deliberately injected 5/4 workload effect.
    let blocks = constant_blocks(1.25);

    // When: the instrument is checked against the known ratio.
    let calibration = check_calibration(&blocks, 1.25, ALPHA)
        .expect("positive calibration ratios should be defined");

    // Then: the CI contains 1.25 and excludes no-effect ratio 1.0.
    assert!(calibration.passed);
    assert!(calibration.observed.ci_low <= 1.25);
    assert!(calibration.observed.ci_high >= 1.25);
    assert!(calibration.observed.ci_low > 1.0 || calibration.observed.ci_high < 1.0);
}

#[test]
fn calibration_whose_interval_contains_one_fails() {
    // Given: a noisy injected effect whose CI contains both 1.0 and 1.25.
    let blocks = alternating_blocks(0.8, 1.5625);

    // When: calibration is checked against the known 1.25 ratio.
    let calibration = check_calibration(&blocks, 1.25, ALPHA)
        .expect("positive calibration ratios should be defined");

    // Then: inability to distinguish the effect from nothing fails calibration.
    assert!(calibration.observed.ci_low <= 1.0);
    assert!(calibration.observed.ci_high >= 1.0);
    assert!(!calibration.passed);
}
