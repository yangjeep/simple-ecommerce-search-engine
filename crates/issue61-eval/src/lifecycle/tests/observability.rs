use super::{run1, FakePort};
use crate::lifecycle::{EventType, EvidenceFile, FileOperation, Operation, Phase, SlotIndex};
use crate::{CampaignSeries, Engine};
use serde_json::{json, Value};

#[test]
fn failed_slot_command_records_the_actual_non_success_outcome() {
    // Given
    let mut fake = FakePort::default();
    fake.fail_slot(
        CampaignSeries::Calibration {
            engine: Engine::Native,
        },
        0,
        SlotIndex::zero(),
    );

    // When
    assert!(run1(&mut fake).is_err());

    // Then
    let command: Value = serde_json::from_str(
        fake.command_lines()
            .next()
            .expect("failed slot emits command evidence"),
    )
    .expect("command evidence is JSON");
    assert_eq!(command["outcome"], json!({"Exited":{"exit_code":1}}));
    assert_ne!(command["outcome"], json!({"Exited":{"exit_code":0}}));
    assert_eq!(
        slot_event_types(&fake),
        vec![
            "SlotStarted",
            "SlotFailed",
            "TeardownStarted",
            "TeardownCompleted",
        ]
    );
}

#[test]
fn command_append_failure_still_emits_failure_and_teardown() {
    // Given
    let mut fake = FakePort::default();
    fake.fail_command_append = true;

    // When
    let error = run1(&mut fake).expect_err("command evidence append must fail the lifecycle");

    // Then
    assert_eq!(error, crate::lifecycle::LifecycleError::CommandEvidence);
    assert_eq!(
        slot_event_types(&fake),
        vec![
            "SlotStarted",
            "SlotFailed",
            "TeardownStarted",
            "TeardownCompleted",
        ]
    );
}

#[test]
fn actual_warm_slot_events_include_dataset_and_projection() {
    // Given
    let mut fake = FakePort::default();

    // When
    run1(&mut fake).expect("lifecycle succeeds");

    // Then
    let event = fake
        .event_lines()
        .into_iter()
        .map(|line| serde_json::from_str::<Value>(line).expect("event evidence is JSON"))
        .find(|event| {
            event["phase"] == json!("Warm") && event["event_type"] == json!("SlotStarted")
        })
        .expect("warm slot event exists");
    assert!(matches!(
        event["dataset"],
        Value::String(ref value) if value == "wands" || value == "esci_electronics"
    ));
    assert!(matches!(
        event["projection"],
        Value::String(ref value)
            if value == "all" || value == "fast-path" || value == "hybrid" || value == "punt"
    ));
}

#[test]
fn evidence_operations_are_create_new_then_append_only() {
    // Given
    let mut fake = FakePort::default();

    // When
    run1(&mut fake).expect("lifecycle succeeds");

    // Then
    let operations = fake.evidence_operations();
    assert_eq!(
        &operations[..EvidenceFile::ALL.len()],
        EvidenceFile::ALL
            .map(|file| Operation::Evidence {
                file,
                operation: FileOperation::CreateNew,
            })
            .as_slice()
    );
    assert!(operations[EvidenceFile::ALL.len()..]
        .iter()
        .all(|operation| {
            matches!(
                operation,
                Operation::Evidence {
                    operation: FileOperation::WriteAll,
                    ..
                }
            )
        }));
}

#[test]
fn gate_start_events_precede_gate_work() {
    // Given
    let mut fake = FakePort::default();

    // When
    run1(&mut fake).expect("lifecycle succeeds");

    // Then
    let operations = fake.operations();
    let audit = position(operations, Operation::AuditEquivalence);
    let index = position(operations, Operation::CaptureIndex);
    let equivalence_start = position(
        operations,
        Operation::AppendEvent {
            phase: Phase::EquivalenceGate,
            event_type: EventType::PhaseStarted,
        },
    );
    let equivalence_evaluation = position(operations, Operation::EvaluateEquivalenceGate);
    let calibration_start = operations
        .iter()
        .position(|operation| {
            matches!(
                operation,
                Operation::AppendEvent {
                    phase: Phase::Calibration,
                    event_type: EventType::PhaseStarted,
                }
            )
        })
        .expect("calibration start is observable");
    let calibration_gate_start = position(
        operations,
        Operation::AppendEvent {
            phase: Phase::CalibrationGate,
            event_type: EventType::PhaseStarted,
        },
    );
    let calibration_analysis = position(operations, Operation::AnalyzeCalibration { records: 120 });
    assert!(audit < index);
    assert!(index < equivalence_start && equivalence_start < equivalence_evaluation);
    assert!(equivalence_evaluation < calibration_start);
    assert!(calibration_start < calibration_gate_start);
    assert!(calibration_gate_start < calibration_analysis);
}

fn position(operations: &[Operation], expected: Operation) -> usize {
    operations
        .iter()
        .position(|operation| *operation == expected)
        .expect("operation is observable")
}

fn slot_event_types(fake: &FakePort) -> Vec<String> {
    fake.event_lines()
        .into_iter()
        .map(|line| serde_json::from_str::<Value>(line).expect("event evidence is JSON"))
        .filter(|event| {
            event["phase"] == json!("Calibration")
                && event["block_index"] == json!(0)
                && event["slot_index"] == json!(0)
        })
        .filter_map(|event| event["event_type"].as_str().map(str::to_owned))
        .collect()
}
