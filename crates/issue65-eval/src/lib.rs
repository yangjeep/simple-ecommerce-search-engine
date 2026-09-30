//! Issue #65 (Infra E5, amendment 1): mixed-workload total-serving-system
//! capacity. Experiment code only; never a product dependency.
//!
//! - [`server`]: N1, the minimal concurrent native harness and H1 router.
//! - [`workload`]: frozen A-F pools, expectations, mixes, sequences.
//! - [`observe`]: normalizes native and Solr responses and checks them
//!   against expectations (validation pre-pass and in-load checks).
//! - [`cgroup`]: serving-budget CPU / memory counters.

pub mod cgroup;
pub mod observe;
pub mod server;
pub mod workload;

pub const EXPERIMENT_ID: &str = "I65-E5";
pub const RAW_SCHEMA_VERSION: u32 = 1;
/// The frozen #79 planner constants.
pub const TAU_F: f64 = 919.497_239_065_769_5;
pub const RHO_S: f64 = 0.083_377_913_197_190_3;
