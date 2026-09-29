//! Issue #63 (Infra E3, amendment 1): primitive CPU-efficiency and
//! bytes-touched microbenchmarks, plus the equal-work facet confirmation's
//! correctness check. Experiment code only; nothing here is a product
//! dependency.
//!
//! - [`alloc`]: a counting global allocator (allocations/op, bytes/op).
//! - [`bench`]: the #74-floor batch runner over thread CPU time.
//! - [`bytes`]: deterministic, analytical bytes-touched estimates.
//! - [`equivalence`]: normalizes engine facet dumps and compares them with
//!   the independent oracle, exactly.
//! - [`synthetic`]: the deterministic multi-variant expansion used for the
//!   same-variant conjunction primitive.

pub mod alloc;
pub mod bench;
pub mod bytes;
pub mod equivalence;
pub mod synthetic;

pub const EXPERIMENT_ID: &str = "I63-E3";
pub const RAW_SCHEMA_VERSION: u32 = 1;

/// The frozen #79 planner constants (`artifacts/issue79/results/planner_constants.json`).
pub const TAU_F: f64 = 919.497_239_065_769_5;
pub const RHO_S: f64 = 0.083_377_913_197_190_3;

/// Nominal clock of the measurement host (Xeon D-1518 @ 2.20 GHz). Cycle
/// figures are `ns * NOMINAL_GHZ` estimates, never hardware counters
/// (`perf_event_paranoid=4` on this host).
pub const NOMINAL_GHZ: f64 = 2.2;
