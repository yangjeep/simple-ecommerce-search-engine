# Issue #66 — Infra E6: fixed workload → minimum CPU/RAM envelope

**Question** (#66 amendment 1 and clarifications C1–C7, C7a; the original body is kept on the issue): at a fixed commerce workload and a fixed latency SLO, does H1 (native N1 + Solr delegate) need materially less CPU, RAM or normalized machine capacity than B0 (Solr-only), at equal correctness?

**Answer: KEEP under the preregistered gate. The win is CPU only. H1 needs ≥ 4x B0's RAM, and whether H1 is the smaller machine overall depends on the vCPU:GiB shape and was not jointly confirmed.**

At the decision point (T2 = 100 QPS on the primary mix, SLO S2), the cores ratio H1/B0 is **0.5**.
- **H1:** 1 core.
- **B0:** 2 cores.
- **Robustness:** the same 0.5 holds at T1 (0.75 vs 1.5 cores). At T3, B0 is infeasible even at 3 cores, while H1 needs 1.5.
- **RAM goes the other way:** H1 needs 5 GiB (it OOMs at 4), while B0 passes at the 1 GiB ladder floor with a 512m heap.
- **Limiting resource:** CPU for B0, RAM for H1.

| at T2 = 100 QPS, primary mix | B0 (Solr-only) | H1 (native + Solr delegate) | H1 / B0 |
|---|---|---|---|
| min cores, S2 (P95 < 100, P99 < 200), at 12 GiB | **2** (2/3 confirmed; P99 173 / 214 / 197) | **1** (2/3 after one step-up; 4 of 6 pooled fresh runs pass) | **0.50**; ladder-bracketed true range [0.375, 0.75] |
| min cores, S1 (#65 SLO: P95 < 50, P99 < 100) | INFEASIBLE ≤ 3 cores | 1 | ≤ 0.33 (bound) |
| warm CPU used at 3 cores | 1.37–1.41 cores (13.6–14.0 ms/query) | 0.60–0.67 cores (5.9–6.7 ms/query) | ≈ 0.45 |
| min RAM, S2, at 3 cores (best heap) | **≤ 1 GiB** (512m heap; passes 3/3 at the ladder floor) | **5 GiB** (512m heap; OOM at 4) | **≥ 4x** (H1 larger) |
| cgroup memory in use, 512m heap | 1.16–1.23 GiB | 4.99–5.25 GiB (native ≈ 4.1 anon + Solr ≈ 0.9) | — |
| joint (min cores, min GiB), S2 | (2, 1 GiB): **fails 0/3** (P99 216–296) | (1, 5 GiB): passes 2/3 (P99 221 / 143 / 167) | — |
| normalized u = max(cores, GiB/4) | 2.0 | 1.25 | 0.625, flagged: B0 not jointly confirmed (C3) |
| u on a 1:2 vCPU:GiB shape (non-gating) | 2.0 | 2.5 | **1.25** (H1 larger) |
| u on a 1:8 vCPU:GiB shape (non-gating) | 2.0 | 1.0 | 0.5 |

## 1. What was frozen

- **Code.** `main` = `2f5d90d` (the #65 merge). PR CI was green, and push CI fired and passed on the merge commit.
- **Treatments, pools, sequences, equal-work bodies, router, N1 and slice:** all from #65.
- **Envelope.** It is set live on the shared slice `i65-serving.slice`:
  - CPU via `cpu.max`, a CFS quota with a 100 ms period over cpuset CPUs 0–2;
  - memory via `memory.max`, with swap 0.
  - The load generator runs on CPU 3.
- **Ladders.**
  - CPU: {3, 2.5, 2, 1.5, 1, 0.75, 0.5} cores, at 12 GiB.
  - RAM: {12, 8, 7, 6, 5, 4, 3, 2, 1.5, 1} GiB, at 3 cores.
  - Solr heap: {3g (frozen), 1g, 512m}, applied to B0 and to H1's delegate alike. CPU descents use 3g; for RAM, each treatment's best heap counts.
- **Tiers.** The #65 primary mix at T1 = 50, T2 = 100 (the decision tier) and T3 = 200 QPS. The structural and lexical mixes run at T2 as non-gating sensitivity (C1).
- **SLOs.** S1 is #65's SLO and S2 doubles it. Both share these conditions:
  - error rate < 0.1%;
  - achieved ≥ 98% of offered;
  - not HARNESS_SATURATED;
  - no correctness-check failure;
  - the serving process is alive and was not OOM-killed.
- **Launch procedure.** Every launch is fresh, then passes a quiescence check (idle < 0.1 cores) and a 300 s unmeasured warm-up at the tier rate.
- **Windows.** Each window is a 60 s settle followed by a 120 s measurement.
- **Candidate rule.** The candidate minimum is the lowest level passing the SLO with every higher level also passing (monotone prefix).
- **Confirmation rule.** A candidate is confirmed by 3 fresh launches, warm-up first and then the level, needing ≥ 2/3 to pass. Failing that, there is one step-up per SLO.

## 2. Correctness

- **Heap pre-pass (C7a).** The first, exact-order F check was invalid. It is preserved and explained in §6.
- **Rerun with a fresh 3g control.**
  - 1g and 512m: 0 structural non-equivalences and 0 exclusions.
  - F, tie-aware against the 3g control: **480/480 EQUIVALENT**, with identical `num_found`, identical top-48 score sequences, and identical ids above the boundary score.
  - The 3g control against the #65 frozen ids: `num_found` 480/480 and the same id set 425/480. The review confirmed that all 55 set differences are ids tied at the boundary score.
- **In-load checks.** Across all 204 measured windows, there were **0 check failures** and 0 HARNESS_SATURATED. Sequences were identical across treatments for every (mix, rate, window) key.
- **Native fixture.** #77's cross-variant fixture reported `all_passed=true` on all 46 H1 launches. The review verified this post hoc; the harness does not enforce it.
- **Scope limit.** As in #65, the in-load check covers `num_found` plus the class-A id. Facets, sort order and F ids are not re-verified under resource pressure.

## 3. CPU envelope (primary mix; 12 GiB; heap 3g)

### 3.1 Confirmed minima

| tier | B0 S1 | B0 S2 | H1 S1 | H1 S2 | S2 ratio |
|---|---|---|---|---|---|
| T1 = 50 QPS | INFEASIBLE | 1.5 (3/3) | 1 (step-up, 3/3) | 0.75 (step-up, 3/3) | **0.50** |
| T2 = 100 QPS | INFEASIBLE | 2 (2/3) | 1 (2/3) | 1 (step-up, 2/3) | **0.50** |
| T3 = 200 QPS | INFEASIBLE | INFEASIBLE (3 cores saturated, errors) | 2 (step-up, 3/3) | 1.5 (2/3) | ≤ 0.50 (bound) |

### 3.2 Descents at T2 (one launch each; levels top to bottom)

| cores | B0 P95 / P99 | B0 cores used | B0 | H1 P95 / P99 | H1 cores used | H1 |
|---|---|---|---|---|---|---|
| 3 | 66 / 129 | 1.41 | S2 | 21 / 38 | 0.67 | S1, S2 |
| 2.5 | 68 / 138 | 1.41 | S2 | 20 / 30 | 0.64 | S1, S2 |
| 2 | 83 / 190 | 1.37 | S2 | 19 / 27 | 0.62 | S1, S2 |
| 1.5 | 331 / 947, errors | 1.38 | — | 19 / 26 | 0.62 | S1, S2 |
| 1 | collapse | 1.00 | — | 20 / 62 | 0.60 | S1, S2 |
| 0.75 | — | — | — | 62 / 139 | 0.60 | S2 |
| 0.5 | — | — | — | collapse | 0.50 | — |

**Reading the minima.**
- **B0 really needs more than 1.5 cores at 100 QPS.** At 1.5 it uses 92% of its quota, and its tail and errors explode.
- **H1 runs at about 0.6 cores.** Its minimum is set by quota-throttling tails, not by mean CPU.
- **The ladder brackets the ratio.** H1's true minimum is in (0.75, 1] and B0's in (1.5, 2], so the true ratio is in [0.375, 0.75]: under the bar, reaching it only at the extreme corner. Mean CPU agrees, at ≈ 0.45.

**Fragility, disclosed.**
- **H1's descent candidates were optimistic.** They failed confirmation 0/3 at T1 0.5 (S2), T1 0.75 (S1), T2 0.75 (S2) and T3 1.5 (S1). B0's candidates held. Confirmation corrected this through step-ups. H1 is more warmth-sensitive at small quotas.
- **H1's 1-core T2 minimum is marginal.** It passed 4 of 6 pooled fresh runs, and both failures had P99 ≈ 313–315 ms.
- **B0's 2-core minimum is also marginal.** P99 was 173 / 214 / 197 against the 200 ms bound.

### 3.3 Sensitivity at T2 (descent only, non-gating; C1)

| mix | B0 S2 candidate | H1 S2 candidate | ratio | B0 / H1 cores used at 3 cores |
|---|---|---|---|---|
| structural (F 10%) | 2 | 0.75 | 0.375 | 1.25 / 0.57 |
| lexical (F 50%) | 2 | 1 | 0.5 | 1.32 / 0.78 |

Under S1, B0 is INFEASIBLE in both mixes. H1's S1 candidate is 0.75 for structural and 1 for lexical.

## 4. RAM envelope (T2; 3 cores)

| heap | B0 lowest passing (S2) | B0 memory in use | H1 lowest passing | H1 failure below |
|---|---|---|---|---|
| 3g | 4 GiB | 3.93–3.97 GiB | 8 GiB | OOM at 7 |
| 1g | 1.5 GiB | 1.50–1.77 GiB | 6 GiB | OOM at 5 |
| 512m | **1 GiB (ladder floor)** | 1.00–1.23 GiB | **5 GiB** | OOM at 4 |

**Confirmations (3 fresh launches each).**
- B0 at (512m, 1 GiB): S2 3/3; S1 is never met.
- H1 at (512m, 5 GiB): S1 and S2 both 3/3.
- No step-up was needed.

**How to read this.**
- **H1's memory is dominated by native's anonymous memory, about 4.1 GiB at any heap.** That is the in-memory catalog and index, E2's REFINE finding. Solr's share at 512m is about 0.9 GiB.
- **H1 at 5 GiB has only about 0.11 GiB of page cache left**, which is right at the edge.
- **B0's true minimum is ≤ 1 GiB,** because the ladder floor truncates it. The H1/B0 RAM ratio is therefore **≥ 4** (H1 > 4 GiB, B0 ≤ 1 GiB).

## 5. Joint envelope and machine units (T2, S2)

**Joint confirmation.**
- **B0 at (2 cores, 1 GiB, 512m) fails 0/3** (P99 216–296, throttled share 0.21–0.27). Under combined CPU and memory pressure, B0 needs more than its separately confirmed minima.
- **H1 at (1 core, 5 GiB, 512m) passes 2/3** (P99 221 / 143 / 167).

**Normalized units, u = max(cores, GiB/4).**
- H1 1.25 vs B0 2.0, a ratio of **0.625**.
- Under C3 it is **flagged**, because B0 is not jointly confirmed. A flagged u cannot produce KEEP on its own, and the verdict does not rest on it.
- B0's true joint minimum is at least 2 units (it needs ≥ 2 cores), so the direction of the u comparison would not reverse on a 1:4 shape.
- With one more GiB of headroom for H1, which its thin page cache suggests, H1's u rises to 1.5 and the ratio to 0.75.

**The answer depends on the machine shape.**

| vCPU:GiB shape | H1 u | B0 u | which is smaller |
|---|---|---|---|
| 1:2 (compute-optimized) | 2.5 | 2.0 | **B0** |
| 1:4 (general purpose) | 1.25 | 2.0 | H1 |
| 1:8 (memory-optimized) | 1.0 | 2.0 | H1 |

## 6. Invalid and corrected evidence (preserved)

- **`results/prepass_INVALID_exact_f_order/`: the first heap pre-pass.**
  - C7's exact-order F rule would have excluded every heap. F ranks variant documents whose BM25 scores tie exactly, and tie order follows docids, which differ between fresh indexes.
  - The run was stopped before any envelope window.
  - C7a, the tie-aware rule with a 3g control, was posted at 07:44:19Z, before the rerun started at 07:44:27Z. The earlier log entry's "07:45 / 07:46" times are slightly off.
  - The first run deleted its re-recorded pools.
- **#65 §3.5's "load-independent Solr background cost" explanation is falsified by the pilot,** which measured idle Solr at about 0.016 cores. A dated correction note is in the #65 decision doc.

## 7. Limitations

- **"Cores" means CFS quota on 3 CPUs, not a k-vCPU machine.** B0's failures at its minimum are throttling-driven. A cpuset-based envelope (fewer CPUs, no quota) was not measured. Utilization, at 92% of quota for B0 at 1.5 cores, suggests the ratio would survive, but this is untested.
- **S2 is a relaxed SLO, 2x #65's.** It was preregistered because B0 is infeasible under S1 at every envelope. Under S1 the result is feasibility (B0 INFEASIBLE ≤ 3 cores), not a resource ratio.
- **Warmth and ordering.**
  - Descents replay the same measured sequence at each level after one warm-up, so Solr caches favour later levels.
  - Fresh-launch confirmations remove this, at the cost of H1's optimistic descent candidates.
- **The heap is a configuration choice.** The frozen 3g heap and the #77/#57 baseline make B0's RAM look 4x larger than it needs to be. With a tuned heap, B0 is the smaller-RAM system by a wide margin.
- **Single host and scope.** One KVM host, WANDS 500k, one mix family and one SLO pair.
- **Stop-condition monitoring.**
  - Ambient probes ran only around descents, not around confirmations or joints.
  - **One probe was outside #65's range:** 5.16 ns/iteration against a maximum of 5.05. It was taken immediately after B0's T2 descent collapsed at 1 core (load average 25). The next probe was 3.70, and no measured window falls between the two. It is judged self-induced, not host drift.
  - **The host carried about 1.1 GiB of pre-existing swap**, from before the pilot. `SwapFree` was flat at ≈ 2.93 GiB throughout, with no active swapping; the slice runs with swap 0.
  - Neither condition is enforced automatically by the harness.

## 8. Adversarial review

A fresh, read-only reviewer recomputed every window from the raw JSON and sidecars.

**What it confirmed:**
- the minima, the step-ups and the gate;
- identical sequences across treatments;
- the code against the preregistration (monotone prefix, 2/3 rule, single step-up, C2 grouping, C3 flag, verdict logic);
- the C7a tie-aware rule;
- 0 check failures.

**Its findings and their dispositions:**

| # | finding | severity | disposition |
|---|---|---|---|
| 1 | Robustness also accepts the T3 bound, though T1 is exact | low | Disclosed. T1's exact 0.5 suffices. |
| 2 | H1's descent candidates failed confirmation 0/3 four times; its 1-core T2 minimum passed 4 of 6 pooled runs | low | Disclosed (§3.2). |
| 3 | "Cores" is a CFS quota, not a vCPU count | medium | Disclosed (§7). |
| 4 | KEEP is correct under the gate but misleading as a headline: H1 needs ≥ 4x RAM, the GiB figure is a lower bound, and the machine-level result depends on shape | high (wording) | The headline is reworded to "KEEP (CPU only)". The GiB ratio is reported as a lower bound (`analyze_i66.py`). Shape sensitivity is shown in §5. |
| 5 | The native fixture is not enforced by the harness; in-load check scope; log times | low | Disclosed (§2, §6). |
| 6 | Ambient-probe gaps; one out-of-range probe; pre-existing swap | low–medium | Disclosed and assessed (§7). |
| 7 | Descents replay the same sequence at every level | low | Disclosed (§7); confirmations are fresh. |

## 9. Consequence

- **#60 (fixed workload → minimum resources).** The infrastructure-reduction thesis is **KEPT for CPU**: H1 needs about half of B0's CPU quota at 50–100 QPS on this mix, and B0 cannot serve 200 QPS within 3 cores at all. It is **not kept for RAM**: H1 needs ≥ 4x, driven by native's in-memory index (E2 REFINE).
- **Machine level.** On general-purpose or memory-rich shapes (1:4, 1:8), H1 is the smaller machine. On compute-optimized 1:2 shapes, it is not. The joint advantage was not confirmed.
- **The biggest lever is native's ≈ 4.1 GiB footprint,** E2's REFINE item. Halving it would make H1 smaller at every shape tested here.
- **For #67 (hybrid offload economics), three inputs:**
  - the warm CPU ratio (≈ 0.45 at 100 QPS);
  - the RAM penalty;
  - B0's latency infeasibility under the strict SLO.
