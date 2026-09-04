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
    Io { path: PathBuf, source: io::Error },
    MissingAggregateCpu,
    InvalidInteger(String),
}

impl fmt::Display for StealError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => {
                write!(formatter, "failed to read {}: {source}", path.display())
            }
            Self::MissingAggregateCpu => write!(formatter, "missing aggregate cpu line"),
            Self::InvalidInteger(value) => write!(formatter, "invalid CPU jiffy count: {value}"),
        }
    }
}

impl Error for StealError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::MissingAggregateCpu | Self::InvalidInteger(_) => None,
        }
    }
}

pub fn parse_proc_stat(content: &str) -> Result<CpuTimes, StealError> {
    let line = content
        .lines()
        .find(|line| line.split_whitespace().next() == Some("cpu"))
        .ok_or(StealError::MissingAggregateCpu)?;
    let values = line
        .split_whitespace()
        .skip(1)
        .map(|value| {
            value
                .parse::<u64>()
                .map_err(|_| StealError::InvalidInteger(value.to_owned()))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let total_jiffies = values.iter().sum();
    let steal_jiffies = values.get(7).copied().unwrap_or(0);
    Ok(CpuTimes {
        total_jiffies,
        steal_jiffies,
    })
}

pub fn read_proc_stat(proc_root: &Path) -> Result<CpuTimes, StealError> {
    let path = proc_root.join("stat");
    let content =
        std::fs::read_to_string(&path).map_err(|source| StealError::Io { path, source })?;
    parse_proc_stat(&content)
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
