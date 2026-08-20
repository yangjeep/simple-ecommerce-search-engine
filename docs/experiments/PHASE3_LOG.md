# Phase 3 Experiment Log — Issue #14, safe fast-path offload frontier over Solr

Follows the same discipline as `docs/experiments/PHASE2_LOG.md`: each entry
states a falsifiable hypothesis before implementation, records raw real-data
results (not just a summary verdict), and ends in KEEP/REJECT (this phase's
per-change decision) feeding toward Issue #14's own final STRONG SUPPORT /
NARROW SUPPORT / NEGATIVE verdict.

## Recap: why Phase 3 exists

Phase 2 (`PHASE2_DECISION.md`, `docs/experiments/PHASE2_LOG.md` P2-E17)
rejected the whole-engine 5-10x QPS/$ replacement thesis: traffic-weighted,
commerce-native was ~2.3-3.0x *slower* than a mature Solr baseline, because
its `Hybrid`/`Punt` paths (99%+ of real traffic) always paid for both
structural planning *and* an embedded Tantivy delegate call, on top of a
single mature Solr call's cost, never instead of it. The one clean physical
win -- `structural_exact_entity`/`variant_scoped_structural`, `FastPath`,
87-105x faster than Solr -- covered under 1% of real traffic on the real
Amazon ESCI catalog/query corpus, and even that win carried a real,
uncorrected relevance cost (`execute_ranked` has no ranking signal when
`query.preferences` is empty, which it always is for this benchmark's
baseline lexicon).

Issue #14 reframes the thesis around that one clean win rather than trying
to salvage `Hybrid`/`Punt`: treat Solr as the permanent, mature safety-net
fallback, and ask how much real traffic a *cheap* admission check can
safely intercept for native `FastPath` execution -- never both commerce-
native *and* a delegate call on the same query. This removes the exact
mechanism P2-E17 found responsible for most of the "slower" result.

## Architecture

```text
query
  -> compile() (cheap, in-process, already excluded from every prior
     phase's own timing convention)
  -> admission::admit() (cheap: ambiguity/residual/no-constraint checks,
     one indexed_candidates bitmap build against a tunable selectivity cap
     -- no delegate call, no Tantivy, no Hybrid narrow-then-rank)
      -> Admit: execute natively (CatalogIndex::execute_ranked, unchanged
         from Phase 2 -- Issue #14's own rule: do not rebuild ranking)
      -> Reject: forward the ORIGINAL, unmodified Solr request (the same
         solr_query_for()-shaped query Phase 2's P1-D already validated as
         a fair baseline construction) -- exactly as if commerce-native did
         not exist for this query
```

`commerce_core::admission` (new this phase) owns the mechanism:
`AdmissionPolicy { max_candidates }`, `AdmissionDecision::{Admit,Reject}`,
`RejectReason` (Ambiguous / UnresolvedResidual / NoStructuralConstraint /
NotSelectiveEnough). `max_candidates` is the one tunable knob Issue #14's
RQ2 coverage-frontier sweep varies; additional knobs are added only when a
specific rejected-query-class experiment (P3-E03+) finds real evidence one
is needed, not speculatively ahead of that evidence.

`crates/phase3-eval` (new this phase) owns measurement, mirroring
`phase2-eval`'s structure but with no Tantivy dependency at all -- Phase 3's
target architecture never calls an embedded delegate.

## P3-E00 — scaffold: `commerce_core::admission` + `phase3-eval`

**Evidence class**: unit-level (hand-built fixtures), no real data yet --
this entry documents the mechanism build itself, before any real-corpus
measurement.

7 tests in `crates/commerce-core/tests/admission.rs`, one rejection reason
isolated per test: candidate count within/at/over the policy cap
(admit/admit/reject, pinning the inclusive boundary explicitly rather than
leaving it to whichever comparison operator got typed); ambiguity rejects
regardless of an otherwise-generous cap; unresolved residual rejects; no
structural constraint at all rejects; a `Preference`-only compiled query
(no hard constraints, per `ir::query::compile`'s own contract that a
`Preference`-resolved phrase always stays in `residual_lexical` --
ADR 0010) rejects even though `preferences` is non-empty, guarding against
a future change mistaking a soft ranking signal for admission-worthy
structure.

Quality gate green: fmt, clippy `-D warnings`, full workspace test suite
(122 tests total across the workspace, up from 115), release build.

**Decision**: KEEP (mechanism only, no real-data verdict yet). Next:
P3-E01, the transparent fallback baseline -- measure the admission check's
own overhead on the reject path before trusting any coverage/relevance
number that depends on it staying cheap.
