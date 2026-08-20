//! Phase 2 (Issue #6): validates ADR 0008's central bet -- that delegating
//! lexical retrieval/ranking to Tantivy recovers real relevance quality
//! `commerce_core` architecturally cannot produce on its own (R1-E04/E07).
//! Reuses `round1_eval`'s real-data loaders read-only; no changes to
//! `commerce_core` or `round1-eval` from this crate.
