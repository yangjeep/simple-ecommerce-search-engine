use super::model::{
    BlockContext, CommandRequest, CyclePath, Event, EvidenceFile, IndexArtifact, LifecycleError,
    PreparedCommand, RawRecord, ResolvedSealManifest, Sequence, SlotContext,
};
use crate::CampaignCycle;
use std::path::Path;

pub(super) struct InitializationFailure {
    pub(super) error: LifecycleError,
    pub(super) events_opened: bool,
}

pub(super) trait LifecyclePort {
    fn validate_static(&mut self) -> Result<(), LifecycleError>;
    fn repository_root(&self) -> &Path;
    fn initialize(&mut self, path: &CyclePath) -> Result<(), InitializationFailure>;
    fn evidence_is_open(&self, file: EvidenceFile) -> bool;
    fn append_event(
        &mut self,
        cycle: CampaignCycle,
        sequence: Sequence,
        event: Event,
    ) -> Result<(), LifecycleError>;
    fn append_command(&mut self, sequence: Sequence, line: String) -> Result<(), LifecycleError>;
    fn audit_equivalence(&mut self) -> Result<(), LifecycleError>;
    fn capture_index(&mut self, cycle: CampaignCycle)
        -> Result<Vec<IndexArtifact>, LifecycleError>;
    fn evaluate_equivalence_gate(&mut self) -> bool;
    fn prescreen(&mut self, block: BlockContext) -> Result<bool, LifecycleError>;
    fn command_request(&self, slot: SlotContext) -> CommandRequest;
    fn execute_slot(&mut self, slot: SlotContext, command: &PreparedCommand) -> SlotExecution;
    fn teardown(&mut self, slot: SlotContext) -> Result<(), LifecycleError>;
    fn teardown_environment(&mut self) -> Result<(), LifecycleError>;
    fn append_raw(&mut self, records: [RawRecord; 2]) -> Result<(), LifecycleError>;
    fn calibration_analyzed(&mut self, records: usize);
    fn flush(&mut self, file: EvidenceFile) -> Result<(), LifecycleError>;
    fn sync(&mut self, file: EvidenceFile) -> Result<(), LifecycleError>;
    fn close(&mut self, file: EvidenceFile) -> Result<(), LifecycleError>;
    fn compute_hash(&mut self, path: &str) -> Result<String, LifecycleError>;
    fn create_seal(&mut self) -> Result<(), LifecycleError>;
    fn write_seal(&mut self, manifest: &ResolvedSealManifest) -> Result<(), LifecycleError>;
    fn flush_seal(&mut self) -> Result<(), LifecycleError>;
    fn sync_seal(&mut self) -> Result<(), LifecycleError>;
    fn close_seal(&mut self) -> Result<(), LifecycleError>;
    fn verify_seal(&mut self) -> Result<bool, LifecycleError>;
    fn regular_file_count(&self) -> usize;
    fn invoke_analyzer(&mut self) -> Result<(), LifecycleError>;
}

pub(super) enum SlotExecution {
    Completed {
        evidence: super::model::ExecutionEvidence,
        record: Box<RawRecord>,
    },
    Failed {
        evidence: super::model::ExecutionEvidence,
        error: LifecycleError,
    },
}
