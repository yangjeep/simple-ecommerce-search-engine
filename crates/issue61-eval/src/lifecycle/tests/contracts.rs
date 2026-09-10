use super::{run1, FakePort, Terminal};
use crate::lifecycle::{
    prepare_command, serialize_command, CommandInput, CommandOutcome, CommandRequest,
    DatasetIdentity, EnvironmentInput, EventType, ExecutionEvidence, LifecycleError, LoggedText,
    Operation, OutputVisibility, Phase, Sequence,
};
use serde_json::json;

#[test]
fn command_json_is_exact_and_redacts_classified_values() {
    // Given
    let prepared = prepare_command(CommandRequest {
        executable: "runner".to_owned(),
        args: vec![
            CommandInput::Public("--cycle".to_owned()),
            CommandInput::Redacted("secret".to_owned()),
        ],
        env: vec![
            EnvironmentInput::redacted("Z", "secret"),
            EnvironmentInput::public("A", "visible"),
        ],
        environment_allowlist: vec!["A".to_owned(), "Z".to_owned()],
        stdout_visibility: OutputVisibility::Public,
        stderr_visibility: OutputVisibility::Redacted,
    })
    .expect("classified command prepares");
    let capture = prepared.capture(ExecutionEvidence {
        outcome: CommandOutcome::Exited { exit_code: 0 },
        stdout: "ok".to_owned(),
        stderr: "secret".to_owned(),
    });

    // When
    let line = serialize_command(crate::CampaignCycle::Run1, Sequence::new(1), capture)
        .expect("classified command serializes");

    // Then
    assert_eq!(
        line,
        concat!(
            "{\"schema_version\":1,\"experiment_id\":\"I61-E1\",\"cycle\":\"run1\",",
            "\"seq\":1,\"executable\":\"runner\",\"args\":[{\"Public\":\"--cycle\"},",
            "{\"Redacted\":\"<redacted>\"}],\"env\":[{\"name\":\"A\",\"value\":",
            "{\"Public\":\"visible\"}},{\"name\":\"Z\",\"value\":",
            "{\"Redacted\":\"<redacted>\"}}],\"outcome\":{\"Exited\":{\"exit_code\":0}},",
            "\"stdout\":{\"Public\":\"ok\"},\"stderr\":{\"Redacted\":\"<redacted>\"}}\n"
        )
    );
}

#[test]
fn command_rejects_unclassified_input_before_execution() {
    // Given
    let mut fake = FakePort::default();
    fake.unclassified_command = true;

    // When
    let result = run1(&mut fake);

    // Then
    assert!(result.is_err());
    assert_eq!(fake.slot_runs, 0);
    assert_eq!(fake.command_lines().count(), 0);
    assert!(!fake.operations().contains(&Operation::ExecuteSlot));
    assert!(!fake.has_event(Phase::Calibration, EventType::SlotStarted));
}

#[test]
fn command_rejects_duplicate_environment_before_execution() {
    // Given
    let mut fake = FakePort::default();
    fake.duplicate_environment = true;

    // When
    let error = run1(&mut fake).expect_err("duplicate environment must stop before execution");

    // Then
    assert_eq!(error, LifecycleError::InvalidEnvironment);
    assert_eq!(fake.slot_runs, 0);
    assert_eq!(fake.command_lines().count(), 0);
    assert!(!fake.operations().contains(&Operation::ExecuteSlot));
    assert!(!fake.has_event(Phase::Calibration, EventType::SlotStarted));
}

#[test]
fn prepared_command_contains_only_classified_inputs_in_sorted_environment_order() {
    // Given
    let request = CommandRequest {
        executable: "runner".to_owned(),
        args: vec![
            CommandInput::Public("visible".to_owned()),
            CommandInput::Redacted("secret".to_owned()),
        ],
        env: vec![
            EnvironmentInput::redacted("Z", "secret"),
            EnvironmentInput::public("A", "visible"),
        ],
        environment_allowlist: vec!["A".to_owned(), "Z".to_owned()],
        stdout_visibility: OutputVisibility::Public,
        stderr_visibility: OutputVisibility::Redacted,
    };

    // When
    let prepared = prepare_command(request).expect("classified request prepares");

    // Then
    assert_eq!(
        prepared.args(),
        [
            LoggedText::Public("visible".to_owned()),
            LoggedText::Redacted("<redacted>".to_owned()),
        ]
    );
    assert_eq!(
        prepared.environment_names().collect::<Vec<_>>(),
        vec!["A", "Z"]
    );
}

#[test]
fn prepared_command_preserves_direct_values_separately_from_redacted_evidence() {
    // Given
    let request = CommandRequest {
        executable: "runner".to_owned(),
        args: vec![CommandInput::Redacted("argument-secret".to_owned())],
        env: vec![EnvironmentInput::redacted("TOKEN", "environment-secret")],
        environment_allowlist: vec!["TOKEN".to_owned()],
        stdout_visibility: OutputVisibility::Public,
        stderr_visibility: OutputVisibility::Redacted,
    };

    // When
    let prepared = prepare_command(request).expect("classified request prepares");

    // Then
    assert_eq!(prepared.executable(), "runner");
    assert_eq!(prepared.direct_args(), ["argument-secret"]);
    assert_eq!(
        prepared.direct_environment().collect::<Vec<_>>(),
        vec![("TOKEN", "environment-secret")]
    );
    assert_eq!(
        prepared.args(),
        [LoggedText::Redacted("<redacted>".to_owned())]
    );
}

#[test]
fn command_rejects_environment_name_outside_declared_allowlist() {
    // Given
    let request = CommandRequest {
        executable: "runner".to_owned(),
        args: Vec::new(),
        env: vec![EnvironmentInput::public("UNEXPECTED", "value")],
        environment_allowlist: vec!["EXPECTED".to_owned()],
        stdout_visibility: OutputVisibility::Public,
        stderr_visibility: OutputVisibility::Public,
    };

    // When
    let result = prepare_command(request);

    // Then
    assert!(matches!(result, Err(LifecycleError::InvalidEnvironment)));
}

#[test]
fn command_rejects_declared_environment_name_missing_from_request() {
    // Given
    let request = CommandRequest {
        executable: "runner".to_owned(),
        args: Vec::new(),
        env: vec![EnvironmentInput::public("PRESENT", "value")],
        environment_allowlist: vec!["MISSING".to_owned(), "PRESENT".to_owned()],
        stdout_visibility: OutputVisibility::Public,
        stderr_visibility: OutputVisibility::Public,
    };

    // When
    let result = prepare_command(request);

    // Then
    assert!(matches!(result, Err(LifecycleError::InvalidEnvironment)));
}

#[test]
fn command_outcome_tags_have_exact_json_shapes() {
    // Given / When / Then
    assert_eq!(
        serde_json::to_value(CommandOutcome::SpawnFailed {
            reason: LoggedText::Public("no binary".to_owned()),
        })
        .expect("spawn failure serializes"),
        json!({"SpawnFailed":{"reason":{"Public":"no binary"}}})
    );
    assert_eq!(
        serde_json::to_value(CommandOutcome::Signaled { signal: 9 }).expect("signal serializes"),
        json!({"Signaled":{"signal":9}})
    );
    assert_eq!(
        serde_json::to_value(CommandOutcome::TimedOut { timeout_ms: 500 })
            .expect("timeout serializes"),
        json!({"TimedOut":{"timeout_ms":500}})
    );
    assert_eq!(
        DatasetIdentity::EsciElectronics.as_str(),
        "esci_electronics"
    );
}

#[test]
fn staged_events_and_global_sequence_are_deterministic() {
    // Given
    let mut first = FakePort::default();
    let mut second = FakePort::default();

    // When
    let first_terminal = run1(&mut first).expect("first lifecycle succeeds");
    let _second_terminal = run1(&mut second).expect("second lifecycle succeeds");

    // Then
    assert_eq!(first_terminal, Terminal::Completed);
    assert_eq!(first.event_lines(), second.event_lines());
    assert_eq!(
        first.command_lines().collect::<Vec<_>>(),
        second.command_lines().collect::<Vec<_>>()
    );
    assert_eq!(
        first.all_sequences(),
        (1..=first.record_count()).collect::<Vec<_>>()
    );
    assert_eq!(
        first.phase_events().take(4).collect::<Vec<_>>(),
        vec![
            (Phase::StaticValidation, EventType::PhaseStarted),
            (Phase::StaticValidation, EventType::PhaseCompleted),
            (Phase::Initialization, EventType::PhaseStarted),
            (Phase::Initialization, EventType::PhaseCompleted),
        ]
    );
}
