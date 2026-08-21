# Benchmarks — reproducibility structure

This directory holds the declarative side of every benchmark: what to run
and how, as opposed to `artifacts/`, which holds the record of what was
actually run and what it found.

```text
benchmarks/
  manifests/   -- what to run: dataset, workload, engine(s), config, repetitions
  workloads/   -- workload definitions/generators referenced by manifests
  analysis/    -- scripts that turn raw results into normalized tables
```

## Current state (through Phase 5)

Phases 0–5 predate this directory and ran their workload generation,
execution, and analysis directly inside each experiment's own binary
(`crates/phase{2,3,4,5}-eval/src/bin/pXeNN_*.rs` — one binary per
experiment, printing its own summary table and writing its own CSV/log
under `docs/research/artifacts/pXeNN_run1/`). That is not being
retroactively rewritten into this structure — see `artifacts/README.md`
for why the historical raw artifacts stay where they are.

`benchmarks/manifests/` currently holds a small number of **worked
examples** — real manifests for already-promoted headline results,
written after the fact, to establish the pattern Phase 6 onward should
follow going forward for *new* experiments (manifest written first,
execution and analysis driven from it, not the other way around):

- `p3e16_finegrained_frontier.yaml` — Phase 3's promoted combined admission
  frontier.
- `p4e01_implication_propose_replay_promote.yaml` — Phase 4's promoted
  Brand-implication mechanism.
- `p5e03_facet_scan_crossover.yaml` — Phase 5's facet-scan crossover
  characterization.

Phase 6A (Issue #23, `crates/phase6a-eval`) follows the same
retroactive-manifest convention, not the "manifest-first" ideal above —
its own benchmark binaries still generate their own workload internally,
same as Phases 0–5:

- `p6a_e00_wands_plp_benchmark.yaml` — the real WANDS PLP filter/facet/
  sort/pagination benchmark and its facet-crossover characterization.
- `p6a_e01_concurrency_sweep.yaml` — the WANDS concurrency sweep.
- `p6b_e00_scale_ladder.yaml` — the WANDS controlled-stress scale-ladder
  follow-on (Retailrocket/H&M/Amazon Reviews 2023/Havenask all confirmed
  blocked; see `PHASE6B_DECISION.md`), including the numeric-range filter
  operator never testable in Phase 6A.
- `p7_e00_tenant_packing.yaml` — the first multi-tenant packing-density
  measurement (Issue #21 Phase 7), using real WANDS category partitions
  as the tenant model; see `PHASE7_DECISION.md`.
- `p7_e01_qps_scaling.yaml` — fixed-tenant throughput/latency as the
  breadth of other, concurrently-touched tenants grows; see
  `PHASE7_DECISION.md`.
- `p7_e02_packing_ceiling.yaml` — controlled-stress tenant-count
  replication testing the packing ceiling (H3) by proxy; see
  `PHASE7_DECISION.md`.
- `p7_e03_cross_process_fixed_cost.yaml` — real per-OS-process baseline
  overhead vs. in-process pooling, the first measured test of
  `docs/WHY.md`'s statistical-multiplexing thesis; see
  `PHASE7_DECISION.md`.
- `p7_e04_long_running_overhead.yaml` — genuinely long-running (not
  spawn-and-exit) resident-process RSS, closing the gap between P7-E03's
  short-lived floor and a real deployed service's actual cost; see
  `PHASE7_DECISION.md`.
- `p7_e05_extended_duration_overhead.yaml` — a 9x longer resident window
  confirming P7-E04's still-rising RSS curve for the largest real tenant
  decelerates toward a plateau rather than growing without bound; see
  `PHASE7_DECISION.md`.
- `p7_e06_cold_tenant_overhead.yaml` — Issue #21's explicitly-named "cold
  tenant overhead" metric: a real, reproducible latency-ratio effect
  between an infrequently- and a continuously-queried same-sized
  tenant, at a practically negligible absolute scale; see
  `PHASE7_DECISION.md`.
- `p7_e07_realistic_demand_mix.yaml` — a replication check of P7-E06's
  finding under a materially different, more realistic full-population
  Zipfian query-arrival pattern: the direction replicates, the magnitude
  does not (roughly 4-6x smaller); see `PHASE7_DECISION.md`.
- `p7_e08_extended_breadth_qps.yaml` — extends P7-E01's fixed-tenant
  throughput/latency-under-breadth finding from WANDS' real 54-other-
  tenant ceiling to 2,000 controlled-stress-replicated tenants (36x
  larger), confirmed cleanly across 3 runs; see `PHASE7_DECISION.md`.
- `p7_e09_slo_tenant_envelope.yaml` — Issue #21's "tenants per fixed
  hardware envelope at target SLO" metric: combines P7-E02/H5's memory
  model with P7-E01/P7-E08's latency findings, discovering this
  container's real cgroup memory limit directly after a first-draft OOM,
  then safely reaching ~3,500 query-capable tenants under a disclosed
  envelope with throughput/latency essentially unaffected; see
  `PHASE7_DECISION.md`.
- `p7_e10_cpu_per_query.yaml` — Issue #21's "CPU/query and CPU/tenant"
  metric: unlike memory's clean linear scaling (H1/H5), CPU cost per
  facet-scan query is sub-linear at small tenant sizes then
  super-linear at large ones, reproduced across 3 runs; see
  `PHASE7_DECISION.md`.
- `p7_e11_high_churn_impact.yaml` — Issue #21's "high-churn tenant
  impact on low-churn tenants" metric: unlike H2's null result for pure
  query load, a co-located tenant undergoing repeated index rebuilds
  materially degrades another tenant's own p99 latency (4.00-6.70x,
  reproduced across 3 runs) — a genuine, actionable isolation gap; see
  `PHASE7_DECISION.md`.
- `p7_e12_lexical_backend_contention.yaml` — Issue #21's
  "lexical-backend contention" metric, the last item on Issue #21's
  required Experiments list: sharing one Solr instance across tenants
  materially degrades a quiet tenant's own p99 latency (2.16-2.48x,
  reproduced across 3 runs after a self-caught JVM cold-start artifact
  was fixed) — Phase 7's third distinct isolation-gap finding; see
  `PHASE7_DECISION.md`.
- `p8_e00_partial_burst.yaml` — Phase 8's first experiment (Issue #21
  Regime B, partially correlated burst): a fixed group of 10 real
  tenants' traffic weight multiplied 10x mid-experiment does not
  measurably degrade an unrelated tenant's own latency (p99 ratio
  0.95x-1.03x, reproduced across 3 runs) — Phase 7's steady-state
  isolation properties extend to a correlated-burst regime; see
  `PHASE8_DECISION.md`.
- `p8_e01_burst_amplified_churn.yaml` — does a correlated burst make
  H14's already-confirmed rebuild-churn isolation gap worse? Yes:
  median amplification 3.62x across 10 runs (after a self-caught fix
  from an initial too-noisy 3-run pass), and more importantly, burst
  converts an intermittent >=2x-degradation coincidence (3/10 idle
  runs) into a near-certain one (10/10 burst runs) — a genuine new
  isolation gap under burst that neither H14 nor H16 alone could
  surface; see `PHASE8_DECISION.md`.
- `p8_e02_burst_amplified_solr_contention.yaml` — the symmetric
  question for Phase 7's other known real isolation gap: does a
  correlated burst (more tenants' traffic joining the shared Solr
  instance) make H15's shared-Solr-contention gap worse? Yes,
  cleanly across all 10 runs with no methodology fix needed this time
  (H17's lesson applied proactively): median amplification 1.80x
  (every individual run 1.60x-2.00x), and the same >=2x-degradation
  hit-rate escalation (5/10 idle runs to 10/10 burst runs); see
  `PHASE8_DECISION.md`.
- `p8_e03_combined_churn_solr_interaction.yaml` — does running H14's
  rebuild-churn and H15's shared-Solr-contention mechanisms
  SIMULTANEOUSLY compound the two isolation gaps beyond either alone?
  A genuine asymmetric finding: combined load degrades the native
  quiet tenant's own latency by 2.11x-3.37x in every one of 20
  measured runs (more reliably than either mechanism alone), but does
  NOT measurably worsen the Solr-side contention. Includes a
  self-caught, honestly-disclosed measurement-window subtlety; see
  `PHASE8_DECISION.md`.
- `p6c_e00_lucene_direct.yaml` — a retroactive Phase 6 audit run after
  Phase 8: is cross-engine validation actually complete when Solr has
  been the only baseline through Phase 8? Live re-check found
  Havenask/Elasticsearch/OpenSearch/Retailrocket/H&M/Amazon Reviews
  2023 all still blocked, but Maven Central is reachable — the first
  raw-Apache-Lucene-direct baseline this project has ever run. First-pass
  finding (superseded by `p6c_e01_lucene_facet_module.yaml` below): a
  naive, hand-rolled Lucene facet-scan loses to Solr's own facet.field
  implementation in 5 of 7 real checkpoints (up to 3.3x-4.0x); see
  `PHASE6C_DECISION.md`.
- `p6c_e01_lucene_facet_module.yaml` — an adversarial self-check of
  P6C-E00's own finding: was "Solr beats raw Lucene" actually true, or
  only true of one naive facet-scan implementation? Re-measured with
  Lucene's own dedicated `SortedSetDocValuesFacetCounts` module instead
  of the hand-rolled scan. Result substantially reverses: Lucene's own
  specialized mechanism beats Solr in 5 of 7 checkpoints (up to 3.0x),
  trailing by a much smaller margin (1.11x-1.30x, not 3.3x-4.0x) in the
  remaining 2 — sharpening the facet-crossover finding into a claim
  about facet algorithms specifically, not generic-engine vs.
  commerce-native faceting; see `PHASE6C_DECISION.md`.
- `p6d_e00_ordinal_facet_counting.yaml` — Phase 6D built the candidate
  fix P6C-E01 surfaced: an ordinal/dictionary-based
  `facet_counts_ordinal` on `CatalogIndex` itself, correctness-gated
  against both `facet_counts_by_scan` (unit test) and Solr's own live
  facets (21/21 exact matches). Result: beats Solr at every one of 7
  real checkpoints (5.2x-69.8x, no exceptions) and beats
  `facet_counts_by_scan` (23.5x-89.3x) — a larger, more consistent
  margin than Lucene's own equivalent module achieved over Solr; see
  `PHASE6D_DECISION.md`.
- `p6d_e01_ordinal_facet_scale_ladder.yaml` — does the ordinal method's
  margin hold across Phase 6B's own 2x-20x controlled-stress scale
  ladder, not just WANDS' natural 1x scale? Beats Solr at all 35
  checkpoint x tier combinations tested (2,002-320,780 candidates), zero
  exceptions -- but the margin narrows (not grows) at the largest
  candidate counts, converging toward ~2.5x-3x; its margin over the
  scan method grows sharply with scale instead; see `PHASE6D_DECISION.md`.

`benchmarks/workloads/` and `benchmarks/analysis/` remain **not**
populated for Phases 0–6A for the same reason (see `artifacts/README.md`)
— the real workload logic and analysis already live inside the
corresponding `crates/phaseN(a)-eval` binaries and are the canonical
source; duplicating them here would create two versions of the truth.
They will hold real content once a later phase genuinely needs shared
workload definitions or cross-engine analysis scripts spanning more than
one binary (e.g. a true multi-engine Phase 9 comparison).

## The traceability chain this repository commits to

For every result promoted in a `PHASE*_DECISION.md`, the following chain
should be walkable, even where — for Phases 0–5 — some links currently
point into a `crates/phaseN-eval` binary rather than a `benchmarks/`
manifest:

```text
claim (PHASE*_DECISION.md)
  -> figure/table (docs/experiments/PHASE*_LOG.md's per-experiment entry)
  -> analysis (the experiment binary's own summary/CSV output, or
     benchmarks/analysis/ for Phase 6+)
  -> raw results (docs/research/artifacts/pXeNN_run1/, or artifacts/ for
     Phase 6+)
  -> experiment manifest (benchmarks/manifests/*.yaml for the worked
     examples above; the experiment binary's own header comment for
     everything else)
  -> workload/dataset/engine/config/git SHA (recorded in each manifest,
     or in the experiment log's "environment" note)
```

## Reproduction

For the worked-example manifests, full reproduction is:

```bash
cargo build --release -p phase3-eval  # or phase4-eval / phase5-eval / phase6a-eval
./target/release/<binary-named-in-the-manifest> [args from the manifest]
```

This requires the real ESCI catalog (`scripts/round1/fetch_esci.sh` +
`export_esci.py`) for the Phase 3–5 manifests, or the real WANDS catalog
(`scripts/datasets/fetch_wands.sh` + `prepare_wands.py`) for the Phase 6A
manifests, and a running Solr instance indexed per
`scripts/round1/solr_index.py` or `scripts/datasets/solr_index_wands.py`
respectively. Analysis-only reproduction (no dataset/engine required)
means reading the already-archived raw output directly from the
`artifacts/manifests/` entry's referenced path under
`docs/research/artifacts/`.
