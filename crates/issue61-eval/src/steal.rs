mod prescreen;

pub use prescreen::{
    assess_assigned_cpu_steal, parse_assigned_proc_stat, probe_assigned_cpu_steal,
    run_assigned_cpu_steal_probe, CpuSet, StealProbeConfig, StealProbeResult, STEAL_PROBE_DURATION,
};
use std::error::Error;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

pub const STEAL_EXCLUSION_THRESHOLD_PCT: f64 = 1.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CpuTimes {
    pub total_jiffies: u64,
    pub steal_jiffies: u64,
}

#[derive(Debug)]
pub enum StealError {
    Io {
        path: PathBuf,
        source: io::Error,
    },
    MissingAggregateCpu,
    InvalidInteger(String),
    InvalidCpuSet(String),
    InvalidSelectedCpu(u32),
    CounterRollback {
        field: &'static str,
        earlier: u64,
        later: u64,
    },
    ZeroTotalDelta,
    ArithmeticOverflow(&'static str),
}

impl fmt::Display for StealError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => {
                write!(formatter, "failed to read {}: {source}", path.display())
            }
            Self::MissingAggregateCpu => write!(formatter, "missing aggregate cpu line"),
            Self::InvalidInteger(value) => write!(formatter, "invalid CPU jiffy count: {value}"),
            error @ (Self::InvalidCpuSet(_)
            | Self::InvalidSelectedCpu(_)
            | Self::CounterRollback { .. }
            | Self::ZeroTotalDelta
            | Self::ArithmeticOverflow(_)) => write!(formatter, "{error:?}"),
        }
    }
}

impl Error for StealError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        if let Self::Io { source, .. } = self {
            Some(source)
        } else {
            None
        }
    }
}

pub fn parse_proc_stat(content: &str) -> Result<CpuTimes, StealError> {
    let line = content
        .lines()
        .find(|line| line.split_whitespace().next() == Some("cpu"))
        .ok_or(StealError::MissingAggregateCpu)?;
    let mut total_jiffies = 0;
    let mut steal_jiffies = 0;
    for (index, raw) in line.split_whitespace().skip(1).enumerate() {
        let value = raw
            .parse::<u64>()
            .map_err(|_| StealError::InvalidInteger(raw.to_owned()))?;
        total_jiffies += value;
        if index == 7 {
            steal_jiffies = value;
        }
    }
    Ok(CpuTimes {
        total_jiffies,
        steal_jiffies,
    })
}

pub fn read_proc_stat(proc_root: &Path) -> Result<CpuTimes, StealError> {
    parse_proc_stat(&read_stat_file(&proc_root.join("stat"))?)
}

pub(super) fn read_stat_file(path: &Path) -> Result<String, StealError> {
    std::fs::read_to_string(path).map_err(|source| StealError::Io {
        path: path.to_path_buf(),
        source,
    })
}

pub fn steal_percent(earlier: &CpuTimes, later: &CpuTimes) -> f64 {
    let total_delta = later.total_jiffies.saturating_sub(earlier.total_jiffies);
    if total_delta == 0 {
        return 0.0;
    }
    let steal_delta = later.steal_jiffies.saturating_sub(earlier.steal_jiffies);
    steal_delta as f64 / total_delta as f64 * 100.0
}

pub fn should_exclude_rep(steal_pct: f64) -> bool {
    steal_pct > STEAL_EXCLUSION_THRESHOLD_PCT
}
