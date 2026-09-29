//! Issue #63 (amendment 1, section 2 "Ambient record"): a fixed-work host
//! probe. A deterministic integer loop (xorshift over a 64 KiB table, so it
//! exercises the ALU and L1/L2 but no memory bandwidth) runs a fixed number
//! of iterations; the thread CPU and wall time are printed as one JSON line.
//! Disclosure only: no measurement is ever corrected with it.

use std::hint::black_box;
use std::time::Instant;

use issue63_eval::bench::thread_cpu_ns;

const ITERATIONS: u64 = 200_000_000;

fn main() {
    let mut table = vec![0u64; 8192];
    let mut x: u64 = 0x9E37_79B9_7F4A_7C15;
    let wall = Instant::now();
    let cpu = thread_cpu_ns();
    for i in 0..ITERATIONS {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        let slot = (x as usize) & (table.len() - 1);
        table[slot] = table[slot].wrapping_add(i);
    }
    let cpu_ns = thread_cpu_ns() - cpu;
    let wall_ns = wall.elapsed().as_nanos();
    let checksum = black_box(table.iter().fold(x, |a, b| a ^ b));
    println!(
        "{}",
        serde_json::json!({
            "probe": "xorshift_table_64k",
            "iterations": ITERATIONS,
            "cpu_ns": cpu_ns,
            "wall_ns": wall_ns,
            "checksum": checksum,
        })
    );
}
