use issue61_eval::{audit_all, audit_query, EngineOutcome, QueryVerdict};

fn ids(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

#[test]
fn identical_id_sets_in_different_order_match() {
    let audit = audit_query(
        "q1",
        &ids(&["a", "b", "a"]),
        &EngineOutcome::Ids(ids(&["b", "a"])),
    );

    assert_eq!(audit.verdict, QueryVerdict::Match);
}

#[test]
fn differing_id_sets_produce_a_named_mismatch_with_both_sides_listed() {
    let audit = audit_query(
        "q1",
        &ids(&["native-z", "shared", "native-a"]),
        &EngineOutcome::Ids(ids(&["engine-z", "shared", "engine-a"])),
    );

    assert_eq!(
        audit.verdict,
        QueryVerdict::Mismatch {
            only_native: ids(&["native-a", "native-z"]),
            only_engine: ids(&["engine-a", "engine-z"]),
        }
    );
}

#[test]
fn transport_error_is_excluded_and_counted_never_matched() {
    let report = audit_all(&[(
        "q1".to_owned(),
        ids(&["a"]),
        EngineOutcome::TransportError("timeout".to_owned()),
    )]);

    assert_eq!(report.matched, 0);
    assert_eq!(report.excluded, 1);
    assert!(matches!(
        report.audits[0].verdict,
        QueryVerdict::ExcludedEngineFailure { .. }
    ));
}

#[test]
fn query_error_and_parse_error_are_also_excluded_not_matched() {
    let report = audit_all(&[
        (
            "q1".to_owned(),
            ids(&["a"]),
            EngineOutcome::QueryError("bad query".to_owned()),
        ),
        (
            "q2".to_owned(),
            ids(&["b"]),
            EngineOutcome::ParseError("bad JSON".to_owned()),
        ),
    ]);

    assert_eq!(report.matched, 0);
    assert_eq!(report.excluded, 2);
}

#[test]
fn match_rate_excludes_failures_from_the_denominator() {
    let report = audit_all(&[
        (
            "q1".to_owned(),
            ids(&["a"]),
            EngineOutcome::Ids(ids(&["a"])),
        ),
        (
            "q2".to_owned(),
            ids(&["b"]),
            EngineOutcome::Ids(ids(&["b"])),
        ),
        (
            "q3".to_owned(),
            ids(&["c"]),
            EngineOutcome::Ids(ids(&["d"])),
        ),
        (
            "q4".to_owned(),
            ids(&["e"]),
            EngineOutcome::TransportError("timeout".to_owned()),
        ),
    ]);

    assert_eq!(report.match_rate(), Some(2.0 / 3.0));
    assert_eq!(report.mismatches().count(), 1);
}

#[test]
fn match_rate_is_none_when_everything_was_excluded() {
    let report = audit_all(&[(
        "q1".to_owned(),
        Vec::new(),
        EngineOutcome::TransportError("timeout".to_owned()),
    )]);

    assert_eq!(report.match_rate(), None);
}

#[test]
fn a_report_with_excluded_failures_does_not_pass() {
    let report = audit_all(&[(
        "q1".to_owned(),
        ids(&["a"]),
        EngineOutcome::TransportError("timeout".to_owned()),
    )]);

    assert!(!report.passes());
}

#[test]
fn empty_native_and_empty_engine_is_a_legitimate_match() {
    let audit = audit_query("q1", &[], &EngineOutcome::Ids(Vec::new()));

    assert_eq!(audit.verdict, QueryVerdict::Match);
}
