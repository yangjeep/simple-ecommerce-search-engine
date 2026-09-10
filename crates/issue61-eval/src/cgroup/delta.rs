use super::{CgroupDelta, CgroupError, CgroupSnapshot, MemoryEvents, MemorySwapEvents};

impl CgroupSnapshot {
    pub fn delta_since(&self, earlier: &Self) -> Result<CgroupDelta, CgroupError> {
        Ok(CgroupDelta {
            usage_usec: delta("usage_usec", earlier.usage_usec, self.usage_usec)?,
            user_usec: delta("user_usec", earlier.user_usec, self.user_usec)?,
            system_usec: delta("system_usec", earlier.system_usec, self.system_usec)?,
            nr_periods: delta("nr_periods", earlier.nr_periods, self.nr_periods)?,
            nr_throttled: delta("nr_throttled", earlier.nr_throttled, self.nr_throttled)?,
            throttled_usec: delta(
                "throttled_usec",
                earlier.throttled_usec,
                self.throttled_usec,
            )?,
            cpu_pressure_some_usec: delta(
                "cpu.pressure some total",
                earlier.cpu_pressure_some_usec,
                self.cpu_pressure_some_usec,
            )?,
            cpu_pressure_full_usec: delta(
                "cpu.pressure full total",
                earlier.cpu_pressure_full_usec,
                self.cpu_pressure_full_usec,
            )?,
            memory_events: MemoryEvents {
                low: delta(
                    "memory.events low",
                    earlier.memory_events.low,
                    self.memory_events.low,
                )?,
                high: delta(
                    "memory.events high",
                    earlier.memory_events.high,
                    self.memory_events.high,
                )?,
                max: delta(
                    "memory.events max",
                    earlier.memory_events.max,
                    self.memory_events.max,
                )?,
                oom: delta(
                    "memory.events oom",
                    earlier.memory_events.oom,
                    self.memory_events.oom,
                )?,
                oom_kill: delta(
                    "memory.events oom_kill",
                    earlier.memory_events.oom_kill,
                    self.memory_events.oom_kill,
                )?,
                oom_group_kill: delta(
                    "memory.events oom_group_kill",
                    earlier.memory_events.oom_group_kill,
                    self.memory_events.oom_group_kill,
                )?,
            },
            memory_swap_events: MemorySwapEvents {
                high: delta(
                    "memory.swap.events high",
                    earlier.memory_swap_events.high,
                    self.memory_swap_events.high,
                )?,
                max: delta(
                    "memory.swap.events max",
                    earlier.memory_swap_events.max,
                    self.memory_swap_events.max,
                )?,
                fail: delta(
                    "memory.swap.events fail",
                    earlier.memory_swap_events.fail,
                    self.memory_swap_events.fail,
                )?,
            },
        })
    }
}

fn delta(field: &'static str, earlier: u64, later: u64) -> Result<u64, CgroupError> {
    later
        .checked_sub(earlier)
        .ok_or(CgroupError::CounterRollback {
            field,
            earlier,
            later,
        })
}
