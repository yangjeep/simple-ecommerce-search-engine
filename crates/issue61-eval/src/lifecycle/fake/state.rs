use super::super::model::{
    Event, EvidenceFile, IndexCell, IndexError, SealStage, SeriesIdentity, SlotContext, SlotIndex,
};
use super::operation::Operation;
use crate::CampaignSeries;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PathState {
    Ready,
    ExistingCycle,
    MissingParent,
    Symlink,
}

pub(crate) struct FakePort {
    pub(crate) path_state: PathState,
    pub(crate) static_validation_passes: bool,
    pub(crate) equivalence_passes: bool,
    pub(crate) fail_equivalence_audit: bool,
    pub(crate) fail_index_capture: bool,
    pub(crate) calibration_passes: bool,
    pub(crate) unclassified_command: bool,
    pub(crate) duplicate_environment: bool,
    pub(crate) contradictory_slot_success: bool,
    pub(crate) duplicate_calibration_identity: bool,
    pub(crate) slot_runs: usize,
    pub(crate) seal_created: bool,
    pub(crate) seal_verified: bool,
    pub(crate) seal_valid: bool,
    pub(crate) analyzer_invoked: bool,
    pub(crate) wrote_after_seal: bool,
    pub(crate) fail_close: Option<EvidenceFile>,
    pub(crate) fail_flush: Option<EvidenceFile>,
    pub(crate) fail_sync: Option<EvidenceFile>,
    pub(crate) fail_seal_stages: Vec<SealStage>,
    pub(crate) fail_open: Option<EvidenceFile>,
    pub(crate) fail_command_append: bool,
    pub(crate) fail_event_append:
        Option<(super::super::model::Phase, super::super::model::EventType)>,
    pub(super) opened_files: usize,
    pub(super) open_files: Vec<EvidenceFile>,
    pub(super) regular_files: usize,
    pub(super) raw_records: usize,
    pub(super) events: Vec<(u64, Event, String)>,
    pub(super) commands: Vec<(u64, String)>,
    pub(super) index_lines: Vec<String>,
    pub(super) seal_lines: Vec<String>,
    pub(super) seal_open: bool,
    pub(super) computed_hash_paths: Vec<String>,
    pub(super) operations: Vec<Operation>,
    pub(super) attempts_seen: Vec<(SeriesIdentity, usize, u32)>,
    pub(super) slot_runs_seen: Vec<SlotContext>,
    pub(super) rejection_rules: Vec<(CampaignSeries, usize, usize)>,
    pub(super) rejections_seen: Vec<CampaignSeries>,
    pub(super) slot_failures: Vec<(CampaignSeries, usize, SlotIndex)>,
    pub(super) closed_files: Vec<EvidenceFile>,
    pub(super) cycle_path: Option<PathBuf>,
    pub(super) cycle_path_safe: bool,
    pub(super) index_error: Option<IndexError>,
    pub(super) index_cells: Vec<IndexCell>,
}

impl Default for FakePort {
    fn default() -> Self {
        Self {
            path_state: PathState::Ready,
            static_validation_passes: true,
            equivalence_passes: true,
            fail_equivalence_audit: false,
            fail_index_capture: false,
            calibration_passes: true,
            unclassified_command: false,
            duplicate_environment: false,
            contradictory_slot_success: false,
            duplicate_calibration_identity: false,
            slot_runs: 0,
            seal_created: false,
            seal_verified: false,
            seal_valid: true,
            analyzer_invoked: false,
            wrote_after_seal: false,
            fail_close: None,
            fail_flush: None,
            fail_sync: None,
            fail_seal_stages: Vec::new(),
            fail_open: None,
            fail_command_append: false,
            fail_event_append: None,
            opened_files: 0,
            open_files: Vec::new(),
            regular_files: 0,
            raw_records: 0,
            events: Vec::new(),
            commands: Vec::new(),
            index_lines: Vec::new(),
            seal_lines: Vec::new(),
            seal_open: false,
            computed_hash_paths: Vec::new(),
            operations: Vec::new(),
            attempts_seen: Vec::new(),
            slot_runs_seen: Vec::new(),
            rejection_rules: Vec::new(),
            rejections_seen: Vec::new(),
            slot_failures: Vec::new(),
            closed_files: Vec::new(),
            cycle_path: None,
            cycle_path_safe: false,
            index_error: None,
            index_cells: Vec::new(),
        }
    }
}

impl FakePort {
    pub(crate) fn reject_before_accept(
        &mut self,
        series: CampaignSeries,
        block: usize,
        count: usize,
    ) {
        self.rejection_rules.push((series, block, count));
    }

    pub(crate) fn fail_slot(&mut self, series: CampaignSeries, block: usize, slot: SlotIndex) {
        self.slot_failures.push((series, block, slot));
    }

    pub(crate) fn corrupt_index(&mut self, error: IndexError) {
        self.index_error = Some(error);
    }
}
