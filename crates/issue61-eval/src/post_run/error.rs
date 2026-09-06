use crate::AnalysisError;
use std::error::Error;
use std::fmt;
use std::io;

#[derive(Debug)]
pub enum PostRunError {
    CliInvocation,
    RepositoryRootMissing,
    RepositoryRootSymlink,
    RepositoryRootNotDirectory,
    CycleDirectoryMissing,
    CycleDirectorySymlink,
    CycleDirectoryNotDirectory,
    AnalysisCollision,
    CycleEntryNonUtf8,
    UnexpectedCycleEntry {
        name: String,
    },
    MissingCycleEntry {
        name: &'static str,
    },
    CycleEntryNotRegular {
        name: String,
    },
    PathSymlink {
        path: String,
    },
    InputNotRegular {
        path: String,
    },
    InputRead {
        path: String,
        source: io::Error,
    },
    MalformedChecksumSeal,
    ChecksumPathsUnsorted,
    DuplicateChecksumPath {
        path: String,
    },
    UnexpectedChecksumPath {
        path: String,
    },
    MissingChecksumPath {
        path: String,
    },
    ChecksumSelfReference,
    ChecksumAnalysisReference,
    FrozenChecksumMismatch {
        path: String,
    },
    ChecksumMismatch {
        path: String,
    },
    JsonlEncoding {
        file: &'static str,
    },
    JsonlLineEndings {
        file: &'static str,
    },
    JsonlBlankLine {
        file: &'static str,
        line: usize,
    },
    JsonlUnknownField {
        file: &'static str,
        line: usize,
        field: String,
    },
    JsonlDuplicateField {
        file: &'static str,
        line: usize,
        field: String,
    },
    JsonlMissingField {
        file: &'static str,
        line: usize,
        field: String,
    },
    JsonlInvalid {
        file: &'static str,
        line: usize,
    },
    WrongRecordCount {
        file: &'static str,
        expected: usize,
        found: usize,
    },
    MissingEngineRequest {
        file: &'static str,
        line: usize,
    },
    InvalidIndexRecord {
        line: usize,
        reason: IndexReason,
    },
    Analysis(AnalysisError),
    Serialization(serde_json::Error),
    Publish {
        source: io::Error,
    },
    PublishResidue {
        source: io::Error,
        cleanup: io::Error,
    },
}

#[derive(Debug, Clone, Copy)]
pub enum IndexReason {
    SchemaVersion,
    ExperimentId,
    Cycle,
    Engine,
    Dataset,
    DocumentCount,
    SerializedBytes,
    SnapshotFields,
    SnapshotIdentity,
    DuplicateCell,
    MissingCell,
}

impl fmt::Display for PostRunError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CliInvocation => formatter.write_str(
                "expected exactly --repository-root <PATH> --cycle <run1|rerun1|rerun2>",
            ),
            Self::RepositoryRootMissing => formatter.write_str("repository root does not exist"),
            Self::RepositoryRootSymlink => {
                formatter.write_str("repository root must not be a symlink")
            }
            Self::RepositoryRootNotDirectory => {
                formatter.write_str("repository root must be a directory")
            }
            Self::CycleDirectoryMissing => formatter.write_str("cycle directory does not exist"),
            Self::CycleDirectorySymlink => {
                formatter.write_str("cycle directory must not be a symlink")
            }
            Self::CycleDirectoryNotDirectory => {
                formatter.write_str("cycle directory must be a directory")
            }
            Self::AnalysisCollision => formatter.write_str("analysis.json already exists"),
            Self::CycleEntryNonUtf8 => {
                formatter.write_str("cycle directory contains a non-UTF-8 entry name")
            }
            Self::UnexpectedCycleEntry { name } => {
                write!(formatter, "cycle directory contains unexpected entry {name}")
            }
            Self::MissingCycleEntry { name } => {
                write!(formatter, "cycle directory is missing required entry {name}")
            }
            Self::CycleEntryNotRegular { name } => {
                write!(formatter, "cycle directory entry {name} must be a regular file")
            }
            Self::PathSymlink { path } => write!(formatter, "{path} must not be a symlink"),
            Self::InputNotRegular { path } => {
                write!(formatter, "{path} must be a regular file")
            }
            Self::InputRead { path, source } => write!(formatter, "failed to read {path}: {source}"),
            Self::MalformedChecksumSeal => formatter.write_str("malformed checksum seal"),
            Self::ChecksumPathsUnsorted => {
                formatter.write_str("checksum seal paths are not bytewise sorted")
            }
            Self::DuplicateChecksumPath { path } => {
                write!(formatter, "checksum seal contains duplicate path {path}")
            }
            Self::UnexpectedChecksumPath { path } => {
                write!(formatter, "checksum seal contains unexpected path {path}")
            }
            Self::MissingChecksumPath { path } => {
                write!(formatter, "checksum seal is missing path {path}")
            }
            Self::ChecksumSelfReference => {
                formatter.write_str("checksum seal must not reference itself")
            }
            Self::ChecksumAnalysisReference => {
                formatter.write_str("checksum seal must not reference analysis.json")
            }
            Self::FrozenChecksumMismatch { path } => {
                write!(formatter, "checksum seal has wrong frozen hash for {path}")
            }
            Self::ChecksumMismatch { path } => write!(formatter, "checksum mismatch for {path}"),
            Self::JsonlEncoding { file } => write!(formatter, "{file} is not valid UTF-8"),
            Self::JsonlLineEndings { file } => {
                write!(formatter, "{file} must use LF line endings with a final LF")
            }
            Self::JsonlBlankLine { file, line } => {
                write!(formatter, "{file} contains a blank line at line {line}")
            }
            Self::JsonlUnknownField { file, line, field } => {
                write!(formatter, "{file} line {line} contains unknown field {field}")
            }
            Self::JsonlDuplicateField { file, line, field } => {
                write!(formatter, "{file} line {line} contains duplicate field {field}")
            }
            Self::JsonlMissingField { file, line, field } => {
                write!(formatter, "{file} line {line} is missing field {field}")
            }
            Self::JsonlInvalid { file, line } => {
                write!(formatter, "{file} contains invalid JSON at line {line}")
            }
            Self::WrongRecordCount { file, expected, found } => {
                write!(formatter, "{file} contains {found} records; expected {expected}")
            }
            Self::MissingEngineRequest { file, line } => {
                write!(formatter, "{file} line {line} has a null engine request block")
            }
            Self::InvalidIndexRecord { line, reason } => {
                write!(formatter, "index_artifacts.jsonl line {line} has invalid {reason}")
            }
            Self::Analysis(source) => write!(formatter, "{source}"),
            Self::Serialization(source) => write!(formatter, "failed to serialize analysis.json: {source}"),
            Self::Publish { source } => write!(formatter, "failed to publish analysis.json: {source}"),
            Self::PublishResidue { source, cleanup } => write!(
                formatter,
                "failed to publish analysis.json: {source}; incomplete analysis.json remains: {cleanup}"
            ),
        }
    }
}

impl fmt::Display for IndexReason {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SchemaVersion => formatter.write_str("schema_version"),
            Self::ExperimentId => formatter.write_str("experiment_id"),
            Self::Cycle => formatter.write_str("cycle"),
            Self::Engine => formatter.write_str("engine"),
            Self::Dataset => formatter.write_str("dataset"),
            Self::DocumentCount => formatter.write_str("document_count"),
            Self::SerializedBytes => formatter.write_str("index_serialized_bytes"),
            Self::SnapshotFields => formatter.write_str("snapshot fields"),
            Self::SnapshotIdentity => formatter.write_str("snapshot identity"),
            Self::DuplicateCell => formatter.write_str("duplicate engine/dataset cell"),
            Self::MissingCell => formatter.write_str("missing engine/dataset cell"),
        }
    }
}

impl Error for PostRunError {}

impl From<AnalysisError> for PostRunError {
    fn from(source: AnalysisError) -> Self {
        Self::Analysis(source)
    }
}
