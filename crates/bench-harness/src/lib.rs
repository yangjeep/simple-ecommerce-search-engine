//! Issue #6's multi-round research campaign needs the same rigor applied
//! consistently across many experiments, not hand-rolled per binary: warm
//! up separately from measurement, repeat the *timed* operation enough
//! times to characterize run-to-run variance (not just query-to-query
//! variance within one run), report full percentile distributions rather
//! than a single number, preserve raw samples, and record enough context
//! (commit SHA, environment, dataset/query-set identity, config, seed) for
//! another run to actually be a replication rather than a different
//! experiment with the same name.
//!
//! **Deliberate scope boundary**: `commerce_core`'s query-compile/plan/
//! execute path is deterministic -- no model call, no randomness. That
//! means relevance/correctness/route-distribution/candidate-set-size
//! numbers computed by a real-data replay are already exactly
//! reproducible bit-for-bit given the same catalog/query/config inputs;
//! repeating that computation adds nothing (this is a real methodological
//! decision, recorded in `docs/research/PAPER_NOTES.md`'s methodology
//! section, not an oversight). Only **wall-clock timing** has genuine
//! run-to-run variance worth repeating (OS scheduling noise, cache state,
//! other processes on a shared machine). This crate's repetition/
//! distribution machinery is therefore aimed specifically at timing
//! measurements; correctness/relevance numbers are computed once and
//! trusted as exact.

mod bootstrap;
mod csv_out;
mod distribution;
mod manifest;
mod repeat;

pub use bootstrap::{bootstrap_ci_diff_of_means, BootstrapCi};
pub use csv_out::{append_raw_samples, append_summary_row};
pub use distribution::Distribution;
pub use manifest::RunManifest;
pub use repeat::{measured_repeat, round_robin_schedule, warmup_then};
