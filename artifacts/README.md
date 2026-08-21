# Artifacts — the record of what was actually run

This directory is the archived-record counterpart to `benchmarks/`
(the declarative "what to run"). It holds, per promoted result:

```text
artifacts/
  manifests/   -- per-run archived metadata: git SHA, dataset version/checksum,
                  seed, engine config, hardware, repetitions, raw-output
                  location, analysis version, decision
  summaries/   -- small normalized result tables derived from the raw output
  figures/     -- generated charts (none yet -- see below)
```

## Why Phase 0–5's raw artifacts are not here

All of Phase 0 through Phase 5's raw experiment output (per-query CSVs,
full run logs, manifests written inline in each `full_run_output.log`)
already lives under `docs/research/artifacts/pXeNN_run1/`, one directory
per experiment, and is referenced by exact path from every
`PHASE*_DECISION.md` and `docs/experiments/PHASE*_LOG.md` entry. Those
documents are a frozen historical record — moving the underlying files to
`artifacts/` would silently invalidate every one of those path references
and would violate `CLAUDE.md`'s "do not rewrite history" rule and Issue
#21's own archive discipline ("never silently edit historical experiment
conclusions"). They stay exactly where they are.

`artifacts/manifests/` currently holds ten **worked-example** records —
written after the fact, alongside their matching `benchmarks/manifests/`
entries — for Phase 3/4/5/6A/6B/7's actual headline promoted results:

- `p3e16_finegrained_frontier.json`
- `p4e01_implication_propose_replay_promote.json`
- `p5e03_facet_scan_crossover.json`
- `p6a_e00_wands_plp_benchmark.json`
- `p6a_e01_concurrency_sweep.json`
- `p6b_e00_scale_ladder.json`
- `p7_e00_tenant_packing.json`
- `p7_e01_qps_scaling.json`
- `p7_e02_packing_ceiling.json`
- `p7_e03_cross_process_fixed_cost.json`
- `p7_e04_long_running_overhead.json`
- `p7_e05_extended_duration_overhead.json`
- `p7_e06_cold_tenant_overhead.json`
- `p7_e07_realistic_demand_mix.json`
- `p7_e08_extended_breadth_qps.json`
- `p7_e09_slo_tenant_envelope.json`
- `p7_e10_cpu_per_query.json`
- `p7_e11_high_churn_impact.json`
- `p7_e12_lexical_backend_contention.json`

Each points at its real `docs/research/artifacts/pXeNN_run1/` (or
`p6a_e0N_*_run1/`) location rather than duplicating the data.
`artifacts/summaries/` and `artifacts/figures/` are **not yet
populated**: no chart-generation tooling exists in this repository yet,
and adding placeholder or retroactively-generated figures would overstate
what has actually been built. This is stated explicitly rather than
filled with stand-ins.

Phase 6B onward should archive new promoted results directly under this
structure as they are produced, rather than repeating Phases 0–6A's
per-binary-console-output convention.
