use std::error::Error;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CgroupSnapshot {
    pub usage_usec: u64,
    pub user_usec: u64,
    pub system_usec: u64,
    pub memory_current_bytes: u64,
    pub memory_peak_bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CgroupDelta {
    pub usage_usec: u64,
    pub user_usec: u64,
    pub system_usec: u64,
}

#[derive(Debug)]
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
            Self::InvalidInteger { .. }
            | Self::MissingField(_)
            | Self::CounterRollback { .. }
            | Self::NotV2 => None,
        }
    }
}

impl CgroupReader {
    pub fn from_proc_cgroup_content(
        content: &str,
        cgroup_mount: &Path,
    ) -> Result<Self, CgroupError> {
        let relative = content
            .lines()
            .find_map(|line| line.strip_prefix("0::"))
            .ok_or(CgroupError::NotV2)?
            .trim_start_matches('/');
        Ok(Self::at_dir(cgroup_mount.join(relative)))
    }

    pub fn for_pid(pid: u32, proc_root: &Path, cgroup_mount: &Path) -> Result<Self, CgroupError> {
        let path = proc_root.join(pid.to_string()).join("cgroup");
        let content = read_file(&path)?;
        Self::from_proc_cgroup_content(&content, cgroup_mount)
    }

    pub fn at_dir(dir: PathBuf) -> Self {
        Self { dir }
    }

    /// Reads CPU counters and memory gauges from this cgroup. Kernels without
    /// `memory.peak` report zero so callers can record the gauge as unavailable.
    pub fn snapshot(&self) -> Result<CgroupSnapshot, CgroupError> {
        let cpu_path = self.dir.join("cpu.stat");
        let cpu_stat = read_file(&cpu_path)?;
        let (usage_usec, user_usec, system_usec) = parse_cpu_stat(&cpu_stat)?;
        let memory_current_bytes = read_integer_file(&self.dir.join("memory.current"))?;
        let peak_path = self.dir.join("memory.peak");
        let memory_peak_bytes = match std::fs::read_to_string(&peak_path) {
            Ok(value) => parse_integer("memory.peak", value.trim())?,
            Err(source) if source.kind() == io::ErrorKind::NotFound => 0,
            Err(source) => {
                return Err(CgroupError::Io {
                    path: peak_path,
                    source,
                })
            }
        };
        Ok(CgroupSnapshot {
            usage_usec,
            user_usec,
            system_usec,
            memory_current_bytes,
            memory_peak_bytes,
        })
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }
}

impl CgroupSnapshot {
    /// Rejects counter rollback because saturating to zero would convert a
    /// reset or wrong-cgroup read into a favourable zero-CPU measurement.
    pub const fn delta_since(&self, earlier: &Self) -> Result<CgroupDelta, CgroupError> {
        let usage_usec = match self.usage_usec.checked_sub(earlier.usage_usec) {
            Some(delta) => delta,
            None => {
                return Err(CgroupError::CounterRollback {
                    field: "usage_usec",
                    earlier: earlier.usage_usec,
                    later: self.usage_usec,
                })
            }
        };
        let user_usec = match self.user_usec.checked_sub(earlier.user_usec) {
            Some(delta) => delta,
            None => {
                return Err(CgroupError::CounterRollback {
                    field: "user_usec",
                    earlier: earlier.user_usec,
                    later: self.user_usec,
                })
            }
        };
        let system_usec = match self.system_usec.checked_sub(earlier.system_usec) {
            Some(delta) => delta,
            None => {
                return Err(CgroupError::CounterRollback {
                    field: "system_usec",
                    earlier: earlier.system_usec,
                    later: self.system_usec,
                })
            }
        };
        Ok(CgroupDelta {
            usage_usec,
            user_usec,
            system_usec,
        })
    }
}

fn read_file(path: &Path) -> Result<String, CgroupError> {
    std::fs::read_to_string(path).map_err(|source| CgroupError::Io {
        path: path.to_path_buf(),
        source,
    })
}

fn read_integer_file(path: &Path) -> Result<u64, CgroupError> {
    let content = read_file(path)?;
    parse_integer(&path.display().to_string(), content.trim())
}

fn parse_integer(field: &str, value: &str) -> Result<u64, CgroupError> {
    value
        .parse::<u64>()
        .map_err(|_| CgroupError::InvalidInteger {
            field: field.to_owned(),
            value: value.to_owned(),
        })
}

fn parse_cpu_stat(content: &str) -> Result<(u64, u64, u64), CgroupError> {
    let mut usage = None;
    let mut user = None;
    let mut system = None;
    for line in content.lines() {
        let mut fields = line.split_whitespace();
        let key = fields.next();
        let value = fields.next();
        match (key, value) {
            (Some("usage_usec"), Some(raw)) => usage = Some(parse_integer("usage_usec", raw)?),
            (Some("user_usec"), Some(raw)) => user = Some(parse_integer("user_usec", raw)?),
            (Some("system_usec"), Some(raw)) => system = Some(parse_integer("system_usec", raw)?),
            _ => {}
        }
    }
    Ok((
        usage.ok_or(CgroupError::MissingField("usage_usec"))?,
        user.ok_or(CgroupError::MissingField("user_usec"))?,
        system.ok_or(CgroupError::MissingField("system_usec"))?,
    ))
}
