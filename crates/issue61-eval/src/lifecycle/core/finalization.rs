use super::Driver;
use crate::lifecycle::model::{
    EventType, EvidenceFile, LifecycleError, Phase, ResolvedSealEntry, ResolvedSealManifest,
    SealHashSource, SealManifest, Terminal,
};
use crate::lifecycle::port::LifecyclePort;

impl<P: LifecyclePort> Driver<'_, P> {
    pub(super) fn finalize(&mut self) -> Result<Terminal, LifecycleError> {
        self.phase(Phase::EvidenceFinalization, EventType::PhaseStarted, None)?;
        for file in EvidenceFile::NON_EVENT_CLOSE_ORDER {
            self.durable_close(file)?;
        }
        self.phase(
            Phase::EvidenceFinalization,
            EventType::EvidenceFinalized,
            None,
        )?;
        self.durable_close(EvidenceFile::Events)?;
        let mut entries = Vec::new();
        for entry in SealManifest::revision9(self.cycle).entries {
            let hash = match entry.hash_source {
                SealHashSource::Computed => self.port.compute_hash(&entry.path)?,
                SealHashSource::Pinned(hash) => hash.to_owned(),
            };
            entries.push(ResolvedSealEntry {
                path: entry.path,
                hash,
            });
        }
        self.write_durable_seal(&ResolvedSealManifest { entries })?;
        if !self.port.verify_seal()? || self.port.regular_file_count() != 7 {
            return Err(LifecycleError::InvalidSeal);
        }
        self.port.invoke_analyzer()?;
        Ok(Terminal::Completed)
    }

    pub(super) fn durable_close(&mut self, file: EvidenceFile) -> Result<(), LifecycleError> {
        let mut primary = None;
        if let Err(error) = self.port.flush(file) {
            primary.get_or_insert(error);
        }
        if let Err(error) = self.port.sync(file) {
            primary.get_or_insert(error);
        }
        if let Err(error) = self.port.close(file) {
            primary.get_or_insert(error);
        }
        match primary {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    fn write_durable_seal(
        &mut self,
        manifest: &ResolvedSealManifest,
    ) -> Result<(), LifecycleError> {
        self.port.create_seal()?;
        let mut primary = None;
        if let Err(error) = self.port.write_seal(manifest) {
            primary.get_or_insert(error);
        }
        if let Err(error) = self.port.flush_seal() {
            primary.get_or_insert(error);
        }
        if let Err(error) = self.port.sync_seal() {
            primary.get_or_insert(error);
        }
        if let Err(error) = self.port.close_seal() {
            primary.get_or_insert(error);
        }
        match primary {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
}
