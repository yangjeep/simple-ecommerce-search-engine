use issue61_eval::{campaign_schedule, Engine, SessionMode, SessionStep, CAMPAIGN_BLOCKS};

#[test]
fn seed_61_campaign_has_exactly_thirty_deterministic_balanced_engine_pairs() {
    // Given / When
    let first = campaign_schedule();
    let second = campaign_schedule();

    // Then
    assert_eq!(first, second);
    assert_eq!(first.len(), CAMPAIGN_BLOCKS);
    assert_eq!(
        first
            .iter()
            .filter(|pair| pair.first() == Engine::Solr)
            .count(),
        15
    );
    assert!(first.iter().all(|pair| pair.first() != pair.second()));
}

#[test]
fn every_frozen_session_mode_has_its_exact_pass_plan() {
    // Given / When / Then
    assert_eq!(SessionMode::Warm.plan().pass_counts(), (3, 2));
    assert_eq!(SessionMode::Cold.plan().pass_counts(), (0, 1));
    assert_eq!(SessionMode::CalibrationFour.plan().pass_counts(), (3, 4));
    assert_eq!(SessionMode::CalibrationFive.plan().pass_counts(), (3, 5));
}

#[test]
fn cold_plan_opens_and_closes_counters_around_one_measured_pass() {
    // Given
    let plan = SessionMode::Cold.plan();

    // When
    let steps = plan.steps().collect::<Vec<_>>();

    // Then
    assert_eq!(
        steps,
        [
            SessionStep::OpenCounters,
            SessionStep::MeasuredPass,
            SessionStep::CloseCounters,
        ]
    );
}
