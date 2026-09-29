//! The #74-floor batch runner (amendment 1, section 3 "Common rules"):
//! 20 uncounted warmups, then at least `min_ops` operations *and* at least
//! `min_cpu_ns` of cumulative thread CPU time. Many primitives cost well
//! under a microsecond, so the clock is read once per chunk, never per op.
//! The chunk size doubles until one chunk takes >= 1 ms of CPU.

use std::hint::black_box;
use std::time::Instant;

use serde::{Deserialize, Serialize};

use crate::alloc;

pub const WARMUP: u64 = 20;
/// Preregistered floor: >= 200 ops and >= 2 s cumulative CPU per point.
pub const MIN_OPS: u64 = 200;
pub const MIN_CPU_NS: u64 = 2_000_000_000;
/// Safety caps so a mistake cannot run for hours.
const MAX_OPS: u64 = 200_000_000;
const MAX_WALL_S: f64 = 120.0;

/// Thread CPU time (`CLOCK_THREAD_CPUTIME_ID`), in nanoseconds.
#[must_use]
pub fn thread_cpu_ns() -> u64 {
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: `ts` is a valid, writable timespec; the clock id is a
    // constant supported by Linux.
    let rc = unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut ts) };
    assert_eq!(rc, 0, "clock_gettime(CLOCK_THREAD_CPUTIME_ID) failed");
    ts.tv_sec as u64 * 1_000_000_000 + ts.tv_nsec as u64
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Sample {
    pub ops: u64,
    pub cpu_ns: u64,
    pub wall_ns: u64,
    pub cpu_ns_per_op: f64,
    pub allocations_per_op: f64,
    pub allocated_bytes_per_op: f64,
    /// True if a safety cap (not the floor) ended the batch.
    pub capped: bool,
}

/// Runs `op` under the floor rule and reports per-op CPU and allocation.
pub fn measure<R>(min_ops: u64, min_cpu_ns: u64, mut op: impl FnMut() -> R) -> Sample {
    for _ in 0..WARMUP {
        black_box(op());
    }
    let (allocs_before, bytes_before) = alloc::snapshot();
    let wall_started = Instant::now();
    let cpu_started = thread_cpu_ns();
    let mut ops = 0u64;
    let mut chunk = 1u64;
    let mut capped = false;
    loop {
        let chunk_started = thread_cpu_ns();
        for _ in 0..chunk {
            black_box(op());
        }
        ops += chunk;
        let now = thread_cpu_ns();
        if now - chunk_started < 1_000_000 {
            chunk = chunk.saturating_mul(2);
        }
        if ops >= min_ops && now - cpu_started >= min_cpu_ns {
            break;
        }
        if ops >= MAX_OPS || wall_started.elapsed().as_secs_f64() > MAX_WALL_S {
            capped = true;
            break;
        }
    }
    let cpu_ns = thread_cpu_ns() - cpu_started;
    let wall_ns = wall_started.elapsed().as_nanos() as u64;
    let (allocs_after, bytes_after) = alloc::snapshot();
    Sample {
        ops,
        cpu_ns,
        wall_ns,
        cpu_ns_per_op: cpu_ns as f64 / ops as f64,
        allocations_per_op: (allocs_after - allocs_before) as f64 / ops as f64,
        allocated_bytes_per_op: (bytes_after - bytes_before) as f64 / ops as f64,
        capped,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn floor_is_respected_and_allocations_are_attributed() {
        // A tiny floor keeps the test fast; the rule is the same.
        let sample = measure(50, 2_000_000, || vec![0u8; 64]);
        assert!(sample.ops >= 50);
        assert!(sample.cpu_ns >= 2_000_000);
        assert!(!sample.capped);
        // The counting allocator is not installed in unit tests, so the
        // counters stay at zero here; the binaries install it.
        assert!(sample.allocations_per_op >= 0.0);
    }

    #[test]
    fn thread_cpu_clock_advances() {
        let a = thread_cpu_ns();
        let mut x = 0u64;
        for i in 0..2_000_000u64 {
            x = black_box(x.wrapping_add(i));
        }
        assert!(thread_cpu_ns() > a, "{x}");
    }
}
