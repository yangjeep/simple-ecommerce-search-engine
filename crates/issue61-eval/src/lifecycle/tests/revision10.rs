use super::{run1, FakePort, Terminal};
use crate::lifecycle::{
    EventType, EvidenceFile, FileOperation, LifecycleError, Operation, Phase, SeriesIdentity,
};
use crate::{AnalysisError, Engine};
use serde_json::{json, Value};

#[test]
fn partial_initialization_after_events_attempts_staged_termination_and_full_cleanup() {
    // Given
    let mut fake = FakePort::default();
    fake.fail_open = Some(EvidenceFile::IndexArtifacts);

    // When
    let error = run1(&mut fake).expect_err("partial initialization must terminate");

    // Then
    assert_eq!(error, LifecycleError::Initialization);
    assert!(fake.has_event(Phase::StaticValidation, EventType::PhaseStarted));
    assert!(fake.last_event_is(EventType::LifecycleTerminated));
    assert!(fake.operations().contains(&Operation::TeardownEnvironment));
    assert!(fake.was_closed(EvidenceFile::Events));
    assert!(!fake.seal_created);
}

#[test]
fn initialization_failure_before_events_creates_no_event_stream() {
    // Given
    let mut fake = FakePort::default();
    fake.fail_open = Some(EvidenceFile::Events);

    // When
    let error = run1(&mut fake).expect_err("pre-events initialization must fail");

    // Then
    assert_eq!(error, LifecycleError::Initialization);
    assert!(fake.event_lines().is_empty());
    assert!(!fake.was_closed(EvidenceFile::Events));
}

#[test]
fn cleanup_continues_after_close_failures_and_preserves_primary_error() {
    // Given
    let mut fake = FakePort::default();
    fake.fail_command_append = true;
    fake.fail_flush = Some(EvidenceFile::CandidateAuditWands);

    // When
    let error = run1(&mut fake).expect_err("operational failure must be primary");

    // Then
    assert_eq!(error, LifecycleError::CommandEvidence);
    for file in EvidenceFile::ALL {
        assert!(
            fake.close_was_attempted(file),
            "close not attempted for {file:?}"
        );
    }
}

#[test]
fn gate_cleanup_surfaces_first_cleanup_error_when_no_operational_error_exists() {
    // Given
    let mut fake = FakePort::default();
    fake.equivalence_passes = false;
    fake.fail_flush = Some(EvidenceFile::Commands);

    // When
    let error = run1(&mut fake).expect_err("cleanup failure must surface");

    // Then
    assert_eq!(error, LifecycleError::Flush(EvidenceFile::Commands));
    assert!(fake.close_was_attempted(EvidenceFile::Events));
}

#[test]
fn process_failure_cannot_be_accepted_as_slot_success() {
    // Given
    let mut fake = FakePort::default();
    fake.contradictory_slot_success = true;

    // When
    let error = run1(&mut fake).expect_err("failed process cannot yield accepted raw evidence");

    // Then
    assert_eq!(error, LifecycleError::SlotFailed);
    assert_eq!(fake.raw_lines(), 0);
}

#[test]
fn calibration_analysis_preserves_exact_engine_identity_in_error_and_event() {
    // Given
    let mut fake = FakePort::default();
    fake.duplicate_calibration_identity = true;

    // When
    let error = run1(&mut fake).expect_err("duplicate calibration identity must stop");

    // Then
    match error {
        LifecycleError::Calibration(AnalysisError::DuplicateIdentity { key, .. }) => {
            assert_eq!(key.engine, Engine::Native);
        }
        other => panic!("unexpected error: {other:?}"),
    }
    let operations = fake.operations();
    let gate_start = operations
        .iter()
        .position(|operation| {
            matches!(
                operation,
                Operation::AppendEvent {
                    phase: Phase::CalibrationGate,
                    event_type: EventType::PhaseStarted,
                }
            )
        })
        .expect("calibration gate start is observable");
    let analysis = operations
        .iter()
        .position(|operation| matches!(operation, Operation::AnalyzeCalibration { records: 120 }))
        .expect("calibration analysis is observable");
    assert!(gate_start < analysis);
    let termination = fake
        .event_lines()
        .into_iter()
        .map(parse_event)
        .find(|event| event["event_type"] == json!("LifecycleTerminated"))
        .expect("termination event exists");
    assert_eq!(termination["phase"], json!("CalibrationGate"));
    assert!(termination["reason"]["Public"]
        .as_str()
        .expect("classified reason is public")
        .contains("engine: Native"));
}

#[test]
fn successful_campaign_has_fixed_accounting_and_exact_phase_grammar() {
    // Given
    let mut fake = FakePort::default();

    // When
    let terminal = run1(&mut fake).expect("campaign succeeds");

    // Then
    assert_eq!(terminal, Terminal::Completed);
    assert_eq!(fake.raw_lines(), 620);
    assert_eq!(fake.command_lines().count(), 620);
    assert_eq!(
        fake.event_lines()
            .into_iter()
            .map(parse_event)
            .filter(|event| event["event_type"] == json!("BlockCompleted"))
            .count(),
        310
    );
    assert!(fake
        .operations()
        .contains(&Operation::AnalyzeCalibration { records: 120 }));
    assert_eq!(fake.slot_runs_in(SeriesIdentity::Calibration), 120);
    for phase in Phase::ALL {
        assert!(fake.has_event(phase, EventType::PhaseStarted));
        if phase == Phase::EvidenceFinalization {
            assert!(!fake.has_event(phase, EventType::PhaseCompleted));
        } else {
            assert!(fake.has_event(phase, EventType::PhaseCompleted));
        }
    }
    let phase_grammar = fake
        .event_lines()
        .into_iter()
        .map(parse_event)
        .filter(|event| event["block_index"].is_null())
        .map(|event| (event["phase"].clone(), event["event_type"].clone()))
        .collect::<Vec<_>>();
    assert_eq!(
        phase_grammar,
        vec![
            (json!("StaticValidation"), json!("PhaseStarted")),
            (json!("StaticValidation"), json!("PhaseCompleted")),
            (json!("Initialization"), json!("PhaseStarted")),
            (json!("Initialization"), json!("PhaseCompleted")),
            (json!("EquivalenceAudit"), json!("PhaseStarted")),
            (json!("EquivalenceAudit"), json!("PhaseCompleted")),
            (json!("IndexCapture"), json!("PhaseStarted")),
            (json!("IndexCapture"), json!("PhaseCompleted")),
            (json!("EquivalenceGate"), json!("PhaseStarted")),
            (json!("EquivalenceGate"), json!("GatePassed")),
            (json!("EquivalenceGate"), json!("PhaseCompleted")),
            (json!("Calibration"), json!("PhaseStarted")),
            (json!("Calibration"), json!("PhaseCompleted")),
            (json!("CalibrationGate"), json!("PhaseStarted")),
            (json!("CalibrationGate"), json!("GatePassed")),
            (json!("CalibrationGate"), json!("PhaseCompleted")),
            (json!("Warm"), json!("PhaseStarted")),
            (json!("Warm"), json!("PhaseCompleted")),
            (json!("Cold"), json!("PhaseStarted")),
            (json!("Cold"), json!("PhaseCompleted")),
            (json!("EvidenceFinalization"), json!("PhaseStarted")),
            (json!("EvidenceFinalization"), json!("EvidenceFinalized")),
        ]
    );
}

#[test]
fn durability_operations_are_write_all_and_flush_sync_close() {
    // Given
    let mut fake = FakePort::default();

    // When
    run1(&mut fake).expect("campaign succeeds");

    // Then
    assert!(fake.evidence_operations().iter().all(|operation| matches!(
        operation,
        Operation::Evidence {
            operation: FileOperation::CreateNew | FileOperation::WriteAll,
            ..
        }
    )));
    for file in EvidenceFile::ALL {
        assert!(fake.has_durable_close(file));
    }
    assert!(fake.has_durable_seal_close());
}

#[test]
fn derived_cycle_path_is_exact_and_component_safe() {
    // Given
    let mut fake = FakePort::default();

    // When
    run1(&mut fake).expect("campaign succeeds");

    // Then
    assert_eq!(
        fake.initialized_cycle_path(),
        "/repo/artifacts/issue61/i61_e1_run1"
    );
    assert!(fake.cycle_path_components_safe());
}

#[test]
fn gate_and_termination_events_have_classified_reasons() {
    // Given
    let mut fake = FakePort::default();
    fake.equivalence_passes = false;

    // When
    run1(&mut fake).expect("gate failure is a valid terminal");

    // Then
    for event in fake
        .event_lines()
        .into_iter()
        .map(parse_event)
        .filter(|event| {
            event["event_type"] == json!("GateFailed")
                || event["event_type"] == json!("LifecycleTerminated")
        })
    {
        assert!(event["reason"].is_object());
    }
}

#[test]
fn equivalence_audit_and_index_capture_failures_are_attributed_separately() {
    // Given / When / Then
    let mut audit = FakePort::default();
    audit.fail_equivalence_audit = true;
    assert_eq!(
        run1(&mut audit).expect_err("audit must fail"),
        LifecycleError::EquivalenceAudit
    );
    assert!(audit.last_event_is(EventType::LifecycleTerminated));

    let mut index = FakePort::default();
    index.fail_index_capture = true;
    assert_eq!(
        run1(&mut index).expect_err("capture must fail"),
        LifecycleError::IndexCapture
    );
    assert!(index.last_event_is(EventType::LifecycleTerminated));
}

fn parse_event(line: &str) -> Value {
    serde_json::from_str(line).expect("evidence line is JSON")
}
