use issue61_eval::{
    parse_proc_stat, should_exclude_rep, steal_percent, CpuTimes, STEAL_EXCLUSION_THRESHOLD_PCT,
};

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
