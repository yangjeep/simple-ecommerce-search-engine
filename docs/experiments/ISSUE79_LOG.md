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

## 2026-09-28 — smoke checks (100k; not preregistered evidence)

- One-cell smoke runs of both native binaries through the scope driver, at 100k, to validate the harness: `facet_high_cardinality_color` and `numeric_range_sort`, one run each.
- These numbers were seen before calibration. They are **not** used for any rule or threshold, and they are not reported as results. Disclosed because they were seen early: at 100k, the F1/F2 facet paths barely changed end-to-end CPU on the full-catalog color facet, while bounded result assembly cut it by about 5x. That pointed toward H-F-b before any preregistered measurement.

## 2026-09-28 — corrected competitor facet baseline (500k, 3 clean runs, scope envelope) — BEFORE any native candidate measurement

- **Harness:** `i77_measure` (#77's corrected request builders) with `I77_RUNTIME=scope`, `--cells` set to the four facet cells plus `numeric_range_sort`, and `--skip-throughput true`. Driver on CPU 3; engine on CPUs 0-2 with CPUQuota 300%, MemoryMax 6G, swap 0. Git SHA in every raw file: `6c80fef`.
- **Results:** all 9 runs (Meilisearch, Typesense, Solr × 3) `status=Ok`, `correctness_all_passed=true`, with `num_found` identical to #77's 500k values (515,928 / 16,332 / 383,604). Raw files are in `artifacts/issue79/results/competitor/`.
- **Backend requests (corrected builders):** 1 on single-facet cells for Typesense and Meilisearch (E3: 2), and 2 on `facet_disjunctive_multi_dim` (E3: 6). Solr is 1 everywhere, unchanged.

Median CPU/query in µs over 3 runs (P95 in ms). The E3 column is #77's 500k median from `yangjeep-dev` (Docker, uncorrected builders), preserved unchanged; the old→corrected ratio is **cross-host**.

| cell | engine | E3b µs (runs) | CV | P95 ms | E3 µs | old→corrected |
|---|---|---|---|---|---|---|
| facet_low_cardinality_style | meilisearch | **9,948** (9,504 / 9,948 / 10,278) | 0.039 | 24.5 | 19,358 | 0.51x |
| | solr | 48,123 | 0.055 | 47.3 | 63,884 | 0.75x |
| | typesense | 268,560 | 0.041 | 171.8 | 375,066 | 0.72x |
| facet_medium_cardinality_primarymaterial | meilisearch | **9,326** (9,326 / 11,006 / 8,051) | 0.157 | 24.0 | 18,939 | 0.49x |
| | solr | 42,470 | 0.046 | 48.0 | 59,578 | 0.71x |
| | typesense | 271,582 | 0.029 | 173.9 | 364,086 | 0.75x |
| facet_high_cardinality_color | meilisearch | **7,987** (7,987 / 7,854 / 8,361) | 0.033 | 20.5 | 17,645 | 0.45x |
| | solr | 43,654 | 0.056 | 60.1 | 58,833 | 0.74x |
| | typesense | 267,879 | 0.033 | 182.3 | 381,389 | 0.70x |
| facet_disjunctive_multi_dim | meilisearch | **20,206** (20,963 / 20,206 / 17,493) | 0.093 | 38.5 | 48,004 | 0.42x |
| | solr | 47,505 | 0.044 | 49.0 | 64,015 | 0.74x |
| | typesense | 313,822 | 0.023 | 220.3 | 578,686 | 0.54x |
| numeric_range_sort | meilisearch | **9,164** (6,678 / 9,164 / 9,249) | 0.175 | 22.9 | 13,235 | 0.69x |
| | solr | 9,432 (8,504 / 10,984 / 9,432) | 0.130 | 8.9 | 11,814 | 0.80x |
| | typesense | 318,875 | 0.003 | 187.8 | 397,761 | 0.80x |

**Reading the delta:** Solr's request builder was not touched by the correction, so its 0.71–0.80x factor estimates the host change alone. Dividing Meilisearch's 0.42–0.51x by that factor attributes roughly 0.57–0.68x to the request-count correction, i.e. the extra backend request cost Meilisearch about 32–43%. Typesense's single-facet delta (0.70–0.75x) matches the host factor, so its extra request cost little relative to its large per-request cost.

The **fastest same-host competitor is Meilisearch on every cell.** On `numeric_range_sort` it effectively ties Solr (9,164 vs 9,432 µs, both CV > 0.13). Solr's P95 there is far lower (8.9 vs 22.9 ms). Classification uses CPU/query as preregistered, and the P95 disagreement is reported.

**Harness incident (disclosed; measurement unaffected):** `run_e3b.sh` was edited (headline branch only) while the competitor phase was running. Bash parses the whole `case … esac` before executing it, so the competitor branch ran as originally parsed. After `esac` it resumed reading at a shifted offset and failed with a syntax error at EOF; only the trailing `phase competitor done` log line was lost. All 9 runs had completed and been stopped cleanly. From here on, scripts are committed before a phase starts and are not edited while it runs.

## 2026-09-28 — N0, gate, calibration, headline, memory

- **N0** (`run_e3b.sh n0`): the frozen binary's sha256 was verified, 3 clean launches × 20 cells, all `ok`. Raw: `artifacts/issue79/results/n0/`. N0 CPU drifted down across launches (FH1: 195 → 171 → 134 ms; SH1: 893 → 632 → 666 ms). Host-level variance; disclosed in the decision doc.
- **500k gate** (`artifacts/issue79/results/gate/`): 980 candidate checks, 0 failures. The 60 baseline divergences are all from legacy sort on ascending requests.
- **Calibration** (`run_e3b.sh calibration`, 3 launches, calibration cells only). Both crossovers exist. τ_F = 919.4972390657695 and ρ_S = 0.0833779131971903 were frozen in commit `4d78435` and posted to #79 **before** the headline phase: `artifacts/issue79/results/calibration_fit.md`, `planner_constants.json`.
- **Headline** (`run_e3b.sh headline` with the frozen constants and FINAL = hybrid:hybrid, 3 launches, 47 (cell, mode) pairs each, all `ok`). Report: `artifacts/issue79/results/headline_report.md`.
- **Memory** (`run_e3b.sh memory`, 5 configurations × 3 launches, all `ok`, fixture green). Report: `artifacts/issue79/results/memory_report.md`. The measured RSS delta (+0.5 MiB) is below the deterministic structure bytes (12.6 MB): cgroup `memory.current` cannot resolve structures this small here.
- **Verdict:** sort RECOVERED / PARITY (0.855x); facets PARTIAL RECOVERY (single-facet worst cell 2.863x; disjunctive 1.280x). See `docs/decisions/ISSUE79_FACET_SORT_RECOVERY_DECISION.md`.

## 2026-09-28 — adversarial review and follow-ups

- The independent review (a fresh read-only agent) found no classification arithmetic error. Confirmed findings:
  - Meilisearch's default `maxValuesPerFacet = 100` made facet work non-like-for-like (and it returned full documents);
  - the preregistered N0′ fidelity check had not been reported;
  - the §5 gate-coverage text was wrong;
  - one table cell and the ES/OpenSearch ratio wording were wrong.

  Plausible concerns: the gate compares product IDs, not ordinals; the resource tag's RSS half is vacuous; phase-timer noise.
- **Follow-ups executed (all raw kept):**
  - Gate rerun with the frozen τ/ρ plus unsorted offset cases: 1,680 checks, 0 failures (`gate_500k_calibrated.json`).
  - **Like-for-like Meilisearch sensitivity** (`run_e3b.sh meili_lfl`, 10:44–11:28 UTC; 100k pre-check confirmed all 2,825 color values with exact counts): facets 9.6 / 14.3 / 48.6 / 47.9 ms, sort 6.0 ms.
  - **Interleaved fidelity A/B** (`run_e3b.sh fidelity`, 11:28–12:05 UTC): N0′/N0 per-pair 0.80–1.15x with identical outputs, so the new server adds no overhead. Across time, however, FINAL was 1.07–1.43x its headline-phase value, i.e. host drift of up to about 40% between sequential phases.
- **Effect on conclusions:** the preregistered verdicts (sort PARITY, facets PARTIAL) are kept. The like-for-like sensitivity (facets FH3/FH4 about 0.52–0.54x, sort 1.31x) is reported separately as not preregistered. Given host drift, the competitive position is recorded as **unresolved**. A confirmation run with interleaved native/competitor launches and equalized facet work is recommended before #64.
