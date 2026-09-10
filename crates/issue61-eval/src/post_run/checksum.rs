use super::error::PostRunError;
use crate::{sha256_hex, CampaignCycle};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Component, Path, PathBuf};

const STATIC_INPUTS: [(&str, &str); 6] = [
    (
        "benchmarks/configs/issue61/solr_esci_electronics_config.json",
        "ae5c0e1c8de23a798a550b6042617f0bedd4b8e04a1ecbe5d8d9633df5bfff5b",
    ),
    (
        "benchmarks/configs/issue61/solr_esci_electronics_schema.json",
        "62266803df8715b3b09485fdc168b580c1ae337d1dd7b5b43ae1ce09e74eca2e",
    ),
    (
        "benchmarks/configs/issue61/solr_wands_config.json",
        "ae5c0e1c8de23a798a550b6042617f0bedd4b8e04a1ecbe5d8d9633df5bfff5b",
    ),
    (
        "benchmarks/configs/issue61/solr_wands_schema.json",
        "997e321ed081133b9a83fce5f35e42a75cfd3333bf91505f876501343600463a",
    ),
    (
        "benchmarks/workloads/i61_esci_electronics.jsonl",
        "531e39d0feda45591c0f3f17adfa25b1b52d73ff70994a3e31cad739364f050e",
    ),
    (
        "benchmarks/workloads/i61_wands_480.jsonl",
        "462b5bf8cae6e12fdcfa2cb5177a648d4de43aec0936c35e61aaffaacb0cad08",
    ),
];

const CYCLE_INPUTS: [&str; 6] = [
    "candidate_audit_esci.jsonl",
    "candidate_audit_wands.jsonl",
    "commands.log",
    "events.jsonl",
    "index_artifacts.jsonl",
    "raw.jsonl",
];

#[derive(Debug)]
pub struct SealedInput {
    pub path: String,
    pub sha256: String,
    pub bytes: Vec<u8>,
}

#[derive(Debug)]
pub struct VerifiedSeal {
    pub inputs: Vec<SealedInput>,
}

impl VerifiedSeal {
    pub fn bytes(&self, path: &str) -> Result<&[u8], PostRunError> {
        self.inputs
            .iter()
            .find(|input| input.path == path)
            .map(|input| input.bytes.as_slice())
            .ok_or_else(|| PostRunError::MissingChecksumPath {
                path: path.to_owned(),
            })
    }
}

struct ManifestEntry {
    sha256: String,
    path: String,
}

pub fn verify_seal(
    repository_root: &Path,
    cycle_directory: &Path,
    cycle: CampaignCycle,
) -> Result<VerifiedSeal, PostRunError> {
    let manifest_path = cycle_directory.join("checksums.sha256");
    let manifest_relative = format!(
        "artifacts/issue61/i61_e1_{}/checksums.sha256",
        cycle.as_str()
    );
    let manifest_bytes = read_regular(repository_root, &manifest_relative, &manifest_path)?;
    let entries = parse_manifest(&manifest_bytes)?;
    let expected = expected_paths(cycle);
    validate_manifest(&entries, &expected)?;

    let mut inputs = Vec::with_capacity(entries.len());
    for entry in entries {
        if let Some((_, frozen)) = STATIC_INPUTS.iter().find(|(path, _)| *path == entry.path) {
            if entry.sha256 != *frozen {
                return Err(PostRunError::FrozenChecksumMismatch { path: entry.path });
            }
        }
        let absolute = repository_root.join(&entry.path);
        let bytes = read_regular(repository_root, &entry.path, &absolute)?;
        if sha256_hex(&bytes) != entry.sha256 {
            return Err(PostRunError::ChecksumMismatch { path: entry.path });
        }
        inputs.push(SealedInput {
            path: entry.path,
            sha256: entry.sha256,
            bytes,
        });
    }
    Ok(VerifiedSeal { inputs })
}

fn parse_manifest(bytes: &[u8]) -> Result<Vec<ManifestEntry>, PostRunError> {
    if bytes.is_empty() || bytes.contains(&b'\r') {
        return Err(PostRunError::MalformedChecksumSeal);
    }
    let text = std::str::from_utf8(bytes).map_err(|_| PostRunError::MalformedChecksumSeal)?;
    let body = text.strip_suffix('\n').unwrap_or(text);
    if body.is_empty() || body.ends_with('\n') {
        return Err(PostRunError::MalformedChecksumSeal);
    }
    body.split('\n').map(parse_manifest_line).collect()
}

fn parse_manifest_line(line: &str) -> Result<ManifestEntry, PostRunError> {
    if line.is_empty() || line.starts_with('#') {
        return Err(PostRunError::MalformedChecksumSeal);
    }
    let Some((digest, path)) = line.split_once("  ") else {
        return Err(PostRunError::MalformedChecksumSeal);
    };
    if digest.len() != 64
        || !digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        || path.is_empty()
        || path.contains("  ")
        || path.contains('\\')
        || !safe_relative_path(path)
    {
        return Err(PostRunError::MalformedChecksumSeal);
    }
    Ok(ManifestEntry {
        sha256: digest.to_owned(),
        path: path.to_owned(),
    })
}

fn safe_relative_path(path: &str) -> bool {
    !Path::new(path).is_absolute()
        && Path::new(path)
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

fn validate_manifest(entries: &[ManifestEntry], expected: &[String]) -> Result<(), PostRunError> {
    if entries
        .iter()
        .any(|entry| entry.path.ends_with("/checksums.sha256"))
    {
        return Err(PostRunError::ChecksumSelfReference);
    }
    if entries
        .iter()
        .any(|entry| entry.path.ends_with("/analysis.json"))
    {
        return Err(PostRunError::ChecksumAnalysisReference);
    }
    let mut seen = BTreeSet::new();
    for entry in entries {
        if !seen.insert(entry.path.as_str()) {
            return Err(PostRunError::DuplicateChecksumPath {
                path: entry.path.clone(),
            });
        }
    }
    if entries.windows(2).any(|pair| pair[0].path >= pair[1].path) {
        return Err(PostRunError::ChecksumPathsUnsorted);
    }
    for entry in entries {
        if !expected.contains(&entry.path) {
            return Err(PostRunError::UnexpectedChecksumPath {
                path: entry.path.clone(),
            });
        }
    }
    for path in expected {
        if !seen.contains(path.as_str()) {
            return Err(PostRunError::MissingChecksumPath { path: path.clone() });
        }
    }
    Ok(())
}

fn expected_paths(cycle: CampaignCycle) -> Vec<String> {
    let prefix = format!("artifacts/issue61/i61_e1_{}", cycle.as_str());
    let mut paths = CYCLE_INPUTS
        .iter()
        .map(|name| format!("{prefix}/{name}"))
        .chain(STATIC_INPUTS.iter().map(|(path, _)| (*path).to_owned()))
        .collect::<Vec<_>>();
    paths.sort_unstable();
    paths
}

pub fn reject_symlink_components(path: &Path, label: &str) -> Result<(), PostRunError> {
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component.as_os_str());
        let metadata = match fs::symlink_metadata(&current) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(source) => {
                return Err(PostRunError::InputRead {
                    path: label.to_owned(),
                    source,
                });
            }
        };
        if metadata.file_type().is_symlink() {
            return Err(PostRunError::PathSymlink {
                path: label.to_owned(),
            });
        }
    }
    Ok(())
}

fn read_regular(
    repository_root: &Path,
    relative: &str,
    absolute: &Path,
) -> Result<Vec<u8>, PostRunError> {
    reject_symlink_components(absolute, relative)?;
    let metadata = fs::symlink_metadata(absolute).map_err(|source| PostRunError::InputRead {
        path: relative.to_owned(),
        source,
    })?;
    if !metadata.file_type().is_file() {
        return Err(PostRunError::InputNotRegular {
            path: relative.to_owned(),
        });
    }
    if !absolute.starts_with(repository_root) {
        return Err(PostRunError::MalformedChecksumSeal);
    }
    fs::read(absolute).map_err(|source| PostRunError::InputRead {
        path: relative.to_owned(),
        source,
    })
}
