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

## 2026-09-30 — heap pre-pass: the invalid first run and C7a

**First run: invalid.** It started 07:38Z and was stopped at 07:45Z, before any envelope window. It is preserved at `results/prepass_INVALID_exact_f_order/`.
- 1g was structurally equivalent, but 478 of 480 F responses differed from the frozen ids in exact order.
- **Cause:** F ranks variant documents, and variants share their text, so exact BM25 ties are common. Order among ties follows docids, and docids differ between fresh indexes.
- **Consequence:** C7's exact-order rule was mis-specified. It would have excluded every heap.
- **Replacement:** clarification C7a, the tie-aware check in `f_equiv.py`, which has unit tests. It was posted on #66 before the rerun.

**Rerun (07:46–07:56Z).**
- **3g control, fresh index.** Structurally clean. Against the frozen ids: `num_found` 480/480, **same id set 425/480, same exact order 2/480**. Exact F order does not reproduce across fresh indexes even at the frozen 3g heap.
- **1g.** Structurally clean, and F is tie-aware EQUIVALENT to the 3g control: 480/480 on `num_found`, score sequence and ids above the boundary. Exact order matches on 2/480.
- **512m.** Same as 1g: clean and EQUIVALENT.
- **Consequence:** all three heaps are admitted to the RAM search.

**Implication for #65 (a disclosure; nothing in #65 relied on the following).**
- Across launches, F responses are equivalent only up to tie order, and only 425/480 have the same top-48 id set, because of tied groups cut at the boundary.
- #65's lexical equivalence claim is unaffected, because it was made *within* a launch: the router responses were byte-identical to direct Solr.
- #65's in-load F check was `num_found` only.

**Measured chain restarted:** cpu → cpusens → mem → cpuconfirm → memconfirm → joint, at 07:57Z.

## 2026-09-30 → 2026-10-01 — measured phases

- **Phase times (UTC):**
  - cpu 07:57–10:35
  - cpusens 10:35–12:26
  - mem 12:26–15:27
  - cpuconfirm 15:27–22:35
  - memconfirm 22:35–23:49
  - joint 23:49–01:04
- **Integrity:** 204 measured windows, with 0 check failures, 0 HARNESS_SATURATED, and sequences identical across treatments for every key.
- **Confirmed minima.** All are in `results/report.md`; the decision doc §3–§5 has the details.

  | | T1 | T2 | T3 |
  |---|---|---|---|
  | B0 cores, S2 | 1.5 | 2 | INFEASIBLE |
  | H1 cores, S2 | 0.75 | 1 | 1.5 |
  | B0 cores, S1 | INFEASIBLE | INFEASIBLE | INFEASIBLE |
  | H1 cores, S1 | 1 | 1 | 2 |

  RAM at T2/S2, best heap 512m: B0 1 GiB (the ladder floor), H1 5 GiB.
- **Joint confirmations:** B0 (2, 1 GiB) 0/3; H1 (1, 5 GiB) 2/3.
- **Gate, per `analyze_i66.py`:** KEEP via cores. The T2/S2 ratio is 0.5, and T1's 0.5 shows it is robust. The GiB ratio is a lower bound of ≥ 4x, with H1 the larger. u = 0.625 is flagged as not jointly confirmed.
- **Step-ups:**
  - H1 needed one-level step-ups at T1 S1 and S2, T2 S2, and T3 S1.
  - B0 needed none.
  - The descent candidates that failed confirmation (0/3) were all H1's.

## 2026-10-01 — adversarial review

The review was fresh and read-only, and recomputed everything from raw data. It confirmed the minima, the step-ups, the gate, sequence identity, the code against the preregistration, and C7a. Findings and dispositions are in the decision doc §8. The headline disclosure: KEEP is on CPU only, H1 needs ≥ 4x RAM, and the machine-level result depends on shape.

Changes made:
- **GiB ratio.** `analyze_i66.py` now labels the GiB ratio as a lower bound when B0 passes at the ladder floor.
- **Stop-condition check, done post hoc.**
  - One ambient probe was outside #65's range: 5.16 ns/iteration against a maximum of 5.05. It was taken after B0's T2 descent collapsed (load average 25). The next probe read 3.70, and no measured window falls between the two. It is judged self-induced.
  - The host had about 1.1 GiB of pre-existing swap, with `SwapFree` flat at ≈ 2.93 GiB throughout.
  - Neither condition is auto-enforced by the harness.
- **Log time correction for the previous entries.** C7a was posted at 07:44:19Z, and the rerun pre-pass started at 07:44:27Z. The "07:45 stop / 07:46 rerun" times recorded above are approximate.
