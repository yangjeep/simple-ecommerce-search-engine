use super::operation::Operation;
use super::state::FakePort;
use crate::lifecycle::model::{LifecycleError, ResolvedSealManifest, SealStage};

impl FakePort {
    pub(super) fn create_seal_impl(&mut self) -> Result<(), LifecycleError> {
        self.operations.push(Operation::CreateSeal);
        if self.fail_seal_stages.contains(&SealStage::CreateNew) {
            return Err(LifecycleError::SealCreateNew);
        }
        self.seal_created = true;
        self.seal_open = true;
        self.regular_files += 1;
        Ok(())
    }

    pub(super) fn write_seal_impl(
        &mut self,
        manifest: &ResolvedSealManifest,
    ) -> Result<(), LifecycleError> {
        self.operations.push(Operation::WriteSeal {
            entries: manifest.entries.len(),
        });
        if self.fail_seal_stages.contains(&SealStage::WriteAll) {
            return Err(LifecycleError::SealWriteAll);
        }
        self.seal_lines = manifest
            .entries
            .iter()
            .map(|entry| format!("{}  {}", entry.hash, entry.path))
            .collect();
        Ok(())
    }

    pub(super) fn flush_seal_impl(&mut self) -> Result<(), LifecycleError> {
        self.operations.push(Operation::FlushSeal);
        if self.fail_seal_stages.contains(&SealStage::Flush) {
            Err(LifecycleError::SealFlush)
        } else {
            Ok(())
        }
    }

    pub(super) fn sync_seal_impl(&mut self) -> Result<(), LifecycleError> {
        self.operations.push(Operation::SyncSeal);
        if self.fail_seal_stages.contains(&SealStage::Sync) {
            Err(LifecycleError::SealSync)
        } else {
            Ok(())
        }
    }

    pub(super) fn close_seal_impl(&mut self) -> Result<(), LifecycleError> {
        self.operations.push(Operation::CloseSeal);
        self.seal_open = false;
        if self.fail_seal_stages.contains(&SealStage::Close) {
            Err(LifecycleError::SealClose)
        } else {
            Ok(())
        }
    }
}
