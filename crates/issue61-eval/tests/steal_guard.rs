use issue61_eval::{
    assess_assigned_cpu_steal, parse_assigned_proc_stat, parse_proc_stat, probe_assigned_cpu_steal,
    should_exclude_rep, steal_percent, CpuSet, CpuTimes, StealError, StealProbeConfig,
    STEAL_EXCLUSION_THRESHOLD_PCT, STEAL_PROBE_DURATION,
};
use std::cell::Cell;
use std::time::Duration;

#[test]
fn parses_aggregate_cpu_line_and_extracts_steal() {
    let times = parse_proc_stat("cpu  10 20 30 40 50 60 70 80 90 100\ncpu0 1 2 3 4 5 6 7 8 9 10\n")
        .expect("aggregate cpu line should parse");

    assert_eq!(times.total_jiffies, 550);
    assert_eq!(times.steal_jiffies, 80);
}

#[test]
fn short_cpu_line_without_steal_field_yields_zero_steal() {
    let times = parse_proc_stat("cpu  10 20 30 40 50 60 70\n")
        .expect("older aggregate format should parse");

    assert_eq!(times.total_jiffies, 280);
    assert_eq!(times.steal_jiffies, 0);
}

#[test]
fn steal_percent_computes_ratio_of_deltas() {
    let earlier = CpuTimes {
        total_jiffies: 1_000,
        steal_jiffies: 20,
    };
    let later = CpuTimes {
        total_jiffies: 1_500,
        steal_jiffies: 30,
    };

    let percent = steal_percent(&earlier, &later);

    assert!((percent - 2.0).abs() < f64::EPSILON);
}

#[test]
fn zero_total_delta_yields_zero_not_nan() {
    let times = CpuTimes {
        total_jiffies: 100,
        steal_jiffies: 5,
    };

    let percent = steal_percent(&times, &times);

    assert_eq!(percent, 0.0);
    assert!(percent.is_finite());
}

#[test]
fn rep_above_one_percent_steal_is_excluded() {
    assert!(should_exclude_rep(STEAL_EXCLUSION_THRESHOLD_PCT + 0.01));
}

#[test]
fn rep_at_exactly_one_percent_is_not_excluded() {
    assert!(!should_exclude_rep(STEAL_EXCLUSION_THRESHOLD_PCT));
}

#[test]
fn cpuset_parses_singletons_and_inclusive_ranges() {
    // Given / When
    let cpus = CpuSet::parse("0-2,4,7-8").expect("valid cpuset");

    // Then
    assert_eq!(cpus.as_slice(), &[0, 1, 2, 4, 7, 8]);
}

#[test]
fn cpuset_rejects_invalid_or_ambiguous_membership() {
    for invalid in ["", "0-", "x", "2-0", "1,1", "0-2,2-3", "0,,2"] {
        // Given / When
        let result = CpuSet::parse(invalid);

        // Then
        assert!(result.is_err(), "{invalid:?} must be rejected");
    }
}

#[test]
fn assigned_stat_sums_selected_cpu_fields_only_through_steal() {
    // Given
    let cpus = CpuSet::parse("0,2").expect("valid cpuset");
    let stat = "cpu 999 999 999 999 999 999 999 999 999 999\n\
                cpu0 1 2 3 4 5 6 7 8 900 901\n\
                cpu1 50 50 50 50 50 50 50 50 50 50\n\
                cpu2 10 20 30 40 50 60 70 80 902 903\n";

    // When
    let times = parse_assigned_proc_stat(stat, &cpus).expect("selected lines should parse");

    // Then
    assert_eq!(times.total_jiffies, 396);
    assert_eq!(times.steal_jiffies, 88);
}

#[test]
fn assigned_stat_rejects_missing_duplicate_short_and_invalid_selected_lines() {
    let cpus = CpuSet::parse("0,2").expect("valid cpuset");
    for invalid in [
        "cpu0 1 2 3 4 5 6 7 8\n",
        "cpu0 1 2 3 4 5 6 7 8\ncpu0 1 2 3 4 5 6 7 8\ncpu2 1 2 3 4 5 6 7 8\n",
        "cpu0 1 2 3 4 5 6 7\ncpu2 1 2 3 4 5 6 7 8\n",
        "cpu0 1 2 3 4 5 6 7 nope\ncpu2 1 2 3 4 5 6 7 8\n",
    ] {
        // Given / When
        let result = parse_assigned_proc_stat(invalid, &cpus);

        // Then
        assert!(result.is_err());
    }
}

#[test]
fn assigned_steal_at_exactly_one_percent_is_accepted() {
    // Given
    let earlier = CpuTimes {
        total_jiffies: 1_000,
        steal_jiffies: 20,
    };
    let later = CpuTimes {
        total_jiffies: 1_100,
        steal_jiffies: 21,
    };

    // When
    let result = assess_assigned_cpu_steal(earlier, later, Duration::from_secs(5))
        .expect("monotonic nonzero delta");

    // Then
    assert_eq!(result.total_delta_jiffies, 100);
    assert_eq!(result.steal_delta_jiffies, 1);
    assert!((result.steal_percent() - 1.0).abs() < f64::EPSILON);
    assert!(!result.rejected);
}

#[test]
fn assigned_steal_above_one_percent_is_rejected() {
    // Given / When
    let result = assess_assigned_cpu_steal(
        CpuTimes {
            total_jiffies: 1_000,
            steal_jiffies: 20,
        },
        CpuTimes {
            total_jiffies: 1_099,
            steal_jiffies: 21,
        },
        Duration::from_secs(5),
    )
    .expect("monotonic nonzero delta");

    // Then
    assert!(result.rejected);
}

#[test]
fn assigned_steal_rejects_zero_delta_and_counter_rollback() {
    let baseline = CpuTimes {
        total_jiffies: 100,
        steal_jiffies: 5,
    };
    let zero = assess_assigned_cpu_steal(baseline, baseline, Duration::ZERO);
    let rollback = assess_assigned_cpu_steal(
        baseline,
        CpuTimes {
            total_jiffies: 99,
            steal_jiffies: 4,
        },
        Duration::ZERO,
    );

    assert!(matches!(zero, Err(StealError::ZeroTotalDelta)));
    assert!(matches!(rollback, Err(StealError::CounterRollback { .. })));
}

#[test]
fn assigned_steal_rejects_steal_rollback_when_total_increases() {
    let result = assess_assigned_cpu_steal(
        CpuTimes {
            total_jiffies: 100,
            steal_jiffies: 5,
        },
        CpuTimes {
            total_jiffies: 200,
            steal_jiffies: 4,
        },
        Duration::from_secs(5),
    );

    assert!(matches!(
        result,
        Err(StealError::CounterRollback {
            field: "steal_jiffies",
            ..
        })
    ));
}

#[test]
fn probe_uses_injected_duration_without_waiting_five_seconds() {
    // Given
    let cpus = CpuSet::parse("0").expect("valid cpuset");
    let configured = Duration::from_millis(7);
    let observed_wait = Cell::new(Duration::ZERO);
    let mut snapshots = [
        "cpu0 10 10 10 10 10 10 10 10\n",
        "cpu0 20 20 20 20 20 20 20 21\n",
    ]
    .into_iter();

    // When
    let result = probe_assigned_cpu_steal(
        StealProbeConfig {
            cpus: &cpus,
            duration: configured,
        },
        || Ok(snapshots.next().expect("two snapshots").to_owned()),
        |duration| observed_wait.set(duration),
    )
    .expect("probe should pass");

    // Then
    assert_eq!(observed_wait.get(), configured);
    assert_eq!(result.total_delta_jiffies, 81);
    assert_eq!(result.steal_delta_jiffies, 11);
    assert!(result.elapsed < STEAL_PROBE_DURATION);
    assert_eq!(STEAL_PROBE_DURATION, Duration::from_secs(5));
    assert_eq!(
        StealProbeConfig::with_default_duration(&cpus).duration,
        STEAL_PROBE_DURATION
    );
}
