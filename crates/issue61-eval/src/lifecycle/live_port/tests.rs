use super::{
    catalog_path, evidence_file_name, exit_outcome, extract_kv, need_for, parse_native_ready_line,
    parse_provision_ok_line, scratch_path, workload_path, EngineNeed,
};
use crate::lifecycle::model::{
    Attempt, BlockContext, DatasetIdentity, EngineIdentity, EvidenceFile, Phase, Projection,
    SeriesIdentity, SlotContext, SlotIndex,
};
use crate::{campaign_plan, CampaignCycle, CampaignSeries, Dataset, Engine};
use std::path::Path;

#[test]
fn native_ready_line_parses_docs_and_index_bytes_from_stdout_or_stderr() {
    // Given
    let stdout = "some preamble\nNATIVE_READY docs=42994 index_bytes=1234567\ntrailer\n";

    // When
    let parsed = parse_native_ready_line(stdout);

    // Then
    assert_eq!(parsed, Some((42_994, 1_234_567)));
}

#[test]
fn native_ready_line_is_absent_without_missing_fields() {
    // Given / When / Then
    assert_eq!(parse_native_ready_line("nothing useful here"), None);
    assert_eq!(parse_native_ready_line("NATIVE_READY docs=1"), None);
}

#[test]
fn provision_ok_line_parses_core_docs_and_index_bytes() {
    // Given
    let stdout = "==> index_bytes=987\nPROVISION_OK core=i61_wands docs=42994 index_bytes=987\n";

    // When
    let parsed = parse_provision_ok_line(stdout);

    // Then
    assert_eq!(parsed, Some(("i61_wands".to_owned(), 42_994, 987)));
}

#[test]
fn provision_ok_line_is_absent_when_marker_missing() {
    assert_eq!(
        parse_provision_ok_line("PROVISION_FAILED core=i61_wands"),
        None
    );
}

#[test]
fn extract_kv_reads_the_named_field_only() {
    assert_eq!(extract_kv("docs=5 index_bytes=6", "docs="), Some(5));
    assert_eq!(extract_kv("docs=5 index_bytes=6", "index_bytes="), Some(6));
    assert_eq!(extract_kv("docs=5", "missing="), None);
}

#[test]
fn need_for_calibration_selects_the_named_engine_on_wands_only() {
    assert_eq!(
        need_for(CampaignSeries::Calibration {
            engine: Engine::Native
        }),
        (Dataset::Wands, EngineNeed::NativeOnly)
    );
    assert_eq!(
        need_for(CampaignSeries::Calibration {
            engine: Engine::Solr
        }),
        (Dataset::Wands, EngineNeed::SolrOnly)
    );
}

#[test]
fn need_for_warm_and_cold_require_both_engines_on_their_dataset() {
    assert_eq!(
        need_for(CampaignSeries::Warm {
            dataset: Dataset::EsciElectronics,
            projection: crate::WorkloadProjection::Hybrid,
        }),
        (Dataset::EsciElectronics, EngineNeed::Both)
    );
    assert_eq!(
        need_for(CampaignSeries::Cold {
            dataset: Dataset::Wands
        }),
        (Dataset::Wands, EngineNeed::Both)
    );
}

#[test]
fn catalog_and_workload_paths_are_dataset_specific() {
    let root = Path::new("/repo");
    assert_ne!(
        catalog_path(root, Dataset::Wands),
        catalog_path(root, Dataset::EsciElectronics)
    );
    assert_ne!(
        workload_path(root, Dataset::Wands),
        workload_path(root, Dataset::EsciElectronics)
    );
    assert!(catalog_path(root, Dataset::Wands)
        .to_string_lossy()
        .contains("wands"));
}

#[test]
fn evidence_file_names_are_pairwise_distinct() {
    let names: Vec<&str> = EvidenceFile::ALL
        .iter()
        .copied()
        .map(evidence_file_name)
        .collect();
    let mut sorted = names.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(
        sorted.len(),
        names.len(),
        "evidence file names must be unique: {names:?}"
    );
}

fn block_context(
    series: SeriesIdentity,
    campaign_series: CampaignSeries,
    dataset: DatasetIdentity,
    projection: Option<Projection>,
    block_index: usize,
) -> BlockContext {
    BlockContext {
        phase: Phase::Warm,
        series,
        campaign_series,
        block_index,
        attempt: Attempt::new(1),
        dataset,
        projection,
    }
}

fn slot_context(
    block: BlockContext,
    engine: EngineIdentity,
    slot: SlotIndex,
    session: crate::SessionSpec,
) -> SlotContext {
    SlotContext {
        block,
        slot,
        engine,
        session,
    }
}

/// Pulls a real block matching `predicate` out of the frozen `run1` plan, so
/// tests exercise real `SessionSpec` values rather than hand-built fakes
/// (`SessionSpec` has no public constructor outside `campaign_plan`).
fn find_block(predicate: impl Fn(CampaignSeries) -> bool) -> crate::BlockSpec {
    campaign_plan(CampaignCycle::Run1)
        .blocks()
        .iter()
        .find(|block| predicate(block.series()))
        .expect("a matching block exists in the frozen plan")
        .clone()
}

#[test]
fn scratch_paths_are_unique_across_series_dataset_projection_block_engine_and_slot() {
    let root = Path::new("/repo");
    let cycle = CampaignCycle::Run1;
    let warm = find_block(|series| {
        matches!(
            series,
            CampaignSeries::Warm {
                dataset: Dataset::Wands,
                projection: crate::WorkloadProjection::All
            }
        )
    });
    let calibration_native = find_block(|series| {
        matches!(
            series,
            CampaignSeries::Calibration {
                engine: Engine::Native
            }
        )
    });
    let calibration_solr = find_block(|series| {
        matches!(
            series,
            CampaignSeries::Calibration {
                engine: Engine::Solr
            }
        )
    });

    let warm_block = block_context(
        SeriesIdentity::Warm,
        warm.series(),
        DatasetIdentity::Wands,
        Some(Projection::All),
        warm.index().get(),
    );
    let calibration_native_block = block_context(
        SeriesIdentity::Calibration,
        calibration_native.series(),
        DatasetIdentity::Wands,
        None,
        calibration_native.index().get(),
    );
    let calibration_solr_block = block_context(
        SeriesIdentity::Calibration,
        calibration_solr.series(),
        DatasetIdentity::Wands,
        None,
        calibration_solr.index().get(),
    );

    let a = slot_context(
        warm_block,
        EngineIdentity::Native,
        SlotIndex::zero(),
        warm.sessions()[0],
    );
    let b = slot_context(
        warm_block,
        EngineIdentity::Solr,
        SlotIndex::one(),
        warm.sessions()[1],
    );
    let c = slot_context(
        calibration_native_block,
        EngineIdentity::Native,
        SlotIndex::zero(),
        calibration_native.sessions()[0],
    );
    let d = slot_context(
        calibration_solr_block,
        EngineIdentity::Solr,
        SlotIndex::zero(),
        calibration_solr.sessions()[0],
    );

    let paths = [
        scratch_path(root, cycle, a),
        scratch_path(root, cycle, b),
        scratch_path(root, cycle, c),
        scratch_path(root, cycle, d),
    ];
    for (i, left) in paths.iter().enumerate() {
        for (j, right) in paths.iter().enumerate() {
            if i != j {
                assert_ne!(left, right, "scratch paths {i} and {j} collided: {left:?}");
            }
        }
    }
}

#[test]
fn exit_outcome_maps_zero_exit_to_success() {
    use std::process::Command;
    let status = Command::new("true").status().expect("true(1) runs");
    let outcome = exit_outcome(status);
    assert!(outcome.succeeded());
}

#[test]
fn exit_outcome_maps_nonzero_exit_to_failure() {
    use std::process::Command;
    let status = Command::new("false").status().expect("false(1) runs");
    let outcome = exit_outcome(status);
    assert!(!outcome.succeeded());
}
