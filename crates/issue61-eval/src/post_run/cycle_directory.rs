use super::checksum::reject_symlink_components;
use super::PostRunError;
use crate::CampaignCycle;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

const REQUIRED_ENTRIES: [&str; 7] = [
    "candidate_audit_esci.jsonl",
    "candidate_audit_wands.jsonl",
    "checksums.sha256",
    "commands.log",
    "events.jsonl",
    "index_artifacts.jsonl",
    "raw.jsonl",
];

pub fn validate_entries(directory: &Path) -> Result<(), PostRunError> {
    let entries = fs::read_dir(directory).map_err(|source| PostRunError::InputRead {
        path: "cycle directory".to_owned(),
        source,
    })?;
    let mut names = entries
        .map(|entry| {
            entry
                .map(|value| value.file_name())
                .map_err(|source| PostRunError::InputRead {
                    path: "cycle directory".to_owned(),
                    source,
                })
        })
        .collect::<Result<Vec<OsString>, _>>()?;
    names.sort_unstable();

    for name in &names {
        let Some(name) = name.to_str() else {
            return Err(PostRunError::CycleEntryNonUtf8);
        };
        if !REQUIRED_ENTRIES.contains(&name) {
            return Err(PostRunError::UnexpectedCycleEntry {
                name: name.to_owned(),
            });
        }
        let metadata = fs::symlink_metadata(directory.join(name)).map_err(|source| {
            PostRunError::InputRead {
                path: format!("cycle directory entry {name}"),
                source,
            }
        })?;
        if !metadata.file_type().is_file() {
            return Err(PostRunError::CycleEntryNotRegular {
                name: name.to_owned(),
            });
        }
    }
    for required in REQUIRED_ENTRIES {
        if !names.iter().any(|name| name == required) {
            return Err(PostRunError::MissingCycleEntry { name: required });
        }
    }
    Ok(())
}

pub struct AnalysisPaths {
    pub repository_root: PathBuf,
    pub cycle_directory: PathBuf,
    pub analysis: PathBuf,
}

impl AnalysisPaths {
    pub fn validate(repository_root: &Path, cycle: CampaignCycle) -> Result<Self, PostRunError> {
        let root_metadata = match fs::symlink_metadata(repository_root) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(PostRunError::RepositoryRootMissing);
            }
            Err(source) => {
                return Err(PostRunError::InputRead {
                    path: "repository root".to_owned(),
                    source,
                });
            }
        };
        if root_metadata.file_type().is_symlink() {
            return Err(PostRunError::RepositoryRootSymlink);
        }
        if !root_metadata.is_dir() {
            return Err(PostRunError::RepositoryRootNotDirectory);
        }
        reject_symlink_components(repository_root, "repository root")?;
        let repository_root =
            fs::canonicalize(repository_root).map_err(|source| PostRunError::InputRead {
                path: "repository root".to_owned(),
                source,
            })?;
        let cycle_directory =
            repository_root.join(format!("artifacts/issue61/i61_e1_{}", cycle.as_str()));
        let cycle_metadata = match fs::symlink_metadata(&cycle_directory) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(PostRunError::CycleDirectoryMissing);
            }
            Err(source) => {
                return Err(PostRunError::InputRead {
                    path: "cycle directory".to_owned(),
                    source,
                });
            }
        };
        if cycle_metadata.file_type().is_symlink() {
            return Err(PostRunError::CycleDirectorySymlink);
        }
        if !cycle_metadata.is_dir() {
            return Err(PostRunError::CycleDirectoryNotDirectory);
        }
        reject_symlink_components(&cycle_directory, "cycle directory")?;
        let analysis = cycle_directory.join("analysis.json");
        if fs::symlink_metadata(&analysis).is_ok() {
            return Err(PostRunError::AnalysisCollision);
        }
        validate_entries(&cycle_directory)?;
        Ok(Self {
            repository_root,
            cycle_directory,
            analysis,
        })
    }
}
