mod core;
#[cfg(test)]
mod fake;
pub mod live;
pub use live_port::run_live;
mod live_port;
#[allow(dead_code)]
mod model;
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
