//! Shared long-running resident-process sampling primitives. Used by
//! P7-E04 (H7: does a live-serving process's RSS materially exceed a
//! spawn-and-exit snapshot?) and P7-E05 (H8: does that growth plateau
//! over a much longer resident window, or does it keep climbing?). Both
//! experiments spawn a child process (via `std::process::Command`) that
//! either sits idle-resident or actively serves real structural queries
//! for a configurable duration, printing periodic RSS samples to stdout
//! so the calling orchestrator process can parse and compare the growth
//! curve -- see `docs/experiments/PHASE7_LOG.md`'s "P7-E04"/"P7-E05"
//! sections for the falsifiable hypotheses stated before either binary
//! was written.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use commerce_core::domain::Catalog;
use commerce_core::index::CatalogIndex;

pub fn current_rss_kb() -> u64 {
    let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    status
        .lines()
        .find_map(|line| line.strip_prefix("VmRSS:"))
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(|kb| kb.parse().ok())
        .unwrap_or(0)
}

pub fn facet_scan_once(index: &CatalogIndex, catalog: &Catalog) -> usize {
    let all = index.indexed_candidates(&[]);
    index.facet_counts_by_scan(&all, catalog, "color").len()
}

/// Sample RSS every `sample_interval` until `run_duration` elapses,
/// printing each sample as it's taken so the growth curve (or lack of
/// one) is visible in the child's own stdout, not just a final number.
pub fn sample_rss_for(run_duration: Duration, sample_interval: Duration) {
    let start = Instant::now();
    let mut elapsed_marker = sample_interval;
    while start.elapsed() < run_duration {
        let remaining = run_duration.saturating_sub(start.elapsed());
        std::thread::sleep(sample_interval.min(remaining));
        if start.elapsed() >= elapsed_marker || start.elapsed() >= run_duration {
            let secs = elapsed_marker.as_secs().min(run_duration.as_secs());
            println!("rss_sample_t{secs}s_kb={}", current_rss_kb());
            elapsed_marker += sample_interval;
        }
    }
}

/// idle-resident: no tenant data at all. `worker_threads` real OS
/// threads are spawned and parked alive for the whole window (a resident
/// but idle worker/connection-handler pool).
pub fn run_idle_resident(worker_threads: usize, run_duration: Duration, sample_interval: Duration) {
    let stop = Arc::new(AtomicBool::new(false));
    let mut handles = Vec::new();
    for _ in 0..worker_threads {
        let stop = Arc::clone(&stop);
        handles.push(std::thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(50));
            }
        }));
    }
    sample_rss_for(run_duration, sample_interval);
    stop.store(true, Ordering::Relaxed);
    for h in handles {
        h.join().unwrap();
    }
}

/// active-resident: `worker_threads` real OS threads continuously
/// execute real structural queries against the one loaded tenant's index
/// for the whole window, simulating a service actually serving sustained
/// traffic rather than sitting idle. Returns total queries served.
pub fn run_active_resident(
    index: Arc<CatalogIndex>,
    catalog: Arc<Catalog>,
    worker_threads: usize,
    run_duration: Duration,
    sample_interval: Duration,
) -> u64 {
    let stop = Arc::new(AtomicBool::new(false));
    let total = Arc::new(AtomicU64::new(0));
    let mut handles = Vec::new();
    for _ in 0..worker_threads {
        let index = Arc::clone(&index);
        let catalog = Arc::clone(&catalog);
        let stop = Arc::clone(&stop);
        let total = Arc::clone(&total);
        handles.push(std::thread::spawn(move || {
            let mut count = 0u64;
            while !stop.load(Ordering::Relaxed) {
                std::hint::black_box(facet_scan_once(&index, &catalog));
                count += 1;
            }
            total.fetch_add(count, Ordering::Relaxed);
        }));
    }
    sample_rss_for(run_duration, sample_interval);
    stop.store(true, Ordering::Relaxed);
    for h in handles {
        h.join().unwrap();
    }
    total.load(Ordering::Relaxed)
}

/// (elapsed_seconds, rss_kb) samples parsed from a child's stdout.
pub fn parse_samples(stdout: &str) -> Vec<(u64, u64)> {
    let mut samples = Vec::new();
    for line in stdout.lines() {
        if let Some(rest) = line.strip_prefix("rss_sample_t") {
            if let Some((secs_str, kb_str)) = rest.split_once("s_kb=") {
                if let (Ok(secs), Ok(kb)) = (secs_str.parse(), kb_str.parse()) {
                    samples.push((secs, kb));
                }
            }
        }
    }
    samples
}

pub fn peak_kb(samples: &[(u64, u64)]) -> u64 {
    samples.iter().map(|&(_, kb)| kb).max().unwrap_or(0)
}

pub fn fmt_samples(samples: &[(u64, u64)]) -> String {
    samples
        .iter()
        .map(|(secs, kb)| format!("t{secs}s={kb}KB"))
        .collect::<Vec<_>>()
        .join(" ")
}
