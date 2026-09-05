mod delta;
mod parse;

use parse::{parse_flat_counters, parse_pressure, required, CpuCounters};
use std::error::Error;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemoryEvents {
    pub low: u64,
    pub high: u64,
    pub max: u64,
    pub oom: u64,
    pub oom_kill: u64,
    pub oom_group_kill: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemorySwapEvents {
    pub high: u64,
    pub max: u64,
    pub fail: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CgroupSnapshot {
    pub usage_usec: u64,
    pub user_usec: u64,
    pub system_usec: u64,
    pub nr_periods: u64,
    pub nr_throttled: u64,
    pub throttled_usec: u64,
    pub cpu_pressure_some_usec: u64,
    pub cpu_pressure_full_usec: u64,
    pub memory_current_bytes: u64,
    pub memory_peak_bytes: u64,
    pub memory_anon_bytes: u64,
    pub memory_file_bytes: u64,
    pub memory_kernel_bytes: u64,
    pub memory_sock_bytes: u64,
    pub memory_events: MemoryEvents,
    pub memory_swap_current_bytes: u64,
    pub memory_swap_peak_bytes: u64,
    pub memory_swap_events: MemorySwapEvents,
    pub cpuset_cpus_effective: String,
    pub cpu_max: String,
    pub memory_max: String,
    pub memory_swap_max: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CgroupDelta {
    pub usage_usec: u64,
    pub user_usec: u64,
    pub system_usec: u64,
    pub nr_periods: u64,
    pub nr_throttled: u64,
    pub throttled_usec: u64,
    pub cpu_pressure_some_usec: u64,
    pub cpu_pressure_full_usec: u64,
    pub memory_events: MemoryEvents,
    pub memory_swap_events: MemorySwapEvents,
}

#[derive(Debug, Clone)]
pub struct CgroupReader {
    dir: PathBuf,
}

#[derive(Debug)]
pub enum CgroupError {
    Io {
        path: PathBuf,
        source: io::Error,
    },
    InvalidInteger {
        field: String,
        value: String,
    },
    MissingField(&'static str),
    CounterRollback {
        field: &'static str,
        earlier: u64,
        later: u64,
    },
    NotV2,
}

impl fmt::Display for CgroupError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => {
                write!(formatter, "failed to read {}: {source}", path.display())
            }
            Self::InvalidInteger { field, value } => {
                write!(formatter, "invalid integer for {field}: {value}")
            }
            Self::MissingField(field) => write!(formatter, "missing cgroup field {field}"),
            Self::CounterRollback {
                field,
                earlier,
                later,
            } => write!(
                formatter,
                "cgroup counter {field} went backwards from {earlier} to {later}"
            ),
            Self::NotV2 => write!(formatter, "no cgroup v2 entry found"),
        }
    }
}

impl Error for CgroupError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

impl CgroupReader {
    pub fn from_proc_cgroup_content(content: &str, mount: &Path) -> Result<Self, CgroupError> {
        let relative = content
            .lines()
            .find_map(|line| line.strip_prefix("0::"))
            .ok_or(CgroupError::NotV2)?
            .trim_start_matches('/');
        Ok(Self::at_dir(mount.join(relative)))
    }

    pub fn for_pid(pid: u32, proc_root: &Path, mount: &Path) -> Result<Self, CgroupError> {
        let content = read_file(&proc_root.join(pid.to_string()).join("cgroup"))?;
        Self::from_proc_cgroup_content(&content, mount)
    }

    pub fn at_dir(dir: PathBuf) -> Self {
        Self { dir }
    }

    pub fn read_memory_current(&self) -> Result<u64, CgroupError> {
        read_integer_file(&self.dir.join("memory.current"))
    }

    pub fn snapshot(&self) -> Result<CgroupSnapshot, CgroupError> {
        let cpu = CpuCounters::parse(&read_file(&self.dir.join("cpu.stat"))?)?;
        let pressure = parse_pressure(&read_file(&self.dir.join("cpu.pressure"))?)?;
        let stat = parse_flat_counters(&read_file(&self.dir.join("memory.stat"))?)?;
        let events = parse_flat_counters(&read_file(&self.dir.join("memory.events"))?)?;
        let swap_events = parse_flat_counters(&read_file(&self.dir.join("memory.swap.events"))?)?;
        Ok(CgroupSnapshot {
            usage_usec: cpu.usage_usec,
            user_usec: cpu.user_usec,
            system_usec: cpu.system_usec,
            nr_periods: cpu.nr_periods,
            nr_throttled: cpu.nr_throttled,
            throttled_usec: cpu.throttled_usec,
            cpu_pressure_some_usec: pressure.0,
            cpu_pressure_full_usec: pressure.1,
            memory_current_bytes: self.read_memory_current()?,
            memory_peak_bytes: read_integer_file(&self.dir.join("memory.peak"))?,
            memory_anon_bytes: required(&stat, "anon")?,
            memory_file_bytes: required(&stat, "file")?,
            memory_kernel_bytes: required(&stat, "kernel")?,
            memory_sock_bytes: required(&stat, "sock")?,
            memory_events: MemoryEvents::parse(&events)?,
            memory_swap_current_bytes: read_integer_file(&self.dir.join("memory.swap.current"))?,
            memory_swap_peak_bytes: read_integer_file(&self.dir.join("memory.swap.peak"))?,
            memory_swap_events: MemorySwapEvents::parse(&swap_events)?,
            cpuset_cpus_effective: read_trimmed(&self.dir.join("cpuset.cpus.effective"))?,
            cpu_max: read_trimmed(&self.dir.join("cpu.max"))?,
            memory_max: read_trimmed(&self.dir.join("memory.max"))?,
            memory_swap_max: read_trimmed(&self.dir.join("memory.swap.max"))?,
        })
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }
}

impl MemoryEvents {
    fn parse(values: &std::collections::HashMap<String, u64>) -> Result<Self, CgroupError> {
        Ok(Self {
            low: required(values, "low")?,
            high: required(values, "high")?,
            max: required(values, "max")?,
            oom: required(values, "oom")?,
            oom_kill: required(values, "oom_kill")?,
            oom_group_kill: required(values, "oom_group_kill")?,
        })
    }
}

impl MemorySwapEvents {
    fn parse(values: &std::collections::HashMap<String, u64>) -> Result<Self, CgroupError> {
        Ok(Self {
            high: required(values, "high")?,
            max: required(values, "max")?,
            fail: required(values, "fail")?,
        })
    }
}

fn read_file(path: &Path) -> Result<String, CgroupError> {
    std::fs::read_to_string(path).map_err(|source| CgroupError::Io {
        path: path.to_path_buf(),
        source,
    })
}

fn read_trimmed(path: &Path) -> Result<String, CgroupError> {
    Ok(read_file(path)?.trim().to_owned())
}

fn read_integer_file(path: &Path) -> Result<u64, CgroupError> {
    parse::parse_integer(&path.display().to_string(), read_file(path)?.trim())
}
