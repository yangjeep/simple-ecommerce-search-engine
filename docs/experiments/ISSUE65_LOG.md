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
