mod command;
mod error;
mod event;
mod index;
mod path;
mod seal;
mod types;

pub(super) use crate::RawRecord;
pub(super) use command::{
    prepare_command, serialize_command, CommandInput, CommandOutcome, CommandRequest,
    EnvironmentInput, ExecutionEvidence, LoggedText, OutputVisibility, PreparedCommand,
};
pub(super) use error::LifecycleError;
pub(super) use event::{Event, EventIdentity, EventRecord, EventType};
// `Snapshot` is only constructed by `#[cfg(test)]` fixtures; production code
// only reads `IndexCell.schema_snapshot`/`config_snapshot`, never builds one.
#[allow(unused_imports)]
pub(super) use index::{validate_index, IndexArtifact, IndexCell, IndexError, Snapshot};
pub(super) use path::CyclePath;
// `SealStage` is only used by the `#[cfg(test)]`-only `FakePort`'s
// `fail_seal_stages` test knob; the live port has no analogous fault
// injection.
#[allow(unused_imports)]
pub(super) use seal::{
    ResolvedSealEntry, ResolvedSealManifest, SealHashSource, SealManifest, SealStage,
};
pub(super) use types::{
    Attempt, BlockContext, DatasetIdentity, EngineIdentity, EvidenceFile, Phase, Projection,
    Sequence, SeriesIdentity, SlotContext, SlotIndex, Terminal,
};
