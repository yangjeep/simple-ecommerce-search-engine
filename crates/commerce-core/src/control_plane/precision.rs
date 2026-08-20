use std::collections::HashMap;

use crate::ir::{compile, ResolvedConstraint, SemanticLexicon};

use super::replay::ReplayResult;

/// How a specific query's compiled resolution performed against real-world
/// evidence. `commerce_core` has no concept of human relevance judgments
/// or a real catalog of its own — that is always external, real data — so
/// this type only carries the aggregate counts a [`PrecisionOracle`]
/// reports, never the judgments themselves.
///
/// Carries both a precision signal (`filtered_relevant`/`filtered_total`:
/// of what the resolution matched, how much is relevant) and a recall
/// signal (`judged_relevant_total`: how many relevant products are known
/// to exist for this query at all, regardless of whether the filter found
/// them). Both are required: an early version of this gate checked
/// precision only, guarded by "only judge when the filter matched
/// something at all" — which a real P2-E03 run
/// (`docs/experiments/PHASE2_LOG.md`) showed is a real loophole, not a
/// hypothetical one. A resolution that compiles to a constraint kind the
/// oracle's execution engine doesn't even implement (e.g. this project's
/// real-data harness only evaluates `Brand`/`color`, so any other
/// constraint kind matches *zero* products by construction) matches
/// nothing at all — `filtered_total == 0` — which trivially "passes" a
/// precision-only check while silently turning a query that would have
/// fallen through to lexical retrieval into a hard zero-result query
/// instead, plausibly *worse* than matching irrelevant products. Recall
/// closes that gap: if relevant products are known to exist for this
/// query and the new resolution finds none of them, that is a failure
/// signal on its own, independent of precision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Judgment {
    /// How many judged products for this query are relevant at all
    /// (independent of whether the compiled resolution matched them).
    pub judged_relevant_total: usize,
    /// How many products the compiled resolution matched.
    pub filtered_total: usize,
    /// Of the resolution's matches, how many are relevant.
    pub filtered_relevant: usize,
}

impl Judgment {
    pub fn precision(&self) -> f64 {
        if self.filtered_total == 0 {
            return 0.0;
        }
        self.filtered_relevant as f64 / self.filtered_total as f64
    }

    /// Fails if the resolution is imprecise (matches mostly irrelevant
    /// products) OR if it has zero recall despite known relevant products
    /// existing for this query (matches nothing relevant, even though
    /// something relevant was known to exist) — see the struct doc for why
    /// both checks are required.
    pub fn fails(&self, min_precision: f64) -> bool {
        let imprecise = self.filtered_total > 0 && self.precision() < min_precision;
        let zero_recall_despite_known_relevant =
            self.judged_relevant_total > 0 && self.filtered_relevant == 0;
        imprecise || zero_recall_despite_known_relevant
    }
}

/// Whether a query's compiled constraints tend to match products a human
/// would call relevant — the missing check Round 1 R1-E06
/// (`docs/experiments/ROUND1_LOG.md`) found: the original coverage-only
/// promotion gate (`ReplayResult::passes_promotion_gate`) is
/// mathematically unable to reject a mapping for a never-before-seen term
/// regardless of whether the mapping means anything, because such a term
/// could never have contributed to any prior query's "fully resolved"
/// status. A `PrecisionOracle` supplies the missing signal: does executing
/// this resolution actually find relevant products, not just *a*
/// resolution. `commerce_core` ships no real implementation (no test may
/// require real judgment data); production and experiment callers supply
/// one backed by real relevance evidence.
pub trait PrecisionOracle {
    /// Judge one query's compiled resolution, or `None` if this oracle has
    /// no evidence for that query at all. Absence of evidence must never
    /// be treated as evidence of a problem — a query the oracle can't
    /// judge is excluded from the precision check entirely, not failed by
    /// default (that would make the gate reject on ignorance, the same
    /// "guess with unearned confidence" failure mode this project
    /// otherwise refuses to accept).
    fn judge(&self, query: &str, constraints: &[ResolvedConstraint]) -> Option<Judgment>;
}

/// A deterministic, fixture-backed [`PrecisionOracle`]: a fixed
/// query-to-[`Judgment`] table, no network access, no real relevance data
/// — satisfying CLAUDE.md's "no test may require a real model API key"
/// discipline applied to judgment data as well as model calls. Mirrors
/// [`super::FixtureModelProvider`]'s existing pattern exactly.
#[derive(Debug, Default)]
pub struct FixtureJudgmentOracle {
    judgments: HashMap<String, Judgment>,
}

impl FixtureJudgmentOracle {
    pub fn new(judgments: impl IntoIterator<Item = (&'static str, Judgment)>) -> Self {
        FixtureJudgmentOracle {
            judgments: judgments
                .into_iter()
                .map(|(q, j)| (q.to_string(), j))
                .collect(),
        }
    }
}

impl PrecisionOracle for FixtureJudgmentOracle {
    fn judge(&self, query: &str, _constraints: &[ResolvedConstraint]) -> Option<Judgment> {
        self.judgments.get(query).copied()
    }
}

/// Result of checking precision on exactly the queries a candidate lexicon
/// *changed* the resolution of (`ReplayResult::newly_resolved`) — the only
/// queries where a newly-added mapping is actually doing new work; queries
/// unaffected by the candidate have nothing to check.
#[derive(Debug, Clone, PartialEq)]
pub struct PrecisionCheck {
    pub min_precision: f64,
    /// Newly-resolved queries the oracle had evidence for at all.
    pub queries_judged: usize,
    /// Newly-resolved queries the oracle had evidence for, but whose
    /// judged precision fell below `min_precision`. Any non-empty list
    /// fails the gate, mirroring `ReplayResult::regressions`'
    /// any-non-empty-fails design.
    pub queries_below_threshold: Vec<String>,
}

impl PrecisionCheck {
    pub fn passes(&self) -> bool {
        self.queries_below_threshold.is_empty()
    }
}

/// Judge every query in `newly_resolved` against `oracle`, using each
/// query's compiled resolution under `candidate`.
pub fn check_precision(
    newly_resolved: &[String],
    candidate: &SemanticLexicon,
    oracle: &dyn PrecisionOracle,
    min_precision: f64,
) -> PrecisionCheck {
    let mut queries_judged = 0;
    let mut queries_below_threshold = Vec::new();
    for query in newly_resolved {
        let compiled = compile(query, candidate);
        let Some(judgment) = oracle.judge(query, &compiled.constraints) else {
            continue;
        };
        queries_judged += 1;
        if judgment.fails(min_precision) {
            queries_below_threshold.push(query.clone());
        }
    }
    PrecisionCheck {
        min_precision,
        queries_judged,
        queries_below_threshold,
    }
}

/// Why `try_promote_with_precision` rejected a candidate: the original
/// coverage/regression gate never even ran a precision check (fails the
/// same way `try_promote` always has), or the coverage gate passed but the
/// precision gate did not (the specific safety gap R1-E06 found and this
/// module exists to close). The precision variant is boxed: it carries
/// both a `ReplayResult` and a `PrecisionCheck` together, which would
/// otherwise make every `Result<SemanticContext, PromotionRejection>`
/// pay for the largest variant's size even on the common `Ok` path.
#[derive(Debug, Clone, PartialEq)]
pub enum PromotionRejection {
    CoverageGateFailed(ReplayResult),
    PrecisionGateFailed(Box<PrecisionGateFailure>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct PrecisionGateFailure {
    pub replay: ReplayResult,
    pub precision: PrecisionCheck,
}
