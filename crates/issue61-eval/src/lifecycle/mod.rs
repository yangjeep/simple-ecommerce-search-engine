#[cfg(test)]
mod core;
#[cfg(test)]
mod fake;
pub mod live;
#[cfg(test)]
mod model;
#[cfg(test)]
mod port;
#[cfg(test)]
mod tests;

#[cfg(test)]
use core::run;
#[cfg(test)]
use fake::{FakePort, FileOperation, Operation, PathState};
#[cfg(test)]
use model::{
    prepare_command, serialize_command, CommandInput, CommandOutcome, CommandRequest,
    DatasetIdentity, EnvironmentInput, EventType, EvidenceFile, ExecutionEvidence, IndexCell,
    IndexError, LifecycleError, LoggedText, OutputVisibility, Phase, Projection, SealStage,
    Sequence, SeriesIdentity, SlotIndex, Terminal,
};
