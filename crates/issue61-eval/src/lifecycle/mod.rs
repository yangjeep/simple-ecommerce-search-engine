mod core;
mod fake;
mod model;
mod port;
mod tests;

use core::run;
use fake::{FakePort, FileOperation, Operation, PathState};
use model::{
    prepare_command, serialize_command, CommandInput, CommandOutcome, CommandRequest,
    DatasetIdentity, EnvironmentInput, EventType, EvidenceFile, ExecutionEvidence, IndexCell,
    IndexError, LifecycleError, LoggedText, OutputVisibility, Phase, Projection, SealStage,
    Sequence, SeriesIdentity, SlotIndex, Terminal,
};
