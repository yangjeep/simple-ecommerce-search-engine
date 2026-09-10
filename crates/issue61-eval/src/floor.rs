use std::hint::black_box;
use std::time::Instant;
use std::{error::Error, fmt};

pub const MIN_FLOOR_MULTIPLE: f64 = 100.0;
pub const MIN_CGROUP_CPU_USEC: u64 = 1_000;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TimerFloor {
    pub clock_resolution_ns: f64,
    pub instant_now_overhead_ns: f64,
}

impl TimerFloor {
    pub fn effective_ns(&self) -> f64 {
        self.clock_resolution_ns.max(self.instant_now_overhead_ns)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MeasurementFloorError {
    WallBelowTimerFloor {
        measured_ns: u64,
        required_ns: f64,
    },
    CpuBelowCgroupFloor {
        measured_usec: u64,
        required_usec: u64,
    },
}

impl fmt::Display for MeasurementFloorError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WallBelowTimerFloor {
                measured_ns,
                required_ns,
            } => write!(
                formatter,
                "wall interval {measured_ns}ns is below timer floor {required_ns}ns"
            ),
            Self::CpuBelowCgroupFloor {
                measured_usec,
                required_usec,
            } => write!(
                formatter,
                "cgroup CPU interval {measured_usec}us is below floor {required_usec}us"
            ),
        }
    }
}

impl Error for MeasurementFloorError {}

pub fn measure_timer_floor(samples: usize) -> TimerFloor {
    assert!(samples > 0, "timer-floor calibration needs samples");

    let overhead_start = Instant::now();
    for _ in 0..samples {
        black_box(Instant::now());
    }
    let instant_now_overhead_ns =
        (overhead_start.elapsed().as_secs_f64() * 1_000_000_000.0 / samples as f64).max(1.0);

    let mut previous = Instant::now();
    let mut clock_resolution_ns = f64::INFINITY;
    for _ in 0..samples {
        let current = Instant::now();
        let delta_ns = current.duration_since(previous).as_secs_f64() * 1_000_000_000.0;
        if delta_ns > 0.0 {
            clock_resolution_ns = clock_resolution_ns.min(delta_ns);
        }
        previous = current;
    }
    if !clock_resolution_ns.is_finite() {
        clock_resolution_ns = instant_now_overhead_ns;
    }

    TimerFloor {
        clock_resolution_ns,
        instant_now_overhead_ns,
    }
}

pub fn is_above_floor(measured_ns: f64, floor: &TimerFloor) -> bool {
    measured_ns >= effective_floor(floor) * MIN_FLOOR_MULTIPLE
}

pub fn required_batch_size(est_op_ns: f64, floor: &TimerFloor) -> usize {
    ((effective_floor(floor) * MIN_FLOOR_MULTIPLE / est_op_ns).ceil() as usize).max(1)
}

pub fn validate_measurement_window(
    wall_elapsed_ns: u64,
    cpu_usage_usec: u64,
    floor: &TimerFloor,
) -> Result<(), MeasurementFloorError> {
    let required_ns = floor.effective_ns() * MIN_FLOOR_MULTIPLE;
    if (wall_elapsed_ns as f64) < required_ns {
        return Err(MeasurementFloorError::WallBelowTimerFloor {
            measured_ns: wall_elapsed_ns,
            required_ns,
        });
    }
    if cpu_usage_usec < MIN_CGROUP_CPU_USEC {
        return Err(MeasurementFloorError::CpuBelowCgroupFloor {
            measured_usec: cpu_usage_usec,
            required_usec: MIN_CGROUP_CPU_USEC,
        });
    }
    Ok(())
}

fn effective_floor(floor: &TimerFloor) -> f64 {
    floor.effective_ns()
}
