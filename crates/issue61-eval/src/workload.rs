use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;
use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum AdmissionClass {
    FastPath,
    Hybrid,
    Punt,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeRequest {
    pub q: String,
    pub params: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SolrRequest {
    pub q: String,
    pub fq: Vec<String>,
    pub params: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrozenQuery {
    pub query_id: String,
    pub text: String,
    pub admission_class: AdmissionClass,
    pub structural_constraint_count: usize,
    pub has_residual_lexical: bool,
    pub rows: usize,
    pub native: Option<NativeRequest>,
    pub solr: Option<SolrRequest>,
}

impl FrozenQuery {
    pub fn with_engine_requests(mut self, native: NativeRequest, solr: SolrRequest) -> Self {
        self.native = Some(native);
        self.solr = Some(solr);
        self
    }
}

#[derive(Debug)]
pub enum WorkloadError {
    Io(std::io::Error),
    Json {
        line: usize,
        source: serde_json::Error,
    },
}

impl fmt::Display for WorkloadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(source) => write!(f, "workload I/O failed: {source}"),
            Self::Json { line, source } => {
                write!(f, "invalid workload JSON at line {line}: {source}")
            }
        }
    }
}

impl Error for WorkloadError {}

pub fn load_workload(path: &Path) -> Result<Vec<FrozenQuery>, WorkloadError> {
    let reader = BufReader::new(File::open(path).map_err(WorkloadError::Io)?);
    reader
        .lines()
        .enumerate()
        .filter_map(|(index, line)| match line {
            Ok(value) if value.trim().is_empty() => None,
            Ok(value) => Some(
                serde_json::from_str(&value).map_err(|source| WorkloadError::Json {
                    line: index + 1,
                    source,
                }),
            ),
            Err(source) => Some(Err(WorkloadError::Io(source))),
        })
        .collect()
}

pub fn write_workload(path: &Path, queries: &[FrozenQuery]) -> Result<(), WorkloadError> {
    let mut writer = BufWriter::new(File::create(path).map_err(WorkloadError::Io)?);
    for query in queries {
        serde_json::to_writer(&mut writer, query)
            .map_err(|source| WorkloadError::Json { line: 0, source })?;
        writer.write_all(b"\n").map_err(WorkloadError::Io)?;
    }
    writer.flush().map_err(WorkloadError::Io)
}
