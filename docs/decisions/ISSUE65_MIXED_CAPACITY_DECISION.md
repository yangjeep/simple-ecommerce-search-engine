# Issue #65 — Infra E5 (amended): realistic mixed workload and max sustainable QPS/core

**Question** (#65 amendment 1, with clarifications C1 and C1a; the original body is kept on the issue): if structural/PLP/facet work and delegated lexical work share one bounded serving system, does the measured execution advantage survive concurrent load? The test is materially higher sustainable QPS/core at the same correctness/relevance and latency SLO.

**Answer under the preregistered rule: BROAD CAPACITY ADVANTAGE.** The rule is met through its c route:

- c = 0.658 ≤ 0.75;
- H1's P99 is under the SLO at H1's confirmed max;
- the sensitivity condition holds.

Taken at face value, the result is two different findings:

1. **A large latency-feasibility advantage.** Under the frozen SLO, H1 is confirmed at **383.2 QPS**. B0 (Solr-only) cannot hold the SLO reliably at any load from 4.6 to 20 QPS: its P95 sits at about 50–64 ms. What limits B0 is the latency of Solr's complete-bucket facets and of cold lexical queries, not CPU. B0 ran at 3–16% utilization.
2. **A modest and fragile total-CPU advantage.** At the low matched loads where both treatments were measured, H1 uses **0.63–0.74 of B0's total serving CPU per query**. That clears the 0.75 bar only narrowly. On the warm sensitivity mixes the ratio is 0.48–0.80 (§3.4). A post-hoc under-load diagnostic is in §3.5.

**Not claimed:**
- a q ratio (undefined);
- a memory advantage (H1 uses 2.1x B0's cgroup memory);
- any H1 efficiency beyond 3 cores.

| | B0: Solr-only | H1: native N1 + Solr delegate (one 3-CPU / 12 GiB slice) |
|---|---|---|
| max sustainable QPS on the primary mix (P95 < 50, P99 < 100, err < 0.1%, ≥ 98% achieved) | **None confirmed.** The downward search passed at 4.0 / 4.95 / 5.43 (cold, windows of about 300 requests). Both confirmation rounds failed **9/9** at 4.6–5.7. | **383.2 QPS confirmed** (2/3 in round 2), i.e. **127.7 QPS/core** |
| total serving CPU/query at R_c = 5.4 QPS (slice counter, 3 + 3 fresh launches) | 31.0–34.0 ms | 21.4–23.0 ms → **c = 0.658** (round 2, used); 0.704 (round 1) |
| q = H1 QPS/core ÷ B0 QPS/core | — | **Undefined.** B0 has no confirmed sustainable rate, and because PASS is not monotone in rate (cold low-rate tails), no valid bound exists. |
| slice CPU utilization | 3–16% at the rates B0 was tested | 71–72% at 383.2 QPS |
| cgroup `memory.current` (not RSS; includes page cache) | 3.68 GiB | **7.82 GiB** (native 4.14 + Solr 3.60) |
| sensitivity (preregistered; requires H1 > B0) | structural 7.1 / lexical 0 | structural 19.1 / lexical 19.1. The condition holds, but H1's values are capped by cold start and are not capacities (§4). |

## 1. What was frozen

- **Code base.** `main` = `31dcfe8`, with the full local gate green (809 tests). The GitHub Actions trigger anomaly on `778e85c` is recorded in amendment §0. Experiment branch: `i65/mixed-capacity`. Fixes made after the first runs are listed in the log and in §7.
- **B0.** Solr 9.10.1 serves every class, using:
  - the #63/#64 equal-work builders (`limit:-1` facets with tagged self-exclusion);
  - #57's lexical configuration (`edismax`, `qf=title description`, default relevance).
- **H1.** N1 serves classes A–E on the native #79/#63 N⁺ path (`hybrid:hybrid:p0r`, frozen τ_F/ρ_S). N1 is also the router: it proxies class F byte-for-byte to the Solr delegate. Class G is omitted (amendment §3). Meilisearch is excluded.
- **N1 server.** `Arc<State>`, thread-per-connection keep-alive, **3 execution slots**.
- **Serving budget.**
  - One user slice, `i65-serving.slice`: `cpu.max 300000/100000`, `memory.max 12 GiB`, swap 0.
  - Every serving scope runs inside that slice on CPUs 0–2. The slice total equals native + Solr to within 0.01%.
  - The load generator runs on CPU 3, outside the slice.
- **Load generator.**
  - Open-loop, on a Poisson schedule pre-generated from (seed 65, mix, rate). `sequence_sha256` is identical across treatments for all 106 (mix, rate, window) keys.
  - 128 keep-alive clients. Latency is measured from the scheduled time over an unbounded dispatch queue, so there is no coordinated omission.
  - Timeout is 2 s. A request that passes its deadline while still queued counts as a timeout.
  - Every response is checked for `num_found`, plus the id for class A.
- **SLO.** P95 < 50 ms, P99 < 100 ms, error rate < 0.1%, ≥ 98% of offered load achieved, and not HARNESS_SATURATED.

## 2. Correctness (before any load)

- **Solr (B0).** All 298 structural requests (A–E) match the oracle exactly: ids, `num_found` + hit count, sort-value sequences and complete facet maps. There were **0 exclusions**.
- **N1.**
  - It matches the oracle on 298/298, and #77's cross-variant fixture passes.
  - Concurrency: 894 shuffled requests over 32 connections gave 0 mismatches. The fixture integration test (16 threads × 50) also passes.
- **Router.** 480/480 lexical responses are identical to direct Solr, so lexical quality is identical by construction.
- **Under load.** Across all 205 load points and all 57 calibration points there were 0 check failures. The only errors were 23 client timeouts, all at overloaded points.
- **Limitation.** The in-load check covers `num_found` plus the class-A id, so facets and sort order are not re-verified *under load*. The pre-pass mitigates this.

## 3. Capacity (primary "representative mixed commerce serving scenario": A 10%, B 15%, C 10%, D 20%, E 20%, F 25%)

### 3.1 Search and confirmation

| step | B0 | H1 |
|---|---|---|
| search from 20 QPS, ×1.5 steps, bisection to 5% | 20 FAIL (P95 58.4, P99 123.4, 16% util) | Q* = 405.5 (416.2 FAIL on P95 50.2) |
| C1 downward bracket | 13.3 / 8.9 / 5.9 FAIL, 4.0 PASS → bisection → Q* = 5.43 (5.67 FAIL) | — |
| confirmation round 1: {0.95, 1, 1.05}·Q* × 3 counterbalanced fresh launches | 0/3 at every rate | 0/3 at every rate (P95 51–125, P99 84–646): **UNSTABLE** |
| confirmation round 2: ladder lowered 10% (the preregistered UNSTABLE rule) | 0/3 at 4.6 / 4.89 / 5.1 | **1/3 at 346.7, 2/3 at 365.0, 2/3 at 383.2 → confirmed max 383.2** |

**Caveats on this table:**
- **B0 is always near the P95 boundary.** At every rate tested from 4 to 13 QPS, its P95 was 48–64 ms, pinned by class E at about 80 ms with a 20% share. A 60 s window at 5 QPS holds about 300 requests, so P99 is roughly the third-worst request and the Poisson count varies by ±10%. B0's search-level PASS/FAIL is therefore close to noise: 5.43 passed in the search and then failed 3/3 in confirmation.
- **H1's confirmed max is confounded by order.** Within each launch the ladder always runs rc → lo → mid → hi, so the highest rate is also the warmest point, and pass counts rise with rate (1/3, 2/3, 2/3). Read 383.2 as "passes about two times in three somewhere in 350–400 QPS", not as a sharp capacity.
- **Round 1's H1 failures were a transient.** Tails were uniform across classes and fell monotonically within each launch: a post-launch warm-up and queueing transient. By contrast, the search reached 405.5 after a gradual ramp of about 12 minutes.
- **H1 failed the SLO at R_c = 5.4 QPS in all 6 confirmation launches** (P95 49–57.5, P99 120–140). It also failed at the 20-QPS precondition in all 6. The cause was class F through the cold delegate (P99 170–227 ms).
- **At the only matched load the protocol defines, neither treatment meets the SLO.** C1's phrasing ("B0 cannot hold the SLO at any tested load, while H1 holds it up to Q*_H1") is incorrect: H1 holds the SLO only once it is warm and loaded.

### 3.2 H1 at its confirmed max (383.2 QPS; medians of 3 runs; routes fixed before measurement)

| class | share | route | P50 | P95 | P99 | errors |
|---|---|---|---|---|---|---|
| A exact lookup | 9.6% | native | 1.4 | 35.7 | 85.8 | 0 |
| B structured filter | 15.1% | native | 2.3 | 40.5 | 91.4 | 0 |
| C numeric range + sort | 9.9% | native | 22.4 | **67.5** | **131.2** | 0 |
| D category PLP | 20.0% | native | 1.9 | 34.0 | 77.8 | 0 |
| E category + facets | 20.5% | native | 10.5 | 52.6 | 102.2 | 0 |
| F open lexical | 24.9% | **Solr delegate** | 14.0 | 43.8 | 80.4 | 0 |

- **Traffic split.** 75.1% of requests were served natively and 24.9% delegated to Solr; none used both.
- **CPU split.** 59% native / 41% Solr, consistent across the 3 runs.
- **CPU at the confirmed max.** Total CPU/query was 5.6 ms at **71–72% slice utilization**. Solr uses about 0.9 cores alongside native's 3 slots, so CPU contention is a plausible part of H1's tail.
- **Worst class: C.** Its native cost is dominated by numeric-range construction, which #63 classed REFINE.

For comparison, B0 at 4.89 QPS kept A–D at P95 ≤ 14 ms. But E reached P95 **84.6 ms** and F reached **107 ms**, and those two classes are 45% of the mix.

### 3.3 Capacity curves

`artifacts/issue65/results/report.md` has the full table for every search, all computed from the slice counter: offered → achieved, slice utilization, CPU/query, P95 / P99 and errors.

On the primary mix, H1 achieved the offered rate at every point up to 512 QPS:
- slice utilization rises from 10.3% at 20 QPS to 78.7% at 512;
- CPU/query falls from 15.3 ms to 4.6 ms as fixed overhead amortizes;
- P95 crosses 50 ms between 405 and 416 QPS.

### 3.4 CPU/query at matched load (all from the slice counter)

| matched point | B0 ms/query | H1 ms/query | c |
|---|---|---|---|
| primary search, 20 QPS | 23.1 | 15.3 | 0.661 |
| confirmation R_c 5.4 QPS, round 1 (3 launches) | 32.5 / 32.9 / 31.0 | 21.5 / 23.0 / 22.8 | 0.664 / 0.699 / 0.737 |
| confirmation R_c 5.4 QPS, round 2 (3 launches; preregistered c) | 33.0 / 31.4 / 34.0 | 21.7 / 22.8 / 21.4 | 0.658 / 0.726 / 0.630 |
| warm diagnostic, structural mix, 20 QPS | 13.6 | 6.5 | 0.478 |
| warm diagnostic, lexical mix, 20 / 30 QPS | 14.7 / 13.8 | 11.7 / 9.8 | 0.795 / 0.711 |

- **These are low-load points.** At the matched points, both treatments' fixed overhead dominates: Solr's background work, plus native's idle cost inside H1. H1's own CPU/query falls about 3x from 20 to 400 QPS.
- **So the preregistered c is a low-load number.** Its per-launch margin under the 0.75 bar is only 0.02–0.12, which is not robust.
- **Withdrawn.** The first draft claimed about 8x CPU efficiency (12% of B0's CPU). That figure came from the accounting bug (§7) and is withdrawn.

### 3.5 Post-hoc diagnostic: CPU/query under load (not preregistered, not used by the verdict)

This diagnostic was posted to #65 before it ran. It uses one fresh launch per treatment and a 20-QPS precondition, then two 60 s primary-mix windows. The SLO is ignored and CPU comes from the slice counter. Raw data: `results/cpuload/`.

| offered QPS | B0 ms/query (util) | H1 ms/query (util) | c | H1 native / Solr CPU in window | B0 P95 / P99 | H1 P95 / P99 |
|---|---|---|---|---|---|---|
| 50 | 20.0 (33%) | 12.9 (22%) | 0.647 | 11.6 s / 27.3 s | 59.9 / 115.3 | 38.6 / 118.9 |
| 100 | 16.2 (54%) | 8.3 (27%) | **0.511** | 21.2 s / 28.2 s | 71.1 / 139.7 | 23.1 / 43.1 |

- **H1's Solr CPU is almost unchanged from 50 to 100 QPS** (27.3 s → 28.2 s), even though class F's traffic doubles. So much of Solr's CPU on a freshly provisioned index is load-independent background work. B0 carries that same fixed cost across all of its traffic, while H1 carries it only for the delegate.
- **This is why c falls as load rises.** The slope between the two windows is (Δ CPU)/(Δ queries): B0 12.3 ms/query, H1 3.6 ms/query, a ratio of about 0.29.
- **Caveats.** The data is 2 points × 1 launch, and the part of Solr's cost that is fixed rather than per-query was not separately measured. A long-lived, fully merged production Solr might not carry this background cost.
- **How to use these numbers.** They are a hypothesis for #66 (does a fixed-workload envelope reward H1 more at higher load?), not a result. The preregistered c remains 0.658.

## 4. Sensitivity (preregistered: one search per mix; the verdict requires H1 > B0 on both)

| mix | B0 Q* | H1 Q* | H1 > B0 |
|---|---|---|---|
| browse/structural-heavy (F 10%) | 7.1 | 19.1 | yes |
| lexical-heavy (F 50%) | **0** (fails at every rate down to 1.8 QPS) | 19.1 | yes |

**H1's values here are not capacities.**
- H1 failed its *first* point, 20 QPS, in both mixes, and only on P99 (104 / 106 ms).
- The whole tail came from class F, the cold-cache delegate (P99 121–157 ms); the native classes stayed at P99 ≤ 36 ms.
- C1a's downward bracket then caps Q* below 20.

**Post-hoc warm diagnostic** (not preregistered): the same searches after an unmeasured 180 s warm-up at 60 QPS.

| mix | B0 Q* | H1 Q* |
|---|---|---|
| structural | 5.9 | 448.2 |
| lexical | 43.1 | 327.3 |

These numbers are reported as data only. They are excluded from the verdict, and this document draws no inference from them.

## 5. Native concurrency scaling (diagnostic; native alone in the slice; `native_plp` mix = B–E)

| harness | Q* | ÷ N1 W=1 | slice util at Q* | CPU/query | P95 / P99 at Q* |
|---|---|---|---|---|---|
| N0 (#79 server; one connection at a time, connection-per-request) | 123.3 | 0.97 | 19.5% | 4.77 ms | 45.8 / 63.1 |
| N1, W = 1 | 126.5 | 1.00 | 20.8% | 4.97 ms | 45.3 / 69.2 |
| N1, W = 2 | 277.5 | 2.19 | 47.7% | 5.21 ms | 47.2 / 79.9 |
| N1, W = 3 (headline configuration) | 416.2 | **3.29** | 68.2% | 4.96 ms | 49.5 / 80.8 |

- **Scaling holds.** Once the single-connection harness is removed, native throughput scales with cores. N0 and N1 at W=1 agree, which cross-checks the new harness against the old one.
- **No shared-state contention.** CPU/query stays flat at 4.8–5.2 ms.
- **Unaffected by the accounting bug**, because native ran alone in the slice.
- **One invalid first run** (a load-generator panic) is preserved under `scaling_INVALID_load_generator_panic/`.

## 6. Limitations

- **The SLO makes B0 latency-bound.** A P95 < 50 ms target against Solr's roughly 80 ms complete-bucket facets means B0 fails on latency, not throughput. A Solr operator would cap the number of returned facet values, but that is not equal work (#63's rule). Under a looser SLO, the capacity comparison would move toward c.
- **Cold start and warm-up.** Fresh launches start with empty Solr caches, so low-rate points are cache-cold. This hurts B0 more because every class goes to Solr. The sensitivity searches and the confirmation ordering both show PASS that is non-monotone in rate.
- **Low-rate statistics.** At B0's rates, P99 is essentially the third-worst request.
- **Finite request pools.** There are 778 distinct requests, so Solr's caches help both treatments' Solr work (disclosed in the amendment).
- **Keep-alive everywhere.** All engines used keep-alive, unlike the `Connection: close` setup in #63/#64.
- **Memory measurement.** Memory is cgroup `memory.current`, not RSS. Process RSS was not recorded.
- **Scope.** One host (a KVM guest with a 3-CPU slice), WANDS 500k, one SLO and one family of mixes. G is omitted and Meilisearch excluded.

## 7. Adversarial review

A fresh, read-only reviewer recomputed the results from raw data.

| # | finding | severity | disposition |
|---|---|---|---|
| 1 | `total_cpu_us` took the first cgroup key (`i65-native` for H1), so H1's CPU/query and utilization left out Solr. | **high** | Fixed in `i65_load.rs`; `analyze_i65.py` now reads the raw slice counter. c corrected from 0.119 to **0.658 / 0.704**, and H1's utilization at max from 42% to **71–72%**. The "8x / 12%" claims are withdrawn. No rerun needed: the raw counters were correct, and PASS/FAIL never used CPU. |
| 2 | c depends on R_c and is fragile. | medium | Tabulated at every matched point (§3.4); post-hoc under-load diagnostic added (§3.5). |
| 3 | H1 failed the SLO at R_c in all 6 launches, and this was undisclosed. | medium-high | Disclosed (§3.1); C1's phrasing corrected. |
| 4 | "q ≥ 70.6" is not a valid bound: PASS is not monotone, and B0 was never searched upward while warm. | medium-high | Withdrawn; q is undefined. |
| 5 | SLO decisions at low rates are close to noise. | medium | Disclosed (§3.1, §6). |
| 6 | H1's confirmed max is confounded by ladder order. | medium | Disclosed (§3.1). |
| 7 | `memory.current` was labelled RSS. | low | Relabelled. |
| 8 | Several small inaccuracies:<ul><li>B0 passed 4.0 / 4.95 / 5.43 in the downward search;</li><li>there were 205 load points;</li><li>the scope of the in-load check;</li><li>`Slots` is not panic-safe or FIFO;</li><li>a missing JSON counts as FAIL.</li></ul> | low | Corrected or disclosed. |
| 9 | An inference was drawn from the warm diagnostic. | low-medium | Inference removed. |

The reviewer could not falsify any of the following:
- how coordinated omission was handled, or the absence of generator saturation;
- slice membership and the shared quota, including that router CPU is counted;
- that sequences, pools and equal-work bodies were identical across treatments;
- that C1, C1a and round 2 each came before the data they govern;
- the scaling diagnostic;
- that the verdict label follows from the preregistered rule via c ≤ 0.75.

## 8. Consequence

- **#60.** The whole-workload thesis is **KEPT, narrowly**: for latency-feasible capacity on this family of mixes, H1 serves about 380 QPS under an SLO that Solr alone cannot meet. The total-CPU efficiency is **modest (c ≈ 0.63–0.74 at matched low load) and fragile**. In the post-hoc diagnostic it improves with load (0.51 at 100 QPS), a pattern consistent with a load-independent Solr background cost. Memory is **not** kept: H1 uses 2.1x B0's cgroup memory, and E2's REFINE stands.
- **#66 (fixed workload → minimum CPU/RAM envelope).** It must weigh, in one experiment, H1's latency feasibility at a given core count, the modest per-query CPU difference, and H1's 2.1x memory. It must also measure B0 at loads above its SLO limit (throughput only) to get a marginal CPU comparison.
- **H1 REFINE candidates** (not done here):
  - numeric-range construction, behind the class C tail;
  - the 3-slot queue behind heavy requests;
  - a warm-up/caching policy for the Solr delegate, since its cold lexical tails are H1's low-load SLO failure.
