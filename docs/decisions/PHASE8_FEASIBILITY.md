# Phase 8 Feasibility Assessment (Issue #21 Phase 8)

## Status

**Verdict: PARTIALLY SUPPORTED.** A meaningful, single-node-scoped
subset of Phase 8's required measurements can be attempted now, reusing
Phase 7's own harness and the Solr installation Phase 7 (H15) already
proved out in this environment, without requiring materially larger
infrastructure. But several of Phase 8's required items genuinely
cannot be tested honestly in this environment without either (a)
building new product surface this epic has deliberately deferred
(admission/backpressure control), or (b) real multi-node
infrastructure this project has deliberately not yet built, per
`CLAUDE.md`'s own "avoid distributed systems work until the
single-node thesis has been measured" instruction — and Phase 7's
just-finished terminal decision is the first complete single-node
campaign. Forcing those specific items now would violate this
project's own stated sequencing and its "do not optimize for feature
count" discipline, not advance it.

This assessment goes through Issue #21's Phase 8 "Required
measurements" list item by item, classifying each as **FEASIBLE NOW**
(reuses existing single-node infrastructure), **FEASIBLE, REFRAMED**
(the single-node analogue of the literal ask is feasible; the literal,
multi-node version is not), or **BLOCKED** (needs materially larger
infrastructure or new product scope), with reasoning tied to concrete
Phase 7 evidence rather than speculation.

## Item-by-item assessment

| Required measurement | Classification | Reasoning |
|---|---|---|
| Aggregate and per-tenant p50/p95/p99 under burst | **FEASIBLE NOW** | Directly extends H4/H10/H11's already-proven quiet/noisy and Zipfian-weighted methodology with a burst multiplier (5x/10x/20x demand) instead of steady demand. No new infrastructure. |
| Throughput saturation curve | **FEASIBLE NOW** | Extends H4's breadth-scaling design: push concurrent load (not breadth) until throughput plateaus or degrades, on the same native harness. |
| Fairness/noisy-neighbor behavior | **FEASIBLE NOW** | Directly extends H10's Zipfian hot/cold design, adding a burst phase. H9/H10 already measured the steady-state baseline this would compare against. |
| Admission/backpressure behavior | **BLOCKED** | This architecture has no admission-control or load-shedding mechanism anywhere in `commerce_core` or `phase7-eval` — every Phase 7 harness is a bare benchmark loop with no queueing/rejection policy. Testing this honestly would require BUILDING a new mechanism first, not just a new experiment — real new product scope this epic (and `CLAUDE.md`'s "avoid production polish... during this epic") explicitly defers. Phase 3/4's "admission policy" is a different concept (which queries route native vs. lexical), not request-rate admission control. |
| Lexical backend saturation | **FEASIBLE NOW** | Directly reuses P7-E12/H15's just-proven Solr HTTP harness: push concurrent query rate against the same cores until Solr saturates. The freshest, most natural next step given H15's infrastructure is already built and working. |
| Memory pressure/cache pollution | **FEASIBLE, REFRAMED** | Memory pressure under burst concurrency is directly testable (combines H12's memory-ceiling methodology with a burst QPS multiplier). "Cache pollution" as literally asked presumes an explicit application-level cache — this architecture has been repeatedly and explicitly disclosed throughout Phase 7 (H9/H10) as having NO such cache; the closest honest analogue is the already-disclosed, unconfirmed CPU-cache-locality hypothesis from H9/H10, not a new application cache to pollute. |
| Tenant packing-density reduction during burst | **FEASIBLE NOW** | Directly extends H12's methodology: does the safe tenant count at target SLO shrink under burst load vs. steady state? Reuses the same incremental-build-with-safety-check pattern. |
| Scale-out startup time | **FEASIBLE, REFRAMED** | True multi-node scale-out (provisioning new machines) is not testable here — this project has never run more than one machine. The single-node analogue (time to bring a new tenant's bundle online on this same node) is feasible and partially already measured (H1's build-time data, H7's cold-start-to-warm curve). |
| Tenant index/context bundle load time | **FEASIBLE NOW** | Already partially measured (P7-E00/H1's build timing, P7-E10/H13's CPU-cost data); a dedicated bundle-load-time measurement across more tenant sizes is a direct, low-effort extension. |
| Warmup time to SLO | **FEASIBLE NOW** | Extends H7/H8/H9's already-collected warm-up curves: measure elapsed time from a tenant's bundle going live until its own query latency stabilizes at the material-regression-bar-defined SLO. |
| Redistribution/rebalancing work | **BLOCKED** | Inherently a multi-node/sharded-deployment concept. This project has deliberately stayed single-node throughout (`CLAUDE.md`: "avoid distributed systems work until the single-node thesis has been measured") — Phase 7's just-finished terminal decision IS that single-node thesis measurement. Attempting real rebalancing now would skip a deliberate sequencing step, not advance it. |
| Mutation behavior during burst | **FEASIBLE NOW** | Directly extends H14's rebuild-churn methodology (already found a REAL, material isolation gap at steady demand) with a burst query-rate multiplier: does H14's already-confirmed effect get worse under burst? A natural, high-value extension of a hypothesis this pass already validated matters. |
| Recovery/downscale behavior after the event | **FEASIBLE, REFRAMED** | Single-node recovery (does RSS/latency return to baseline after a burst ends) is directly testable with the same resident-sampling infrastructure H7/H8 already built. Multi-node "downscale" (releasing machines) is blocked for the same reason as scale-out/redistribution above. |
| Normal-day vs. burst capacity cost | **FEASIBLE NOW** | A natural extension of `PHASE7_ECONOMIC_MODEL.md`'s own methodology and its explicit physical-units-only discipline (no dollar conversion) — combine already-established steady-state figures with newly-measured burst figures into a normal-vs-burst physical-cost comparison. |
| "Do immutable bundles + mutable overlays beat generic cluster/shard lifecycle" (Phase 8's core comparative question) | **BLOCKED** | Requires a real distributed baseline to compare against (e.g. SolrCloud's own sharding/replica scale-out behavior) — this environment's Solr installation is a single standalone instance (proven working for H15), not a SolrCloud cluster. Sound comparative measurement needs that baseline actually configured, which is materially more setup than reusing the existing standalone instance H15 already validated. |

## What this means

Roughly two-thirds of Phase 8's required measurements (9 of 14 items,
plus the normal-vs-burst economic synthesis) can be attempted now with
no new infrastructure, by extending methodologies Phase 7 already
built and validated (H1, H4, H7, H8, H9, H10, H12, H14, and H15's fresh
Solr harness). The remaining items — admission/backpressure control,
true multi-node redistribution/rebalancing, and the immutable-bundle-
vs-cluster-lifecycle comparative claim — genuinely require either new
product surface or new infrastructure this epic has deliberately not
yet built, and forcing them now would contradict this project's own
stated sequencing (`CLAUDE.md`: single-node thesis first; avoid
production polish and distributed systems work during this epic).

## Recommended next step

Rather than attempting the full Phase 8 required-measurement list at
once, the highest-value single next experiment is a **partially-
correlated burst on the native path** (Issue #21's Regime B): extend
H10's already-validated Zipfian hot/cold design with a burst phase
where a SUBSET of tenants (not all 55) simultaneously shift to a much
higher demand multiplier, mid-run, and measure whether the already-
confirmed fairness/isolation properties (H2, H4, H10, H11) hold when
that shift is sudden and correlated rather than gradual. This is the
single most direct test of Phase 8's own stated thesis ("cheap under
heterogeneous steady state, elastic under correlated burst") that is
fully supported by this environment right now.

A close second, given how fresh and directly relevant it is: **lexical
backend saturation** (reusing P7-E12/H15's Solr harness at increasing
concurrent load), since it directly extends the one real isolation gap
Phase 7 just found at that layer.

## What should explicitly not be attempted yet

Building a request-admission/backpressure mechanism, standing up a
SolrCloud multi-node cluster, or attempting genuine multi-machine
scale-out/rebalancing, purely to satisfy Phase 8's full required list.
Each of these is a real, disclosed gap in what this environment can
test — not something to build hastily just to check a box. Per
`CLAUDE.md`'s own discipline, these are named as explicit,
deliberately-not-pursued gaps, the same honest-disclosure standard
Phase 7 applied throughout (e.g. H14/H15's isolation gaps, both named
without a designed mitigation rather than assumed solved).
