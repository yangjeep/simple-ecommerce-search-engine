# Issue #66 — Infra E6: fixed workload → minimum CPU/RAM envelope (experiment log)

This log is append-only. The protocol is #66 amendment 1 plus clarifications C1–C7, as posted on the issue.

## 2026-09-30 — preconditions

- **Main and CI.** `main` = `2f5d90d`, the #65 merge (PR #84). The PR's CI was green, and push CI **fired and passed** on the merge commit (Rust CI, Push on main). The `778e85c` trigger anomaly did not repeat.
- **Branch.** `i66/resource-envelope`, cut from `2f5d90d`.
- **Local gate.** The full local gate last ran green on the #65 PR head (817 tests), which has the same tree as `2f5d90d`.
- **Host.** 4 vCPUs and 29 GiB RAM. The serving slice is CPUs 0–2 and the generator runs on CPU 3, so the envelope is at most 3 cores.

## 2026-09-30 — protocol-feasibility pilot (declared on #66 before it ran; no SLO verdict read)

Results are in `artifacts/issue66/results/pilot/`.

**1. Idle Solr CPU.** After provisioning and force-merge, idle Solr uses **0.014–0.019 cores** over 300 s, and 0.015–0.022 cores in the 180 s after a 50-QPS window.
- This falsifies the "load-independent Solr background cost" explanation in #65 §3.5. The flat delegate CPU there is more likely warm-up confounded with window order.
- A correction is posted on #65, and a dated note is added to the #65 decision doc on this branch.

**2. Live `CPUQuota` binds.** `cpu.max` updates, and the throttle counters grow.

| quota | cpu.max | CPU used (cores) | P95 at 20 QPS | P99 at 20 QPS |
|---|---|---|---|---|
| 1.0 | 100000 | 0.49 | 82 ms | 211 ms |
| 0.5 | 50000 | 0.46 | 605 ms | 1,296 ms |

B0 costs 21.6–23.2 ms of CPU per query here, early in the launch.

**3. A 1g Solr heap is equivalent and saves about 2 GiB.**
- The Solr pre-pass at 1g: 0 structural non-equivalences, 0 exclusions, 480 F recorded.
- At 50 QPS the two heaps match on CPU and latency, but not on memory:

  | heap | CPU/query | P95 | P99 | cgroup memory |
  |---|---|---|---|---|
  | 3g | 23.3 ms | 68 ms | 118 ms | 3.68 GiB |
  | 1g | 23.2 ms | 66 ms | 124 ms | **1.67 GiB** |

**Consequence:** amendment 1 posted, with S2 = 2x S1, the heap ladder {3g, 1g, 512m}, a 300 s warm-up and fresh-launch confirmations.

## 2026-09-30 — harness

**Scripts** (`scripts/issue66/`):
- `run_i66.sh`: phases pilot, smoke, prepass, cpu, cpusens, mem, cpuconfirm, memconfirm and joint.
- `judge.py`: judges one window against S1 and S2.
- `candidates.py`: the monotone-prefix candidate and the step-up level.
- `plan.py`: confirmation, step-up and joint job plans, plus the summary.
- `analyze_i66.py`: the frontier, the minima and the gate.
- `test_plan.py`: 13 synthetic tests covering the judge, candidates, plan and verdict.

**Other changes:**
- `scripts/issue77/provision_solr.sh` gets an optional `I66_SOLR_HEAP` override. When it is unset, the frozen heap is used.
- The treatments, pools, load generator and slice are unchanged from #65.

**Smoke test** (`artifacts/issue66/results/smoke*`, 5/10/10 s windows, no verdict). It exercised:
- live quota binding;
- the sidecar (throttled share 0.20 at 1 core);
- the OOM path (H1 at 3 GiB: `oom_kill_delta` 1, dead, both SLOs FAIL);
- the job runner.

It also showed that warmth dominates at small envelopes. After a 10 s warm-up, a fresh H1 launch at 1 core had P99 ≈ 1 s, against 68 ms at the same level inside a warm descent. This is recorded in C1–C7; the 300 s warm-up is unchanged.

**Measured chain:** prepass → cpu → cpusens → mem → cpuconfirm → memconfirm → joint, started 2026-09-30T07:38Z.
