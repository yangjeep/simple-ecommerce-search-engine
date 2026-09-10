use super::operation::Operation;
use super::raw_fixture::record_for;
use super::state::FakePort;
use crate::lifecycle::model::{
    CommandInput, CommandOutcome, CommandRequest, EnvironmentInput, EvidenceFile,
    ExecutionEvidence, LifecycleError, OutputVisibility, PreparedCommand, RawRecord, SlotContext,
};
use crate::lifecycle::port::SlotExecution;

impl FakePort {
    pub(super) fn command_request_impl(&self, slot: SlotContext) -> CommandRequest {
        let args = if self.unclassified_command {
            vec![CommandInput::Unclassified("blocked".to_owned())]
        } else {
            vec![CommandInput::Public(format!(
                "{:?}:{}:{}",
                slot.block.series,
                slot.block.block_index,
                slot.slot.get()
            ))]
        };
        let env = if self.duplicate_environment {
            vec![
                EnvironmentInput::public("DUPLICATE", "first"),
                EnvironmentInput::public("DUPLICATE", "second"),
            ]
        } else {
            Vec::new()
        };
        CommandRequest {
            executable: "fake-runner".to_owned(),
            args,
            env,
            environment_allowlist: if self.duplicate_environment {
                vec!["DUPLICATE".to_owned()]
            } else {
                Vec::new()
            },
            stdout_visibility: OutputVisibility::Public,
            stderr_visibility: OutputVisibility::Public,
        }
    }

    pub(super) fn execute_slot_impl(
        &mut self,
        slot: SlotContext,
        _command: &PreparedCommand,
    ) -> SlotExecution {
        let failed = self.slot_failures.iter().any(|failure| {
            failure.0 == slot.block.campaign_series
                && failure.1 == slot.block.block_index
                && failure.2 == slot.slot
        });
        self.operations.push(Operation::ExecuteSlot);
        self.slot_runs += 1;
        self.slot_runs_seen.push(slot);
        let evidence = ExecutionEvidence {
            outcome: CommandOutcome::Exited {
                exit_code: if failed || self.contradictory_slot_success {
                    1
                } else {
                    0
                },
            },
            stdout: "ok".to_owned(),
            stderr: String::new(),
        };
        if failed {
            SlotExecution::Failed {
                evidence,
                error: LifecycleError::SlotFailed,
            }
        } else {
            SlotExecution::Completed {
                evidence,
                record: Box::new(record_for(
                    slot,
                    self.calibration_passes,
                    self.duplicate_calibration_identity,
                )),
            }
        }
    }

    pub(super) fn teardown_impl(&mut self, _slot: SlotContext) -> Result<(), LifecycleError> {
        Ok(())
    }

    pub(super) fn teardown_environment_impl(&mut self) -> Result<(), LifecycleError> {
        self.operations.push(Operation::TeardownEnvironment);
        Ok(())
    }

    pub(super) fn append_raw_impl(
        &mut self,
        records: [RawRecord; 2],
    ) -> Result<(), LifecycleError> {
        self.operations
            .push(Operation::write_all(EvidenceFile::Raw));
        self.raw_records += records.len();
        Ok(())
    }

    pub(super) fn calibration_analyzed_impl(&mut self, records: usize) {
        self.operations
            .push(Operation::AnalyzeCalibration { records });
    }
}
