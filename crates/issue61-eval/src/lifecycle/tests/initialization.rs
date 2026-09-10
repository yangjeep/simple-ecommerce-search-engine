use super::{run1, FakePort};
use crate::lifecycle::{LifecycleError, Operation, PathState};

#[test]
fn failed_static_validation_never_initializes_or_persists_evidence() {
    // Given
    let mut fake = FakePort::default();
    fake.static_validation_passes = false;

    // When
    let error = run1(&mut fake).expect_err("static validation must fail closed");

    // Then
    assert_eq!(error, LifecycleError::StaticValidation);
    assert_eq!(fake.operations(), &[Operation::ValidateStatic]);
    assert_eq!(fake.opened_evidence_files(), 0);
    assert!(fake.event_lines().is_empty());
}

#[test]
fn initialize_failure_persists_no_staged_events() {
    // Given
    let mut fake = FakePort::default();
    fake.path_state = PathState::ExistingCycle;

    // When
    let error = run1(&mut fake).expect_err("existing cycle must fail initialization");

    // Then
    assert_eq!(error, LifecycleError::Initialization);
    assert_eq!(
        fake.operations(),
        &[Operation::ValidateStatic, Operation::InitializeCreateNew]
    );
    assert_eq!(fake.opened_evidence_files(), 0);
    assert!(fake.event_lines().is_empty());
}
