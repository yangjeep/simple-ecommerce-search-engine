use issue61_eval::{
    calibration_schedule, campaign_plan, campaign_schedule, CalibrationOrder, CampaignCycle,
    CampaignPhase, CampaignSeries, Dataset, Engine, EngineOrder, PairOrder, SessionMode,
    WorkloadProjection, CALIBRATION_BLOCKS_PER_ENGINE, COLD_BLOCKS_PER_DATASET, EXACT_INDEX_CELLS,
    STABILITY_CELLS, WARM_BLOCKS_PER_SERIES,
};
use std::str::FromStr;

#[test]
fn campaign_has_the_frozen_phase_and_session_cardinalities() {
    // Given / When
    let plan = campaign_plan(CampaignCycle::Run1);

    // Then
    assert_eq!(plan.blocks().len(), 310);
    assert_eq!(plan.sessions().count(), 620);
    for (phase, blocks, sessions) in [
        (CampaignPhase::Warm, 240, 480),
        (CampaignPhase::Calibration, 60, 120),
        (CampaignPhase::Cold, 10, 20),
    ] {
        assert_eq!(
            plan.blocks()
                .iter()
                .filter(|block| block.series().phase() == phase)
                .count(),
            blocks
        );
        assert_eq!(
            plan.sessions()
                .filter(|session| session.series().phase() == phase)
                .count(),
            sessions
        );
    }
    assert_eq!(STABILITY_CELLS, 48);
    assert_eq!(EXACT_INDEX_CELLS, 4);

    let calibration_end = 2 * CALIBRATION_BLOCKS_PER_ENGINE;
    let warm_end = calibration_end + 8 * WARM_BLOCKS_PER_SERIES;
    assert!(plan.blocks()[..calibration_end]
        .iter()
        .all(|block| block.series().phase() == CampaignPhase::Calibration));
    assert!(plan.blocks()[calibration_end..warm_end]
        .iter()
        .all(|block| block.series().phase() == CampaignPhase::Warm));
    assert!(plan.blocks()[warm_end..]
        .iter()
        .all(|block| block.series().phase() == CampaignPhase::Cold));
}

#[test]
fn warm_series_cover_every_dataset_projection_and_existing_engine_schedule() {
    // Given
    let plan = campaign_plan(CampaignCycle::Run1);
    let schedule = campaign_schedule();

    // When / Then
    for dataset in [Dataset::Wands, Dataset::EsciElectronics] {
        for projection in [
            WorkloadProjection::All,
            WorkloadProjection::FastPath,
            WorkloadProjection::Hybrid,
            WorkloadProjection::Punt,
        ] {
            let series = CampaignSeries::Warm {
                dataset,
                projection,
            };
            let blocks = plan
                .blocks()
                .iter()
                .filter(|block| block.series() == series)
                .collect::<Vec<_>>();
            assert_eq!(blocks.len(), WARM_BLOCKS_PER_SERIES);
            for (index, block) in blocks.into_iter().enumerate() {
                assert_eq!(block.index().get(), index);
                assert_eq!(block.order(), PairOrder::Engine(schedule[index]));
                assert_eq!(block.sessions()[0].engine(), schedule[index].first());
                assert_eq!(block.sessions()[1].engine(), schedule[index].second());
            }
        }
    }
}

#[test]
fn calibration_is_wands_only_per_engine_and_balanced_by_pass_order() {
    // Given
    let plan = campaign_plan(CampaignCycle::Run1);
    let first_schedule = calibration_schedule();

    // When
    let repeated_schedule = calibration_schedule();

    // Then
    assert_eq!(first_schedule, repeated_schedule);
    assert_eq!(
        first_schedule
            .iter()
            .filter(|order| **order == CalibrationOrder::FourFirst)
            .count(),
        15
    );
    assert_eq!(
        first_schedule
            .iter()
            .filter(|order| **order == CalibrationOrder::FiveFirst)
            .count(),
        15
    );
    for engine in [Engine::Native, Engine::Solr] {
        let series = CampaignSeries::Calibration { engine };
        let blocks = plan
            .blocks()
            .iter()
            .filter(|block| block.series() == series)
            .collect::<Vec<_>>();
        assert_eq!(blocks.len(), CALIBRATION_BLOCKS_PER_ENGINE);
        for (index, block) in blocks.iter().enumerate() {
            assert_eq!(block.index().get(), index);
            assert_eq!(block.order(), PairOrder::Calibration(first_schedule[index]));
        }
        assert!(blocks.iter().all(|block| {
            block.sessions()[0].engine() == engine
                && block.sessions()[1].engine() == engine
                && block.series().dataset() == Dataset::Wands
        }));
    }
}

#[test]
fn every_block_has_two_adjacent_sessions_with_phase_derived_plans() {
    // Given / When
    let plan = campaign_plan(CampaignCycle::Run1);

    // Then
    for block in plan.blocks() {
        let [first, second] = block.sessions();
        assert_eq!(first.series(), block.series());
        assert_eq!(second.series(), block.series());
        assert_eq!(first.block_index(), block.index());
        assert_eq!(second.block_index(), block.index());
        assert_eq!(first.slot(), EngineOrder::First);
        assert_eq!(second.slot(), EngineOrder::Second);
        match block.series().phase() {
            CampaignPhase::Warm => {
                assert_eq!(first.plan(), SessionMode::Warm.plan());
                assert_eq!(second.plan(), SessionMode::Warm.plan());
            }
            CampaignPhase::Calibration => {
                assert!(first.plan().mode().is_calibration());
                assert!(second.plan().mode().is_calibration());
                assert_ne!(first.plan(), second.plan());
                match block.order() {
                    PairOrder::Calibration(CalibrationOrder::FourFirst) => {
                        assert_eq!(first.plan(), SessionMode::CalibrationFour.plan());
                        assert_eq!(second.plan(), SessionMode::CalibrationFive.plan());
                    }
                    PairOrder::Calibration(CalibrationOrder::FiveFirst) => {
                        assert_eq!(first.plan(), SessionMode::CalibrationFive.plan());
                        assert_eq!(second.plan(), SessionMode::CalibrationFour.plan());
                    }
                    PairOrder::Engine(_) => panic!("calibration block has engine-pair ordering"),
                }
            }
            CampaignPhase::Cold => {
                assert_eq!(first.plan(), SessionMode::Cold.plan());
                assert_eq!(second.plan(), SessionMode::Cold.plan());
            }
        }
    }
}

#[test]
fn cold_series_are_aggregate_five_block_engine_pairs() {
    // Given
    let plan = campaign_plan(CampaignCycle::Run1);
    let schedule = campaign_schedule();

    // When / Then
    for dataset in [Dataset::Wands, Dataset::EsciElectronics] {
        let series = CampaignSeries::Cold { dataset };
        let blocks = plan
            .blocks()
            .iter()
            .filter(|block| block.series() == series)
            .collect::<Vec<_>>();
        assert_eq!(blocks.len(), COLD_BLOCKS_PER_DATASET);
        for (index, block) in blocks.into_iter().enumerate() {
            assert_eq!(block.index().get(), index);
            assert_eq!(block.order(), PairOrder::Engine(schedule[index]));
            assert_eq!(block.series().projection(), WorkloadProjection::All);
        }
    }
}

#[test]
fn campaign_cycles_accept_only_the_three_frozen_names() {
    // Given / When / Then
    for (value, cycle) in [
        ("run1", CampaignCycle::Run1),
        ("rerun1", CampaignCycle::Rerun1),
        ("rerun2", CampaignCycle::Rerun2),
    ] {
        assert_eq!(CampaignCycle::from_str(value), Ok(cycle));
        assert_eq!(cycle.as_str(), value);
    }
    assert!(CampaignCycle::from_str("rerun3").is_err());
}
