use super::operation::Operation;
use super::state::FakePort;
use crate::lifecycle::model::{EvidenceFile, IndexCell, IndexError, LifecycleError, Snapshot};
use crate::lifecycle::port::InitializationFailure;

impl FakePort {
    pub(super) fn initialization_failure(&self) -> InitializationFailure {
        InitializationFailure {
            error: LifecycleError::Initialization,
            events_opened: self.open_files.contains(&EvidenceFile::Events),
        }
    }

    pub(super) fn note_write(&mut self, file: EvidenceFile) {
        self.wrote_after_seal |= self.seal_created;
        self.operations.push(Operation::write_all(file));
    }

    pub(super) fn corrupt_cells(&self, cells: &mut [IndexCell]) {
        match self.index_error {
            None => {}
            Some(IndexError::WrongSchema) => cells[0].schema_version = 2,
            Some(IndexError::WrongExperiment) => cells[0].experiment_id = "wrong",
            Some(IndexError::WrongCycle) => cells[0].cycle = "rerun1",
            Some(IndexError::WrongOrder) => cells.swap(0, 1),
            Some(IndexError::WrongDocumentCount) => cells[0].document_count = 0,
            Some(IndexError::WrongSerializedBytes) => cells[0].index_serialized_bytes = 0,
            Some(IndexError::WrongSnapshot) => {
                cells[2].schema_snapshot = Some(Snapshot {
                    path: "wrong",
                    sha256: "wrong",
                })
            }
            Some(IndexError::DuplicateCell) => cells[3] = cells[0].clone(),
        }
    }
}
