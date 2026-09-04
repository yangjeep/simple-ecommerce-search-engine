use std::hint::black_box;
use std::time::Instant;

pub const MIN_FLOOR_MULTIPLE: f64 = 100.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TimerFloor {
    pub clock_resolution_ns: f64,
    pub instant_now_overhead_ns: f64,
}

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

fn effective_floor(floor: &TimerFloor) -> f64 {
    floor.clock_resolution_ns.max(floor.instant_now_overhead_ns)
}
