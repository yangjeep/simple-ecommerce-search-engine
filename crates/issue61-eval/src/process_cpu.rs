use nix::sys::resource::{getrusage, UsageWho};
use serde::{Deserialize, Serialize};
use std::error::Error;
use std::fmt;

const USEC_PER_SEC: u64 = 1_000_000;
const MAX_DISAGREEMENT_PERCENT: u128 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProcessCpuSnapshot {
    pub pid: u32,
    pub user_usec: u64,
    pub system_usec: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcessCpuDelta {
    pub pid: u32,
    pub user_usec: u64,
    pub system_usec: u64,
    pub total_usec: u64,
}

#[derive(Debug)]
pub enum ProcessCpuError {
    Getrusage(nix::Error),
    InvalidTimeval {
        field: &'static str,
        value: i64,
    },
    CounterRollback {
        field: &'static str,
        earlier: u64,
        later: u64,
    },
    PidChanged {
        earlier: u32,
        later: u32,
    },
    Overflow,
    ZeroCgroupCpu,
    Disagreement {
        process_usec: u64,
        cgroup_usec: u64,
    },
}

impl ProcessCpuSnapshot {
    #[must_use]
    pub const fn new(pid: u32, user_usec: u64, system_usec: u64) -> Self {
        Self {
            pid,
            user_usec,
            system_usec,
        }
    }

    pub fn capture_self() -> Result<Self, ProcessCpuError> {
        let usage = getrusage(UsageWho::RUSAGE_SELF).map_err(ProcessCpuError::Getrusage)?;
        let user = usage.user_time();
        let system = usage.system_time();
        Self::from_timevals(
            std::process::id(),
            (user.tv_sec(), user.tv_usec()),
            (system.tv_sec(), system.tv_usec()),
        )
    }

    pub fn from_timevals(
        pid: u32,
        user: (i64, i64),
        system: (i64, i64),
    ) -> Result<Self, ProcessCpuError> {
        Ok(Self::new(
            pid,
            timeval_to_usec("user", user.0, user.1)?,
            timeval_to_usec("system", system.0, system.1)?,
        ))
    }

    pub fn delta_since(self, earlier: &Self) -> Result<ProcessCpuDelta, ProcessCpuError> {
        if self.pid != earlier.pid {
            return Err(ProcessCpuError::PidChanged {
                earlier: earlier.pid,
                later: self.pid,
            });
        }
        let user_usec = checked_delta("user_usec", earlier.user_usec, self.user_usec)?;
        let system_usec = checked_delta("system_usec", earlier.system_usec, self.system_usec)?;
        let total_usec = user_usec
            .checked_add(system_usec)
            .ok_or(ProcessCpuError::Overflow)?;
        Ok(ProcessCpuDelta {
            pid: self.pid,
            user_usec,
            system_usec,
            total_usec,
        })
    }
}

impl ProcessCpuDelta {
    pub fn reconcile_cgroup(self, cgroup_usec: u64) -> Result<f64, ProcessCpuError> {
        if cgroup_usec == 0 {
            return Err(ProcessCpuError::ZeroCgroupCpu);
        }
        let difference = self.total_usec.abs_diff(cgroup_usec);
        if u128::from(difference) * 100 > u128::from(cgroup_usec) * MAX_DISAGREEMENT_PERCENT {
            return Err(ProcessCpuError::Disagreement {
                process_usec: self.total_usec,
                cgroup_usec,
            });
        }
        Ok(difference as f64 * 100.0 / cgroup_usec as f64)
    }
}

fn timeval_to_usec(
    field: &'static str,
    seconds: i64,
    microseconds: i64,
) -> Result<u64, ProcessCpuError> {
    if seconds < 0 || !(0..1_000_000).contains(&microseconds) {
        return Err(ProcessCpuError::InvalidTimeval {
            field,
            value: if seconds < 0 { seconds } else { microseconds },
        });
    }
    u64::try_from(seconds)
        .map_err(|_| ProcessCpuError::InvalidTimeval {
            field,
            value: seconds,
        })?
        .checked_mul(USEC_PER_SEC)
        .and_then(|value| value.checked_add(u64::try_from(microseconds).ok()?))
        .ok_or(ProcessCpuError::Overflow)
}

fn checked_delta(field: &'static str, earlier: u64, later: u64) -> Result<u64, ProcessCpuError> {
    later
        .checked_sub(earlier)
        .ok_or(ProcessCpuError::CounterRollback {
            field,
            earlier,
            later,
        })
}

impl fmt::Display for ProcessCpuError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Getrusage(source) => write!(formatter, "getrusage(RUSAGE_SELF) failed: {source}"),
            Self::InvalidTimeval { field, value } => {
                write!(formatter, "invalid {field} timeval component {value}")
            }
            Self::CounterRollback {
                field,
                earlier,
                later,
            } => write!(
                formatter,
                "process counter {field} went backwards from {earlier} to {later}"
            ),
            Self::PidChanged { earlier, later } => {
                write!(formatter, "native PID changed from {earlier} to {later}")
            }
            Self::Overflow => write!(formatter, "process CPU microseconds overflowed"),
            Self::ZeroCgroupCpu => write!(formatter, "cgroup CPU delta is zero"),
            Self::Disagreement {
                process_usec,
                cgroup_usec,
            } => write!(
                formatter,
                "process CPU {process_usec} us disagrees with cgroup CPU {cgroup_usec} us by more than 2%"
            ),
        }
    }
}

impl Error for ProcessCpuError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Getrusage(source) => Some(source),
            Self::InvalidTimeval { .. }
            | Self::CounterRollback { .. }
            | Self::PidChanged { .. }
            | Self::Overflow
            | Self::ZeroCgroupCpu
            | Self::Disagreement { .. } => None,
        }
    }
}
