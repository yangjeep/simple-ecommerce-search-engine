use std::error::Error;
use std::fmt::{Display, Formatter};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NativePidIdentity {
    pub cgroup_host_pid: u32,
    pub pid_namespace: u32,
}

impl NativePidIdentity {
    pub fn read(
        proc_root: &Path,
        cgroup_host_pid: u32,
        endpoint_pid: u32,
    ) -> Result<Self, PidNamespaceError> {
        let status_path = proc_root.join(cgroup_host_pid.to_string()).join("status");
        let status = std::fs::read_to_string(&status_path).map_err(|source| {
            PidNamespaceError::ReadStatus {
                path: status_path.display().to_string(),
                source,
            }
        })?;
        let pid_namespace = parse_namespace_pid(&status)?;
        if pid_namespace != endpoint_pid {
            return Err(PidNamespaceError::EndpointMismatch {
                mapped: pid_namespace,
                endpoint: endpoint_pid,
            });
        }
        Ok(Self {
            cgroup_host_pid,
            pid_namespace,
        })
    }

    pub fn ensure_stable(&self, later: &Self) -> Result<(), PidNamespaceError> {
        if self == later {
            return Ok(());
        }
        Err(PidNamespaceError::Changed {
            before_host: self.cgroup_host_pid,
            after_host: later.cgroup_host_pid,
            before_namespace: self.pid_namespace,
            after_namespace: later.pid_namespace,
        })
    }
}

fn parse_namespace_pid(status: &str) -> Result<u32, PidNamespaceError> {
    let mut matches = status
        .lines()
        .filter_map(|line| line.strip_prefix("NSpid:"));
    let values = matches.next().ok_or(PidNamespaceError::MissingNSpid)?;
    if matches.next().is_some() {
        return Err(PidNamespaceError::DuplicateNSpid);
    }
    let pids = values
        .split_ascii_whitespace()
        .map(str::parse::<u32>)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| PidNamespaceError::MalformedNSpid)?;
    pids.last().copied().ok_or(PidNamespaceError::EmptyNSpid)
}

#[derive(Debug)]
pub enum PidNamespaceError {
    ReadStatus {
        path: String,
        source: std::io::Error,
    },
    MissingNSpid,
    DuplicateNSpid,
    EmptyNSpid,
    MalformedNSpid,
    EndpointMismatch {
        mapped: u32,
        endpoint: u32,
    },
    Changed {
        before_host: u32,
        after_host: u32,
        before_namespace: u32,
        after_namespace: u32,
    },
}

impl Display for PidNamespaceError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ReadStatus { path, source } => {
                write!(formatter, "failed to read process status {path}: {source}")
            }
            Self::MissingNSpid => write!(formatter, "process status is missing NSpid"),
            Self::DuplicateNSpid => write!(formatter, "process status contains duplicate NSpid"),
            Self::EmptyNSpid => write!(formatter, "process status contains an empty NSpid"),
            Self::MalformedNSpid => write!(formatter, "process status contains malformed NSpid"),
            Self::EndpointMismatch { mapped, endpoint } => write!(
                formatter,
                "mapped namespace PID {mapped} differs from endpoint PID {endpoint}"
            ),
            Self::Changed {
                before_host,
                after_host,
                before_namespace,
                after_namespace,
            } => write!(
                formatter,
                "native PID identity changed from host={before_host},namespace={before_namespace} to host={after_host},namespace={after_namespace}"
            ),
        }
    }
}

impl Error for PidNamespaceError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::ReadStatus { source, .. } => Some(source),
            _ => None,
        }
    }
}
