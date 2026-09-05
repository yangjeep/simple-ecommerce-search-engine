use issue61_eval::{ProcessCpuError, ProcessCpuSnapshot};

#[test]
fn process_delta_rejects_counter_rollback_and_total_overflow() {
    // Given
    let earlier = ProcessCpuSnapshot::new(41, 20, 30);
    let rollback = ProcessCpuSnapshot::new(41, 19, 31);
    let overflow = ProcessCpuSnapshot::new(41, u64::MAX, 1);

    // When
    let rollback_error = rollback
        .delta_since(&earlier)
        .expect_err("rollback must fail closed");
    let overflow_error = overflow
        .delta_since(&ProcessCpuSnapshot::new(41, 0, 0))
        .expect_err("total overflow must fail closed");

    // Then
    assert!(matches!(
        rollback_error,
        ProcessCpuError::CounterRollback {
            field: "user_usec",
            earlier: 20,
            later: 19
        }
    ));
    assert!(matches!(overflow_error, ProcessCpuError::Overflow));
}

#[test]
fn reconciliation_accepts_exactly_two_percent_and_rejects_strictly_above() {
    // Given
    let delta = ProcessCpuSnapshot::new(41, 0, 980)
        .delta_since(&ProcessCpuSnapshot::new(41, 0, 0))
        .expect("valid delta");

    // When
    let boundary = delta
        .reconcile_cgroup(1_000)
        .expect("exact boundary passes");
    let above = delta
        .reconcile_cgroup(1_001)
        .expect_err("strictly above boundary fails");

    // Then
    assert_eq!(boundary, 2.0);
    assert!(matches!(above, ProcessCpuError::Disagreement { .. }));
}

#[test]
fn reconciliation_rejects_zero_cgroup_cpu() {
    // Given
    let delta = ProcessCpuSnapshot::new(41, 10, 10)
        .delta_since(&ProcessCpuSnapshot::new(41, 0, 0))
        .expect("valid delta");

    // When
    let error = delta
        .reconcile_cgroup(0)
        .expect_err("zero cgroup CPU is uncontrolled");

    // Then
    assert!(matches!(error, ProcessCpuError::ZeroCgroupCpu));
}

#[test]
fn signed_timeval_conversion_rejects_invalid_components_and_overflow() {
    // Given / When / Then
    assert!(ProcessCpuSnapshot::from_timevals(41, (-1, 0), (0, 0)).is_err());
    assert!(ProcessCpuSnapshot::from_timevals(41, (0, -1), (0, 0)).is_err());
    assert!(ProcessCpuSnapshot::from_timevals(41, (0, 1_000_000), (0, 0)).is_err());
    assert!(ProcessCpuSnapshot::from_timevals(41, (i64::MAX, 0), (0, 0)).is_err());
}
