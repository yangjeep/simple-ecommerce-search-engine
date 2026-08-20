use rand::seq::SliceRandom;
use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;

/// Run `f` `warmup` times and discard the results, then run it again and
/// return whatever the final call produced. Use before a timed section
/// that has any warm-state dependency (JIT-equivalent branch prediction/
/// cache warmup, OS page cache, Tantivy reader segment caching) --
/// everything this project's real-data experiments touch.
pub fn warmup_then<T>(warmup: usize, mut f: impl FnMut() -> T) -> T {
    for _ in 0..warmup.saturating_sub(1) {
        f();
    }
    f()
}

/// Time `f` `reps` times (after `warmup` untimed calls) and return the
/// raw elapsed-nanosecond sample for every timed rep -- never just a
/// summary. Per Issue #6: >=10 reps while exploring, >=30 for any number
/// that ends up in an architecture decision or a paper table. This
/// function does not enforce either minimum (callers decide, since
/// exploration vs. decision-grade is a per-experiment judgment); pass the
/// count the calling experiment's own stage in the loop calls for.
pub fn measured_repeat(warmup: usize, reps: usize, mut f: impl FnMut()) -> Vec<f64> {
    for _ in 0..warmup {
        f();
    }
    let mut samples = Vec::with_capacity(reps);
    for _ in 0..reps {
        let start = std::time::Instant::now();
        f();
        samples.push(start.elapsed().as_secs_f64() * 1000.0); // ms
    }
    samples
}

/// A balanced, seed-deterministic execution order across `method_count`
/// methods, `reps_per_method` reps each: one full pass over all methods
/// (in a freshly shuffled order) per "round," `reps_per_method` rounds
/// total. Interleaving methods this way -- rather than running method 0's
/// reps back-to-back, then method 1's -- prevents a slow monotonic drift
/// in shared system state (thermal throttling, memory fragmentation,
/// cache pressure from a growing process) from silently favoring
/// whichever method happens to run first or last. Returns a flat sequence
/// of method indices (`0..method_count`) for the caller to iterate;
/// deterministic given the same `seed` so a run is a genuine replication,
/// not a coincidence.
pub fn round_robin_schedule(method_count: usize, reps_per_method: usize, seed: u64) -> Vec<usize> {
    let mut rng = ChaCha8Rng::seed_from_u64(seed);
    let mut schedule = Vec::with_capacity(method_count * reps_per_method);
    for _ in 0..reps_per_method {
        let mut round: Vec<usize> = (0..method_count).collect();
        round.shuffle(&mut rng);
        schedule.extend(round);
    }
    schedule
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn warmup_then_discards_every_call_but_the_last() {
        let calls = Cell::new(0);
        let result = warmup_then(5, || {
            calls.set(calls.get() + 1);
            calls.get()
        });
        assert_eq!(calls.get(), 5, "warmup=5 must make exactly 5 calls total");
        assert_eq!(result, 5, "the returned value must be the final call's");
    }

    #[test]
    fn warmup_then_with_zero_warmup_still_calls_once() {
        let calls = Cell::new(0);
        let result = warmup_then(0, || {
            calls.set(calls.get() + 1);
            calls.get()
        });
        assert_eq!(calls.get(), 1);
        assert_eq!(result, 1);
    }

    #[test]
    fn measured_repeat_produces_exactly_reps_samples_after_warmup() {
        let calls = Cell::new(0);
        let samples = measured_repeat(3, 10, || {
            calls.set(calls.get() + 1);
        });
        assert_eq!(calls.get(), 13, "3 warmup + 10 measured = 13 total calls");
        assert_eq!(samples.len(), 10);
        assert!(samples.iter().all(|&s| s >= 0.0));
    }

    #[test]
    fn round_robin_schedule_visits_every_method_exactly_reps_per_method_times() {
        let schedule = round_robin_schedule(4, 30, 7);
        assert_eq!(schedule.len(), 120);
        let mut counts = [0usize; 4];
        for &m in &schedule {
            counts[m] += 1;
        }
        assert_eq!(counts, [30, 30, 30, 30]);
    }

    #[test]
    fn round_robin_schedule_is_deterministic_given_the_same_seed() {
        let a = round_robin_schedule(5, 10, 42);
        let b = round_robin_schedule(5, 10, 42);
        assert_eq!(a, b);
    }

    #[test]
    fn round_robin_schedule_does_not_run_one_method_back_to_back_across_every_round() {
        // Not a proof of good interleaving, just a real regression guard:
        // with 4 distinct methods and 30 rounds, the schedule should not
        // degenerate into every round happening to start with method 0
        // (which would defeat the anti-drift purpose of shuffling).
        let schedule = round_robin_schedule(4, 30, 7);
        let first_of_each_round: Vec<usize> = schedule.chunks(4).map(|c| c[0]).collect();
        let all_same_start = first_of_each_round
            .iter()
            .all(|&m| m == first_of_each_round[0]);
        assert!(
            !all_same_start,
            "seed=7 shuffling degenerated to a fixed method always starting each round"
        );
    }
}
