use issue61_eval::{
    is_above_floor, measure_timer_floor, required_batch_size, validate_measurement_window,
    MeasurementFloorError, TimerFloor, MIN_CGROUP_CPU_USEC, MIN_FLOOR_MULTIPLE,
};

fn realistic_floor() -> TimerFloor {
    TimerFloor {
        clock_resolution_ns: 10.0,
        instant_now_overhead_ns: 20.0,
    }
}

#[test]
fn measurement_window_requires_both_frozen_floors() {
    let floor = realistic_floor();

    assert!(matches!(
        validate_measurement_window(1_999, 1_000, &floor),
        Err(MeasurementFloorError::WallBelowTimerFloor { .. })
    ));
    assert!(matches!(
        validate_measurement_window(2_000, 999, &floor),
        Err(MeasurementFloorError::CpuBelowCgroupFloor { .. })
    ));
    assert!(validate_measurement_window(2_000, 1_000, &floor).is_ok());
    assert_eq!(MIN_CGROUP_CPU_USEC, 1_000);
}

#[test]
fn measured_floor_is_positive_and_finite() {
    let floor = measure_timer_floor(10_000);

    assert!(floor.clock_resolution_ns > 0.0);
    assert!(floor.clock_resolution_ns.is_finite());
    assert!(floor.instant_now_overhead_ns > 0.0);
    assert!(floor.instant_now_overhead_ns.is_finite());
}

#[test]
fn a_nanosecond_scale_measurement_is_below_floor() {
    assert!(!is_above_floor(1.0, &realistic_floor()));
}

#[test]
fn a_millisecond_scale_measurement_is_above_floor() {
    assert!(is_above_floor(1_000_000.0, &realistic_floor()));
}

#[test]
fn required_batch_size_is_at_least_one_even_for_a_huge_op() {
    assert_eq!(required_batch_size(1_000_000_000.0, &realistic_floor()), 1);
}

#[test]
fn required_batch_size_scales_inversely_with_op_cost() {
    let floor = realistic_floor();
    let expensive = required_batch_size(100.0, &floor);
    let cheap = required_batch_size(10.0, &floor);

    assert!(cheap >= expensive * 10);
    assert_eq!(MIN_FLOOR_MULTIPLE, 100.0);
}
