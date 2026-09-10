use super::{run1, FakePort, Terminal};
use crate::lifecycle::{EventType, EvidenceFile, LifecycleError, Operation, Phase};

#[test]
fn finalization_closes_five_files_then_events_then_seals_and_analyzes() {
    // Given
    let mut fake = FakePort::default();

    // When
    let terminal = run1(&mut fake).expect("lifecycle succeeds");

    // Then
    assert_eq!(terminal, Terminal::Completed);
    assert_eq!(
        fake.finalization_operations(),
        vec![
            Operation::Flush(EvidenceFile::CandidateAuditEsci),
            Operation::Sync(EvidenceFile::CandidateAuditEsci),
            Operation::close(EvidenceFile::CandidateAuditEsci),
            Operation::Flush(EvidenceFile::CandidateAuditWands),
            Operation::Sync(EvidenceFile::CandidateAuditWands),
            Operation::close(EvidenceFile::CandidateAuditWands),
            Operation::Flush(EvidenceFile::Commands),
            Operation::Sync(EvidenceFile::Commands),
            Operation::close(EvidenceFile::Commands),
            Operation::Flush(EvidenceFile::IndexArtifacts),
            Operation::Sync(EvidenceFile::IndexArtifacts),
            Operation::close(EvidenceFile::IndexArtifacts),
            Operation::Flush(EvidenceFile::Raw),
            Operation::Sync(EvidenceFile::Raw),
            Operation::close(EvidenceFile::Raw),
            Operation::AppendEvent {
                phase: Phase::EvidenceFinalization,
                event_type: EventType::EvidenceFinalized,
            },
            Operation::Flush(EvidenceFile::Events),
            Operation::Sync(EvidenceFile::Events),
            Operation::close(EvidenceFile::Events),
            Operation::ComputeHash,
            Operation::ComputeHash,
            Operation::ComputeHash,
            Operation::ComputeHash,
            Operation::ComputeHash,
            Operation::ComputeHash,
            Operation::CreateSeal,
            Operation::WriteSeal { entries: 12 },
            Operation::FlushSeal,
            Operation::SyncSeal,
            Operation::CloseSeal,
            Operation::VerifySeal,
            Operation::InvokeAnalyzer,
        ]
    );
    assert!(fake.last_event_is(EventType::EvidenceFinalized));
    assert!(!fake.wrote_after_seal);
}

#[test]
fn failed_non_event_close_terminates_through_events_without_seal() {
    // Given
    let mut fake = FakePort::default();
    fake.fail_close = Some(EvidenceFile::Commands);

    // When
    let error = run1(&mut fake).expect_err("close failure terminates");

    // Then
    assert_eq!(error, LifecycleError::Close(EvidenceFile::Commands));
    assert!(fake.last_event_is(EventType::LifecycleTerminated));
    assert!(fake.was_closed(EvidenceFile::Events));
    assert!(!fake.seal_created);
    assert!(!fake.analyzer_invoked);
}

#[test]
fn failed_non_event_flush_completes_durable_close_and_preserves_first_error() {
    // Given
    let mut fake = FakePort::default();
    fake.fail_flush = Some(EvidenceFile::Commands);
    fake.fail_close = Some(EvidenceFile::Events);

    // When
    let error = run1(&mut fake).expect_err("flush failure terminates");

    // Then
    assert_eq!(error, LifecycleError::Flush(EvidenceFile::Commands));
    assert!(fake.has_durable_close(EvidenceFile::Commands));
    assert!(fake.was_closed(EvidenceFile::Events));
    assert!(!fake.seal_created);
    assert!(!fake.analyzer_invoked);
}

#[test]
fn failed_non_event_sync_completes_durable_close_without_seal() {
    // Given
    let mut fake = FakePort::default();
    fake.fail_sync = Some(EvidenceFile::Commands);

    // When
    let error = run1(&mut fake).expect_err("sync failure terminates");

    // Then
    assert_eq!(error, LifecycleError::Sync(EvidenceFile::Commands));
    assert!(fake.has_durable_close(EvidenceFile::Commands));
    assert!(fake.was_closed(EvidenceFile::Events));
    assert!(!fake.seal_created);
    assert!(!fake.analyzer_invoked);
}

#[test]
fn incomplete_or_invalid_seal_never_invokes_analyzer() {
    // Given
    let mut fake = FakePort::default();
    fake.seal_valid = false;

    // When
    let error = run1(&mut fake).expect_err("invalid seal stops");

    // Then
    assert_eq!(error, LifecycleError::InvalidSeal);
    assert!(fake.seal_created);
    assert!(!fake.analyzer_invoked);
}

#[test]
fn verified_seven_file_cycle_is_the_only_analyzer_entry() {
    // Given
    let mut fake = FakePort::default();

    // When
    run1(&mut fake).expect("valid seal succeeds");

    // Then
    assert_eq!(fake.regular_file_count(), 7);
    assert!(fake.seal_verified);
    assert!(fake.analyzer_invoked);
    assert!(fake.has_event(Phase::EvidenceFinalization, EventType::EvidenceFinalized));
}

#[test]
fn evidence_finalized_append_failure_closes_events_without_sealing_or_analysis() {
    // Given
    let mut fake = FakePort::default();
    fake.fail_event_append = Some((Phase::EvidenceFinalization, EventType::EvidenceFinalized));

    // When
    let error = run1(&mut fake).expect_err("final event append must fail finalization");

    // Then
    assert_eq!(error, LifecycleError::EventEvidence);
    assert!(fake.was_closed(EvidenceFile::Events));
    assert!(!fake.seal_created);
    assert!(!fake.analyzer_invoked);
}
