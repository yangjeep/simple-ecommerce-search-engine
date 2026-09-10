use super::operation::Operation;
use super::state::{FakePort, PathState};
use crate::lifecycle::model::{
    BlockContext, CommandRequest, CyclePath, DatasetIdentity, Event, EventRecord, EvidenceFile,
    IndexArtifact, IndexCell, LifecycleError, PreparedCommand, RawRecord, ResolvedSealManifest,
    Sequence, SlotContext,
};
use crate::lifecycle::port::{InitializationFailure, LifecyclePort, SlotExecution};
use crate::CampaignCycle;
use std::path::Path;

impl LifecyclePort for FakePort {
    fn validate_static(&mut self) -> Result<(), LifecycleError> {
        self.operations.push(Operation::ValidateStatic);
        self.static_validation_passes
            .then_some(())
            .ok_or(LifecycleError::StaticValidation)
    }

    fn repository_root(&self) -> &Path {
        Path::new("/repo")
    }

    fn initialize(&mut self, path: &CyclePath) -> Result<(), InitializationFailure> {
        self.operations.push(Operation::InitializeCreateNew);
        match self.path_state {
            PathState::Ready => {}
            PathState::ExistingCycle | PathState::MissingParent | PathState::Symlink => {
                return Err(self.initialization_failure())
            }
        }
        self.cycle_path = Some(path.as_path().to_path_buf());
        self.cycle_path_safe = path.as_path().components().all(|component| {
            matches!(
                component,
                std::path::Component::RootDir | std::path::Component::Normal(_)
            )
        });
        for file in EvidenceFile::ALL {
            if self.fail_open == Some(file) {
                return Err(self.initialization_failure());
            }
            self.operations.push(Operation::create_new(file));
            self.open_files.push(file);
            self.opened_files += 1;
            self.regular_files += 1;
        }
        Ok(())
    }

    fn evidence_is_open(&self, file: EvidenceFile) -> bool {
        self.open_files.contains(&file)
    }

    fn append_event(
        &mut self,
        cycle: CampaignCycle,
        sequence: Sequence,
        event: Event,
    ) -> Result<(), LifecycleError> {
        self.note_write(EvidenceFile::Events);
        self.operations.push(Operation::AppendEvent {
            phase: event.phase(),
            event_type: event.event_type(),
        });
        if self.fail_event_append == Some((event.phase(), event.event_type())) {
            return Err(LifecycleError::EventEvidence);
        }
        let mut line = serde_json::to_string(&EventRecord::new(cycle, sequence, event.clone()))
            .map_err(|_| LifecycleError::Serialization)?;
        line.push('\n');
        self.events.push((sequence.get(), event, line));
        Ok(())
    }

    fn append_command(&mut self, sequence: Sequence, line: String) -> Result<(), LifecycleError> {
        self.note_write(EvidenceFile::Commands);
        if self.fail_command_append {
            return Err(LifecycleError::CommandEvidence);
        }
        self.commands.push((sequence.get(), line));
        Ok(())
    }

    fn audit_equivalence(&mut self) -> Result<(), LifecycleError> {
        self.operations.push(Operation::AuditEquivalence);
        if self.fail_equivalence_audit {
            return Err(LifecycleError::EquivalenceAudit);
        }
        self.note_write(EvidenceFile::CandidateAuditEsci);
        self.note_write(EvidenceFile::CandidateAuditWands);
        Ok(())
    }

    fn capture_index(
        &mut self,
        cycle: CampaignCycle,
    ) -> Result<Vec<IndexArtifact>, LifecycleError> {
        self.operations.push(Operation::CaptureIndex);
        if self.fail_index_capture {
            return Err(LifecycleError::IndexCapture);
        }
        self.note_write(EvidenceFile::IndexArtifacts);
        let mut cells = vec![
            IndexCell::native(cycle, DatasetIdentity::Wands, 10),
            IndexCell::native(cycle, DatasetIdentity::EsciElectronics, 11),
            IndexCell::solr(cycle, DatasetIdentity::Wands, 12),
            IndexCell::solr(cycle, DatasetIdentity::EsciElectronics, 13),
        ];
        self.corrupt_cells(&mut cells);
        self.index_lines = cells
            .iter()
            .map(|cell| {
                serde_json::to_string(cell)
                    .map(|mut line| {
                        line.push('\n');
                        line
                    })
                    .map_err(|_| LifecycleError::Serialization)
            })
            .collect::<Result<Vec<_>, _>>()?;
        self.index_cells = cells.clone();
        Ok(cells.into_iter().map(IndexArtifact).collect())
    }

    fn evaluate_equivalence_gate(&mut self) -> bool {
        self.operations.push(Operation::EvaluateEquivalenceGate);
        self.equivalence_passes
    }

    fn prescreen(&mut self, block: BlockContext) -> Result<bool, LifecycleError> {
        let rule = self
            .rejection_rules
            .iter()
            .position(|rule| rule.0 == block.campaign_series && rule.1 == block.block_index);
        if rule.is_some() {
            self.attempts_seen
                .push((block.series, block.block_index, block.attempt.get()));
        }
        match rule {
            Some(index) if self.rejection_rules[index].2 > 0 => {
                self.rejection_rules[index].2 -= 1;
                self.rejections_seen.push(block.campaign_series);
                Ok(false)
            }
            Some(index) => {
                self.rejection_rules.remove(index);
                Ok(true)
            }
            None => Ok(true),
        }
    }

    fn command_request(&self, slot: SlotContext) -> CommandRequest {
        self.command_request_impl(slot)
    }

    fn execute_slot(&mut self, slot: SlotContext, command: &PreparedCommand) -> SlotExecution {
        self.execute_slot_impl(slot, command)
    }

    fn teardown(&mut self, slot: SlotContext) -> Result<(), LifecycleError> {
        self.teardown_impl(slot)
    }

    fn teardown_environment(&mut self) -> Result<(), LifecycleError> {
        self.teardown_environment_impl()
    }

    fn append_raw(&mut self, records: [RawRecord; 2]) -> Result<(), LifecycleError> {
        self.append_raw_impl(records)
    }

    fn calibration_analyzed(&mut self, records: usize) {
        self.calibration_analyzed_impl(records);
    }

    fn flush(&mut self, file: EvidenceFile) -> Result<(), LifecycleError> {
        if !self.open_files.contains(&file) {
            return Ok(());
        }
        self.operations.push(Operation::Flush(file));
        if self.fail_flush == Some(file) {
            Err(LifecycleError::Flush(file))
        } else {
            Ok(())
        }
    }

    fn sync(&mut self, file: EvidenceFile) -> Result<(), LifecycleError> {
        if !self.open_files.contains(&file) {
            return Ok(());
        }
        self.operations.push(Operation::Sync(file));
        if self.fail_sync == Some(file) {
            Err(LifecycleError::Sync(file))
        } else {
            Ok(())
        }
    }

    fn close(&mut self, file: EvidenceFile) -> Result<(), LifecycleError> {
        if !self.open_files.contains(&file) {
            return Ok(());
        }
        self.operations.push(Operation::Close(file));
        self.open_files.retain(|open| *open != file);
        self.closed_files.push(file);
        if self.fail_close == Some(file) {
            Err(LifecycleError::Close(file))
        } else {
            Ok(())
        }
    }

    fn compute_hash(&mut self, path: &str) -> Result<String, LifecycleError> {
        self.operations.push(Operation::ComputeHash);
        self.computed_hash_paths.push(path.to_owned());
        Ok(format!("{:064x}", self.computed_hash_paths.len()))
    }

    fn create_seal(&mut self) -> Result<(), LifecycleError> {
        self.create_seal_impl()
    }

    fn write_seal(&mut self, manifest: &ResolvedSealManifest) -> Result<(), LifecycleError> {
        self.write_seal_impl(manifest)
    }

    fn flush_seal(&mut self) -> Result<(), LifecycleError> {
        self.flush_seal_impl()
    }

    fn sync_seal(&mut self) -> Result<(), LifecycleError> {
        self.sync_seal_impl()
    }

    fn close_seal(&mut self) -> Result<(), LifecycleError> {
        self.close_seal_impl()
    }

    fn verify_seal(&mut self) -> Result<bool, LifecycleError> {
        self.operations.push(Operation::VerifySeal);
        self.seal_verified = self.seal_valid;
        Ok(self.seal_valid)
    }

    fn regular_file_count(&self) -> usize {
        self.regular_files
    }

    fn invoke_analyzer(&mut self) -> Result<(), LifecycleError> {
        self.operations.push(Operation::InvokeAnalyzer);
        self.analyzer_invoked = true;
        Ok(())
    }
}
