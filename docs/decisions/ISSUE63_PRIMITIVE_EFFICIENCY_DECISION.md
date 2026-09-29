# Issue #63 — Infra E3 (amended): primitive CPU-efficiency, equal-work facet confirmation

**Questions** (#63 amendment 1 and clarification C1; the original body is preserved on the issue):

- **(A)** Under equal logical facet work, measured in the same window with counterbalanced order, is native faceting faster than, at parity with, or slower than the fastest mature engine?
- **(B)** What are the two residual native costs #79 isolated, full-catalog candidate materialization and dense high-cardinality counting, made of?
- **(C)** What does each structural primitive cost?

**Answer.**

| | result |
|---|---|
| **A, native FINAL (#79's structures, frozen τ/ρ)** | FH1 **PARITY** (0.790) · FH2 **PARITY** (0.888) · FH3 **MATERIAL FACET ADVANTAGE** (0.702, robust) · FH4 **MATERIAL FACET ADVANTAGE** (0.513, robust). This is the preregistered verdict. §7 gives a post-review Solr-configuration sensitivity for the FH3/FH4 margin. |
| **A, native N⁺ = FINAL + P0r (frozen before Part A by the §3.4 rule)** | FH1 **0.100** · FH2 **0.144** · FH3 **0.425** · FH4 **0.346**: **MATERIAL FACET ADVANTAGE on every cell, robust on every cell** |
| **Scope of A** | Every headline cell is **full-catalog-shaped**. FH1–FH3 have no filter at all. FH4's cost is dominated by its self-excluded color facet, which is computed over the whole catalog: FINAL FH4 ≈ FH3, and Meilisearch's second FH4 request is an unfiltered color facet. **Part A has no category-scoped facet cell.** The result says nothing yet about facets over an explicit, filtered candidate set. That is #64's job. |
| **B** | Full-catalog candidate materialization was per-element `RoaringBitmap` insertion: 4.48 ms of hot-loop CPU at 516k, against 16.6 µs for the identical bitmap built by `insert_range` (270x). Color is hardest because every V-proportional term grows with V = 2825 (output materialization, counter working set, and the per-value dictionary loop on the bitmap path). The \|C\|-proportional bitmap-iteration term, shared by all full-catalog facets, is the largest single counting cost. |
| **C** | Enum bitmap filters and conjunctions: **KEEP** (same-window r 0.07–0.14). Numeric range construction and the 5-way conjunction it dominates: **REFINE**, because a broad range costs 3.9 ms (value-ordered insertion). Lexical: **DELEGATE**. Details are in §4. |
| **Consequence for #64** (preregistered) | No FH cell is in FACET DISADVANTAGE and FH4 ≤ 1.25, for both N and N⁺. **#64 continues after its own amendment**: corrected equal-work semantics, the #79 + #63 (N⁺) structures, no legacy E3 facet path. |

"Fastest competitor" means the fastest equivalent-work competitor. It is Meilisearch on every cell. r = native median CPU/query ÷ fastest competitor median CPU/query. The classes use #60's bar: ≤0.75 material advantage, ≤1.25 parity, otherwise disadvantage. "Robust" means all three per-run paired ratios fall in the same class.

Raw evidence is under `artifacts/issue63/results/`. The log is `docs/experiments/ISSUE63_LOG.md`.

## 1. What was frozen

- **Code and host.** `main` at `31a82e9`, after the #57 salvage (#81).
  - Every preregistered #63 measurement used binaries built from `b66af57`; sha256 prefixes are in the log.
  - Later commits add only post-hoc tooling. `i63_cold_probe` is new. `i77_measure` gains the opt-in `I63_SOLR_FACET_SORT` flag for the §7 sensitivity, which rebuilt that binary. The native server and driver binaries stay byte-identical.
  - The #63 native binary includes two FINAL-path refactors made after the #79 revalidation: `indexable_intersection` and the generic `presorted_walk`. They are semantically identical, and the gate and unit tests cover them. It is FINAL's *outputs*, not its code, that are byte-identical to #79's. The host is `athos-dev`, a KVM guest with a Xeon D-1518, 4 vCPUs and 30 GB, running #79's cgroup-v2 scope envelope (3 CPUs, 6 GiB, no swap; driver on CPU 3).
- **Native FINAL.** #79 `hybrid:hybrid`, with τ_F = 919.497… and ρ_S = 0.0834 frozen and **not recalibrated**.
- **#79 revalidation first.** On `31a82e9`: the gate passed with 1,680 checks and 0 failures, and outputs and paths were identical. FH3/FH4 CPU was flagged at 1.62x / 1.49x #79's headline FINAL and kept flagged. The investigation found no code change and the same level in #79's own fidelity window, while an N0 control was not inflated. The spread is per-launch, and it is the reason Part A reports CV, min and max per arm. See `ISSUE79_LOG.md`.
- **N⁺.** It was selected from native-only microbenchmarks by the preregistered §3.4 rule and posted to #63 before Part A (commit `79bf0e2`).
  - **P0r** was adopted. It passed the correctness criterion (3,312 gate checks, 0 failures), the ≥25% reduction on an FH cell (95.7%), and the no-regression criterion (worst cell 1.044).
  - P1, P2 and P2b failed criterion 3 (worst 1.10004 / 1.185 / 1.203), on cells whose code path is unchanged. Two things limit the criterion on this host.
    - **Noise.** Run-to-run spread for identical in-process work is about ±20%.
    - **A fixed-order position effect** (review finding 5). The microbenchmark measured modes in the fixed order p0 → p0r → p1 → p2 → p2b within each cell. On explicit-candidate cells, P2b executes P2's code exactly, yet it read higher in all three runs on fc6: 71.6 / 85.8 / 71.1 against 53.9 / 65.4 / 55.5 µs.

    The rule was applied as written anyway. P2/P2b's larger reductions are characterization only (§3). A re-test must rotate mode order, not only add runs.

## 2. Part A — same-window, equal-work facet confirmation (500k, N = 515,928)

### 2.1 Protocol, as run

- **Order.** A 3×3 Latin square in one window, 07:51–08:54 UTC on 2026-09-29: run1 N,N⁺ → M → S; run2 S → N⁺,N → M; run3 M → S → N,N⁺.
- **State.** Every arm was a fresh launch: native reloaded its index, and Meilisearch/Solr re-provisioned a fresh index through the frozen #77/#79 scripts.
- **Engines.**
  - Meilisearch 1.11.3: `maxValuesPerFacet=3000`, `attributesToRetrieve:["id"]`.
  - Solr 9.10.1: JSON Facet `limit:-1`, `fields:id`.
  - Typesense: excluded as dominated (≥5.5x Solr on every FH cell in #79's same-host runs).
  - ES/OpenSearch: not re-measured (#79 §2). Vespa: environmentally excluded.
- **Equal work.** In every run, one response per cell per engine was compared exactly with the independent oracle (`num_found` and every facet's complete `{value: count}` map). Hits had to be ID-only.
  - **Deviation (review finding 7):** the preregistration says the check runs before any timed request. The dumps were captured before timing (competitors) or from the last timed response (native), but the comparison itself ran after each run's three arms. Nothing depends on this, because every cell was EQUIVALENT.
  - **Coverage gap (review finding 6):** the check covers `num_found`, the facet maps and the first hit's keys. It does not cover the hit count or hit IDs. Every dumped request body has `limit`/`rows` 48, and the competitor dumps come from the same bodies that were timed.
  - **All 3 runs × 4 arms × 8 cells were EQUIVALENT.** No cell was excluded.
  - A negative control, Meilisearch's default configuration at 100k, was correctly flagged `NOT_EQUIVALENT_WORK`: it returned 100 of 2,825 color values and full documents.
- **Metric.** CPU/query is the engine cgroup's `usage_usec` over ≥200 measured requests after 20 warmups, plus a ≥2 s window for native. Everything is single-connection, as in #77/#79.

### 2.2 Headline table

| cell | native FINAL (runs) | N⁺ (runs) | Meilisearch (runs) | Solr (runs) | fastest | FINAL r (per run) | class | N⁺ r (per run) | class |
|---|---|---|---|---|---|---|---|---|---|
| FH1 style (V=65) | **8,530** (7,057 / 9,873 / 8,530) | **1,077** (1,077 / 1,192 / 1,059) | 10,798 (11,783 / 10,798 / 10,334) | 48,978 | Meili | **0.790** (0.599 / 0.914 / 0.825) | PARITY (median-only) | **0.100** (0.091 / 0.110 / 0.102) | MATERIAL, robust |
| FH2 primarymaterial (V=244) | **12,470** | **2,019** | 14,038 | 50,948 | Meili | **0.888** (0.888 / 1.007 / 0.884) | PARITY, robust | **0.144** | MATERIAL, robust |
| FH3 color (V=2825) | **37,219** (35,947 / 37,219 / 37,942) | **22,550** (19,734 / 22,550 / 23,788) | 53,050 (57,944 / 53,050 / 52,085) | 93,560 | Meili | **0.702** (0.620 / 0.702 / 0.728) | MATERIAL, robust | **0.425** (0.341 / 0.425 / 0.457) | MATERIAL, robust |
| FH4 disjunctive, 5 facets | **37,394** (32,836 / 42,609 / 37,394) | **25,223** | 72,899 (2 backend requests) | 93,353 | Meili | **0.513** (0.450 / 0.572 / 0.516) | MATERIAL, robust | **0.346** | MATERIAL, robust |

Values are CPU/query in µs, medians of 3 counterbalanced runs. The per-run values of every arm, P50/P95/P99, CV, min and max are in `artifacts/issue63/results/confirm_report.md`.

**Latency (P50 / P95 ms, medians over runs):**

| cell | native FINAL | N⁺ | Meilisearch | Solr |
|---|---|---|---|---|
| FH3 | 36.8 / 50.1 | 23.8 / 35.2 | 67.0 / 81.7 | 104.0 / 137.0 |
| FH4 | 39.4 / 49.9 | 27.3 / 34.0 | 86.3 / 102.6 | 115.0 / 137.5 |
| FH1 | 5.7 / 18.4 | 1.3 / 1.9 | 11.0 / 14.6 | 42.3 / 53.6 |

On FH1, FINAL's P95 is wider than Meilisearch's (18.4 vs 14.6 ms), while its P99 is not (20.6 vs 22.2). On every FH cell N⁺'s P50/P95 are the lowest of all arms. On SH1 they are not: N⁺ is at 10.3 / 14.2 ms against Meilisearch's 7.0 / 9.6 and Solr's 4.8 / 6.6.

**Secondary cells** (same launches; not part of the facet verdict; they feed §4):

| cell | FINAL r | N⁺ r | fastest |
|---|---|---|---|
| SH1 numeric range + sort | 1.151 PARITY (median-only: 1.025 / 1.386 / 1.015) | 1.276 DISADVANTAGE (1.276 / 1.360 / 1.373) | Meili |
| filter_depth_1 (color=white) | 0.069 | 0.070 | Meili |
| filter_depth_3 (3 enum filters) | 0.098 | 0.094 | Meili |
| filter_depth_5 (4 enum filters) | 0.144 | 0.139 | Meili |

**SH1, N⁺ vs FINAL.** SH1's range constraint is never match-all, so P0r executes FINAL's exact code path there. In run 2, where N⁺ was measured first, it read 7,331 against FINAL's 7,473. In runs 1 and 3, measured second, it read 8,169/9,969 against 6,564/7,371. The N⁺-vs-FINAL SH1 gap is therefore within-launch order and noise, not P0r. It is reported as measured and not re-judged. Sort's position relative to Meilisearch (1.0–1.4 per run) is **not** a native advantage. That agrees with #79's like-for-like sensitivity (1.31).

### 2.3 Reading

- **For full-catalog-shaped facet requests, #79's unresolved question is answered.** Measured in one counterbalanced window with complete facet counts on every engine, native **FINAL is ≥25% cheaper than Meilisearch, the fastest equal-work competitor, on the two hard cells.** FH3, full-catalog color, is 0.70. FH4 is 0.51; it is filtered and disjunctive, but its cost is its full-catalog self-excluded color facet. Low- and medium-cardinality full-catalog facets are at parity.
  - The margin over **Solr** depends on Solr's bucket-return configuration (§7). Solr was fastest on FH3/FH4 in #79 when it returned only the top 200 buckets.
  - N⁺'s margin holds under every scenario tried.
- **#79's preregistered PARTIAL verdict (FH3 2.86, FH4 1.28) came from unequal work, not from drift.** Drift worked in native's *favour* in #79: its headline FINAL FH3 was 22.9 ms, in a favourable window, against 37.2 ms here. Equal work reverses the picture.
  - Meilisearch's FH3 cost was 8.0 ms in #79, capped at 100 values with full documents, and is 53 ms here at equal work.
  - Solr's FH3 cost was 43.7 ms in #79, returning the top 200 buckets, and is 93.6 ms here returning all buckets.
  - Those are cross-window comparisons, so they are indicative only.
- **N⁺ makes every FH cell a material advantage (0.10–0.43).** P0r removes the per-element match-all construction, which in the server costs 12–13 ms per match-all.
- **Scope.** FH1–FH3 and FH4's dominant color facet have no indexable constraint on the faceted domain (clarification C1). Both N⁺'s gains and FINAL's FH3/FH4 margin are therefore **full-catalog results**. Category-scoped PLPs have an explicit candidate set, and Part A did not measure them. #64 must add category-scoped facet cells before any claim about realistic PLP facet economics.
- **Host.** It is a KVM guest. The fixed-work probe ranged from 629 to 846 ms across arms (about ±15%), and per-launch spread was CV ≤ 0.14. The Latin square spreads that drift over the arms, but classes near a boundary stay fragile. FH1 FINAL is median-only, and one of its runs is below 0.75.

## 3. Part B — the two residual physical costs

### 3.1 B1: match-all candidate representation (500k, in-process hot loop, median of 3 runs)

| rep | construction CPU/op | allocations/op | bytes allocated/op | bytes written (est.) | persistent memory | 100k → 500k |
|---|---|---|---|---|---|---|
| P0 `(0..N).collect()` (#79 FINAL) | **4,482 µs** | 106 | 328,000 | 131,072 | 0 | 1,154 → 4,482 µs (linear) |
| P0r `insert_range(0..N)` (**N⁺**) | **16.6 µs** | 10 | 65,920 | 65,536 | 0 | 3.7 → 16.6 µs |
| P1 prebuilt, borrowed | ~0 | 0 | 0 | 0 | 65,608 B | — |
| P2 `MatchAll` sentinel | ~0 | 0 | 0 | 0 | 0 own, plus P1's bitmap kept for the top-K fallback | — |

- **Where P0's cost comes from.** `FromIterator` inserts 516k ordinals one by one, about 8.7 ns each. Every 65,536-ordinal chunk grows an array container and then converts it to an 8 KiB bitmap container, which gives the 106 allocations. P0r writes the same eight bitmap containers directly.
- **Downstream pipeline** (FINAL facet/sort algorithms, in-process µs):

| cell | P0 = FINAL | P0r | P1 | P2 | P2b |
|---|---|---|---|---|---|
| FH1 | 4,883 | 208 | 233 | 30 | 37 |
| FH2 | 5,408 | 535 | 493 | 157 | 177 |
| FH3 | 13,360 | 9,170 | 9,151 | 4,800 | 3,161 |
| FH4 | 14,824 | 10,487 | 10,806 | 5,975 | 5,673 |

- **Share of FINAL.** P0 construction is 88% of FH1's in-process cost, 83% of FH2's, 32% of FH3's and about 29% of FH4's (its second, self-excluded match-all).
- **P2/P2b** would roughly halve FH3/FH4 again, because their dense paths skip bitmap iteration entirely. They were not adopted (§1). As a result, the measured N⁺ keeps FINAL's per-candidate counting over a materialized full bitmap.

### 3.2 B2: why color (V = 2825) is the hardest cell

Full catalog, \|C\| = 515,928, ns per candidate unless noted:

| attr | V | ordinal total | d1 bitmap iteration | d2 +column gather | d3 +counting | d3 with u32 counters | materialization (ns/value) | dense column scan | bitmap path (ns/value) |
|---|---|---|---|---|---|---|---|---|---|
| style | 65 | 11.5 | 8.1 | 9.3 | 12.2 | 11.7 | 180 | 3.7 | 2,089 |
| material | 162 | 10.1 | 8.1 | 8.8 | 10.9 | 10.5 | 234 | 2.8 | 1,870 |
| primarymaterial | 244 | 11.2 | 8.1 | 8.4 | 11.5 | 11.5 | 277 | 3.4 | 1,758 |
| color | 2825 | **15.4** | 8.1 | 8.4 | 12.6 | 12.6 | 478 | 7.4 | 2,313 |

Each candidate cause the preregistration named, as measured:

- **Candidate representation (bitmap iteration)** is the largest term, and it is shared by every full-catalog facet: 8.1 ns/candidate, 53% of color's cost and 70–80% of the others'. Only a dense match-all path (P2) removes it: color drops to 7.4 ns/candidate, style to 3.7.
- **Output materialization** is V-proportional: 478 ns/value, 1.35 ms for color against about 12 µs for style. It is the largest term separating color from the other attributes.
- **Memory locality (counter working set):** counting costs 4.2 ns/candidate for color against 2.1–3.1 for the others. Color's counter array is 22.6 KB, about 70% of the 32 KB L1D; style's is 0.5 KB. u32 counters change nothing (12.57 vs 12.61).
- **Dictionary/value-loop overhead versus bitmap intersection** on the bitmap path, which costs 1.8–2.3 µs per value in total (6.5 ms at V = 2825):
  - The per-value dictionary loop is a string-keyed `HashMap` lookup plus materialization. The dense bitmap-`len()` path runs the same loop without intersecting, at 0.33–0.92 µs per value.
  - The remaining ~1.4 µs per value is `intersection_len` against the full candidate bitmap's containers.
  - At V = 2825 both terms matter, which is why the planner sends color to the ordinal path.
- **Column gather:** 0.3–1.1 ns/candidate. It is sequential, so not a bottleneck.
- **Cache misses were not measured.** Hardware counters are unavailable (`perf_event_paranoid=4`), so the locality statements rest on working-set sizes and the u32/u64 contrast.

### 3.3 Residual attribution of FINAL's service CPU

The source is the median-CPU Part A run, using the native server's own phase timers from the same process and window. B2's decomposition only splits the facet-counting share, as a proportional model.

| component | FH3 (service 37,219 µs) | FH4 (service 37,394 µs) |
|---|---|---|
| match-all construction (P0) | 13,388 µs · **36.0%** (measured phase) | 12,130 µs · **32.4%** (the self-excluded color facet's second match-all; **inferred**, see below) |
| base candidates | — | 33 µs · 0.1% |
| facet counting: bitmap iteration | 11,644 · 31.3% | 12,341 · 33.0% |
| facet counting: column gather | 444 · 1.2% | 470 · 1.3% |
| facet counting: counter increments | 5,999 · 16.1% | 6,358 · 17.0% |
| facet counting: output materialization | 3,986 · 10.7% | 4,225 · 11.3% |
| output assembly (48 ids) | 172 · 0.5% | 154 · 0.4% |
| HTTP parse, JSON serialization, kernel, other | 1,583 · 4.3% | 1,682 · 4.5% |

FH4's facet-counting rows include its four small facets over the 16,332-candidate color=black set. They cost about 0.8 ms hot-loop, so they fall inside the split.

FH4's construction figure is **inferred, not measured** (review finding 8). It is FINAL's `facets_us` minus N⁺'s, taken from separate batches of the same launch. For comparison, on FH3 the same facet code differs by 1.3–1.9 ms between those two batches. Read it as ±2 ms.

**Disclosed gap: identical code runs 2.3–6.4x slower inside the server than in the hot loop.** For example, FH3's match-all construction is 13.4 ms in the server against 4.5 ms in the hot loop, and color counting 22.1 against 7.9 ms.
- A post-hoc diagnostic (`diagnostic_cold_probe/`, not preregistered) evicted 64 MiB between operations. It left the large operations unchanged: cold/hot 1.05 for FH3 FINAL, 0.98 for N⁺ and 1.13 for P0 construction. **Cache-cold execution does not explain the gap for them.** Only tiny operations are cache-sensitive (FD1 8.5x).
- **The mechanism is not identified.** Candidates are per-request vCPU migration across the 3-CPU cpuset, KVM steal time, and allocator or page-fault state in the long-lived server.
- Every engine is measured by the same service-CPU rule, so the Part A classification is unaffected. The hot-loop microbenchmarks are best-case costs: their *relative* decompositions transfer, their absolute µs do not.

## 4. Part C — structural primitives (500k; in-process hot loop, median of 3 runs)

| primitive | CPU/op | allocations/op | bytes touched (est.) | result | 100k→500k | same-window service r | class |
|---|---|---|---|---|---|---|---|
| exact lookup (variant id → ordinal + record) | 229 ns | 0 | ~256 B | 1 | 1.16x | — | **KEEP (provisional)** |
| single bitmap filter, color=white (clone) | 2.9 µs | 11 | 69 KB | 17,256 | 5.4x | 0.069 (filter_depth_1) | **KEEP** |
| same, borrowed (no clone) | 8 ns | 0 | 0 | 17,256 | — | — | KEEP (provisional) |
| single bitmap filter, category Accent Chairs | 2.4 µs | 9 | 53 KB | 13,236 | 6.6x | — | KEEP (provisional) |
| 3-way enum conjunction | 42 µs | 33 | 166 KB | 36 | 4.3x | 0.098 (filter_depth_3) | **KEEP** |
| 5-way conjunction (4 enums + `average_rating ≥ 4`) | 4,330 µs | 150 | 6.3 MB | 12 | 4.4x | none matching. filter_depth_5 is the 4 enums only (r 0.144 → the enum part is KEEP). | **REFINE** (the range term is about 99% of the cost) |
| numeric range, broad (`average_rating ≥ 4`) | 3,876 µs | 106 | 6.2 MB | 383,604 | 4.1x | 1.151 (SH1: range + sort) | **REFINE** |
| numeric range, narrow (`review_count ≥` p99) | 276 µs | 69 | 73 KB | 4,032 | 4.7x | — | **REFINE** |
| same-variant conjunction (synthetic multi-variant, 516k variants, 100k-derived) | 56 µs | 30 | 163 KB | 16,122, **0 cross-variant false matches** (16,123 trap products) | — | — | **REFINE** by the mechanical rule; see below |
| lexical residual (2 and 3 tokens) | 50–111 µs | 26–34 | 131–196 KB | 1,116–12,432 | 4.5–7.9x | — | **DELEGATE** (reference; #57) |

**Class rules (§4).** With a same-window service cell: KEEP if r ≤ 0.75, REFINE if ≤ 1.25, DELEGATE otherwise. Without one: KEEP-provisional if the in-process cost is ≤ 10% of native's own service floor, and REFINE otherwise. The floor is 396 µs (filter_depth_1).

**Readings.**
- **Structural filters and enum conjunctions** cost 7–14% of the fastest mature engine's CPU for the same logical request at the service boundary. That is the architecture's clearest measured advantage. It survives equal work and same-window measurement.
- **Numeric range construction is REFINE.**
  - `numeric_range` collects ordinals in value order into a roaring bitmap, which is random insertion.
  - It costs 3.9 ms for 383,604 matches (about 10 ns/element, 106 allocations) and 276 µs for 4,032 (array-container shifting).
  - It dominates the 5-way conjunction (4.3 ms against 42 µs for 3 enums) and SH1's candidate phase, where sort is only at parity.
  - The evident fix is to sort ordinals before `from_sorted_iter`, or to use range-bucket bitmaps. It was **not** implemented or measured here, so it is recorded as the next REFINE candidate, not as a result.
- **Same-variant conjunction** is correct by construction: 0 false matches against 16,123 trap products. It is classed REFINE only because the preregistered rule has no service cell for it and 56 µs exceeds 10% of the 396 µs floor. It is not slow in any absolute sense. #57 already showed engines with per-variant documents can match it.
- **Lexical residual** remains DELEGATE, per #57/#61.

## 5. Physical-design implication

| primitive | class | action |
|---|---|---|
| match-all candidate materialization | **REFINE → fixed** | P0r (`insert_range`): 270x cheaper, 0 extra memory, correctness-identical. Available as `cand_mode=p0r` (N⁺) and `CatalogIndex::all_ordinals_bitmap_by_range`. #64 uses it. Making it `indexed_candidates`' default is recommended. It is left for #64's amendment so that #79/#63 FINAL stays byte-reproducible. |
| logical MatchAll sentinel / dense full-catalog paths (P2/P2b) | **REFINE (not adopted)** | Correct (3,312 checks), and a further ~2x on FH3/FH4 in-process. Rejected by the no-regression criterion, which on this host is limited by noise and by a fixed mode-order position effect. A re-test needs rotated mode order and enough runs to resolve 10%. |
| high-cardinality facet counting | **KEEP**: material advantage at equal work, full-catalog shape only, with the Solr-configuration caveat in §7 | Named residuals: bitmap iteration (the largest term), V-proportional materialization, and counter working set. Category-scoped cells are untested (#64). |
| low/medium-cardinality facets | **KEEP** | FINAL at parity; N⁺ at a material advantage. |
| enum bitmap filters / conjunctions | **KEEP** | Same-window r 0.07–0.14. |
| conjunction that includes a broad numeric range | **REFINE** | The range term dominates (4.3 ms against 42 µs for 3 enums). |
| exact lookup | **KEEP (provisional)** | 229 ns; no same-window competitor cell. |
| numeric range construction | **REFINE** | Value-order insertion; the fix is named but not measured. |
| numeric sort | **PARITY, not an advantage** | SH1 1.0–1.4 against Meilisearch. |
| same-variant conjunction | **KEEP semantics / REFINE class by rule** | Correct by construction. |
| lexical residual | **DELEGATE** | Per #57. |

No single score is computed.

## 6. Limitations

- **One host, one dataset.** A single KVM host, WANDS replicated 12x, single-connection serving. Throughput and concurrency are out of scope (the E3 confound).
- **Excluded competitors.** Typesense, ES/OpenSearch and Vespa were not re-measured. The "fastest competitor" assumption for them rests on #77/#79 evidence.
- **Equal work versus identical output shape.** Equal work means identical facet *content*. Solr returns buckets sorted by count, Meilisearch and native by value, and native's JSON also carries a small `diag` object. Serialization differences are inside each engine's CPU.
- **Microbenchmark conditions.** They are hot-loop and single-CPU (§3.3 gap), and allocations are counted with a counting allocator, which adds a few ns per allocation to every primitive.
- **Early exposure (disclosed in the log).** Smoke-level 100k numbers were seen before N⁺ was frozen and before Part A. No rule depends on them.

## 7. Adversarial review and the post-review sensitivity

A fresh, read-only agent tried to falsify the favourable conclusions. It recomputed everything from raw data and found **no arithmetic error**. Every median, r, per-run ratio, class, robust flag and §3.4 adoption figure reproduces. It also **could not falsify** the following:

- that every equal-work check is exact, every run, for every engine;
- that the competitor dumps come from the timed bodies;
- that Meilisearch is fastest in every run and cell;
- the Latin square as executed, with fresh state for every arm;
- N⁺'s robust material advantage on every FH cell;
- the correctness of P0r;
- the #64 gate outcome.

Its findings and their dispositions:

| # | finding | disposition |
|---|---|---|
| 2 (high for FINAL only) | Solr's configuration competence was not checked. Solr's FH3/FH4 cost doubled from `limit:200` (#79) to `limit:-1`, and it sorts buckets by count. | **Sensitivity run** (below). Bucket order is not the cost driver, and Solr stays well behind Meilisearch at equal work. The FH3/FH4 verdict is against Meilisearch in any case. |
| 3 | Scope overstated: every cell is full-catalog-shaped, FH4 included, and no category-scoped facet cell exists. | Accepted. The scope row in the answer, §2.3, the architecture note and the index row are rewritten, and #64 must add category-scoped cells. |
| 4 | #79's PARTIAL was not caused by drift. Drift favoured native in #79. | Accepted. §2.3 is corrected. |
| 5 | Criterion 3's failures include a fixed mode-order position effect, not only noise. | Accepted. §1, §5 and ADR 0015 are corrected. A re-test must rotate order. The adoption outcome is unchanged. |
| 6 / 7 | The equivalence check misses the hit count and IDs, and it ran after the arms rather than before timing. | Disclosed in §2.1. No consequence, because every body had `limit`/`rows` 48 and every cell was EQUIVALENT. |
| 8 | FH4's construction share is inferred. | Labelled ±2 ms in §3.3 and the ADR. |
| 9 | Two latency statements were wrong: SH1 tails, and FH1 at P99. | Corrected in §2.2. |
| 10 | Provenance statement stale, and "byte-reproducible" true of outputs, not code. | Corrected in §1. |
| 11 | "No pressure stalls" was wrong. | Corrected in the log with PSI totals. The Solr arms' CPU PSI is most likely from indexing. Memory PSI did not rise during the Meilisearch arms. |
| 12 | The bitmap path's per-value cost mixes dictionary lookup with intersection. | Split in §3.2 (0.33–0.92 µs lookup and materialization, about 1.4 µs intersection). |
| 13 | The 5-way conjunction's KEEP rested on a service cell (FD5) without its dominant range term. | Reclassified **REFINE** in §4 and in `analyze_i63.py`. The enum part keeps FD5's KEEP. |
| 14 / 15 | Fairness: no bias against competitors found; the within-launch order effect penalises FINAL. Correctness: sound; the MultiEnum and NaN branches are unit-tested only, since WANDS has neither. | Recorded. |

### 7.1 Solr facet-sort sensitivity

This is post-review and **not preregistered**. The preregistered verdict does not change.

- **Solr configuration:** equal-work Solr (`limit:-1`) with `sort:"index asc"`, i.e. buckets in term order instead of count order, which avoids a count sort.
- **Interleaving:** 3 pairs with native in one window, 09:29–09:49 UTC, in alternating order (S→N, N→S, S→N). FH1–FH4 only.
- **Equivalence:** EQUIVALENT in all 3 runs.
- **Raw data:** `artifacts/issue63/results/solr_sensitivity/`.

| cell | Solr count-sorted (Part A) | **Solr index-sorted** (runs) | Meilisearch (Part A) | native FINAL / N⁺ in this window |
|---|---|---|---|---|
| FH1 | 48,978 | **61,539** (64,459 / 60,772 / 61,539) | 10,798 | 7,736 / 1,280 |
| FH2 | 50,948 | **55,245** | 14,038 | 12,375 / 1,970 |
| FH3 | 93,560 | **84,031** (87,110 / 80,488 / 84,031) | 53,050 | 36,230 / 24,408 |
| FH4 | 93,353 | **93,781** | 72,899 | 39,225 / 27,401 |

- **Bucket order is not the driver.** Index order is 10% cheaper on FH3, the same on FH4, and dearer on FH1/FH2. Returning every bucket is what roughly doubles Solr's cost relative to its #79 top-200 configuration, and that top-200 configuration was not equal work.
- **What it would take for Solr to change the verdict.** Solr would have to beat Meilisearch at equal work, i.e. drop below 53 ms on FH3, about 40% below both configurations tried. Only then could it become the reference competitor. Other Solr facet methods (`method: dv | uif | enum`) and faceting modules were **not** tried, and they remain the open competence question.
- **Native was stable across windows.** In this window FINAL is 0.68 of Meilisearch's Part A median on FH3 and 0.54 on FH4. That comparison crosses windows, so it is indicative only. It sits beside Part A's same-window 0.70 / 0.51.
