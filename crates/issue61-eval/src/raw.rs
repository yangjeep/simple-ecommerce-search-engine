use serde::{Deserialize, Deserializer, Serialize};
use std::error::Error;
use std::fmt;
use std::fs::{File, OpenOptions};
use std::io::{self, BufRead, BufReader, BufWriter, Write};
use std::path::Path;

pub const RAW_SCHEMA_VERSION: u32 = 4;

#[derive(Deserialize)]
struct RawSchema {
    schema_version: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RawRecord {
    pub schema_version: u32,
    pub experiment_id: String,
    pub run_id: String,
    pub rep: usize,
    pub engine_order: usize,
    pub calibration: bool,
    pub engine: String,
    pub dataset: String,
    pub query_class: String,
    pub regime: String,
    pub queries: u64,
    pub wall_elapsed_us: u64,
    pub timer_floor_clock_resolution_ns: f64,
    pub timer_floor_instant_now_overhead_ns: f64,
    pub timer_floor_effective_ns: f64,
    pub cpu_usage_usec: u64,
    pub cpu_user_usec: u64,
    pub cpu_system_usec: u64,
    pub cpu_nr_periods: u64,
    pub cpu_nr_throttled: u64,
    pub cpu_throttled_usec: u64,
    pub cpu_pressure_some_usec: u64,
    pub cpu_pressure_full_usec: u64,
    #[serde(deserialize_with = "required_option")]
    pub native_cgroup_host_pid: Option<u32>,
    #[serde(deserialize_with = "required_option")]
    pub native_pid_namespace: Option<u32>,
    #[serde(deserialize_with = "required_option")]
    pub process_cpu_user_usec: Option<u64>,
    #[serde(deserialize_with = "required_option")]
    pub process_cpu_system_usec: Option<u64>,
    #[serde(deserialize_with = "required_option")]
    pub process_cpu_total_usec: Option<u64>,
    #[serde(deserialize_with = "required_option")]
    pub process_cgroup_disagreement_pct: Option<f64>,
    pub cgroup_memory_footprint_bytes: u64,
    pub cgroup_memory_current_median_bytes: u64,
    pub cgroup_memory_current_max_bytes: u64,
    pub cgroup_memory_peak_bytes: u64,
    pub memory_anon_bytes: u64,
    pub memory_file_bytes: u64,
    pub memory_kernel_bytes: u64,
    pub memory_sock_bytes: u64,
    pub memory_swap_current_bytes: u64,
    pub memory_swap_peak_bytes: u64,
    pub memory_events_low: u64,
    pub memory_events_high: u64,
    pub memory_events_max: u64,
    pub memory_events_oom: u64,
    pub memory_events_oom_kill: u64,
    pub memory_events_oom_group_kill: u64,
    pub memory_swap_events_high: u64,
    pub memory_swap_events_max: u64,
    pub memory_swap_events_fail: u64,
    pub cpuset_cpus_effective: String,
    pub cpu_max: String,
    pub memory_max: String,
    pub memory_swap_max: String,
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

fn required_option<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer)
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
        let schema: RawSchema = serde_json::from_str(&line).map_err(|source| RawError::Json {
            line: index + 1,
            source,
        })?;
        if schema.schema_version != RAW_SCHEMA_VERSION {
            return Err(RawError::UnsupportedSchemaVersion {
                found: schema.schema_version,
                expected: RAW_SCHEMA_VERSION,
            });
        }
        let record: RawRecord = serde_json::from_str(&line).map_err(|source| RawError::Json {
            line: index + 1,
            source,
        })?;
        records.push(record);
    }
    Ok(records)
}
