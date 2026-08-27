//! Paired-comparison failure accounting. Every existing comparator
//! binary in this workspace picked, ad hoc, one of three behaviors when
//! a Solr lookup failed: silently score it as `0.0`
//! (`phase9-eval/p9_e02_wands_physical_advantage.rs`, the confirmed live
//! bug), silently drop it from the sample with no counter
//! (`issue55-eval/i55_e15_automotive_hybrid_gap_probe.rs`,
//! `phase9-eval/p9_e04_isolated_ranking_and_execution.rs`,
//! `phase2-eval/p1d_physical_advantage_eval.rs`), or count it and abort
//! the run before publishing any number (`issue35-eval/src/eval.rs`, the
//! only binary that got this right end to end).
//!
//! [`PairedComparison`] makes the first two behaviors structurally
//! impossible: there is no method that lets a caller push a metric for a
//! failed lookup, and [`PairedComparison::finish`] forces an explicit
//! choice between issue35-eval's abort-before-publishing discipline (the
//! default) and a named, auditable partial-report escape hatch
//! (matching `i55_e14_paired_comparator_freeze`'s existing, disclosed
//! warn-and-continue behavior) rather than silently defaulting to
//! neither.

use crate::outcome::EngineLookup;

/// One query that failed to get a comparable answer from the backend.
#[derive(Debug, Clone)]
pub struct ComparatorFailure {
    pub query_id: String,
    pub reason: String,
}

/// Accumulates paired native/other metrics (e.g. `(ndcg, recall, mrr)`,
/// or a bare `f64`) across a query set, refusing to accept a metric for
/// any query whose comparator lookup did not succeed.
pub struct PairedComparison<M> {
    native: Vec<M>,
    other: Vec<M>,
    failures: Vec<ComparatorFailure>,
}

impl<M> Default for PairedComparison<M> {
    fn default() -> Self {
        Self {
            native: Vec::new(),
            other: Vec::new(),
            failures: Vec::new(),
        }
    }
}

impl<M> PairedComparison<M> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Records a query where both sides produced a real, scoreable
    /// answer. Never call this for a failed lookup -- there is
    /// deliberately no `EngineLookup` parameter here to make that
    /// mistake impossible; use [`Self::record_lookup_failure`] instead
    /// when `lookup` was not [`EngineLookup::Success`].
    pub fn record_success(&mut self, native: M, other: M) {
        self.native.push(native);
        self.other.push(other);
    }

    /// Records a query the comparator backend failed to answer. `lookup`
    /// must not be [`EngineLookup::Success`] -- this method exists so
    /// every call site's failure path reads identically instead of each
    /// binary hand-rolling its own "what do I do with a `None`" logic.
    pub fn record_lookup_failure(&mut self, query_id: impl Into<String>, lookup: &EngineLookup) {
        let reason = lookup
            .failure_description()
            .unwrap_or_else(|| "record_lookup_failure called with a Success lookup".to_string());
        self.failures.push(ComparatorFailure {
            query_id: query_id.into(),
            reason,
        });
    }

    /// Records a query dropped for a reason other than a backend lookup
    /// failure (e.g. `translate::translate_all` returned a non-empty
    /// `Unresolvable` list, meaning the `fq` this run would have sent
    /// Solr does not actually enforce every constraint native enforced).
    pub fn record_translation_failure(
        &mut self,
        query_id: impl Into<String>,
        reason: impl Into<String>,
    ) {
        self.failures.push(ComparatorFailure {
            query_id: query_id.into(),
            reason: format!("fq_translation_failed: {}", reason.into()),
        });
    }

    pub fn failure_count(&self) -> usize {
        self.failures.len()
    }

    pub fn success_count(&self) -> usize {
        self.native.len()
    }

    /// The issue35-eval discipline: only returns the paired metric
    /// vectors when every recorded query succeeded. On any failure,
    /// returns every recorded [`ComparatorFailure`] instead -- the
    /// caller's job is then to report them and abort (typically
    /// `eprintln!` each failure and `std::process::exit(1)`, mirroring
    /// `issue35_eval::eval::run_vertical_eval`) rather than publish a
    /// number that silently rests on a smaller, uncertified sample.
    pub fn finish(self) -> Result<(Vec<M>, Vec<M>), Vec<ComparatorFailure>> {
        if self.failures.is_empty() {
            Ok((self.native, self.other))
        } else {
            Err(self.failures)
        }
    }

    /// The named escape hatch for a binary that deliberately wants
    /// `i55_e14_paired_comparator_freeze`'s existing warn-and-continue
    /// behavior instead of issue35-eval's hard abort. Always succeeds;
    /// the caller must still inspect and disclose the returned failure
    /// list (e.g. print it) rather than discard it -- this method
    /// returns it precisely so a caller cannot "finish" a partial run
    /// without the failures being visible in the return type.
    pub fn finish_partial(self) -> ((Vec<M>, Vec<M>), Vec<ComparatorFailure>) {
        ((self.native, self.other), self.failures)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finish_succeeds_with_paired_vectors_when_nothing_failed() {
        let mut cmp: PairedComparison<f64> = PairedComparison::new();
        cmp.record_success(0.8, 0.6);
        cmp.record_success(1.0, 0.9);
        let (native, other) = cmp.finish().expect("no failures recorded");
        assert_eq!(native, vec![0.8, 1.0]);
        assert_eq!(other, vec![0.6, 0.9]);
    }

    #[test]
    fn finish_fails_loudly_when_any_lookup_failed_even_if_others_succeeded() {
        let mut cmp: PairedComparison<f64> = PairedComparison::new();
        cmp.record_success(0.8, 0.6);
        cmp.record_lookup_failure("q2", &EngineLookup::TransportError("boom".to_string()));
        let failures = cmp.finish().expect_err("one failure should block finish()");
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0].query_id, "q2");
    }

    #[test]
    fn a_failed_lookup_can_never_contribute_a_scored_metric() {
        // There is no method on PairedComparison that accepts both an
        // EngineLookup and a metric for the non-Success case -- this
        // test documents that invariant via finish_partial's shape: the
        // failure appears only in the failures list, never in `other`.
        let mut cmp: PairedComparison<f64> = PairedComparison::new();
        cmp.record_success(0.5, 0.5);
        cmp.record_lookup_failure("q2", &EngineLookup::ParseError("bad body".to_string()));
        let ((native, other), failures) = cmp.finish_partial();
        assert_eq!(native.len(), 1);
        assert_eq!(other.len(), 1);
        assert_eq!(failures.len(), 1);
    }

    #[test]
    fn translation_failure_is_reported_distinctly_from_a_lookup_failure() {
        let mut cmp: PairedComparison<f64> = PairedComparison::new();
        cmp.record_translation_failure("q1", "BrandAny group partially unresolvable");
        let failures = cmp
            .finish()
            .expect_err("translation failure should also block finish()");
        assert!(failures[0].reason.starts_with("fq_translation_failed:"));
    }
}
