use issue61_eval::{
    audit_all, audit_candidate_sets, audit_query, candidate_digest, frozen_native_query,
    AdmissionClass, AuditVerdict, EngineOutcome, FrozenQuery, QueryVerdict,
};

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

fn frozen_query() -> FrozenQuery {
    FrozenQuery {
        query_id: "q1".to_string(),
        text: "chair".to_string(),
        admission_class: AdmissionClass::Hybrid,
        structural_constraint_count: 1,
        has_residual_lexical: true,
        rows: 10,
        native: None,
        solr: None,
    }
}

#[test]
fn candidate_digest_sorts_deduplicates_and_has_no_trailing_newline() {
    let digest = candidate_digest(&ids(&["b", "a", "b"]));

    assert_eq!(
        digest,
        "7e18f737311b2dc3b2f269dd78396b0351f14fb66efa879f768cb23181883c78"
    );
    assert_ne!(digest, issue61_eval::sha256_hex(b"a\nb\n"));
}

#[test]
fn candidate_mismatch_emits_sorted_directional_differences() {
    let record = audit_candidate_sets(
        "wands",
        &frozen_query(),
        Ok(ids(&["shared", "native-z", "native-a"])),
        EngineOutcome::Ids(ids(&["engine-z", "shared", "engine-a"])),
    );

    assert_eq!(record.verdict, AuditVerdict::Mismatch);
    assert_eq!(record.only_native, ids(&["native-a", "native-z"]));
    assert_eq!(record.only_engine, ids(&["engine-a", "engine-z"]));
}

#[test]
fn engine_failure_is_never_counted_as_a_match() {
    let record = audit_candidate_sets(
        "wands",
        &frozen_query(),
        Ok(ids(&["a"])),
        EngineOutcome::ParseError("invalid page".to_string()),
    );

    assert_eq!(record.verdict, AuditVerdict::EngineFailure);
    assert_eq!(record.native_count, Some(1));
    assert_eq!(record.engine_count, None);
    assert_eq!(
        record.failure_reason.as_deref(),
        Some("parse error: invalid page")
    );
}

#[test]
fn equal_digests_with_different_counts_are_a_mismatch() {
    let record = audit_candidate_sets(
        "wands",
        &frozen_query(),
        Ok(ids(&["a", "a"])),
        EngineOutcome::Ids(ids(&["a"])),
    );

    assert_eq!(record.verdict, AuditVerdict::Mismatch);
}

#[test]
fn audit_requires_the_frozen_native_request_block() {
    let error = frozen_native_query(&frozen_query()).expect_err("missing native request must fail");

    assert!(error.contains("query q1 is missing its native request block"));
}
