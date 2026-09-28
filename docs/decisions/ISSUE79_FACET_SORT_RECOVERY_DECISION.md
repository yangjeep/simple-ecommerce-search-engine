# Issue #79 — Infra E3b: Facet & Sort Physical-Design Recovery

**Question (#79):** Are E3's (#77) severe faceting and sort negatives caused by the commerce-native execution model itself, or does the current implementation lack physical execution structures suited to those workloads?

**Answer, per dimension (preregistered classification; details below):**

| dimension | E3 (frozen) | E3b same-host gap before (N0 ÷ fastest) | E3b FINAL ÷ fastest (r) | verdict |
|---|---|---|---|---|
| Sort (`numeric_range_sort`, held-out) | 71x slower | 72.7x slower | **0.855** | **RECOVERED / PARITY**; resource-neutral by the preregistered tag (+24.4 B/doc on-heap estimate = +0.29% RSS, +10.8% of the index estimate; §8) |
| Facet (a): single facet, full catalog (FH1–FH3; worst cell sets the class) | 9.4–11.5x slower | 17.2x / 18.7x / 23.7x slower | 0.858 / 1.200 / **2.863** | **PARTIAL RECOVERY** (8.3–20.0x vs N0; style and primarymaterial reach parity, high-cardinality color does not) |
| Facet (b): disjunctive, 5 facets, active filter (FH4, the core realistic cell) | 1.9x slower | 2.9x slower | **1.280** | **PARTIAL RECOVERY** (2.2x vs N0; misses the parity band by 0.03) |
| Overall facet STRONG condition (r ≤ 0.75 on FH4 plus one more cell) | — | — | not met | no material facet *advantage* shown |
| Memory / physical footprint (E2) | REFINE (negative at 1M) | — | — | **unchanged, not addressed** |

**Reading.** The E3 sort negative was a missing-physical-structure problem, with no fundamental execution-model limit behind it.

- A bounded top-K over a precomputed value order brings sort from 72.7x slower to 0.855x (parity, not a material win). The sort phase itself is 0.15 ms (48 IDs inspected); the rest of the request is the unchanged range-filter candidate construction.

The E3 facet negative was **mostly** an implementation artifact, but not entirely.

- The dominant cost in E3's facet cells was not facet counting. It was the facet-independent, O(|candidates|) result-assembly loop in `i77_native_plp_server`, which hash-looked-up every one of 515,928 candidates to return 48 IDs (H-F-b). The second-largest cost was the vocabulary-driven, materializing `facet_counts` (H-F-a).
- With both fixed, low- and medium-cardinality full-catalog facets reach parity with the fastest same-host competitor (Meilisearch).
- High-cardinality full-catalog counting (color, V = 2825, over 516k candidates), and the disjunctive cell whose excluded color facet needs exactly that, remain 1.28–2.86x slower. The residual cost is attributed in §6: full-catalog candidate-bitmap construction (an unchanged retrieval path, out of scope) plus about 12 ms of per-candidate ordinal counting.

So the answer is "missing physical implementation" for sort and for most of faceting. For the high-cardinality full-catalog case, a residual native disadvantage remains after the targeted design attempt. It is not recovered here.

Raw evidence is under `artifacts/issue79/results/`. The log is `docs/experiments/ISSUE79_LOG.md`, and the preregistration, with its source-level attribution, is GitHub #79.

## 1. Frozen evidence and scope

E3's results (`ISSUE77_PLP_FACETING_DECISION.md`) are not modified or reinterpreted. E3's numbers were measured on `yangjeep-dev` under Docker and remain the historical record. No E3b ratio mixes them with E3b numbers.

The E2 memory negative (`ISSUE62_PHYSICAL_FOOTPRINT_DECISION.md`) is untouched. No mmap, persistence, compaction, allocator, HashMap-replacement, server-concurrency or semantic work was done.

Only the following changed:

- facet/sort algorithms;
- two optional sort structures;
- two deterministic path rules;
- the harness and correctness coverage.

## 2. Environment amendment (preregistered in #79 §2)

This session's host (`athos-dev`: Xeon D-1518, 4 logical CPUs, 30 GB) has no Docker and no passwordless sudo. The frozen E3 envelope values (3 CPUs, 6 GiB, no swap) were therefore enforced with a user cgroup-v2 scope:

```
systemd-run --user --scope -p CPUQuota=300% -p MemoryMax=6G -p MemorySwapMax=0 \
  taskset -c 0-2 <engine>
```

These are the same kernel knobs Docker's `--cpus` / `--memory` / `--memory-swap` set. The driver ran on `taskset -c 3`. CPU/query is the scope's `cpu.stat usage_usec` delta over the batch.

Because the host changed, **every compared number in E3b was measured on this host**, including a same-host rerun of the fastest competitors. Vespa could not run without Docker (`EXCLUDED_ENVIRONMENTALLY`). ES and OpenSearch were not re-measured: their builders were unaffected by the correction, and they were ≥2.4x slower than Solr/Meilisearch on every target cell in E3.

## 3. Order of operations (as preregistered; commits on the PR branch)

1. Preregistration #79 and the #60 reorder note.
2. Harness: the `I77_RUNTIME=scope` runtime and `--cells` / `--skip-throughput`. Commit `6c80fef`.
3. **Corrected competitor baseline**: Meilisearch, Typesense and Solr, 500k, 3 clean runs each. Commit `7c12cdb`.
4. **N0**: the unchanged `9482f5d` `i77_native_plp_server` binary (sha256 `91060ad1…6333b`), 500k, 3 clean runs, all 20 cells.
5. **500k correctness gate**: 980 candidate checks, 0 failures.
6. **Calibration** (calibration cells only), then the F3/S3 constants frozen in commit `4d78435` and posted to #79 before any held-out measurement.
7. **Held-out headline phase**, 3 clean runs.
8. Memory/build launches.

Disclosed deviation: the candidate code was *written* before N0 was measured; no candidate was *measured* before steps 3–4. See the log.

## 4. Corrected competitor facet baseline (500k, same host; old → corrected)

The fastest same-host competitor is Meilisearch on every cell. The full per-engine table, with per-run values and CVs, is in the log.

| cell | Meilisearch µs (E3b, corrected) | E3 µs | old→corrected (cross-host) | backend requests E3 → E3b |
|---|---|---|---|---|
| facet_low_cardinality_style | 9,948 | 19,358 | 0.51x | 2 → 1 |
| facet_medium_cardinality_primarymaterial | 9,326 | 18,939 | 0.49x | 2 → 1 |
| facet_high_cardinality_color | 7,987 | 17,645 | 0.45x | 2 → 1 |
| facet_disjunctive_multi_dim | 20,206 | 48,004 | 0.42x | 6 → 2 |
| numeric_range_sort | 9,164 (Solr 9,432) | 13,235 | 0.69x | 1 → 1 |

Solr's builder is untouched by the correction, and it moved 0.71–0.80x from the host change alone. On that basis, about 0.57–0.68x of Meilisearch's delta is the request-count correction. As #77's review predicted, the correction makes native's E3 facet gap *larger* on this host (N0 ÷ Meilisearch: 17–24x on single facets, 2.9x on disjunctive).

## 5. Correctness (gate: `e3b_correctness_gate`, 500k, before interpretation)

The gate covers every E3b cell and every variant (legacy/F1/F2 × legacy/S1/S2, plus hybrid × 4 τ × 3 ρ, including the calibrated constants). It adds asc, offset-10 and "sorted facet cell" probes. Everything is compared against an independent oracle, a `Catalog` + `effective_attributes` linear scan with its own comparator.

- **980 candidate checks, 0 failures.** Each check covers:
  - filter-result ordinals (equal to the oracle);
  - `num_found`;
  - facet maps for all five WANDS facet fields, including disjunctive self-exclusion, nulls never counted and zeros omitted;
  - the exact top-48 `(id, sort_value)` sequence (field, asc/desc, ties by ordinal ascending, missing values last, filter before sort).
- **60 baseline divergences, all from the legacy (N0) sort comparator on ascending requests.** #77's `Option` ordering puts missing values *first* ascending, and E3b defines them as last. They are recorded, not "fixed", in N0.
- All five facet fields are single-valued `Enum` (the ordinal path is exact), and there are 0 NaN values. The `MultiEnum` guard is unit-tested.
- The #77 same-product cross-variant fixture (`/correctness`) passed in every one of the 24 native launches (N0, calibration, headline, memory). New unit tests (`crates/commerce-core/tests/e3b_facet_sort.rs`) exhaustively check every candidate subset of a 9-variant, 4-product fixture with product- and variant-level attributes, missing values, ties and ±0.0.
- **A real candidate bug was caught before any measurement.** Strategy B mis-ordered `-0.0` / `0.0` ties, because the pre-existing `numeric_index` is `total_cmp`-ordered. It was fixed; see the log.

## 6. Held-out headline results (500k, median of 3 clean launches, CPU/query µs)

FINAL = F3 + S3 with the frozen constants. The "cand / facets / sort" columns are in-process phase timers (the mean over the batch, median over runs), used for decomposition only; the headline is batched cgroup CPU. The complete table for every variant, with CV/P50/P95/IDs inspected/paths, is in `artifacts/issue79/results/headline_report.md`.

| cell | N0 (unchanged binary) | F-best + legacy result path | legacy facets + S3 | **FINAL** | FINAL phases: cand / facets / sort | Meilisearch | r | FINAL P95 vs Meili P95 (ms) |
|---|---|---|---|---|---|---|---|---|
| FH1 style (V = 65) | 170,669 | 185,975 (hybrid) | 16,179 | **8,538** | 7,521 / 571 / 117 | 9,948 | 0.858 | 12.8 vs 24.5 |
| FH2 primarymaterial (V = 244) | 174,457 | 180,097 (hybrid) | 19,065 | **11,186** | 9,087 / 1,539 / 125 | 9,326 | 1.200 | 20.6 vs 24.0 |
| FH3 color (V = 2825) | 188,889 | 194,407 (hybrid) | 41,554 | **22,868** | 8,683 / 12,332 / 126 | 7,987 | 2.863 | 34.9 vs 20.5 |
| FH4 disjunctive (16,332 cand.) | 57,773 | 34,414 (hybrid) | 61,890 | **25,857** | 30 / 24,301 / 122 | 20,206 | 1.280 | 47.5 vs 38.5 |
| SH1 range + sort (383,604 cand.) | 666,125 | — | 7,038 | **7,832** | 7,292 / 1 / 146 | 9,164 | 0.855 | 14.5 vs 22.9 |
| SH2 medium sort (17,256 cand.) | 43,474 | — | 538 | **646** | 23 / 0 / 269 | — | — | — |
| SH3 narrow sort (36 cand.) | 607 | — | 506 | **653** | 153 / 1 / 115 | — | — | — |

**Causal decomposition (facet cells).**

- **H-F-b, facet-independent work, dominated E3's facet negative.** Changing only the facet algorithm while keeping N0's result path (F1/F2/F3 + legacy) moves full-catalog CPU by less than 10%. Changing only the result path (legacy facets + S3's bounded assembly) removes about 90% of it.
- **H-F-a, the facet algorithm, is the second term.** The facet phase goes from 7.2 ms (legacy) to 0.57 ms (bitmap) on style, and from 61.1 ms to 24.3 ms (ordinal) on the disjunctive cell. Legacy's per-facet candidate recomputation and materialized intersections are gone.
- **H-F-c, the residual.** For FH3, about 8.7 ms is `all_ordinals()`, the full-catalog candidate bitmap built by per-element insertion in the *unchanged* retrieval path (`indexed_candidates(&[])`, out of scope here). About 12.3 ms is counting a 2825-value facet over 516k candidates. Ordinal (13.1 ms) and bitmap (12.7 ms) cost the same there, so neither preregistered strategy gets this cell to parity. FH4's 24.3 ms facet phase is dominated by exactly this same full-catalog color facet, which disjunctive self-exclusion forces, plus a second `all_ordinals()` for it.

**Sort.** The legacy per-candidate `effective_attributes` clone plus full sort (sort phase 867 ms) is replaced by:

- **S2** (presorted walk, 48 IDs inspected, 0.15 ms);
- **S1** (bounded heap over 383,604 candidates, 10.5 ms).

What is left of FINAL is the unchanged numeric-range candidate construction (`numeric_range` collecting 383,604 ordinals in value order, 7.3 ms). SH3 (36 candidates) was already cheap in N0 and stays so (653 vs 607 µs, within spread). There, presorted would have been a 25x regression (15.7 ms), and the planner avoided it.

**N0 spread (disclosed).** The unchanged N0 binary's CPU drifted downward across its three launches: FH1 was 195 → 171 → 134 ms, and SH1 893 → 632 → 666 ms (CV about 0.16–0.19). FINAL's CVs are 0.02–0.20. Classifications use FINAL ÷ competitor, which is not affected. N0 ÷ FINAL multipliers carry the N0 spread.

## 7. Crossover tables and planner validation

**Calibration cells** (in-process phase time, median of 3 runs; full table in `artifacts/issue79/results/calibration_fit.md`):

| facet cell | \|C\| | V | x = \|C\|/V | ordinal µs | bitmap µs | winner |
|---|---|---|---|---|---|---|
| FC1 full × material | 515,928 | 162 | 3,185 | 6,742 | 973 | bitmap 6.9x |
| FC2 full × shape | 515,928 | 94 | 5,489 | 6,721 | 671 | bitmap 10.0x |
| FC3 style=modern × color | 97,188 | 2825 | 34.4 | 3,234 | 9,958 | ordinal 3.1x |
| FC4 color=white × style | 17,256 | 65 | 265 | 725 | 4,044 | ordinal 5.6x |
| FC5 color=white × primarymaterial | 17,256 | 244 | 70.7 | 788 | 10,320 | ordinal 13x |
| FC6 depth-3 × shape | 36 | 94 | 0.38 | 18 | 1,049 | ordinal 58x |
| FC7 Pergolas × color | 156 | 2825 | 0.055 | 31 | 12,174 | ordinal 395x |

| sort cell | \|C\| | y = \|C\|²/(kN) | top-K µs | presorted µs | winner |
|---|---|---|---|---|---|
| SC1 full, rating_count desc | 515,928 | 10,749 | 12,428 | 168 | presorted 74x |
| SC2 style=modern, average_rating asc | 97,188 | 381 | 3,453 | 98 | presorted 35x |
| SC3 Accent Chairs, review_count desc | 13,236 | 7.07 | 1,188 | 549 | presorted 2.2x |
| SC4 color=black, rating_count asc | 16,332 | 10.8 | 1,303 | 289 | presorted 4.5x |
| SC5 Pergolas, average_rating desc | 156 | 0.00098 | 156 | 10,464 | top-K 67x |
| SC6 depth-5, review_count desc | 12 | 5.8e-6 | 32 | 12,144 | top-K 380x |

**Crossovers.** Both exist under the preregistered "≥25% win on each side" rule. The preregistered minimum-summed-time procedure then fixed:

- **τ_F = 919.5** (bitmap iff |C_f| ≥ τ·V_f). The feasible interval is (265, 3185).
- **ρ_S = 0.0834** (presorted iff |C|² ≥ ρ·k·N). The feasible interval is (0.00098, 7.07), which is wide; the geometric midpoint was used.

Both were frozen before the headline phase.

**Held-out check.** The hybrid chose the cheaper phase path on 6 of 7 headline cells:

- style → bitmap (571 vs 7,693 µs);
- primarymaterial → bitmap (1,539 vs 8,872);
- disjunctive → ordinal on all 5 facets (24,301 vs 44,159);
- SH1 → presorted (141 vs 10,478);
- SH2 → presorted (246 vs 1,370);
- SH3 → top-K (96 vs 15,293).

On FH3 (color, full catalog) it chose ordinal (x = 183), where ordinal and bitmap tie (13.1 vs 12.7 ms, a 3% gap within run spread). There is no held-out case where the rule picked a materially worse path.

## 8. Memory / build accounting (on-heap estimates are not disk; disk footprint NOT COMPARABLE)

Measured with five launch configurations of the new server, 3 clean launches each, `--cells none`. RSS is cgroup `memory.current` 2 s after ready. The N0 row comes from the N0 runs. Raw data: `artifacts/issue79/results/memory/`; table: `memory_report.md`.

| configuration | new structures | deterministic owned bytes (on-heap estimate) | bytes/doc | RSS after load, MiB (3 launches) | build time | launch→ready s |
|---|---|---|---|---|---|---|
| N0 binary (#77) | — | — | — | 4206.5 / 4206.0 / 4206.1 | — | 107.6 |
| base (new server, none built) | — | 0 | 0 | 4205.8 / 4206.0 / 4206.2 | index 85.8 s | 96.4 |
| facet structures only (F1/F2/F3) | none: reuses `enum_columns` + `enum_bitmaps`, already in the base index | **0** (the `MultiEnum` guard set is empty on WANDS) | 0 | 4206.5 / 4206.0 / 4206.0 | 0 | 91.2 |
| sort structures, S1 only | 3 × `NumericSortColumn` | **12,382,272** | **24.0** | 4206.0 / 4206.0 / 4206.0 | 10.4 ms | 92.0 |
| sort structures, S2 only | 3 × `PresenceBitmap` (the order is the pre-existing `numeric_index`) | **196,824** | **0.38** | 4206.0 / 4206.0 / 4205.8 | 52.1 ms | 100.8 |
| facet + sort combined (FINAL) | all of the above | **12,579,096** | **24.4** | 4206.2 / 4206.7 / 4206.5 | 10.6 + 61.6 ms | 93.8 |

For scale:

- the whole base index's on-heap estimate (`approximate_size_bytes`) is 116,915,570 B, of which ordinal-facet structures are 29,361,380 B;
- measured RSS is about 4,410 MB. It is dominated by the materialized catalog, E2's finding, which is unchanged.

The combined new structures are therefore **+10.8% of the index on-heap estimate** and **+0.29% of measured RSS**.

**Measured RSS delta.** Combined − base is +0.5 MiB median, and the per-launch ranges overlap (base max 4206.2 = combined min 4206.2). That is *below* the deterministic 12.6 MB, so this measurement cannot resolve a structure of this size. The likely reason is that the columns are allocated after load into heap already freed by the build: allocator reuse of load-time churn. The deterministic estimate is the primary number. Server phase timings include all structure work, and nothing runs in the background.

**Build/startup delta.** Structure build takes 72 ms in total, against about 85 s of index build. Launch→ready differences between configurations (91–101 s) are within run noise (77–104 s).

**Resource tag (preregistered rule).**

- Incremental on-heap is ≤1% of base RSS (0.29%), and the measured RSS delta is within run spread, so the sort recovery is tagged **resource-neutral**.
- Caveat: that rests on RSS being dominated by E2's materialized catalog. Relative to the index's own structures, the sort columns are a real +10.8%.
- A design that must also fix E2's footprint would feel that 24 B/doc, whereas S2 alone (0.38 B/doc) pays almost nothing: S2 is the path used on every cell where |C| is not narrow.

Disk footprint: **NOT COMPARABLE**, since native still persists nothing.

## 9. Verdict and follow-on (per #79 §10)

- **Sort: RECOVERED / PARITY.** The E3 sort negative (71x) is explained by missing physical sort structures, and closes to parity (0.855x the fastest same-host competitor) with a bounded top-K over the pre-existing value order plus a presence bitmap. That is not a material native advantage: Meilisearch and Solr are within 25%. Resource tag: resource-neutral (+24.4 B/doc on-heap estimate, 0.29% of RSS), with the +10.8%-of-index caveat in §8.
- **Facet: PARTIAL RECOVERY** on both sub-dimensions.
  - The E3 facet negative was predominantly an artifact of the E3 server: facet-independent O(|candidates|) result assembly, plus a legacy counting routine, while an already-accepted ordinal path (ADR 0011) sat unused.
  - Once removed, low/medium-cardinality full-catalog facets reach parity.
  - A real residual disadvantage remains for high-cardinality full-catalog counting (2.86x) and for the disjunctive cell that needs it (1.28x).
  - No STRONG facet recovery: there is no ≥25% native facet advantage anywhere.
- **Branch taken.** Neither facet nor sort "still clearly fails", so #64 is **not** paused on this evidence. The next step is **#63**, attributing the primitives with bytes-touched measurements. It should start with the two residuals this round isolated but was not allowed to change:
  1. full-catalog candidate materialization (`all_ordinals()` / match-all representation, about 7–9 ms at 516k);
  2. high-cardinality counting over dense candidate sets.

  Before #64 executes, its preregistration must be amended for the new facet/sort physical paths, and it must not assume a native facet cost advantage: this round measured none.
- **Unchanged:** E2's memory REFINE, and E3's throughput / single-connection confound (out of scope here).

## 10. Limitations

- **Different host and runtime from E3.** A cgroup scope instead of Docker, and cpuset via affinity. All E3b comparisons are same-host, but E3b ratios are not directly comparable with E3's.
- **Competitor coverage.** Vespa was excluded environmentally, and ES/OpenSearch were not re-measured. "Fastest competitor" assumes E3's ranking holds for those two, which were ≥2.4x slower than Solr/Meilisearch in E3.
- **In-process phase timers** are wall-clock inside one single-threaded request, used for decomposition and calibration only. The headline is batched cgroup CPU (≥200 requests and ≥2 s windows, per the #74 rule).
- **FH4's r = 1.280 is within one run-spread of the 1.25 PARITY bound** (FINAL CV 0.195; runs 25.9 / 23.5 / 33.8 ms). The preregistered class is PARTIAL, and it is reported as such.
- **Bounded unsorted assembly also changes E3's non-facet unsorted PLP path** (e.g. `base_plp_broad`). Those cells were not re-measured or re-claimed here.
- **The sort boundary ρ is loosely constrained** by calibration (a feasible interval spanning about 4 orders of magnitude).
- **The in-process server is still single-connection** (E3 confound, out of scope). Latency and CPU were measured with serial requests.
