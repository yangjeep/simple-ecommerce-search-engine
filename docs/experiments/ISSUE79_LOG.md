# Issue #79 (Infra E3b) — Facet & Sort Physical-Design Recovery: log

Append-only. The preregistration (the question, source-level attribution, the environment amendment, variants, cells, crossover rules, classification) is GitHub issue #79. The verdict is in `docs/decisions/ISSUE79_FACET_SORT_RECOVERY_DECISION.md`.

## 2026-09-28 — setup (before any measurement)

- **Starting point:** `main` @ `9482f5d`, green. The branch is `i79/e3b-facet-sort-recovery`.
- **Host:** `athos-dev`. At session start it was a 4-vCPU QEMU VM; the host was then re-provisioned mid-session, before any measurement, into an Intel Xeon D-1518 with 4 logical CPUs, 30 GB RAM and AVX2. It has no Docker, no passwordless sudo and no uidmap. It had no Rust toolchain either; rustup was installed in user space (cargo 1.98.1). There was no WANDS data; it was fetched with `scripts/datasets/fetch_wands.sh` (pinned commit, checksums verified), prepared with `prepare_wands.py` and replicated with `replicate_wands_scale.py 12` and `3`. Line counts are 515,928 and 128,982, matching #77's `I77_TIER_*_DOCS`.
- **Frozen N0 binary:** `target/release/i77_native_plp_server`, built from unmodified `9482f5d` and copied to `~/.cache/e3b-engines/n0/i77_native_plp_server_9482f5d`. Its sha256 is `91060ad12cd2ed8bcd5199776ca2ababe3438c48f2274ae3647d5b819916333b`, and `run_e3b.sh n0` verifies it before every N0 run.
- **Competitor binaries** (official downloads, in `~/.cache/e3b-engines/`):
  - Typesense 27.1 (`dl.typesense.org`);
  - Meilisearch 1.11.3 (GitHub release);
  - Solr 9.10.1 (`archive.apache.org`);
  - Eclipse Temurin JRE 21.0.12.1 (Adoptium).
- **Scope envelope** (#79 section 2): `scripts/issue79/scope_runtime.sh`. Launched with `I77_RUNTIME=scope`. The `i77_measure` request builders and accounting are unchanged; only launching, cgroup lookup, and the `--cells` / `--skip-throughput` filters were added. The #77 provisioning scripts got an `I77_RUNTIME=scope` branch and keep their Docker path byte-identical apart from indentation.
- **Harness fix:** `issue61_eval::CgroupReader::snapshot` required `cpuset.cpus.effective`, which a user-delegated scope does not have (cpuset controller not delegated). The first 100k smoke test therefore recorded `mean_cpu_usec_per_query = null`. Fix: that one field falls back to `"not-delegated"`, and every CPU/memory counter is still required.
- **Implementation order deviation (disclosed):** commerce-core candidate code (`facet_counts_bitmap`, `index::sort`) was written *before* the N0 baseline was measured, not after as #79 section 3 lists. This cannot affect N0, because N0 is the frozen, checksummed `9482f5d` binary. No candidate path was *measured* before the corrected competitor baseline and N0 finished.

### RED test caught a real candidate bug before any measurement

The exhaustive subset test `both_sort_strategies_match_the_reference_for_every_subset_direction_and_limit` (`crates/commerce-core/tests/e3b_facet_sort.rs`) failed for strategy B on the candidate set {ordinal 6 = `0.0`, ordinal 7 = `-0.0`}, ascending.

Cause: the pre-existing `numeric_index` is ordered by `f64::total_cmp`, which puts `-0.0` before `0.0`, while the preregistered order treats them as equal and breaks the tie by ordinal. Strategy B's "equal-value run is ordinal-ascending" assumption was therefore false for ±0.

Fix: walk runs by numeric `==` and merge a run's sign-split sub-runs by ordinal. Strategy A normalizes `-0.0` in its comparator. WANDS has no negative values, so this could not have changed an E3b measurement, but it was a wrong-answer bug.

### Correctness gate, 100k calibration tier (pre-measurement sanity)

`e3b_correctness_gate --catalog catalog_3x.jsonl` compared 980 candidate-variant checks against the oracle, with **0 failures**. It also recorded 60 baseline divergences, all from the legacy (N0) sort component: #77's comparator orders missing values *first* on ascending sorts (`Option` ordering), whereas the preregistered semantics put them last. They show up in N0' and in F1/F2 × legacy-sort combinations, which keep N0's result path by design. All five facet fields are single-valued `Enum` (the ordinal path is exact), and there are 0 NaN numeric values.
