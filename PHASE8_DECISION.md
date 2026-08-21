# Phase 8 Decision (Issue #21 Phase 8) — P8-E00 first pass

**Decision: PROCEED**, with the first Phase 8 hypothesis (H16) confirmed
cleanly across 3 independent runs. This is a first pass, not a terminal
decision — see `PHASE8_FEASIBILITY.md` for the full item-by-item
assessment of what this environment can and cannot test toward Issue
#21's complete Phase 8 required-measurements list, and "What would be
built next" below for the specific remaining items.

## Recap: what Phase 8 was asked to answer

Issue #21's Phase 8 goal: test the regime where normal statistical
multiplexing fails — "cheap under heterogeneous steady state, elastic
under correlated burst." Three burst regimes are named: (A) independent
peaks, (B) partially correlated promotion (a subset of tenants bursts
simultaneously), (C) BFCM shock (a large majority/all tenants,
5x/10x/20x). `PHASE8_FEASIBILITY.md` found 9 of Issue #21's 14 required
Phase 8 measurements (plus a normal-vs-burst economic synthesis)
testable now by extending Phase 7's own validated methodologies with a
burst multiplier, and recommended Regime B as the highest-value first
experiment.

- **H16** (P8-E00): a correlated demand burst affecting a subset of
  tenants (traffic weight multiplied by a burst factor) does not
  measurably degrade the OTHER, non-bursting tenants' own query
  latency/throughput — extending H2/H4/H10/H11's steady-state
  isolation findings (all from Phase 7) to a correlated-burst regime,
  Phase 8's own stated thesis, for the first time.

## Architecture tested

The same plain, independent `commerce_core::index::CatalogIndex`
instances Phase 7 tested throughout, over the same real 55-tenant
WANDS `category_depth_1` partition. No new mechanism — this
deliberately asks whether the architecture Phase 7 already validated
for steady-state multi-tenancy also holds under a sudden, correlated
demand shift, before considering any burst-specific engineering
(admission control, elastic scale-out, etc.).

## Measured results

**H16 — CONFIRMED, reproduced cleanly across 3 independent runs
(P8-E00).** A fixed group of 10 tenants (of 55, ~18%, the
lowest-weight/longest-tail group by Zipfian rank) had their traffic
weight multiplied 10x mid-experiment, simulating a correlated
sale/promotion event; a separate, fixed "bystander" tenant (rank 10,
outside the burst group) was tracked throughout.

Sanity check: the burst group's own throughput grew 9.80-10.58x across
the 3 runs, matching the intended 10x multiplier and confirming the
mechanism worked as designed. Bystander p99 ratio (burst vs. steady):
**0.95x-1.03x** — essentially flat, no material degradation in either
direction, in every run. Bystander p50 ratio: 0.98x-1.01x, equally
flat. Aggregate throughput across the whole 55-tenant population rose
~40% (1,105-1,112 rps steady to 1,533-1,568 rps burst) without any
cost to the bystander's own latency.

**This is a real, positive answer to Phase 8's own stated thesis**: the
steady-state cross-tenant isolation properties Phase 7 already
established for the native in-process path (H2's query-load isolation,
H4/H11's breadth-independence, H10's Zipfian-demand fairness) extend
cleanly to a correlated-burst regime, at least for query load (not
rebuild churn or lexical-backend contention — see "Named limitations"
and "What would be built next").

**Named limitations**: only one burst multiplier (10x of Issue #21's
named 5x/10x/20x range) and one burst-group size (10 of 55 tenants)
were tested. Only one bystander tenant was tracked, at a fixed rank
outside the burst group; a bystander adjacent in rank to the burst
group is untested. This experiment tests QUERY-load burst only — it
does NOT combine with H14's already-confirmed rebuild-churn isolation
gap or H15's already-confirmed shared-Solr-contention gap, both real
limitations Phase 7 found. Regime C (BFCM shock: majority/all tenants
bursting at once) is untested; this experiment is Regime B only.

Full details, raw CSV: `docs/experiments/PHASE8_LOG.md`,
`docs/research/artifacts/p8_e00_partial_burst_run1/`.

## Failed / fixed experiments (preserved, not erased)

None yet for Phase 8 — P8-E00's first draft ran cleanly with no
self-caught methodology issues, unlike several Phase 7 experiments.
This section exists as a placeholder for this project's standing
"record failed experiments, do not erase evidence" discipline and will
be populated if a future Phase 8 experiment's first draft needs
correction.

## Unresolved risks

1. **H16 tested only one burst multiplier (10x) and one burst-group
   size (10 tenants).** Whether the isolation property holds at Issue
   #21's other named multipliers (5x, 20x) or at Regime C's
   majority/all-tenant scale is untested.
2. **H16 tests query-load burst in isolation from Phase 7's two known
   real isolation gaps.** Whether a correlated burst makes H14's
   rebuild-churn gap or H15's shared-Solr-contention gap materially
   worse (a real, plausible risk, given both already show real effects
   at steady demand) is untested and is this pass's most important
   named follow-up.
3. **This is single-node testing only**, per `PHASE8_FEASIBILITY.md`'s
   own disclosed scope boundary — true multi-node scale-out,
   redistribution, and the immutable-bundle-vs-cluster-lifecycle
   comparative question remain genuinely untested in this environment.

## What would be built next if scaling up

Per `PHASE8_FEASIBILITY.md`'s remaining feasible-now items: burst-load
versions of the lexical-backend-saturation test (reusing P7-E12/H15's
Solr harness at increasing concurrent load), the packing-density-
reduction-under-burst test (extending H12's methodology), the
mutation-during-burst test (extending H14's rebuild-churn methodology
with a burst query-rate multiplier — the single highest-value next
experiment, since H14 is this project's own confirmed real isolation
gap and burst conditions are exactly when it would matter most in
production), warmup-time-to-SLO and bundle-load-time measurements
(extending H1/H7/H8), and the normal-day-vs-burst economic-cost
synthesis (extending `PHASE7_ECONOMIC_MODEL.md`'s own discipline).

## What should explicitly not be built yet

No admission-control or load-shedding mechanism, no multi-node
scale-out/rebalancing infrastructure, and no SolrCloud cluster setup
purely to chase Phase 8 coverage — all three are named in
`PHASE8_FEASIBILITY.md` as genuine, disclosed gaps requiring new
product surface or new infrastructure this epic has deliberately not
yet built, not gaps to force closed with hasty scaffolding.

## What this decision does and does not claim

**Does claim**: a correlated demand burst affecting a subset of
tenants' query load does not measurably degrade an unrelated tenant's
own latency on the native in-process path, reproduced across 3
independent runs, at one tested burst multiplier (10x) and one tested
burst-group size (10 of 55 tenants).

**Does not claim**: that this result holds at other burst multipliers,
burst-group sizes, or under Regime C's majority/all-tenant burst; that
Phase 7's two known real isolation gaps (H14's rebuild churn, H15's
shared-Solr contention) behave the same way under burst as they do at
steady demand — untested, and plausibly worse, not better; that any of
Phase 8's genuinely blocked items (admission/backpressure control,
real multi-node redistribution, the immutable-bundle-vs-cluster-
lifecycle comparison) have been answered — see `PHASE8_FEASIBILITY.md`
for why they remain out of scope for this environment.

**Decision: PROCEED** to the next Phase 8 sub-experiment (the burst
version of H14's rebuild-churn test, the highest-value item named
above) without changing the underlying commerce-native mechanism. H16's
clean, reproduced confirmation is real evidence that this
architecture's steady-state multi-tenant isolation properties are not
merely a steady-state artifact — but it is explicitly a floor on the
claim (one burst multiplier, one burst-group size, one burst regime,
query load only), not a ceiling on what Phase 8 still has to test.
