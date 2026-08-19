# Experiment Log

Append-only research log for the current architecture epic.

Do not rewrite failed experiments into success stories. If an experiment is superseded, keep the original entry and add the follow-up.

---

## E000 — Baseline / repository reset

**Question**  
Can the repository be converted into a clean Rust experimental baseline with reproducible correctness and performance harnesses before implementing the commerce thesis?

**Hypothesis**  
A minimal Rust workspace, deterministic fixtures, CI quality gates, and benchmark harness can replace the legacy C active path without carrying forward irrelevant architecture.

**Workload**  
To be established by Gate 0.

**Metrics / decision rule**  
Gate 0 passes only when formatting, clippy, unit/integration tests, release build, and at least one reproducible benchmark/replay command pass from a clean checkout.

**Implementation**  
Pending.

**Results**  
Pending.

**Interpretation**  
Pending.

**Regression check**  
Pending.

**Next question**  
After Gate 0, prove variant-safe commerce semantics before optimizing retrieval structures.
