use crate::{CgroupError, CgroupReader, Dataset};
use std::error::Error;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

const NATIVE_IMAGE: &str =
    "debian@sha256:88200866dfff7ea7f5cbcb6ec7c8a701889efe6fe859fe64d6990e4b07ea4171";
const NATIVE_CONTAINER: &str = "i61-native-live";
const NATIVE_BINARY: &str = "/opt/i61_native_server";
const CONTAINER_DATASET: &str = "/dataset";

const REQUIRED_STATIC_FILES: [&str; 10] = [
    "benchmarks/configs/issue61/container_limits.env",
    "benchmarks/configs/issue61/solr_esci_electronics_config.json",
    "benchmarks/configs/issue61/solr_esci_electronics_schema.json",
    "benchmarks/configs/issue61/solr_wands_config.json",
    "benchmarks/configs/issue61/solr_wands_schema.json",
    "benchmarks/workloads/i61_esci_electronics.jsonl",
    "benchmarks/workloads/i61_wands_480.jsonl",
    "target/release/i61_analyze",
    "target/release/i61_bench",
    "target/release/i61_native_server",
];

#[derive(Debug)]
pub struct StaticValidationError {
    path: PathBuf,
    kind: StaticValidationErrorKind,
}

#[derive(Debug)]
enum StaticValidationErrorKind {
    Canonicalize(io::Error),
    NonCanonical { canonical: PathBuf },
    MissingRequiredFile,
    Inspect(io::Error),
}

impl fmt::Display for StaticValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "static validation failed for {}: ",
            self.path.display()
        )?;
        match &self.kind {
            StaticValidationErrorKind::Canonicalize(error) => write!(formatter, "{error}"),
            StaticValidationErrorKind::NonCanonical { canonical } => write!(
                formatter,
                "repository root is not canonical (resolved to {})",
                canonical.display()
            ),
            StaticValidationErrorKind::MissingRequiredFile => {
                write!(formatter, "required regular file is absent")
            }
            StaticValidationErrorKind::Inspect(error) => write!(formatter, "{error}"),
        }
    }
}

impl Error for StaticValidationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match &self.kind {
            StaticValidationErrorKind::Canonicalize(error)
            | StaticValidationErrorKind::Inspect(error) => Some(error),
            StaticValidationErrorKind::NonCanonical { canonical: _ }
            | StaticValidationErrorKind::MissingRequiredFile => None,
        }
    }
}

pub fn validate_repository_root(root: &Path) -> Result<PathBuf, StaticValidationError> {
    let canonical = std::fs::canonicalize(root).map_err(|error| StaticValidationError {
        path: root.to_path_buf(),
        kind: StaticValidationErrorKind::Canonicalize(error),
    })?;
    if canonical != root {
        return Err(StaticValidationError {
            path: root.to_path_buf(),
            kind: StaticValidationErrorKind::NonCanonical { canonical },
        });
    }
    for relative in REQUIRED_STATIC_FILES {
        let path = root.join(relative);
        let metadata = std::fs::metadata(&path).map_err(|error| StaticValidationError {
            path: path.clone(),
            kind: if error.kind() == io::ErrorKind::NotFound {
                StaticValidationErrorKind::MissingRequiredFile
            } else {
                StaticValidationErrorKind::Inspect(error)
            },
        })?;
        if !metadata.is_file() {
            return Err(StaticValidationError {
                path,
                kind: StaticValidationErrorKind::MissingRequiredFile,
            });
        }
    }
    Ok(canonical)
}

pub struct NativeLaunchContract {
    repository_root: PathBuf,
    dataset: Dataset,
}

impl NativeLaunchContract {
    pub fn for_dataset(repository_root: &Path, dataset: Dataset) -> Self {
        Self {
            repository_root: repository_root.to_path_buf(),
            dataset,
        }
    }

    pub const fn image(&self) -> &'static str {
        NATIVE_IMAGE
    }

    pub const fn container_name(&self) -> &'static str {
        NATIVE_CONTAINER
    }

    pub const fn cpus(&self) -> &'static str {
        "3"
    }

    pub const fn cpuset_cpus(&self) -> &'static str {
        "0-2"
    }

    pub const fn memory(&self) -> &'static str {
        "6g"
    }

    pub const fn memory_swap(&self) -> &'static str {
        "6g"
    }

    pub const fn port_binding(&self) -> (u16, u16) {
        (9900, 9900)
    }

    pub const fn endpoint(&self) -> &'static str {
        "http://127.0.0.1:9900"
    }

    pub const fn readiness_path(&self) -> &'static str {
        "/ping"
    }

    pub const fn readiness_interval(&self) -> Duration {
        Duration::from_secs(1)
    }

    pub const fn readiness_timeout(&self) -> Duration {
        Duration::from_secs(90)
    }

    pub const fn teardown_target(&self) -> &'static str {
        NATIVE_CONTAINER
    }

    pub const fn teardown_timeout(&self) -> Duration {
        Duration::from_secs(30)
    }

    pub fn binary_mount(&self) -> (PathBuf, PathBuf, bool) {
        (
            self.repository_root
                .join("target/release/i61_native_server"),
            PathBuf::from(NATIVE_BINARY),
            true,
        )
    }

    pub fn dataset_mount(&self) -> (PathBuf, PathBuf, bool) {
        (
            self.repository_root.join(match self.dataset {
                Dataset::Wands => "dataset_cache/wands",
                Dataset::EsciElectronics => "dataset_cache/esci_electronics",
            }),
            PathBuf::from(CONTAINER_DATASET),
            true,
        )
    }

    pub const fn argv(&self) -> [&'static str; 7] {
        match self.dataset {
            Dataset::Wands => [
                NATIVE_BINARY,
                "--catalog",
                "/dataset/catalog.jsonl",
                "--dataset",
                "wands",
                "--port",
                "9900",
            ],
            Dataset::EsciElectronics => [
                NATIVE_BINARY,
                "--catalog",
                "/dataset/esci_electronics_products.jsonl",
                "--dataset",
                "esci_electronics",
                "--port",
                "9900",
            ],
        }
    }

    pub fn cgroup_reader_for_pid(
        &self,
        pid: u32,
        proc_root: &Path,
        mount: &Path,
    ) -> Result<CgroupReader, CgroupError> {
        CgroupReader::for_pid(pid, proc_root, mount)
    }
}
