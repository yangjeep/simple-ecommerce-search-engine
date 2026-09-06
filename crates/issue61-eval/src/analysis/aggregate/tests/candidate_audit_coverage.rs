mod identity_errors;

use super::{analyze_with_candidate_audits, campaign_records, exact_observations};
use crate::analysis::candidate_audit::validate_candidate_audits;
use crate::analysis::test_support::CompleteCandidateAuditFixture;
use crate::{
    AuditVerdict, CandidateAuditError, CandidateAuditRecord, CandidateAuditSummary,
    CandidateSetSide, Dataset, GateVerdict,
};

fn validate(
    fixture: &CompleteCandidateAuditFixture,
) -> Result<CandidateAuditSummary, CandidateAuditError> {
    let evidence = fixture.evidence();
    validate_candidate_audits(&evidence)
}

fn assert_error(fixture: &CompleteCandidateAuditFixture, expected: CandidateAuditError) {
    assert_eq!(validate(fixture), Err(expected));
}

fn configure_failure(record: &mut CandidateAuditRecord, verdict: AuditVerdict) {
    record.verdict = verdict;
    record.failure_reason = Some("captured engine failure".to_owned());
    record.only_native.clear();
    record.only_engine.clear();
    match verdict {
        AuditVerdict::NativeFailure => {
            record.native_count = None;
            record.native_digest = None;
        }
        AuditVerdict::EngineFailure => {
            record.engine_count = None;
            record.engine_digest = None;
        }
        AuditVerdict::Match | AuditVerdict::Mismatch => {
            unreachable!("helper accepts only failure verdicts")
        }
    }
}

#[test]
fn count_and_digest_are_atomic_on_both_candidate_sides() {
    // Given/When: either half of either candidate pair is absent.
    for (side, missing_count) in [
        (CandidateSetSide::Native, true),
        (CandidateSetSide::Native, false),
        (CandidateSetSide::Engine, true),
        (CandidateSetSide::Engine, false),
    ] {
        let mut fixture = CompleteCandidateAuditFixture::complete();
        let record = &mut fixture.wands_records[0];
        match (side, missing_count) {
            (CandidateSetSide::Native, true) => record.native_count = None,
            (CandidateSetSide::Native, false) => record.native_digest = None,
            (CandidateSetSide::Engine, true) => record.engine_count = None,
            (CandidateSetSide::Engine, false) => record.engine_digest = None,
        }

        // Then: the exact side-specific atomicity error is returned.
        assert_error(
            &fixture,
            CandidateAuditError::UnpairedCandidateFields {
                dataset: Dataset::Wands,
                record: 0,
                side,
            },
        );
    }
}

#[test]
fn candidate_digests_require_exactly_64_lowercase_hex_characters() {
    // Given/When: a digest is uppercase, non-hex, or the wrong length.
    for invalid in ["A".repeat(64), "g".repeat(64), "a".repeat(63)] {
        let mut fixture = CompleteCandidateAuditFixture::complete();
        fixture.wands_records[0].native_digest = Some(invalid);

        // Then: syntax fails before verdict comparison.
        assert_error(
            &fixture,
            CandidateAuditError::InvalidCandidateDigest {
                dataset: Dataset::Wands,
                record: 0,
                side: CandidateSetSide::Native,
            },
        );
    }

    // Given/When: both sides use the same valid lowercase digest.
    let mut fixture = CompleteCandidateAuditFixture::complete();
    fixture.wands_records[0].native_digest = Some("a".repeat(64));
    fixture.wands_records[0].engine_digest = Some("a".repeat(64));

    // Then: the digest syntax is accepted.
    assert!(validate(&fixture).is_ok());
}

#[test]
fn match_requires_equal_pairs_empty_differences_and_no_failure() {
    // Given/When: each forbidden Match shape is introduced independently.
    for case in 0..5 {
        let mut fixture = CompleteCandidateAuditFixture::complete();
        let record = &mut fixture.wands_records[0];
        match case {
            0 => record.engine_count = Some(2),
            1 => record.engine_digest = Some("b".repeat(64)),
            2 => record.only_native.push("native-only".to_owned()),
            3 => record.only_engine.push("engine-only".to_owned()),
            4 => record.failure_reason = Some("unexpected failure".to_owned()),
            _ => unreachable!("the test matrix contains five cases"),
        }

        // Then: every malformed Match returns its exact shape error.
        assert_error(
            &fixture,
            CandidateAuditError::InvalidVerdictShape {
                dataset: Dataset::Wands,
                record: 0,
                verdict: AuditVerdict::Match,
            },
        );
    }
}

#[test]
fn mismatch_shape_distinguishes_count_only_and_digest_differences() {
    // Given: a count-only inequality with no enumerated differences.
    let mut count_only = CompleteCandidateAuditFixture::complete();
    count_only.wands_records[0].verdict = AuditVerdict::Mismatch;
    count_only.wands_records[0].engine_count = Some(2);

    // When/Then: count-only inequality is complete negative evidence.
    let summary = validate(&count_only).expect("count-only mismatch must be valid");
    assert_eq!(summary.mismatched_records, 1);
    assert!(!summary.equivalence_passed);

    // Given/When: equal pairs are labelled Mismatch.
    let mut equal = CompleteCandidateAuditFixture::complete();
    equal.wands_records[0].verdict = AuditVerdict::Mismatch;

    // Then: the exact shape error rejects the false mismatch.
    assert_error(
        &equal,
        CandidateAuditError::InvalidVerdictShape {
            dataset: Dataset::Wands,
            record: 0,
            verdict: AuditVerdict::Mismatch,
        },
    );

    // Given/When: digests differ without enumerated differences.
    let mut digest_only = CompleteCandidateAuditFixture::complete();
    digest_only.wands_records[0].verdict = AuditVerdict::Mismatch;
    digest_only.wands_records[0].engine_digest = Some("b".repeat(64));

    // Then: digest inequality requires concrete difference evidence.
    assert_error(
        &digest_only,
        CandidateAuditError::InvalidVerdictShape {
            dataset: Dataset::Wands,
            record: 0,
            verdict: AuditVerdict::Mismatch,
        },
    );
}

#[test]
fn digest_mismatch_with_directional_differences_blocks_the_gate() {
    // Given: valid unequal digests with concrete differences in both directions.
    let records = campaign_records();
    let exact_index = exact_observations();
    let mut fixture = CompleteCandidateAuditFixture::complete();
    let record = &mut fixture.wands_records[0];
    record.verdict = AuditVerdict::Mismatch;
    record.engine_digest = Some("b".repeat(64));
    record.only_native.push("native-only".to_owned());
    record.only_engine.push("engine-only".to_owned());

    // When: the candidate evidence and complete campaign are evaluated.
    let summary = validate(&fixture).expect("directional digest mismatch must be valid");
    let analysis = analyze_with_candidate_audits(&records, &exact_index, &fixture)
        .expect("valid mismatch evidence must analyze");

    // Then: the mismatch is retained as complete negative evidence.
    assert_eq!(summary.mismatched_records, 1);
    assert!(!summary.equivalence_passed);
    assert_eq!(analysis.gate_report.verdict(), GateVerdict::FixMeasurement);
}

#[test]
fn mismatch_with_failure_reason_returns_exact_shape_error() {
    // Given: an otherwise valid count mismatch also claims a failure.
    let mut fixture = CompleteCandidateAuditFixture::complete();
    let record = &mut fixture.wands_records[0];
    record.verdict = AuditVerdict::Mismatch;
    record.engine_count = Some(2);
    record.failure_reason = Some("unexpected failure".to_owned());

    // When/Then: mismatch and failure evidence cannot be conflated.
    assert_error(
        &fixture,
        CandidateAuditError::InvalidVerdictShape {
            dataset: Dataset::Wands,
            record: 0,
            verdict: AuditVerdict::Mismatch,
        },
    );
}

#[test]
fn valid_failure_verdicts_are_complete_negative_evidence() {
    // Given: either engine side failed with a reason and no claimed differences.
    for verdict in [AuditVerdict::NativeFailure, AuditVerdict::EngineFailure] {
        let records = campaign_records();
        let exact_index = exact_observations();
        let mut fixture = CompleteCandidateAuditFixture::complete();
        configure_failure(&mut fixture.wands_records[0], verdict);

        // When: the complete campaign is analyzed.
        let analysis = analyze_with_candidate_audits(&records, &exact_index, &fixture)
            .expect("valid failure evidence must analyze");

        // Then: negative evidence is retained and blocks the gate.
        assert!(!analysis.candidate_audit_summary.equivalence_passed);
        assert_eq!(analysis.gate_report.verdict(), GateVerdict::FixMeasurement);
    }
}

#[test]
fn every_failure_requires_reason_empty_differences_and_failed_side_absence() {
    // Given/When: each required failure-shape field is violated on each failed side.
    for verdict in [AuditVerdict::NativeFailure, AuditVerdict::EngineFailure] {
        for case in 0..5 {
            let mut fixture = CompleteCandidateAuditFixture::complete();
            let record = &mut fixture.wands_records[0];
            configure_failure(record, verdict);
            match case {
                0 => record.failure_reason = None,
                1 => record.failure_reason = Some("  ".to_owned()),
                2 => record.only_native.push("native-only".to_owned()),
                3 => record.only_engine.push("engine-only".to_owned()),
                4 => match verdict {
                    AuditVerdict::NativeFailure => {
                        record.native_count = Some(1);
                        record.native_digest = Some("a".repeat(64));
                    }
                    AuditVerdict::EngineFailure => {
                        record.engine_count = Some(1);
                        record.engine_digest = Some("a".repeat(64));
                    }
                    AuditVerdict::Match | AuditVerdict::Mismatch => unreachable!(),
                },
                _ => unreachable!("the failure-shape matrix contains five cases"),
            }

            // Then: the exact verdict-specific shape error is returned.
            assert_error(
                &fixture,
                CandidateAuditError::InvalidVerdictShape {
                    dataset: Dataset::Wands,
                    record: 0,
                    verdict,
                },
            );
        }
    }
}
