# Phase 8 Experiment Log (Issue #21 Phase 8: correlated retail burst / BFCM elasticity)

See `PHASE8_FEASIBILITY.md` for the item-by-item assessment of which of
Issue #21's Phase 8 required measurements this environment can test
honestly right now, and why. This log records each Phase 8 experiment's
falsifiable hypothesis (stated before implementation, per this
project's standing `CLAUDE.md` discipline), design, and result, the
same way `docs/experiments/PHASE7_LOG.md` did for Phase 7.

## P8-E00: partially-correlated burst on the native path (H16, stated before implementation)

Issue #21's Phase 8 names three burst regimes: (A) independent peaks,
(B) partially correlated promotion (a subset of tenants enters a
sale/campaign burst simultaneously), (C) BFCM shock (a large
majority/all tenants, 5x/10x/20x). `PHASE8_FEASIBILITY.md` recommended
Regime B as the highest-value first Phase 8 experiment: it directly
tests Phase 8's own stated thesis ("cheap under heterogeneous steady
state, elastic under correlated burst") using only single-node
infrastructure this environment already has proven out (H10/P7-E07's
Zipfian-weighted, all-55-real-tenant design).

**H16**: a correlated demand burst affecting a subset of tenants (a
fixed group's traffic weight multiplied by a burst factor) does not
measurably degrade the OTHER, non-bursting tenants' own query
latency/throughput — extending H2/H4/H10/H11's steady-state isolation
findings to a correlated-burst regime. Same pass bar used throughout
Phase 7 (p99 growth <2x vs. the pre-burst baseline).

**Design**: reuses H10/P7-E07's exact Zipfian-weighted, all-55-real-tenant,
shared-`WeightedIndex` methodology (weight(rank) = 1/rank, deterministic
per-thread `ChaCha8Rng` seed) for direct continuity, but runs TWO
sequential phases per repeat instead of one static weight assignment:

- **STEADY** (60s): the plain Zipfian baseline, all 55 tenants at their
  natural rank-based weight.
- **BURST** (60s): the 10 lowest-weight/longest-tail tenants (ranks
  46-55, a "subset" per Regime B's own definition) have their weight
  multiplied by 10x (Issue #21's named middle burst value from its
  5x/10x/20x range) — simulating a sudden, correlated sale/promotion
  event affecting only that group. Every other tenant's weight,
  including a fixed, tracked "bystander" tenant (rank 10, "Baby &
  Kids", chosen for a robust sample count and to sit clearly outside
  the burst group), is unchanged.

Both phases within a run use the SAME per-thread RNG seed (identical to
each other and across the 3 repeated runs) — the logical query
sequence up to the phase boundary is identical every time; only the
weight distribution changes between phases, isolating that as the only
variable, matching H10's own discipline for attributing any difference
to the real effect rather than sampling noise.

## P8-E00 result: H16 CONFIRMED, reproduced cleanly across 3 independent runs (all 3 built into one binary invocation, matching H9/H10's convention)

Raw data in `docs/research/artifacts/p8_e00_partial_burst_run1/results.csv`.

| Run | Phase | Bystander n | Bystander p50/p99 (ms) | Burst-group rps | Aggregate rps |
|---|---|---|---|---|---|
| 1 | steady | 1,446 | 1.146 / 1.541 | 48.3 | 1,107.7 |
| 1 | burst | 1,441 | 1.126 / 1.559 | 486.9 | 1,557.9 |
| 2 | steady | 1,434 | 1.123 / 1.585 | 46.3 | 1,104.5 |
| 2 | burst | 1,523 | 1.096 / 1.508 | 490.0 | 1,567.0 |
| 3 | steady | 1,444 | 1.122 / 1.490 | 48.9 | 1,112.2 |
| 3 | burst | 1,482 | 1.137 / 1.533 | 479.0 | 1,532.5 |

**Sanity check, passed**: the burst group's own throughput grew
9.80-10.58x across the 3 runs — matching the intended 10x weight
multiplier almost exactly, confirming the weight-distribution mechanism
itself worked correctly before trusting the bystander comparison built
on it.

**Bystander p99 ratio (burst/steady): 0.95x-1.03x. Bystander p50 ratio:
0.98x-1.01x.** Both are essentially flat — no material degradation in
either direction, in any of the 3 runs. **H16 is CONFIRMED**: the
bystander tenant's own latency is completely unaffected by a correlated
10x burst hitting 10 other tenants simultaneously, even though the
burst group's own request volume grew roughly 10-fold and aggregate
throughput across the whole population rose ~40% (1,105-1,112 rps
steady to 1,533-1,568 rps burst). The steady-state isolation properties
this project already established for the native in-process path (H2's
cross-tenant query-load isolation, H4/H11's breadth-independence,
H10's Zipfian-demand fairness) extend cleanly to this correlated-burst
regime — a real, positive, directly-relevant answer to Phase 8's own
stated thesis, using only the single-node infrastructure this
environment actually has.

**Named limitations**: only one burst multiplier (10x) and one burst
group size (10 of 55 tenants, ~18%) were tested; Issue #21's own
5x/20x alternatives and its Regime C ("BFCM shock," majority/all
tenants) are untested. Only one bystander tenant was tracked; whether
a tenant closer in rank to the burst group (i.e., adjacent in the
weight distribution rather than at rank 10) would show a different
result is untested. This experiment tests QUERY-load burst only, not a
combination with H14's already-confirmed rebuild-churn isolation gap
or H15's already-confirmed shared-Solr contention — whether either of
those two real gaps gets WORSE under a correlated burst (a natural,
higher-value follow-up given they are this project's two known real
limitations) is untested here.
