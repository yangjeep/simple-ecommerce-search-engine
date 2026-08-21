# Phase 8 Decision (Issue #21 Phase 8) — first pass (P8-E00, P8-E01)

**Decision: PROCEED, with one real new isolation gap confirmed.** H16
(pure query-load burst) is confirmed clean across 3 independent runs;
H17 (does burst make the already-known rebuild-churn gap worse) is also
confirmed, and materially — burst turns H14's intermittent tail-latency
hit into a near-certain one. This is a first pass, not a terminal
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
- **H17** (P8-E01): does a correlated burst make H14's already-confirmed
  rebuild-churn isolation gap (a co-located tenant's index rebuild
  degrading a quiet tenant's own p99 by 4.00-6.70x at steady demand)
  materially worse — the single highest-priority follow-up this pass's
  own "Unresolved risks" section named.

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

**H17 — CONFIRMED, with a self-caught mid-experiment fix (P8-E01).**
Three conditions measured in the same run: TRUE_BASELINE (quiet tenant
"Rugs" alone), IDLE_CHURN (Rugs plus a dedicated thread continuously
rebuilding co-located tenant "Furniture" — reproducing H14/P7-E11
exactly), and BURST_CHURN (identical Rugs+churn threads, plus 4
background worker threads issuing Zipfian-weighted queries across the
other 53 tenants, including Furniture's own live snapshot).

A first pass of 3 runs gave amplification factors (burst_ratio /
idle_ratio) of 1.04x, 3.21x, 0.90x — too scattered to trust from a
noisy tail statistic driven by only ~5 rebuild events per 5s window.
**Fixed** by raising to 10 runs and switching the pass/fail statistic
from per-run min/max to the median (mirroring P7-E09's own "use p50 as
primary metric" fix). Result: **median amplification = 3.62x**, clearing
the pre-registered 1.25x bar by a wide margin.

The more actionable finding is a secondary statistic: using Phase 7's
own 2.0x material-regression bar, **IDLE_CHURN showed a >=2x
degradation event in only 3/10 runs (30%)**, while **BURST_CHURN showed
one in 10/10 runs — every single time**, reproduced across two
independent 10-run passes taken during this experiment's development.
Burst does not just make the typical bad event somewhat worse; it turns
an intermittent coincidence (whether a query happens to land inside a
rebuild's brief disruptive window) into a near-certainty, plausibly
because background CPU/memory-bandwidth contention from other tenants'
own queries widens that window's effective hit probability.

**This is a genuine, new, real isolation gap that neither H14 (idle
system only) nor H16 (pure query burst, no churn) could have surfaced
alone.** Full details, raw CSV: `docs/experiments/PHASE8_LOG.md`,
`docs/research/artifacts/p8_e01_burst_amplified_churn_run1/`.

## Failed / fixed experiments (preserved, not erased)

**P8-E01's first draft** used only 3 repeated runs with a per-run
min/max pass/fail rule, matching every other Phase 7/8 experiment's
convention. The resulting amplification factors (1.04x, 3.21x, 0.90x)
straddled every threshold in the pre-registered bar — trusting a
verdict from 3 such noisy runs would have overclaimed a level of
precision the underlying tail statistic (~5 discrete rebuild events per
5s window) cannot support. Fixed by raising to 10 runs and switching to
a median-based verdict, with the full range still reported rather than
hidden. See `docs/experiments/PHASE8_LOG.md`'s "P8-E01 result" section
for the full account.

## Unresolved risks

1. **H16 tested only one burst multiplier (10x) and one burst-group
   size (10 tenants).** Whether the isolation property holds at Issue
   #21's other named multipliers (5x, 20x) or at Regime C's
   majority/all-tenant scale is untested.
2. **H17's mechanism (CPU/memory-bandwidth contention widening a
   rebuild's effective disruptive window) is a plausible hypothesis
   consistent with the data, not independently confirmed via
   profiling.** Whether the 100% hit-rate effect holds at a different
   burst intensity (more/fewer background workers), a different
   churn-tenant catalog size, or Regime C's larger scale is untested.
   **No mitigation for this gap has been designed or tested** — it is
   named as necessary future work, not smoothed over.
3. **H17 does not combine with H15 (shared-Solr contention)** — a
   three-way burst + churn + shared-lexical-backend interaction, which
   would plausibly compound further, remains untested.
4. **This is single-node testing only**, per `PHASE8_FEASIBILITY.md`'s
   own disclosed scope boundary — true multi-node scale-out,
   redistribution, and the immutable-bundle-vs-cluster-lifecycle
   comparative question remain genuinely untested in this environment.

## What would be built next if scaling up

Per `PHASE8_FEASIBILITY.md`'s remaining feasible-now items: burst-load
versions of the lexical-backend-saturation test (reusing P7-E12/H15's
Solr harness at increasing concurrent load, and ideally combined with
H17's churn setup for the three-way interaction named as risk #3
above), the packing-density-reduction-under-burst test (extending
H12's methodology), warmup-time-to-SLO and bundle-load-time
measurements (extending H1/H7/H8), and the normal-day-vs-burst
economic-cost synthesis (extending `PHASE7_ECONOMIC_MODEL.md`'s own
discipline). Separately, and now higher-priority given H17's
confirmed finding: a designed mitigation for rebuild-churn-under-burst
(e.g., scheduling non-urgent rebuilds away from detected burst windows,
or a rebuild strategy that avoids the allocation-heavy disruption
`CatalogIndex::build()` currently causes) is worth exploring before any
production deployment relies on this architecture during a real BFCM
event with active catalog mutation.

## What should explicitly not be built yet

No admission-control or load-shedding mechanism, no multi-node
scale-out/rebalancing infrastructure, and no SolrCloud cluster setup
purely to chase Phase 8 coverage — all three are named in
`PHASE8_FEASIBILITY.md` as genuine, disclosed gaps requiring new
product surface or new infrastructure this epic has deliberately not
yet built, not gaps to force closed with hasty scaffolding.

## What this decision does and does not claim

**Does claim**: (1) a correlated demand burst affecting a subset of
tenants' QUERY load does not measurably degrade an unrelated tenant's
own latency on the native in-process path, reproduced across 3
independent runs, at one tested burst multiplier (10x) and one tested
burst-group size (10 of 55 tenants) — H16. (2) A correlated burst
materially worsens H14's already-confirmed rebuild-churn isolation
gap — not just in magnitude (median 3.62x amplification) but in kind:
it converts an intermittent tail-latency coincidence (~30% of
measurement windows) into a near-certain one (100% of measurement
windows), reproduced across 10 runs and replicated across two
independent 10-run passes — H17.

**Does not claim**: that H16's clean result holds at other burst
multipliers, burst-group sizes, or under Regime C's majority/all-tenant
burst; that H15's shared-Solr-contention gap behaves the same way under
burst as it does at steady demand, or combined with H17's churn+burst
finding — both untested, and, given H17's result, plausibly worse, not
better; that a mitigation exists for H17's confirmed gap — none has
been designed or tested; that any of Phase 8's genuinely blocked items
(admission/backpressure control, real multi-node redistribution, the
immutable-bundle-vs-cluster-lifecycle comparison) have been answered —
see `PHASE8_FEASIBILITY.md` for why they remain out of scope for this
environment.

**Decision: PROCEED**, carrying H17 forward as a real, disclosed
isolation gap rather than smoothing it over — the same discipline
Phase 7 applied to H14 and H15. The next Phase 8 sub-experiments are
named above ("What would be built next"): burst-load lexical-backend
saturation (ideally combined with H17's churn setup), packing-density-
reduction under burst, warmup-time-to-SLO, and the normal-vs-burst
economic synthesis. H16's clean confirmation and H17's confirmed,
now-quantified gap are both real evidence — this architecture's
steady-state multi-tenant isolation properties mostly extend to a
correlated-burst regime, but the one known mutation-related exception
(H14) gets reliably worse, not better, under burst, and that must be
treated as a real production risk, not an edge case.
