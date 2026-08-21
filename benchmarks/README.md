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
