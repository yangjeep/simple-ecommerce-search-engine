# Issue #64 (Infra E4, amended) — Facet-heavy commerce workload economics: log

Append-only.
- **Preregistration:** GitHub issue #64, i.e. the original body plus **amendment 1 (2026-09-29)** appended to it.
- **Verdict:** `docs/decisions/ISSUE64_FACET_ECONOMICS_DECISION.md`.
- **Raw evidence:** `artifacts/issue64/results/`.

## 2026-09-29 — preconditions

- #63 merged (PR #82, `160ee09`). Its preregistered consequence, "continue after amendment", applied: no FH cell was in FACET DISADVANTAGE and FH4 ≤ 1.25. GitHub CI on `160ee09` was green.
- Amendment 1 was posted to #64 before any #64 code. Scopes were chosen from catalog composition only: sizes, and attribute coverage over {color, style, primarymaterial, material, shape}. No performance data was used.
- Branch `i64/facet-economics`, from `160ee09`. Host, envelope and competitors are as in #63.

## 2026-09-29 — implementation (before any preregistered measurement)

- **`issue77_eval::i64cells`** is the single definition of the 44 cells, shared by the native harness and the competitor harness.
  - A unit test pins the grid: 44 cells, 32 in R, 6 baselines, the ladders, and scope depths never faceted.
  - The test also pins the exact name list printed by `scripts/issue64/cell_names.py`.
- **Multi-select** is implemented in the experiment executor only. There is no `commerce-core` IR change, because native has no `EnumAny` constraint.
  - `plp::PlpRequest.any_filters` is parsed from `anyfilter=attr:v1|v2`; no WANDS value contains `|` (checked).
  - Semantics: a union of value bitmaps, AND-ed with every other constraint. A faceted attribute self-excludes its own multi-select. A set restricted by a multi-select is never match-all.
  - The oracle (`issue79_eval::oracle`) gained the same semantics independently.
  - **RED first:** `tests/i64_any_filters.rs` failed to compile before `any_filters` existed. It checks 8 requests × 46 mode combinations against the oracle, plus parser round-trip and error cases.
- **`i77_measure`** gains `WorkloadKind::Plp64`, appended to the matrix only when `I64_CELLS=1`. #77's own 20 cells and their indices are unchanged.
  - **Solr:** plain `fq`s for the scope. Tagged `fq`s with `excludeTags` for selections on faceted attributes. Multi-select is an OR of phrase terms inside one tagged `fq`. One request.
  - **Meilisearch:** one base request, plus one `limit: 0` request per self-excluded facet. Multi-select uses `IN [...]`.
  - **Escaping:** values are escaped for `\` and `"`. One scope, `4' x 6' Area Rugs`, contains apostrophes.
  - ES/Typesense/Vespa panic on #64 cells, since they are not #64 arms.
  - Three builder unit tests cover this.
- **Equivalence checker:** it now also requires the returned hit count to equal min(48, `num_found`), whenever the dump records it (every #64 dump does). Native, Solr and Meilisearch dumps all record `hit_count`.
- **`e3b_native_measure`:** `--cells i64`, with names resolved across all cell lists. **`e3b_correctness_gate`:** `--include-i64 true`.
- **Driver** `scripts/issue64/run_i64.sh`. The `confirm` phase checks equivalence after each arm, before the next arm starts. That was #63's disclosed deviation, and it is fixed here as amendment §4 requires.
- **Analysis** `scripts/issue64/analyze_i64.py`: the section 5 verdict, cost-curve slopes, facet-only CPU, breakpoints and cores per 1,000 QPS.

## 2026-09-29 — correctness gate (500k)

- **First run: 312 candidate failures**, all on the two multi-select cells (`*_k5_style2`).
  - `num_found` and facets matched. Only `candidates_ok` was false, plus 4 legacy-sort docs checks whose components were already known to diverge.
  - **Cause:** the gate recomputes base retrieval itself (constraints only), and that recomputation did not apply `any_filters`. It was **a bug in the gate, not in the executor**. The executor's `num_found` and facets had matched the oracle.
- **Fix:** the gate's recomputation now applies multi-select.
- **Rerun:** `E3B_GATE rows=515928 candidate_checks=9984 candidate_failures=0 baseline_mismatches=192`. 6,864 of the checks are on #64 cells. All 192 baseline divergences are the known legacy (#77) ascending-sort comparator in P0 docs.
- Raw data: `artifacts/issue64/results/gate/`.

## 2026-09-29 — smoke at 100k (not evidence)

- One pass of native N⁺, Meilisearch and Solr over all 44 cells at 100k, with dumps. The equivalence checker returned **132/132 EQUIVALENT**, including the multi-select, apostrophe and hit-count checks.
- The analysis script was dry-run on these outputs to check exit status, row counts and verdict mechanics. Its tables were not inspected, and no rule depends on them.
- A first smoke attempt was aborted before measuring anything. It had extracted the cell list wrongly, which led to the shared `cell_names.py`, and then my own `pkill` pattern killed it.

## 2026-09-29 — binary provenance

- The Part A binaries were built at `f20e4de`; the gate binary was rebuilt at `a7c60be` with its fix.
- `git diff f20e4de HEAD -- crates Cargo.lock` shows only that gate fix and a test-only addition in `i64cells.rs`.
- sha256 prefixes:

| binary | sha256 prefix |
|---|---|
| `e3b_native_plp_server` | `c74e6ba5324e9262` |
| `e3b_native_measure` | `b863d932c87764a8` |
| `i77_measure` | `b01fc049b1ec7b7a` |
| `i63_facet_equivalence` | `b86d3cf6fd6a48c7` |
| `e3b_correctness_gate` | `71d0f7bbc5e1f019` |

## 2026-09-29 — confirm phase: 3×3 Latin square (10:57–12:50 UTC)

- **Run:** `run_i64.sh confirm`, with the order exactly as preregistered: run1 N → M → S; run2 S → N → M; run3 M → S → N.
  - All 9 arms exited 0, with `status=ok` and correctness passing.
  - **Equal-work verification ran after every arm, before the next arm started: 9/9 all EQUIVALENT**, covering 44 cells per arm.
  - Raw data: `artifacts/issue64/results/confirm/run{1,2,3}/` (raw JSON, per-arm dumps and equivalence); report `confirm_report.{json,md}`.
- **Preregistered verdict (section 5): KEEP.** All 32 cells of the realistic grid R are MATERIAL FACET ADVANTAGE, and all are robust. r ranges from 0.068 (S5, k=3) to 0.458 (S1, k=10). No cell is NOT_EQUIVALENT_WORK.
- **Facet-only CPU** (cell minus same-scope k=0; a reported metric, not gated):
  - native ÷ the cheapest competitor's facet-only CPU is 0.12–0.53 on S0–S4.
  - On S5 (600 docs), Solr's facet-only CPU is within noise: its k=0 costs 7,203 µs, and some faceted cells cost less than that. So S5's advantage is the per-request floor.
- **Per-request floor (k=0):** native 370–438 µs, Meilisearch 5,977–6,587 µs, Solr 5,508–20,134 µs.
- **Incremental CPU per added facet** (slope, µs/facet), native / Meilisearch / Solr:

| scope | native | Meilisearch | Solr |
|---|---|---|---|
| S0 | 5,137 | 13,396 | 22,014 |
| S1 | 3,371 | 6,263 | 11,649 |
| S2 | 2,075 | 5,079 | 6,346 |
| S3 | 732 | 4,048 | 2,624 |
| S4 | 188 | 1,766 | **138** |
| S5 | 59 | 2,395 | 188 |

  At S4, Solr's marginal cost per facet is below native's.
- **RSS**, median over runs: native 4.41 GB, Meilisearch 5.55 GB, Solr 3.85 GB (3 GiB JVM heap). E2's memory REFINE stands.

## 2026-09-29 — adversarial review and corrections

- **Review:** a fresh read-only reviewer recomputed everything from raw data and could not falsify the preregistered KEEP. Its 10 dispositioned findings are in the decision's §7.
- **Corrections to the decision:**
  - The floor is measured under `Connection: close`, not under #61 R2.1's persistent connection.
  - Post-review floor-neutral sensitivity: with the competitors given native's own k=0 floor, all 27 R cells on S1–S4 stay MATERIAL (0.17–0.57), while S5 is unclassifiable because Solr's facet-only CPU is ≤ 0 there.
  - Facet emptiness was measured from the native run-1 dumps. S0–S3 facets are essentially all non-empty; on S4/S5 most are empty; in-scope color cardinality is 2,825 / 834 / 951 / 381 / 97 / 0.
  - The S4 slope observation is retracted as a breakpoint: no class changes.
  - "Cores per unit throughput" is rewritten as CPU per query on a single connection.
  - Host-probe spread: 615–920 ms.
  - Disclosed: Solr's k=0 carry-over under fixed cell order, untried Solr facet methods (`dvhash`), and the non-independence of the gate's multi-select recomputation.
- **First gate run, from its console output** (its raw JSON was overwritten by the rerun, as disclosed):

  ```
  E3B_GATE rows=515928 nan=0 candidate_checks=9984 candidate_failures=312 baseline_mismatches=192
  ```

  All 312 failures are on `i64_s2_k5_style2` and `i64_s3_k5_style2`: 154 + 154 with only `candidates_ok = false`, plus 2 + 2 whose docs also diverged, all on legacy-sort (`sorted_rating_count_asc`) probes.
- **Harness:** `run_i64.sh` now logs each measurement's own exit status in `arm_exit`. As first run, the competitor arms logged `e3b_scope_stop`'s status. Their success is confirmed by `MEASURE_OK … status=Ok` and by raw `status=ok`.
- **Smoke:** the 100k equivalence output is archived under `artifacts/issue64/results/smoke_100k_not_evidence/`.
