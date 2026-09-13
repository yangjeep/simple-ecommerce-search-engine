# Issue #61/#73 — E1 authoritative live execution

Log: `docs/experiments/ISSUE61_LOG.md`. Protocol: `docs/experiments/ISSUE61_PROTOCOL.md`
(Revisions 10-12). Raw evidence: `artifacts/issue61/i61_e1_run1_aborted_slotfailed_20260913/`,
`artifacts/issue61/i61_e1_rerun1_aborted_stuckconnection_20260913/`,
`artifacts/issue61/i61_e1_rerun2/` (all preserved exactly as produced).

## Verdict: REFINE — measurement-contract limitation, not a live-adapter defect

Three authoritative attempts (`run1`, `rerun1`, `rerun2` — the frozen protocol's
full cycle budget) each got further than the last, converging on a real,
disclosed limitation in the pre-existing (Issue #61, not #73) process-vs-cgroup
CPU reconciliation check rather than a live-adapter bug. No cycle reached
`EvidenceFinalization`/sealing, so **no cycle produced an `i61_analyze`
verdict** — this decision is a manual, adversarially-reviewed read of the raw
evidence each aborted cycle preserved, not an automated analyzer output.

## What happened, in order

1. **`run1`** — Equivalence audit passed both datasets byte-identical to the
   historical Revision 5 untimed audit (`i61_wands_equivalence_rev5.jsonl`,
   `i61_esci_electronics_equivalence_rev5.jsonl`). Index capture, equivalence
   gate, calibration (120/120 sessions, both engines), and calibration gate
   all passed. The first Warm-phase session (native, block 0) failed with a
   client-side HTTP read timeout.
2. Host investigation found the *first* real cause was unrelated to the
   application: the session got SIGKILL'd earlier that same day by the
   harness's low-memory guard on a then-10 GiB host running several other
   concurrent sessions; the user upgraded the host to 32 GiB before any
   further attempt. That killed attempt is **not measurement evidence** —
   preserved for provenance only, never analyzed.
3. **`rerun1`** (post-upgrade, 30 GiB confirmed via preflight smoke test and
   host-provenance logging) — failed immediately, before any Warm session,
   on a newly-added native self-check (added specifically to catch what run1
   hit). Root-caused by direct reproduction (a Python `requests.Session`
   script reproducing the exact readiness-then-query sequence): `i61_native_server`
   is a single-threaded, one-connection-at-a-time TCP server, and `ureq`'s
   persistent `Agent` keeps HTTP connections alive by default. The readiness
   poll's kept-alive `/ping` connection permanently blocked the server from
   accepting the *next* connection. Fixed by sending `Connection: close` on
   both the readiness poll and the self-check request — verified to resolve
   the reproduction (instant success vs. hard timeout) before retrying.
4. **`rerun2`** — Equivalence audit, index capture, equivalence gate,
   calibration (120/120), and calibration gate all passed again. Warm phase
   then completed **all 30 `all`-projection blocks and all 30 `fast-path`
   blocks** (both engines, WANDS) — independently confirming the
   `Connection: close` fix past the exact point that killed `run1`. It then
   failed 9 blocks into the `hybrid` projection: `i61_bench`'s pre-existing
   process-vs-cgroup CPU reconciliation check (`ProcessCpuDelta::reconcile_cgroup`,
   `crates/issue61-eval/src/process_cpu.rs`, not touched by #73) rejected a
   native session where process CPU (13313µs) and cgroup CPU (13029µs)
   disagreed by 2.13%, one hundredth of a percentage point over the frozen
   2% threshold.

Both infra defects (item 3's connection bug, and the calibration/warm
engine-schedule mismatch bug documented in Revision 12 §26.8) are fixed and
covered by regression tests; neither recurred once fixed.

## The CPU-reconciliation finding, forensically

Per the adversarial-review directive, every session across all three cycles
with a captured process CPU value (native sessions only — Solr has no
process-level cross-check) was pooled and grouped by workload class:

| regime | class | n | mean abs delta (µs) | mean cpu usage (µs) | mean disagreement % | max % |
|---|---|---:|---:|---:|---:|---:|
| calibration-five | all | 60 | 253.4 | 5,157,181 | 0.005% | 0.010% |
| calibration-four | all | 60 | 260.5 | 4,121,281 | 0.006% | 0.009% |
| warm | all | 30 | 247.7 | 2,096,353 | 0.012% | 0.016% |
| warm | fast-path | 30 | 270.4 | 121,401 | 0.226% | 0.317% |
| warm | hybrid | 9 (of 30) | 259.7 | 16,281 | 1.615% | 1.892% |

Pooled across all 189 records regardless of class: mean absolute delta
257.8µs, median 251.0µs, stdev 40.8µs, range 157-507µs.

**The absolute discrepancy is statistically constant** — every group's mean
sits within a 23µs band (247.7-270.4µs) whether the session used 16
thousand or 5 million microseconds of CPU. It does not scale with workload
size; it is a fixed instrumentation/accounting cost (most plausibly, the
non-zero wall-clock gap between reading `/rusage` inside the container and
reading the host's `cpu.stat` for the same window). The **relative
percentage** the frozen gate actually thresholds on is therefore purely a
function of the denominator: negligible for calibration's ~4-5M µs sessions,
still comfortable at fast-path's ~121K µs, and marginal-to-over-threshold at
hybrid's ~16K µs, where a ~250-300µs fixed cost is 1.5-2.1% of the total.

The failed observation (2.13%) is not an outlier requiring exclusion — it is
the same ~284µs fixed discrepancy as every passing hybrid session (which
ranged 1.14-1.89%), landing over 2% only because this particular session's
total CPU (13,029µs) happened to be slightly smaller than its passing
siblings' (15,405-21,626µs). Per instruction, it is preserved exactly as
`FAIL` and not relabeled — it is presented here as confirming evidence for
the pattern, not as a data-quality anomaly to discard.

**Interpretation:** the current relative-only 2% process-vs-cgroup
reconciliation gate has insufficient resolution for very-low-CPU sessions,
because a roughly fixed absolute accounting discrepancy becomes visible as
`>2%` once per-session CPU drops to roughly 10-15ms. This is a genuine
measurement-contract limitation in already-merged Issue #61 code, not
something introduced by the #73 live adapter — the live adapter's job
(execute real sessions against real engines) is precisely what exposed it;
Revisions 1-9's untimed correctness audits and the `#[cfg(test)]`-only
lifecycle core could never have surfaced it.

## Coverage actually achieved (and not)

Usable (equivalence-gate and calibration-gate passed, sealed by pattern
consistency across two independent cycles):

- Equivalence: WANDS (480/480) and ESCI-electronics (600/600), both `Match`.
- Index capture: all 4 cells (native/Solr × WANDS/ESCI).
- Calibration: both engines, 120/120 sessions, gate passed, in both `run1`
  and `rerun2` independently.
- Warm, WANDS only: `all` projection (30/30 blocks) and `fast-path`
  projection (30/30 blocks), both engines.

Not reached — **must not enter any headline H2H comparison**:

- Warm, WANDS: `hybrid` projection past block 8, and `punt` projection
  entirely.
- Warm, ESCI-electronics: no projection was reached (campaign order is
  WANDS-then-ESCI).
- Cold phase: neither dataset was reached.
- No cycle sealed, so no `i61_analyze` GateReport/CellStability output
  exists for any metric.

This is an incomplete E1 baseline, not a completed one. The two infra fixes
are real, verified, and durable; the CPU-reconciliation limitation is a
separate, pre-existing measurement-contract gap that the completed live
adapter correctly refused to paper over.

## Issue #62 entry

Issue #62 (E2 — physical footprint and SKU-density scaling) measures index
bytes and RSS at catalog-size tiers; it does not measure per-query CPU at the
magnitudes that expose this reconciliation limitation. #62 is **not blocked**
by this finding and may proceed once this PR merges to green `main`.

Any future experiment that needs low-CPU per-query CPU/latency comparisons
(small-facet PLP queries, autocomplete keystroke-level sessions) **must**
wait for the follow-up measurement-contract issue below to land a validated
reconciliation rule first.

## Follow-up

A dedicated measurement-contract issue is opened to evaluate a hybrid
absolute+relative reconciliation tolerance (conceptually
`abs(process_cpu - cgroup_cpu) <= max(relative_limit * reference_cpu,
absolute_noise_floor)`, with the absolute floor derived from this document's
observed noise distribution, not chosen to make any specific run pass) —
see the issue for its own preregistered protocol. #61's frozen 2% threshold
is **not** changed by this PR; only this new issue may revise it, with its
own preregistration and evidence.
