use super::{analyze, campaign_records, exact_observations};
use crate::{AnalysisError, Engine, ProcessReconciliationError, RawRecord, SessionMode};

fn native_record(records: &mut [RawRecord], mode: SessionMode) -> (usize, &mut RawRecord) {
    records
        .iter_mut()
        .enumerate()
        .find(|(_, record)| {
            record.engine == Engine::Native.as_str() && record.regime == mode.as_str()
        })
        .expect("complete campaign must contain the requested native phase")
}

fn solr_record(records: &mut [RawRecord]) -> (usize, &mut RawRecord) {
    records
        .iter_mut()
        .enumerate()
        .find(|(_, record)| record.engine == Engine::Solr.as_str())
        .expect("complete campaign must contain Solr records")
}

fn assert_error(records: &[RawRecord], expected: ProcessReconciliationError) {
    let result = analyze(records, &exact_observations());
    assert!(matches!(
        result,
        Err(AnalysisError::ProcessReconciliation(actual)) if actual == expected
    ));
}

#[test]
fn each_missing_process_field_is_rejected_in_every_native_phase() {
    // Given: each operation removes one member of the native process tuple.
    let clear_fields: [fn(&mut RawRecord); 4] = [
        |record| record.process_cpu_user_usec = None,
        |record| record.process_cpu_system_usec = None,
        |record| record.process_cpu_total_usec = None,
        |record| record.process_cgroup_disagreement_pct = None,
    ];

    // When: every field is removed independently from every session mode.
    for mode in [
        SessionMode::CalibrationFour,
        SessionMode::CalibrationFive,
        SessionMode::Warm,
        SessionMode::Cold,
    ] {
        for clear_field in clear_fields {
            let mut records = campaign_records();
            let (index, record) = native_record(&mut records, mode);
            clear_field(record);

            // Then: every phase and field returns the exact source-indexed error.
            assert_error(
                &records,
                ProcessReconciliationError::IncompleteNativeTuple { record: index },
            );
        }
    }
}

#[test]
fn native_user_and_system_total_must_match_without_overflow() {
    // Given/When: the stored total differs from user plus system.
    let mut mismatch = campaign_records();
    let (mismatch_index, record) = native_record(&mut mismatch, SessionMode::Warm);
    record.process_cpu_total_usec = Some(1_001);

    // Then: the exact total mismatch is returned.
    assert_error(
        &mismatch,
        ProcessReconciliationError::CpuTotalMismatch {
            record: mismatch_index,
        },
    );

    // Given/When: user plus system exceeds u64.
    let mut overflow = campaign_records();
    let (overflow_index, record) = native_record(&mut overflow, SessionMode::Warm);
    record.process_cpu_user_usec = Some(u64::MAX);
    record.process_cpu_system_usec = Some(1);

    // Then: overflow is distinguished from an ordinary mismatch.
    assert_error(
        &overflow,
        ProcessReconciliationError::CpuTotalOverflow {
            record: overflow_index,
        },
    );
}

#[test]
fn native_cgroup_cpu_must_be_nonzero() {
    // Given: a complete native tuple with a zero cgroup denominator.
    let mut records = campaign_records();
    let (index, record) = native_record(&mut records, SessionMode::Warm);
    record.cpu_usage_usec = 0;

    // When/Then: zero cannot produce a percentage.
    assert_error(
        &records,
        ProcessReconciliationError::ZeroCgroupCpu { record: index },
    );
}

#[test]
fn stored_disagreement_must_be_finite_and_nonnegative() {
    // Given/When: stored disagreement is NaN, infinite, or negative.
    for invalid in [f64::NAN, f64::INFINITY, -0.1] {
        let mut records = campaign_records();
        let (index, record) = native_record(&mut records, SessionMode::Warm);
        record.process_cgroup_disagreement_pct = Some(invalid);

        // Then: each value returns the exact stored-percentage error.
        assert_error(
            &records,
            ProcessReconciliationError::InvalidStoredDisagreement { record: index },
        );
    }
}

#[test]
fn stored_disagreement_must_equal_the_recomputed_percentage() {
    // Given: valid totals with an incorrect stored percentage.
    let mut records = campaign_records();
    let (index, record) = native_record(&mut records, SessionMode::Warm);
    record.process_cgroup_disagreement_pct = Some(1.0);

    // When/Then: stale derived evidence is rejected exactly.
    assert_error(
        &records,
        ProcessReconciliationError::StoredDisagreementMismatch { record: index },
    );
}

#[test]
fn any_solr_process_field_is_rejected() {
    // Given/When: each forbidden Solr process field appears independently.
    for field in 0..4 {
        let mut records = campaign_records();
        let (index, record) = solr_record(&mut records);
        match field {
            0 => record.process_cpu_user_usec = Some(1),
            1 => record.process_cpu_system_usec = Some(1),
            2 => record.process_cpu_total_usec = Some(1),
            3 => record.process_cgroup_disagreement_pct = Some(0.0),
            _ => unreachable!("the Solr process tuple has four fields"),
        }

        // Then: every partial Solr tuple returns the exact source-indexed error.
        assert_error(
            &records,
            ProcessReconciliationError::UnexpectedSolrTuple { record: index },
        );
    }
}
