# Issue #65 (Infra E5, amended) — Realistic mixed workload and max sustainable QPS/core: log

Append-only.
- **Preregistration:** GitHub issue #65, i.e. the original body plus **amendment 1 (2026-09-29)** appended to it.
- **Verdict:** `docs/decisions/ISSUE65_MIXED_CAPACITY_DECISION.md`.
- **Raw evidence:** `artifacts/issue65/`.

## 2026-09-29 — preconditions

- `main` = `31dcfe8` (#64 plus Dependabot #58, Maven POMs only), with 0 open PRs.
- **Full local gate on `31dcfe8`: green.** fmt and clippy are clean, 809 tests pass, and the release build succeeds. GitHub CI on `31dcfe8` is green.
- The GitHub Actions trigger anomaly on `778e85c` is recorded in amendment §0. No empty commit was made.

## 2026-09-29 — implementation (before any measurement)

- **New crate `issue65-eval`:**
  - `server` (N1): `Arc<State>`, thread-per-connection with keep-alive, and a W-slot execution semaphore gating all native CPU work. `/solr/*` requests are proxied as the H1 router without holding a slot. Endpoints: `/lookup`, `/plp`, `/noop`, `/correctness`.
  - `workload`: the frozen A–F pools with oracle expectations, the mixes, and deterministic Poisson sequences.
  - `observe`: native/Solr response normalization and checks.
  - `cgroup`: counters.
  - Binaries: `i65_server`, `i65_workload`, `i65_validate` and `i65_load`, the open-loop generator.
- **Tests:**
  - 6 unit tests: the slot bound, sequence determinism and mix shares, exclusion, Solr/native request shapes, and observation equivalence.
  - `tests/n1_concurrency.rs`: 16 connections × 50 mixed and identical requests against a live N1 over a fixture must equal the direct single-threaded answers.
- **`scope_runtime.sh`** gains optional `E3B_SLICE` / `E3B_SCOPE_MEMORY`. Both are unset for #77/#79, whose behaviour is unchanged.
- **Serving slice verified:** `i65-serving.slice` has `cpu.max = 300000 100000`, `memory.max = 12884901888` and `memory.swap.max = 0`. Every serving scope is created inside it.
- **Pools** (`i65_workload`, 500k): 778 requests: A 200, B 58, C 12, D 20, E 8, F 480.
  - **Clarification on B:** amendment §8's summary said "40", but its own construction rule gives 58. That is 40 single filters (the top-20 colors and top-20 styles) plus the 18 depth-3/4/5 × top-6-color requests. The rule is applied as written; the count was an arithmetic slip in the summary.
- **Disclosed deviation (native scaling diagnostic):** N0 (#79's server) has no `/lookup` endpoint. The diagnostic therefore uses `native_plp` (B–E of the primary mix, renormalized) for N0 **and** every N1 worker count, instead of "A–E".
- **Fixed before any result (verdict precedence).** The amendment's bands overlap: "an improvement that misses 25%" versus "0.8 < q < 1.25 and 0.75 < c < 1.25". `analyze_i65.py` applies these rules in order:
  1. **NEGATIVE:** q ≤ 0.8 or c ≥ 1.25.
  2. **BROAD:** the primary mix clears (q ≥ 1.25 or c ≤ 0.75), H1's P99 at its confirmed max is < 100 ms, and H1 beats B0 on both sensitivity mixes (ratio > 1).
  3. **MODEST / MIX-DEPENDENT:** the primary clears, or there is any improvement (q > 1 or c < 1).
  4. **NO MATERIAL ADVANTAGE:** otherwise.
- **Calibration points** are diagnostic and shortened to 5 s warm-up + 20 s window. Workload points keep 10 s + 60 s as preregistered.

## 2026-09-29 — correctness pre-pass (validate phase, H1 system: Solr + N1 router in one slice)

- **Solr (B0):** all 298 A–E requests match the oracle (ids, `num_found` + hit count, sort-value sequences, complete facet maps). **0 NOT_EQUIVALENT_WORK and 0 exclusions.** All 480 F requests recorded their ranked top-48 as the expectation.
- **Native (N1):** 298/298 match the oracle, and #77's cross-variant fixture passes.
- **Concurrency:** 894 requests (298 × 3 shuffled rounds) over 32 connections, with **0 mismatches** against the single-threaded observations.
- **Router:** 480/480 lexical responses through H1's delegate path are identical to Solr's direct ranked top-48.
- **Frozen pools:** `artifacts/issue65/workload/pools_frozen.json`, with SHA-256 in `results/validate/report.json`.

## 2026-09-29/30 — calibration, primary search, clarifications C1/C1a, confirmations

- **Transport calibration** (diagnostic; 5 s + 20 s points). Native N1 `/noop` Q* = 3,889 QPS, where the generator itself saturates: at 5,834 QPS, generator CPU is 0.96 and the run is flagged HARNESS_SATURATED. Solr health endpoint Q* = 3,727. Router → Solr health Q* = 3,403. **The harness is therefore valid for workload capacities up to about 1,700 QPS** (≥ 2x rule). Every workload capacity measured is ≤ 405 QPS.
- **Primary search (preregistered; same launch per search).**
  - **H1 Q* = 405.5 QPS.** 416.2 FAILs on P95 50.2 ms.
  - **B0 failed its first point, 20 QPS:** P95 58.4 ms, P99 123.4 ms, at 16% CPU utilization, with no errors and full throughput. The per-class breakdown shows class E (complete-bucket Solr facets) at P95 98 ms and cold-cache lexical class F at P95 62 ms.
- **Clarification C1** (posted before any further data). The amendment did not say what happens when the first point fails. C1 fixed a B0 downward bracket to 1.8 QPS and the fallback R_c. B0's downward bracket found Q*_B0 = **5.43 QPS** (5.67 FAIL) at 3–11% CPU utilization: its limit is latency feasibility, not throughput. R_c = min(5.43, 405.5) = 5.4 QPS.
- **Clarification C1a** (posted before any further data): the downward bracket applies to every search whose first point fails.
- **Confirmation round 1** (preregistered ladder {0.95, 1, 1.05}·Q*, 3 counterbalanced fresh-launch runs): **UNSTABLE for both treatments**, with no rate passing in ≥ 2/3.
  - H1 at 385–426 QPS had P95 51–125 ms and P99 84–646 ms.
  - The tails are uniform across all classes, including native exact lookups, at 35–45% CPU utilization. They fall monotonically within each launch.
  - Reading: after a fresh launch, the jump from 5.4 QPS to the ladder shows a warm-up and queueing transient that the search's gradual ramp did not. This is recorded as a limitation, and no rule was changed.
- **Confirmation round 2** (the preregistered UNSTABLE rule: ladder lowered by 10%):
  - **H1:** 346.7 QPS passes in 1/3 runs, 365.0 in 2/3 and 383.2 in 2/3. **H1's confirmed max is 383.2 QPS**, i.e. 127.7 QPS/core, with median P99 91.7 ms there.
  - **B0:** 0 of 9 points pass at 4.6–5.1 QPS (P95 52–76 ms). **B0 has no confirmed max**, since the rule allows only one extra round. Its search Q* of 5.43, with a FAIL at 5.67, is an upper bound, so **q ≥ 383.2 / 5.43 = 70.6** (a lower bound).
- **CPU/query at R_c = 5.4 QPS**, median per round:
  - round 1: B0 32.5 ms and H1 3.87 ms, so c = 0.119;
  - round 2: B0 33.0 ms and H1 3.94 ms, so **c = 0.119**.
- **Harness fix:** `pools_sha256` in the raw files was nondeterministic, because a HashMap of facet expectations serialized in random order. It is now canonical. Every run loaded the same unchanged `pools_frozen.json`, file sha256 `c1152ef7e8c8e6fb76e6249aad9d0951474f97ee48cc5e4b80275a7cdd2b15ce`.

## 2026-09-30 — sensitivity, warm diagnostic, native scaling

- **Sensitivity** (preregistered, one search each, C1a):

  | mix | B0 Q* | H1 Q* |
  |---|---|---|
  | structural | 7.1 | 19.1 |
  | lexical | 0 | 19.1 |

  H1 > B0 on both mixes, so the preregistered sensitivity condition holds.
  - H1 failed its first point (20 QPS) in both mixes, only on P99 (104 / 106 ms). The whole tail came from class F: the cold-cache Solr delegate at P99 121–157 ms. The native classes were at P99 ≤ 36 ms.
  - C1a's downward bracket therefore caps H1 below 20. These values are not capacities.
- **Warm diagnostic** (post-hoc, posted to #65 before it ran, not used by the verdict): each sensitivity search after an unmeasured 180 s warm-up at 60 QPS. Structural: B0 5.9, H1 448.2. Lexical: B0 43.1, H1 327.3.
- **Native scaling** (diagnostic, `native_plp` = B–E).
  - A first run was **invalid**: `i65_load` panicked building a target for class F under the native-only treatment, although F is never scheduled there, and every point was a crash. It is preserved under `results/scaling_INVALID_load_generator_panic/`. The fix is an inert target plus an assertion that no unroutable class is ever scheduled.
  - Rerun: N0 123.3, N1 W=1 126.5, W=2 277.5, W=3 416.2 QPS (3.29x W=1), with CPU/query flat at 4.8–5.2 ms.
- **Decomposition control:** not run. It is conditional on H1 missing the bar on the primary mix (amendment §14), and H1 clears it (c = 0.119; q ≥ 70.6).
- **Verdict, per `analyze_i65.py` with the precedence fixed before results:** BROAD CAPACITY ADVANTAGE.
  - Primary: q ≥ 70.6 as a lower bound, and c = 0.119.
  - H1's P99 at its confirmed max is 91.7 ms, under 100.
  - Sensitivity holds.

## 2026-09-30 — adversarial review; CPU-accounting correction (supersedes the c / q / utilization numbers above)

A fresh, read-only reviewer recomputed the results from the raw JSONs. Its findings and their dispositions are listed below and in the decision doc (§7). The numbers above are kept as they were first recorded. **Where the two disagree, this entry supersedes them.**

**1. HIGH: the CPU accounting was wrong.**
- **Bug:** `i65_load` took `total_cpu_us` from the *first* key of the cgroup BTreeMap. For H1 that key is `i65-native`, so the "total" left out the Solr delegate.
- **Scope:** H1's `cpu_per_query_ms` and `util` fields, in every H1 point, were native-only.
- **Fix:** use the `total` key, which is the slice counter. `analyze_i65.py` now computes both fields from the raw `cpu_usec.total` in every file. Nothing was rerun: the raw counters were always recorded correctly, and PASS/FAIL never depended on CPU.
- **Corrected values:**

  | quantity | as recorded above | corrected |
  |---|---|---|
  | c, round 2 (per launch) | 0.119 | **0.658** (0.658 / 0.726 / 0.630) |
  | c, round 1 | 0.119 | **0.704** |
  | H1 CPU/query at 383.2 QPS | 3.3 ms | **5.6 ms** |
  | H1 slice utilization at 383.2 QPS | 42% | **71–72%** |
  | H1 utilization over the primary search | 2.6–45% | **10.3–78.7%** |

  The "≈ 8x CPU efficiency" claim is withdrawn. The native-only scaling diagnostic is unaffected.
- **Verdict:** under the preregistered precedence, the result is still **BROAD CAPACITY ADVANTAGE**, now via c = 0.658 ≤ 0.75 with only a small margin.

**2. The bound "q ≥ 70.6" is invalid.** PASS is not monotone in rate: B0 passed at 4.0, 4.95 and 5.43 in the search but failed 9/9 at 4.6–5.7 in confirmation. B0 was also never searched upward while warm. q is therefore undefined, and the bound has been removed from the analysis.

**3. H1 failed the SLO at R_c = 5.4 QPS, and at the 20 QPS precondition, in all 6 confirmation launches.** Its P99 was 120–140 ms, driven by the cold delegate (class F). This is now disclosed, and C1's phrasing is corrected.

**Other findings, now disclosed:**
- **4. c depends on R_c.** It is reported at every matched point.
- **5. Low-rate PASS/FAIL is close to noise**, with about 300 requests per window.
- **6. Ladder order confounds H1's confirmed max.** The highest rate is always the warmest point.
- **7. Memory is `memory.current`, not RSS.** It has been relabelled.
- **8. Minor points:**
  - the load-point count is 205, not 213;
  - the in-load check covers `num_found` plus the class-A id only;
  - `Slots` is not panic-safe or FIFO;
  - a missing JSON counts as FAIL.
- **9. The inference from the warm diagnostic has been removed.** Its numbers are reported as data only.

**Post-hoc CPU-under-load diagnostic (`cpuload` phase).** This was posted to #65 before it ran. It is not preregistered and is not used by the verdict. Each treatment gets one fresh launch and a 20 QPS precondition, then primary-mix points at 50 and 100 QPS with the SLO ignored. It records slice CPU/query only. Results are in the next entry.

## 2026-09-30 — post-hoc CPU-under-load diagnostic (not preregistered; not used by the verdict)

- **Setup:** `run_i65.sh cpuload`, primary mix, SLO ignored, one fresh launch per treatment plus a 30 s precondition at 20 QPS.
- **Slice CPU/query (utilization in parentheses):**

  | offered | B0 | H1 | c |
  |---|---|---|---|
  | 50 QPS | 20.0 ms (33%) | 12.9 ms (22%) | 0.647 |
  | 100 QPS | 16.2 ms (54%) | 8.3 ms (27%) | 0.511 |

- **Solr is mostly fixed cost here.** Within H1, Solr's window CPU is 27.3 s at 50 QPS and 28.2 s at 100 QPS: nearly load-independent, even though F traffic doubled. The native share went from 11.6 s to 21.2 s.
- **Marginal slope (Δ CPU / Δ queries):** B0 12.3 ms/query, H1 3.6 ms/query, a ratio of about 0.29.
- **SLO at these points** (reported only):
  - B0 failed at both 50 and 100, with P95 60 / 71 ms driven by E and F.
  - H1 failed at 50 on P99 118.9 ms (cold F P99 155).
  - H1 passed at 100.
- **Caveats:** 2 points × 1 launch, and the fixed-versus-marginal decomposition of Solr's cost was not measured separately.
- **Reading:** a hypothesis for #66, not a result.
