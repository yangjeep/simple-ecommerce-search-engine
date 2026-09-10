use super::{run1, FakePort, Terminal};
use crate::lifecycle::{
    DatasetIdentity, EventType, IndexCell, IndexError, LifecycleError, PathState, Phase,
    Projection, SeriesIdentity, SlotIndex,
};
use crate::{CampaignCycle, CampaignSeries, Dataset, Engine, WorkloadProjection};

#[test]
fn initialization_refuses_existing_missing_parent_and_symlink_paths() {
    // Given / When / Then
    for state in [
        PathState::ExistingCycle,
        PathState::MissingParent,
        PathState::Symlink,
    ] {
        let mut fake = FakePort::default();
        fake.path_state = state;
        assert!(run1(&mut fake).is_err(), "accepted {state:?}");
        assert!(!fake.seal_created);
        assert!(!fake.analyzer_invoked);
    }
}

#[test]
fn equivalence_gate_failure_stops_before_calibration() {
    // Given
    let mut fake = FakePort::default();
    fake.equivalence_passes = false;

    // When
    let terminal = run1(&mut fake).expect("gate failure is a clean terminal state");

    // Then
    assert_eq!(terminal, Terminal::GateFailed(Phase::EquivalenceGate));
    assert!(fake.has_event(Phase::EquivalenceGate, EventType::GateFailed));
    assert!(fake.has_event(Phase::EquivalenceGate, EventType::LifecycleTerminated));
    assert_eq!(fake.slot_runs_in(SeriesIdentity::Calibration), 0);
    assert!(!fake.seal_created);
}

#[test]
fn prescreen_retries_same_block_without_block_started() {
    // Given
    let mut fake = FakePort::default();
    fake.reject_before_accept(
        CampaignSeries::Calibration {
            engine: Engine::Native,
        },
        0,
        1,
    );

    // When
    run1(&mut fake).expect("single rejection retries");

    // Then
    assert_eq!(fake.rejection_count(SeriesIdentity::Calibration), 1);
    assert_eq!(fake.attempts(SeriesIdentity::Calibration, 0), vec![1, 2]);
    assert_eq!(
        fake.block_started_attempts(SeriesIdentity::Calibration, 0),
        vec![2]
    );
}

#[test]
fn seventh_long_series_and_second_cold_rejections_terminate() {
    // Given / When / Then
    for (campaign_series, series, limit) in [
        (
            CampaignSeries::Warm {
                dataset: Dataset::Wands,
                projection: WorkloadProjection::All,
            },
            SeriesIdentity::Warm,
            7,
        ),
        (
            CampaignSeries::Cold {
                dataset: Dataset::Wands,
            },
            SeriesIdentity::Cold,
            2,
        ),
    ] {
        let mut fake = FakePort::default();
        for block in 0..limit {
            fake.reject_before_accept(campaign_series, block, 1);
        }
        let error = run1(&mut fake).expect_err("rejection limit must terminate");
        assert_eq!(
            error,
            LifecycleError::RejectionLimit {
                series: campaign_series
            }
        );
        assert_eq!(fake.rejection_count(series), limit);
        assert!(!fake.seal_created);
    }
}

#[test]
fn slot_zero_failure_prevents_slot_one_and_writes_no_raw() {
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
    assert_eq!(
        fake.slot_runs_for_block(SeriesIdentity::Calibration, 0),
        vec![0]
    );
    assert_eq!(fake.raw_lines(), 0);
}

#[test]
fn slot_one_failure_discards_both_staged_raw_records() {
    // Given
    let mut fake = FakePort::default();
    fake.fail_slot(
        CampaignSeries::Calibration {
            engine: Engine::Native,
        },
        0,
        SlotIndex::one(),
    );

    // When
    assert!(run1(&mut fake).is_err());

    // Then
    assert_eq!(
        fake.slot_runs_for_block(SeriesIdentity::Calibration, 0),
        vec![0, 1]
    );
    assert_eq!(fake.raw_lines(), 0);
}

#[test]
fn index_capture_requires_exact_order_counts_and_snapshots() {
    // Given / When / Then
    for error in [
        IndexError::WrongOrder,
        IndexError::WrongDocumentCount,
        IndexError::WrongSnapshot,
        IndexError::DuplicateCell,
    ] {
        let mut fake = FakePort::default();
        fake.corrupt_index(error);
        assert_eq!(
            run1(&mut fake).expect_err("invalid index must stop"),
            LifecycleError::Index(error)
        );
        assert_eq!(fake.slot_runs, 0);
    }
    let expected = [
        IndexCell::native(CampaignCycle::Run1, DatasetIdentity::Wands, 10),
        IndexCell::native(CampaignCycle::Run1, DatasetIdentity::EsciElectronics, 11),
        IndexCell::solr(CampaignCycle::Run1, DatasetIdentity::Wands, 12),
        IndexCell::solr(CampaignCycle::Run1, DatasetIdentity::EsciElectronics, 13),
    ];
    let mut fake = FakePort::default();
    run1(&mut fake).expect("valid index succeeds");
    assert_eq!(fake.captured_index_cells(), expected);
}

#[test]
fn calibration_gate_failure_stops_before_warm() {
    // Given
    let mut fake = FakePort::default();
    fake.calibration_passes = false;

    // When
    let terminal = run1(&mut fake).expect("gate failure is clean");

    // Then
    assert_eq!(terminal, Terminal::GateFailed(Phase::CalibrationGate));
    assert_eq!(fake.slot_runs_in(SeriesIdentity::Calibration), 120);
    assert_eq!(fake.slot_runs_in(SeriesIdentity::Warm), 0);
    assert!(fake.has_event(Phase::CalibrationGate, EventType::GateFailed));
    assert!(!fake.seal_created);
    assert!(!fake.saw_projection(Projection::All));
}
