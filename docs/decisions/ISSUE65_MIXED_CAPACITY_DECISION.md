# Issue #65 — Infra E5 (amended): realistic mixed workload and max sustainable QPS/core

**Question** (#65 amendment 1 and clarifications C1/C1a; the original body is preserved on the issue): combine structural/PLP/facet work and delegated lexical work into one bounded serving system. Does the measured execution advantage survive concurrent load as materially higher sustainable QPS/core, at the same correctness/relevance and latency SLO?

**Answer (preregistered rule): BROAD CAPACITY ADVANTAGE, with a qualification that changes what it means.**

| | B0: Solr-only | H1: native N1 + Solr delegate (one 3-CPU / 12 GiB slice) |
|---|---|---|
| max sustainable QPS, primary mix (P95 < 50 ms, P99 < 100 ms, err < 0.1%, ≥ 98% achieved) | **unconfirmed at ≥ 4.6 QPS**. The search found 5.43 (5.67 FAIL); both confirmation rounds failed 9/9 at 4.6–5.7 | **383.2 QPS confirmed** (2/3 in round 2), i.e. **127.7 QPS/core** |
| CPU/query at R_c = 5.4 QPS (6 fresh launches) | 31.0–33.0 ms | 3.7–3.9 ms. **c = 0.119** |
| q = H1 QPS/core ÷ B0 QPS/core | — | **≥ 70.6**, a lower bound (B0's unconfirmed search Q* is an upper bound on its max) |
| CPU utilization at the treatment's own limit | 3–11% of 3 cores | 42% of 3 cores |
| RSS at the sustainable point | 3.68 GB | **7.82 GB** (native 4.14 + Solr 3.60) |
| sensitivity mixes (preregistered; H1 > B0 required) | structural 7.1 / lexical 0 | structural 19.1 / lexical 19.1. Holds (see §4 for why H1's values are not capacities) |

**What the qualification is.** The "q ≥ 70" is **not** a 70x CPU-throughput ratio.
- **B0's limit is latency feasibility, not throughput.** Solr serving this mix alone misses the frozen SLO at 3–11% CPU utilization, and at every tested load down to 4.6 QPS. The cause is two classes:
  - complete-bucket JSON facets over large category scopes (class E, P95 ≈ 80–98 ms at any rate), which the equal-work rule requires;
  - cold-cache lexical queries (class F, P95 42–107 ms at low rates).
- **The throughput-efficiency result is c ≈ 0.12:** H1 spends 12% of B0's serving CPU per logical query at matched load.
- **H1's own limit is also latency** (tail queueing behind heavy requests; class C range + sort is the worst tail), at 42% CPU.
- **Memory:** H1 needs 2.1x B0's RSS. No memory advantage is claimed. That tension is #66's question.

## 1. What was frozen

- **Code:** `main` = `31dcfe8`; full local gate green (809 tests). The GitHub Actions trigger anomaly on `778e85c` is recorded in amendment §0.
- **Branch:** `i65/mixed-capacity`. Binaries were built on the branch; the provenance and the fixes after first runs are in the log.
- **Treatments.**
  - **B0:** Solr 9.10.1 for every class. It uses the #63/#64 equal-work builders (`limit:-1` facets with tagged self-exclusion) and #57's lexical configuration (`edismax`, `qf=title description`, default relevance).
  - **H1:** N1 (the native #79/#63 N⁺ path, `hybrid:hybrid:p0r`, frozen τ_F/ρ_S) serves A–E. N1 is also the router, proxying F to the Solr delegate. Class G was omitted (amendment §3); Meilisearch was excluded.
- **N1** is the minimal concurrent harness: `Arc<State>`, thread-per-connection with keep-alive, and **3 execution slots**. There is no async runtime.
- **Serving budget:** one user slice, `i65-serving.slice`, with `cpu.max 300000/100000` and `memory.max 12 GiB`, swap 0, and every serving process on CPUs 0–2. For H1, native, Solr and router share it. The load generator ran on CPU 3, outside the slice.
- **Load generator:** open loop; a Poisson schedule pre-generated from (seed 65, mix, rate) and identical for both treatments; 128 keep-alive clients; latency measured from the scheduled time; a 2 s timeout; every response checked against its expectation (`num_found`, plus the id for class A).
- **SLO:** P95 < 50 ms, P99 < 100 ms, error rate < 0.1%, achieved ≥ 98% of offered, and not HARNESS_SATURATED.

## 2. Correctness (before any load)

- **Solr (B0):** all 298 structural requests (A–E) match the oracle exactly: ids, `num_found` + hit count, sort-value sequences, complete facet maps. There were **0 exclusions**.
- **N1:** 298/298 match the oracle, and #77's cross-variant fixture passes.
- **Concurrency:** 894 shuffled requests over 32 connections gave 0 mismatches, and the fixture integration test passes (16 threads × 50).
- **Router:** 480/480 lexical responses are identical to direct Solr. Lexical quality is therefore identical by construction.
- **During load:** 0 check failures across all 213 measured points in every phase. The only errors are 23 client timeouts, all at overloaded points.

## 3. Capacity (primary "representative mixed commerce serving scenario": A 10%, B 15%, C 10%, D 20%, E 20%, F 25%)

### 3.1 Search and confirmation

| step | B0 | H1 |
|---|---|---|
| search from 20 QPS, ×1.5, bisect to 5% | 20 FAIL (P95 58.4, P99 123.4) | Q* = 405.5 (416.2 FAIL on P95 50.2) |
| C1 downward bracket | Q* = 5.43 (5.67 FAIL) | — |
| confirmation round 1: {0.95, 1, 1.05}·Q* × 3 counterbalanced fresh launches | 0/3 at every rate | 0/3 at every rate (P95 51–125, P99 84–646). **UNSTABLE** |
| confirmation round 2: ladder lowered 10% (the preregistered UNSTABLE rule) | 0/3 at 4.6 / 4.89 / 5.1 | **1/3 at 346.7, 2/3 at 365.0, 2/3 at 383.2**. Confirmed max **383.2** |

- **Round 1 was UNSTABLE for H1 for a reason visible in the data.** Each confirmation launch goes from a 30 s precondition at 20 QPS, through R_c at 5.4 QPS, straight to the ladder. Tails were inflated uniformly across all classes, including 1 ms native lookups, at 35–45% CPU utilization, and they fell monotonically within each launch. The search, by contrast, reached its Q* after a gradual 12-minute ramp. This is a post-launch warm-up and queueing transient. No rule was changed; it is reported (§6).
- **H1 at its confirmed max** (median of the 3 runs): P50 9.7 ms, P95 47.5 ms, P99 91.7 ms, 0 errors, 382.2 QPS achieved, CPU 3.28 ms/query, and **42% utilization** of the 3 cores.

### 3.2 Per class and route (H1 at 383.2 QPS, medians of 3 runs; routes fixed before measurement)

| class | share | route | P50 | P95 | P99 | errors |
|---|---|---|---|---|---|---|
| A exact lookup | 9.6% | native | 1.4 | 35.7 | 85.8 | 0 |
| B structured filter | 15.1% | native | 2.3 | 40.5 | 91.4 | 0 |
| C numeric range + sort | 9.9% | native | 22.4 | **67.5** | **131.2** | 0 |
| D category PLP | 20.0% | native | 1.9 | 34.0 | 77.8 | 0 |
| E category + facets | 20.5% | native | 10.5 | 52.6 | 102.2 | 0 |
| F open lexical | 24.9% | **Solr delegate** | 14.0 | 43.8 | 80.4 | 0 |

- **Shares:** 75.1% of requests are native-served, 24.9% Solr-delegated, and 0% both (G omitted). H1's CPU is split 59% native / 41% Solr (per-scope `cpu.stat`, all 3 runs).
- **H1's binding classes** are C and the queue behind the three slots. Even microsecond lookups have P95 36 ms at this load. C's native cost is dominated by numeric-range construction, which #63 classed **REFINE** (value-order insertion, 3.9 ms hot loop at 383k matches).
- **B0 at 4.89 QPS, by contrast:** A–D all have P95 ≤ 14 ms. But **E has P95 84.6 ms** and **F has P95 107 ms**, and E and F alone are 45% of the mix.

### 3.3 Capacity curves

The full offered → achieved / CPU-utilization / P95 / P99 / error tables for every search are in `artifacts/issue65/results/report.md`.
- **H1, primary:** achieved = offered (1.000) at every point up to 512 QPS. CPU utilization rises linearly from 2.6% at 20 QPS to 45% at 512. P95 crosses 50 ms between 405 and 416 QPS.
- **B0:** achieved = offered at every point, with CPU at 3–16%. P95 exceeds 50 ms at every rate above 5.43 QPS.

## 4. Sensitivity (preregistered: one search per mix; the verdict needs H1 > B0 on both)

| mix | B0 Q* | H1 Q* | H1 > B0 |
|---|---|---|---|
| browse/structural-heavy (F 10%) | 7.1 | 19.1 | yes |
| lexical-heavy (F 50%) | **0** (fails at every rate down to 1.8 QPS) | 19.1 | yes |

**H1's sensitivity values are not capacities.**
- H1 failed its *first* point (20 QPS) in both mixes only on P99 (104 / 106 ms). All of that tail came from class F: the cold-cache Solr delegate at P99 121–157 ms. The native classes were at P99 ≤ 36 ms.
- The C1a downward bracket then caps Q* below 20.
- **Post-hoc diagnostic** (not preregistered and not used by the verdict): the same searches after an unmeasured 180 s warm-up at 60 QPS, identical for both treatments:

  | mix | B0 Q* (warm) | H1 Q* (warm) | ratio |
  |---|---|---|---|
  | structural-heavy | 5.9 | **448.2** | 76x |
  | lexical-heavy | **43.1** | **327.3** | 7.6x |

  With warm caches, Solr alone serves the lexical-heavy mix at 43 QPS. H1 still sustains 7.6x more with half its traffic delegated to Solr. The advantage does not depend on the mix, and the delegated lexical share is what bounds it.

## 5. Native concurrency scaling (diagnostic; native alone in the slice, `native_plp` mix = B–E)

| harness | Q* | ÷ N1 W=1 | CPU utilization at Q* | CPU/query | P95 / P99 at Q* |
|---|---|---|---|---|---|
| N0 (#79 server, one connection at a time, connection-per-request) | 123.3 | 0.97 | 19.5% | 4.77 ms | 45.8 / 63.1 |
| N1, W = 1 | 126.5 | 1.00 | 20.8% | 4.97 ms | 45.3 / 69.2 |
| N1, W = 2 | 277.5 | 2.19 | 47.7% | 5.21 ms | 47.2 / 79.9 |
| N1, W = 3 (the headline configuration) | 416.2 | **3.29** | 68.2% | 4.96 ms | 49.5 / 80.8 |

- **Native throughput scales with cores** once the single-connection bottleneck is removed. N0 and N1 W=1 agree, which cross-checks the new harness against the old one.
- **CPU/query is flat** (4.8–5.2 ms) across W, so there is **no shared-state contention**. The immutable `Arc<State>` and the per-request scratch hold up.
- The better-than-linear step from W=1 to W=2 is a queueing effect: at W=1 the P95 SLO binds at 62% of one core.
- The first scaling run was invalid (a load-generator panic building an unused class-F target). It is preserved under `scaling_INVALID_load_generator_panic/` and was rerun after the fix.

## 6. Limitations

- **The SLO makes B0 latency-bound.** P95 < 50 ms against Solr's ≈ 80 ms complete-bucket facets means B0 fails on per-request latency, not capacity. A Solr operator would cap returned facet values, but that is not equal work (#63's rule). Under a looser SLO, the comparison would move toward c (≈ 0.12).
- **Cold start and warm-up.** Fresh launches start with empty Solr caches. Low-rate points are cache-cold, and confirmation jumps expose a warm-up transient. Both treatments are affected, and B0 more, because it sends every class to Solr. Round 1's UNSTABLE result and the capped sensitivity values come from this.
- **Finite pools.** There are 778 distinct requests, and Solr's filter/query-result caches help B0 (disclosed in the amendment). Native has no result cache.
- **Keep-alive.** The #65 load uses keep-alive for every engine, unlike #63/#64's `Connection: close`. N1's per-request floor is therefore not comparable to #64's.
- **Scope.** One host (KVM, 3-CPU slice), WANDS 500k, one SLO, and one mix family. Class G (hybrid-both) was omitted, and Meilisearch was excluded.
- **Memory.** H1's RSS is 2.1x B0's. E2's REFINE stands. #65 claims CPU/latency capacity only.

## 7. Consequence

- **#60:** the whole-workload efficiency thesis is **KEPT** for CPU and latency capacity on this mix family. It is **not** kept for memory.
- **#66**, fixed workload → minimum CPU/RAM envelope, must now weigh H1's ≈ 8x CPU efficiency against its 2.1x RSS, in the same experiment.
- Candidates for H1's own REFINE (not done here):
  - numeric-range construction, the class C tail;
  - the 3-slot queue behind heavy requests;
  - a cache warm-up policy for the Solr delegate.
