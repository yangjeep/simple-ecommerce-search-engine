use issue61_eval::{
    evaluate_cell, evaluate_exact_artifact, MetricKind, MAX_CPU_LATENCY_REL_HALFWIDTH,
    MAX_FOOTPRINT_REL_HALFWIDTH, MIN_BLOCKS,
};

#[test]
fn a_tight_low_variance_cell_passes() {
    // Given: thirty low-variance paired-block observations.
    let samples: Vec<f64> = (0..MIN_BLOCKS)
        .map(|block| match block % 3 {
            0 => 99.0,
            1 => 100.0,
            _ => 101.0,
        })
        .collect();

    // When: stability is evaluated with the small-sample Student-t interval.
    let cell = evaluate_cell(
        "native/wands/warm",
        "cpu_us_per_query",
        MetricKind::CpuOrLatency,
        &samples,
    );

    // Then: both precision and variability satisfy the instrument gate.
    assert!(cell.passes);
    assert!(cell.rel_halfwidth <= MAX_CPU_LATENCY_REL_HALFWIDTH);
    assert_eq!(cell.max_relative_halfwidth, MAX_CPU_LATENCY_REL_HALFWIDTH);
    assert!(!cell.underpowered);
}

#[test]
fn a_noisy_cell_fails_the_relative_halfwidth_bound() {
    // Given: thirty observations with persistent high dispersion.
    let samples: Vec<f64> = (0..MIN_BLOCKS)
        .map(|block| if block % 2 == 0 { 50.0 } else { 150.0 })
        .collect();

    // When: the cell is evaluated.
    let cell = evaluate_cell(
        "native/wands/warm",
        "latency_p95_us",
        MetricKind::CpuOrLatency,
        &samples,
    );

    // Then: its interval is too wide and the cell fails.
    assert!(cell.rel_halfwidth > MAX_CPU_LATENCY_REL_HALFWIDTH);
    assert!(!cell.passes);
}

#[test]
fn footprint_cells_use_the_same_student_t_stability_check() {
    // Given: the same powered sample for CPU and footprint metrics.
    let samples = [100.0; MIN_BLOCKS];

    // When: both inferential metric kinds are evaluated.
    let cpu = evaluate_cell("cell", "cpu", MetricKind::CpuOrLatency, &samples);
    let footprint = evaluate_cell("cell", "rss", MetricKind::Footprint, &samples);

    // Then: neither metric is routed through the exact-artifact check.
    assert_eq!(cpu.ci_low, footprint.ci_low);
    assert_eq!(cpu.ci_high, footprint.ci_high);
    assert!(footprint.passes);
}

#[test]
fn footprint_uses_a_two_percent_relative_halfwidth_limit() {
    // Given: a powered sample whose CV passes while relative halfwidth is between 2% and 7.5%.
    let samples: Vec<f64> = (0..MIN_BLOCKS)
        .map(|block| if block % 2 == 0 { 92.0 } else { 108.0 })
        .collect();

    // When: the same observations are evaluated as CPU and footprint metrics.
    let cpu = evaluate_cell("cell", "cpu", MetricKind::CpuOrLatency, &samples);
    let footprint = evaluate_cell("cell", "rss", MetricKind::Footprint, &samples);

    // Then: only the footprint metric fails its stricter preregistered precision limit.
    assert!(footprint.rel_halfwidth > MAX_FOOTPRINT_REL_HALFWIDTH);
    assert!(footprint.rel_halfwidth < MAX_CPU_LATENCY_REL_HALFWIDTH);
    assert!(footprint.cv <= 0.10);
    assert_eq!(
        footprint.max_relative_halfwidth,
        MAX_FOOTPRINT_REL_HALFWIDTH
    );
    assert_eq!(cpu.max_relative_halfwidth, MAX_CPU_LATENCY_REL_HALFWIDTH);
    assert!(cpu.passes);
    assert!(!footprint.passes);
}

#[test]
fn a_cell_with_zero_mean_fails_instead_of_producing_nan() {
    // Given: a powered but impossible zero resource measurement.
    let samples = [0.0; MIN_BLOCKS];

    // When: the cell is evaluated.
    let cell = evaluate_cell(
        "native/wands/warm",
        "cpu_us_per_query",
        MetricKind::CpuOrLatency,
        &samples,
    );

    // Then: zero cannot become a favourable stable result.
    assert!(!cell.rel_halfwidth.is_finite());
    assert!(!cell.passes);
}

#[test]
fn fewer_than_thirty_blocks_marks_the_cell_underpowered() {
    // Given: one fewer independent block than preregistered.
    let samples = [100.0; MIN_BLOCKS - 1];

    // When: an otherwise perfectly repeatable cell is evaluated.
    let cell = evaluate_cell("cell", "cpu", MetricKind::CpuOrLatency, &samples);

    // Then: repeatability does not conceal insufficient information.
    assert!(cell.passes);
    assert!(cell.underpowered);
    assert_eq!(cell.n_blocks, MIN_BLOCKS - 1);
}

#[test]
fn exact_artifact_within_tolerance_passes_without_bootstrapping() {
    // Given: deterministic artifact observations within declared byte tolerance.
    let samples = [1_000.0, 1_000.5, 999.5];

    // When: the exact-artifact path checks agreement directly.
    let cell = evaluate_exact_artifact("native/wands", "index_bytes", &samples, 1.0);

    // Then: no inferential interval is invented around deterministic values.
    assert!(cell.passes);
    assert_eq!(cell.kind, MetricKind::ExactArtifact);
    assert_eq!(cell.ci_low, cell.mean);
    assert_eq!(cell.ci_high, cell.mean);
    assert!(!cell.underpowered);
}
