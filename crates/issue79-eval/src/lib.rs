//! Issue #79 (Infra E3b) facet & sort physical-design recovery.
//! Experiment code only; never a dependency of product crates.
//!
//! - [`cells`]: the preregistered headline (held-out, E3) and calibration
//!   cells.
//! - [`plp`]: the single `/plp` executor serving every variant
//!   (N0'/F1/F2/F3 x legacy/S1/S2/S3), shared by the server and the gate.
//! - [`oracle`]: the independent `Catalog`-scan correctness oracle.

pub mod cells;
pub mod oracle;
pub mod plp;

pub const EXPERIMENT_ID: &str = "I79-E3b";
pub const RAW_SCHEMA_VERSION: u32 = 1;
