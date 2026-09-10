use super::operation::{FileOperation, Operation};
use super::state::FakePort;
use crate::lifecycle::model::{
    EventType, EvidenceFile, IndexCell, Phase, Projection, SeriesIdentity,
};
use crate::CampaignSeries;

impl FakePort {
    pub(crate) fn command_lines(&self) -> impl Iterator<Item = &str> {
        self.commands.iter().map(|(_, line)| line.as_str())
    }

    pub(crate) fn index_lines(&self) -> impl Iterator<Item = &str> {
        self.index_lines.iter().map(String::as_str)
    }

    pub(crate) fn seal_lines(&self) -> impl Iterator<Item = &str> {
        self.seal_lines.iter().map(String::as_str)
    }

    pub(crate) fn computed_hash_paths(&self) -> Vec<&str> {
        self.computed_hash_paths
            .iter()
            .map(String::as_str)
            .collect()
    }

    pub(crate) fn event_lines(&self) -> Vec<&str> {
        self.events
            .iter()
            .map(|(_, _, line)| line.as_str())
            .collect()
    }

    pub(crate) fn all_sequences(&self) -> Vec<u64> {
        let mut values = self
            .events
            .iter()
            .map(|(sequence, _, _)| *sequence)
            .chain(self.commands.iter().map(|(sequence, _)| *sequence))
            .collect::<Vec<_>>();
        values.sort_unstable();
        values
    }

    pub(crate) fn record_count(&self) -> u64 {
        u64::try_from(self.events.len() + self.commands.len()).expect("test record count fits u64")
    }

    pub(crate) fn phase_events(&self) -> impl Iterator<Item = (Phase, EventType)> + '_ {
        self.events
            .iter()
            .map(|(_, event, _)| (event.phase(), event.event_type()))
    }

    pub(crate) fn has_event(&self, phase: Phase, event_type: EventType) -> bool {
        self.phase_events()
            .any(|event| event == (phase, event_type))
    }

    pub(crate) fn last_event_is(&self, event_type: EventType) -> bool {
        self.events
            .last()
            .is_some_and(|(_, event, _)| event.event_type() == event_type)
    }

    pub(crate) fn rejection_count(&self, series: SeriesIdentity) -> usize {
        self.events
            .iter()
            .filter(|(_, event, _)| {
                event.series() == Some(series) && event.event_type() == EventType::PrescreenRejected
            })
            .count()
    }

    pub(crate) fn exact_rejection_count(&self, series: CampaignSeries) -> usize {
        self.rejections_seen
            .iter()
            .filter(|seen| **seen == series)
            .count()
    }

    pub(crate) fn attempts(&self, series: SeriesIdentity, block: usize) -> Vec<u32> {
        self.attempts_seen
            .iter()
            .filter_map(|(seen_series, seen_block, attempt)| {
                (*seen_series == series && *seen_block == block).then_some(*attempt)
            })
            .collect()
    }

    pub(crate) fn block_started_attempts(&self, series: SeriesIdentity, block: usize) -> Vec<u32> {
        self.events
            .iter()
            .filter_map(|(_, event, _)| {
                (event.series() == Some(series)
                    && event.block_index() == Some(block)
                    && event.event_type() == EventType::BlockStarted)
                    .then(|| event.attempt())
                    .flatten()
            })
            .take(1)
            .collect()
    }

    pub(crate) fn slot_runs_in(&self, series: SeriesIdentity) -> usize {
        self.slot_runs_seen
            .iter()
            .filter(|slot| slot.block.series == series)
            .count()
    }

    pub(crate) fn slot_runs_for_block(&self, series: SeriesIdentity, block: usize) -> Vec<u8> {
        self.slot_runs_seen
            .iter()
            .filter(|slot| slot.block.series == series && slot.block.block_index == block)
            .map(|slot| slot.slot.get())
            .collect()
    }

    pub(crate) const fn raw_lines(&self) -> usize {
        self.raw_records
    }

    pub(crate) const fn opened_evidence_files(&self) -> usize {
        self.opened_files
    }

    pub(crate) fn captured_index_cells(&self) -> Vec<IndexCell> {
        self.index_cells.clone()
    }

    pub(crate) fn saw_projection(&self, projection: Projection) -> bool {
        self.events
            .iter()
            .any(|(_, event, _)| event.projection() == Some(projection))
    }

    pub(crate) fn operations(&self) -> &[Operation] {
        &self.operations
    }

    pub(crate) fn evidence_operations(&self) -> Vec<Operation> {
        self.operations
            .iter()
            .copied()
            .filter(|operation| {
                matches!(
                    operation,
                    Operation::Evidence {
                        operation: FileOperation::CreateNew | FileOperation::WriteAll,
                        ..
                    }
                )
            })
            .collect()
    }

    pub(crate) fn finalization_operations(&self) -> Vec<Operation> {
        self.operations
            .iter()
            .copied()
            .filter(|operation| match operation {
                Operation::Close(_)
                | Operation::Flush(_)
                | Operation::Sync(_)
                | Operation::ComputeHash
                | Operation::CreateSeal
                | Operation::WriteSeal { .. }
                | Operation::FlushSeal
                | Operation::SyncSeal
                | Operation::CloseSeal
                | Operation::VerifySeal
                | Operation::InvokeAnalyzer => true,
                Operation::AppendEvent {
                    phase: Phase::EvidenceFinalization,
                    event_type: EventType::EvidenceFinalized,
                } => true,
                Operation::ValidateStatic
                | Operation::InitializeCreateNew
                | Operation::Evidence { .. }
                | Operation::AppendEvent { .. }
                | Operation::AuditEquivalence
                | Operation::CaptureIndex
                | Operation::EvaluateEquivalenceGate
                | Operation::AnalyzeCalibration { .. }
                | Operation::ExecuteSlot
                | Operation::TeardownEnvironment => false,
            })
            .collect()
    }

    pub(crate) fn was_closed(&self, file: EvidenceFile) -> bool {
        self.closed_files.contains(&file)
    }

    pub(crate) fn close_was_attempted(&self, file: EvidenceFile) -> bool {
        self.operations.contains(&Operation::Close(file))
    }

    pub(crate) fn has_durable_close(&self, file: EvidenceFile) -> bool {
        let expected = [
            Operation::Flush(file),
            Operation::Sync(file),
            Operation::Close(file),
        ];
        self.operations
            .windows(expected.len())
            .any(|operations| operations == expected)
    }

    pub(crate) fn has_durable_seal_close(&self) -> bool {
        self.operations.windows(4).any(|operations| {
            operations
                == [
                    Operation::WriteSeal { entries: 12 },
                    Operation::FlushSeal,
                    Operation::SyncSeal,
                    Operation::CloseSeal,
                ]
        })
    }

    pub(crate) fn initialized_cycle_path(&self) -> &str {
        self.cycle_path
            .as_deref()
            .and_then(std::path::Path::to_str)
            .expect("fake cycle path is UTF-8")
    }

    pub(crate) const fn cycle_path_components_safe(&self) -> bool {
        self.cycle_path_safe
    }

    pub(crate) const fn regular_file_count(&self) -> usize {
        self.regular_files
    }
}
