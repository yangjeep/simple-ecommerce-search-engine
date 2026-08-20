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

## P3-E01 — transparent fallback baseline: fallback tax

**Evidence class**: real (full 1,215,854-product Amazon ESCI catalog,
full 22,458-query judged corpus, fresh local Apache Solr 9.10.1 re-indexed
with the identical catalog -- same evidence base Phase 2 used).

**Hypothesis**: `admission::admit()`'s own overhead on the reject path
(the one cost every rejected query pays) is close to zero relative to a
single Solr round trip -- Issue #14's invariant 1, initial target <=5%
fallback tax.

**Method**: `crates/phase3-eval/src/bin/p3e01_fallback_baseline.rs`. Whole
corpus admission pass (cheap, no Solr calls) with `AdmissionPolicy {
max_candidates: 50 }` as an initial conservative starting point (not yet
calibrated -- that is P3-E02's job), reporting coverage and a reject-
reason breakdown. Then a seeded-random (`ChaCha8Rng`, not "smallest-N-by-
id" -- addressing the P2-E17 adversarial review's own finding about Phase
2's sampling) sample of 20 rejected queries, 30 reps/arm, interleaved via
`bench_harness::round_robin_schedule`, comparing `solr_baseline` (a direct
`solr_search` call, using the exact `round1_eval::solr::solr_query_for`
construction Phase 2's P1-D already validated as fair) against
`admission_then_solr` (`admit()` -- which returns `Reject` for every
query in this sample by construction -- immediately followed by the
identical `solr_search` call). `compile()` itself is excluded from both
arms' timed block, symmetric with every prior phase's own convention.

**Result** (raw: `docs/research/artifacts/p3e01_run1/`):

Whole-corpus admission pass (`max_candidates=50`): **admitted 82/22,458
(0.37%)**; rejected ambiguous 5,005 (22.29%); rejected unresolved-residual
17,268 (76.89%); rejected not-selective-enough 103 (0.46%); rejected
no-structural-constraint 0 (0.00%, absent from the corpus entirely at
this lexicon).

**A strong, unplanned cross-validation against Phase 2's own established
9-class counts**, all exact: rejected-ambiguous (5,005) equals Phase 2's
`ambiguous_punt` class size exactly; rejected-unresolved-residual (17,268)
equals the sum of Phase 2's four residual-bearing classes exactly
(`structural_plus_lexical_residual` 3,253 + `structural_plus_semantic_residual`
57 + `lexical_first` 8,274 + `long_tail_noisy` 5,684 = 17,268); admitted +
not-selective-enough (82 + 103 = 185) equals Phase 2's total FastPath-
eligible population exactly (`structural_exact_entity` 153 +
`selective_multi_attribute_structural` 0 + `variant_scoped_structural` 30
+ `range_plus_structural` 2 = 185). Same lexicon, same `compile()`, same
underlying query population -- `admission::admit`'s ambiguity/residual
checks land on precisely the same population boundaries `classify9`
already established, which is exactly what should happen since both read
the same compiled-query shape, and is a real (if incidental) regression
check that the new module isn't silently drawing its lines somewhere
`classify9` doesn't.

A genuinely new number this run reveals: **of the 185 queries that would
have been FastPath-eligible under Phase 2's plain "no residual, no
ambiguity" test, more than half (103/185, 55.7%) resolve to a candidate
set larger than 50** -- i.e. a strict, safety-first selectivity cap
rejects a majority of Phase 2's own FastPath population outright. This is
concrete evidence for why P3-E02's coverage-frontier sweep matters: the
raw "FastPath-eligible" population Phase 2 measured is not automatically
"safe" by this phase's own selectivity-conscious admission bar.

Fallback-tax latency (interleaved, 30 reps/arm): `solr_baseline`
mean=2.5603ms (sd=0.4102ms); `admission_then_solr` mean=2.4983ms
(sd=0.2702ms). Bootstrap CI (diff = `admission_then_solr` - `solr_baseline`):
**diff=-0.0620ms, CI=[-0.2377, 0.1050], excludes_zero=false** -- the
admission check's own cost is not statistically distinguishable from
zero at this sample size, and the point estimate is negative (i.e.
`admission_then_solr` measured *faster*, which is measurement noise, not
a real negative cost -- `admit()` cannot make a Solr call faster). As a
percentage of the `solr_baseline` mean: **-2.42%**, comfortably within
Issue #14's <=5% target with headroom to spare.

**Threats to validity, stated rather than assumed away**: n=20 unique
queries (not a random draw from the full 17,376-query rejected
population at this policy) -- adequate for this exploratory measurement
per Issue #14's own distinction between exploratory and paper-grade final
replication, but the *exact* tax percentage should not be over-read from
one run at this sample size; the CI already including zero is the
important, robust part of this result, not the -2.42% point estimate.
The `not_selective_enough` rejection path (which does build a real
`indexed_candidates` bitmap, unlike the other three cheap short-circuit
rejections) was not isolated from the cheaper rejection paths in this
run's 20-query sample by construction -- a targeted follow-up sampling
only `NotSelectiveEnough`-rejected queries would more precisely bound the
selectivity-check's own marginal cost specifically, if a future
experiment needs that finer breakdown.

**Decision**: KEEP. The admission check's overhead is close to zero and
statistically indistinguishable from a pure Solr call, meeting Issue
#14's invariant 1 with margin. Proceed to P3-E02: sweep `max_candidates`
on the full real corpus to trace the safe-coverage/relevance frontier.
