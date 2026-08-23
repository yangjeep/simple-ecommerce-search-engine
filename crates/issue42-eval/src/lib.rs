//! Issue #42: R1 (typed ambiguity/corroborated resolution), R2 (residual
//! lexical semantics), R3 (identifier serving primitive), and their shared
//! infrastructure. See `docs/experiments/ISSUE42_PROTOCOL.md` for the full
//! preregistered hypotheses, treatments, workloads, and GO-gate thresholds
//! -- committed before any treatment implementation, per this crate's own
//! "do not trust the experiment author" governance.
//!
//! `oracle` is the one module every R1/R2/R3 metric is required to check
//! against: it is structurally independent of `issue38_e2e3_eval`'s own
//! `generate_workload`/`ground_truth` judgment maps, so a bug shared
//! between a catalog generator and its own labels cannot hide from it.

pub mod oracle;
pub mod r1_experimental;
pub mod r1_workload;
pub mod r2_experimental;
pub mod r2_workload;
pub mod r3_experimental;
pub mod r3_workload;
pub mod regression;
