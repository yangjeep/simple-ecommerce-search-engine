use serde::{Deserialize, Serialize};
use std::error::Error;
use std::fmt;
use std::fs::{File, OpenOptions};
use std::io::{self, BufRead, BufReader, BufWriter, Write};
use std::path::Path;

pub const RAW_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RawRecord {
    pub schema_version: u32,
    pub experiment_id: String,
    pub run_id: String,
    pub rep: usize,
    pub engine: String,
    pub dataset: String,
    pub query_class: String,
    pub regime: String,
    pub queries: u64,
    pub wall_elapsed_us: u64,
    pub cpu_usage_usec: u64,
    pub cpu_user_usec: u64,
    pub cpu_system_usec: u64,
    pub rss_current_bytes: u64,
    pub rss_peak_bytes: u64,
    pub index_serialized_bytes: u64,
    pub latency_p50_us: f64,
    pub latency_p95_us: f64,
    pub latency_p99_us: f64,
    pub latency_mean_us: f64,
    pub steal_pct: f64,
    pub excluded: bool,
    pub exclusion_reason: Option<String>,
    pub git_sha: String,
    pub host: String,
    pub timestamp_utc: String,
}

impl RawRecord {
    pub fn cpu_us_per_query(&self) -> Option<f64> {
        (self.queries > 0).then(|| self.cpu_usage_usec as f64 / self.queries as f64)
    }
}

#[derive(Debug)]
pub enum RawError {
    Io(io::Error),
    Json {
        line: usize,
        source: serde_json::Error,
    },
    UnsupportedSchemaVersion {
        found: u32,
        expected: u32,
    },
}

impl fmt::Display for RawError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(source) => write!(formatter, "raw artifact I/O failed: {source}"),
            Self::Json { line, source } => {
                write!(
                    formatter,
                    "invalid raw artifact JSON at line {line}: {source}"
                )
            }
            Self::UnsupportedSchemaVersion { found, expected } => write!(
                formatter,
                "unsupported raw schema version {found}; expected {expected}"
            ),
        }
    }
}

impl Error for RawError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(source) => Some(source),
            Self::Json { source, .. } => Some(source),
            Self::UnsupportedSchemaVersion { .. } => None,
        }
    }
}

pub fn write_jsonl(path: &Path, records: &[RawRecord]) -> io::Result<()> {
    let file = OpenOptions::new().create(true).append(true).open(path)?;
    let mut writer = BufWriter::new(file);
    for record in records {
        serde_json::to_writer(&mut writer, record)
            .map_err(|source| io::Error::new(io::ErrorKind::InvalidData, source))?;
        writer.write_all(b"\n")?;
    }
    writer.flush()
}

pub fn read_jsonl(path: &Path) -> Result<Vec<RawRecord>, RawError> {
    let reader = BufReader::new(File::open(path).map_err(RawError::Io)?);
    let mut records = Vec::new();
    for (index, line) in reader.lines().enumerate() {
        let line = line.map_err(RawError::Io)?;
        if line.trim().is_empty() {
            continue;
        }
        let record: RawRecord = serde_json::from_str(&line).map_err(|source| RawError::Json {
            line: index + 1,
            source,
        })?;
        if record.schema_version != RAW_SCHEMA_VERSION {
            return Err(RawError::UnsupportedSchemaVersion {
                found: record.schema_version,
                expected: RAW_SCHEMA_VERSION,
            });
        }
        records.push(record);
    }
    Ok(records)
}
