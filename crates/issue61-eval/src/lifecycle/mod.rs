mod core;
#[cfg(test)]
mod fake;
pub mod live;
pub use live_port::run_live;
mod live_port;
// `model` now compiles unconditionally (see `mod core`/`mod port` above) so
// `live_port` can drive real execution, but some items (e.g. `Snapshot`,
// `SealStage`) are still only ever constructed by the `#[cfg(test)]`-only
// `fake`/`tests` modules.
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
