# Issue #63 (Infra E3, amended) — Primitive CPU-efficiency and bytes-touched: log

Append-only. The preregistration is GitHub issue #63: the original body, **amendment 1 (2026-09-29)** appended to it, and **clarification C1** (issue comment, the same day). The verdict is in `docs/decisions/ISSUE63_PRIMITIVE_EFFICIENCY_DECISION.md`. Raw evidence is under `artifacts/issue63/results/`.

## 2026-09-29 — preconditions (before any #63 code or measurement)

- **Stale PR queue closed.**
  - #53 closed; #71 and #80 merged.
  - PR #59's unique Issue #57 content was salvaged onto fresh `main` in PR #81: 13 cherry-picked commits, mechanical conflict resolution, and a dated salvage note. A fresh focused review found three wording inaccuracies in that note, and they were fixed before merge. #81 was merged as `31a82e9`, and #59 was closed as superseded.
  - The only open PR is an unrelated dependabot bump (#58).
- **Final `main` gate** at `31a82e9`, run locally from a clean detached checkout:
  - `cargo fmt --all -- --check`: clean.
  - `cargo clippy --workspace --all-targets --all-features -- -D warnings`: clean.
  - `cargo test --workspace --all-features`: 786 passed, 0 failed.
  - `cargo build --workspace --release`: ok.
  - GitHub CI on `31a82e9`: `quality-gate` and every CodeQL job succeeded.
- **Reproducible binaries.** The release `e3b_native_plp_server` / `e3b_native_measure` / `e3b_correctness_gate` built from `31a82e9` have the same sha256 as the copies built earlier from the byte-identical salvage-branch tree: `ab24b45b…`, `2aa90330…`, `ead705bc…`.
- **Amendment 1 and clarification C1** were posted to #63 before any #63 code existed.
- **Host:** `athos-dev`, Xeon D-1518, 4 logical CPUs, 30 GB, and the #79 cgroup-v2 user-scope envelope. `perf_event_paranoid=4` with no sudo, so no hardware counters are available (amendment §1). They are not used.
