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
