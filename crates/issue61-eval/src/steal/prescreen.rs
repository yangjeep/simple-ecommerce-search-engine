use super::{read_stat_file, CpuTimes, StealError};
use std::path::Path;
use std::time::{Duration, Instant};

pub const STEAL_PROBE_DURATION: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CpuSet {
    cpus: Vec<u32>,
}

#[derive(Debug, Clone, Copy)]
pub struct StealProbeConfig<'a> {
    pub cpus: &'a CpuSet,
    pub duration: Duration,
}

impl<'a> StealProbeConfig<'a> {
    pub const fn with_default_duration(cpus: &'a CpuSet) -> Self {
        Self {
            cpus,
            duration: STEAL_PROBE_DURATION,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StealProbeResult {
    pub total_delta_jiffies: u64,
    pub steal_delta_jiffies: u64,
    pub elapsed: Duration,
    pub rejected: bool,
}

impl StealProbeResult {
    #[must_use]
    pub fn steal_percent(self) -> f64 {
        self.steal_delta_jiffies as f64 / self.total_delta_jiffies as f64 * 100.0
    }
}

impl CpuSet {
    pub fn parse(content: &str) -> Result<Self, StealError> {
        let content = content.trim();
        if content.is_empty() {
            return Err(StealError::InvalidCpuSet(content.to_owned()));
        }
        let mut cpus = std::collections::BTreeSet::new();
        for member in content.split(',') {
            let (start, end) = match member.split_once('-') {
                Some((start, end)) if !end.contains('-') => {
                    (parse_cpu_id(start, content)?, parse_cpu_id(end, content)?)
                }
                Some(_) => return Err(StealError::InvalidCpuSet(content.to_owned())),
                None => {
                    let cpu = parse_cpu_id(member, content)?;
                    (cpu, cpu)
                }
            };
            if start > end {
                return Err(StealError::InvalidCpuSet(content.to_owned()));
            }
            for cpu in start..=end {
                if !cpus.insert(cpu) {
                    return Err(StealError::InvalidCpuSet(content.to_owned()));
                }
            }
        }
        Ok(Self {
            cpus: cpus.into_iter().collect(),
        })
    }

    pub fn as_slice(&self) -> &[u32] {
        &self.cpus
    }
}

fn parse_cpu_id(value: &str, cpuset: &str) -> Result<u32, StealError> {
    value
        .parse::<u32>()
        .map_err(|_| StealError::InvalidCpuSet(cpuset.to_owned()))
}

pub fn parse_assigned_proc_stat(content: &str, cpus: &CpuSet) -> Result<CpuTimes, StealError> {
    let mut seen = std::collections::BTreeSet::new();
    let mut total_jiffies = 0_u64;
    let mut steal_jiffies = 0_u64;
    for line in content.lines() {
        let mut fields = line.split_whitespace();
        let Some(cpu) = fields
            .next()
            .and_then(|label| label.strip_prefix("cpu"))
            .and_then(|id| id.parse::<u32>().ok())
        else {
            continue;
        };
        if cpus.cpus.binary_search(&cpu).is_err() {
            continue;
        }
        if !seen.insert(cpu) {
            return Err(StealError::InvalidSelectedCpu(cpu));
        }
        let mut values = [0_u64; 8];
        for value in &mut values {
            let raw = fields.next().ok_or(StealError::InvalidSelectedCpu(cpu))?;
            *value = raw
                .parse::<u64>()
                .map_err(|_| StealError::InvalidInteger(raw.to_owned()))?;
        }
        total_jiffies = values.iter().try_fold(total_jiffies, |total, value| {
            total
                .checked_add(*value)
                .ok_or(StealError::ArithmeticOverflow("total_jiffies"))
        })?;
        steal_jiffies = steal_jiffies
            .checked_add(values[7])
            .ok_or(StealError::ArithmeticOverflow("steal_jiffies"))?;
    }
    for cpu in cpus.as_slice() {
        if !seen.contains(cpu) {
            return Err(StealError::InvalidSelectedCpu(*cpu));
        }
    }
    Ok(CpuTimes {
        total_jiffies,
        steal_jiffies,
    })
}

pub fn assess_assigned_cpu_steal(
    earlier: CpuTimes,
    later: CpuTimes,
    elapsed: Duration,
) -> Result<StealProbeResult, StealError> {
    let total_delta_jiffies =
        checked_delta("total_jiffies", earlier.total_jiffies, later.total_jiffies)?;
    if total_delta_jiffies == 0 {
        return Err(StealError::ZeroTotalDelta);
    }
    let steal_delta_jiffies =
        checked_delta("steal_jiffies", earlier.steal_jiffies, later.steal_jiffies)?;
    let scaled_steal = steal_delta_jiffies
        .checked_mul(100)
        .ok_or(StealError::ArithmeticOverflow("steal threshold"))?;
    Ok(StealProbeResult {
        total_delta_jiffies,
        steal_delta_jiffies,
        elapsed,
        rejected: scaled_steal > total_delta_jiffies,
    })
}

fn checked_delta(field: &'static str, earlier: u64, later: u64) -> Result<u64, StealError> {
    later
        .checked_sub(earlier)
        .ok_or(StealError::CounterRollback {
            field,
            earlier,
            later,
        })
}

pub fn probe_assigned_cpu_steal(
    config: StealProbeConfig<'_>,
    mut read_stat: impl FnMut() -> Result<String, StealError>,
    wait: impl FnOnce(Duration),
) -> Result<StealProbeResult, StealError> {
    let earlier = parse_assigned_proc_stat(&read_stat()?, config.cpus)?;
    let started = Instant::now();
    wait(config.duration);
    let later = parse_assigned_proc_stat(&read_stat()?, config.cpus)?;
    assess_assigned_cpu_steal(earlier, later, started.elapsed())
}

pub fn run_assigned_cpu_steal_probe(
    proc_root: &Path,
    cpus: &CpuSet,
) -> Result<StealProbeResult, StealError> {
    let stat_path = proc_root.join("stat");
    probe_assigned_cpu_steal(
        StealProbeConfig::with_default_duration(cpus),
        || read_stat_file(&stat_path),
        std::thread::sleep,
    )
}
