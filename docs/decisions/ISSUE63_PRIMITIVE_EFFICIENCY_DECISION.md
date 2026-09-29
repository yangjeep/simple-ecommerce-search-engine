# Issue #63 — Infra E3 (amended): primitive CPU-efficiency, equal-work facet confirmation

**Questions** (#63 amendment 1 and clarification C1; the original body is preserved on the issue):

- **(A)** Under equal logical facet work, measured in the same window with counterbalanced order, is native faceting faster than, at parity with, or slower than the fastest mature engine?
- **(B)** What are the two residual native costs #79 isolated, full-catalog candidate materialization and dense high-cardinality counting, made of?
- **(C)** What does each structural primitive cost?

**Answer.**

| | result |
|---|---|
| **A, native FINAL (#79's structures, frozen τ/ρ)** | FH1 **PARITY** (0.790) · FH2 **PARITY** (0.888) · FH3 **MATERIAL FACET ADVANTAGE** (0.702, robust) · FH4 **MATERIAL FACET ADVANTAGE** (0.513, robust) |
| **A, native N⁺ = FINAL + P0r (frozen before Part A by the §3.4 rule)** | FH1 **0.100** · FH2 **0.144** · FH3 **0.425** · FH4 **0.346**: **MATERIAL FACET ADVANTAGE on every cell, robust on every cell** |
| **B** | Full-catalog candidate materialization was per-element `RoaringBitmap` insertion: 4.48 ms of hot-loop CPU at 516k, against 16.6 µs for the identical bitmap built by `insert_range` (270x). Color is hardest because every V-proportional term grows with V = 2825 (output materialization, counter working set, and the per-value dictionary loop on the bitmap path). The \|C\|-proportional bitmap-iteration term, shared by all full-catalog facets, is the largest single counting cost. |
| **C** | Structural filters and conjunctions: **KEEP** (same-window r 0.07–0.14). Numeric range construction: **REFINE**, because a broad range costs 3.9 ms (value-ordered insertion). Lexical: **DELEGATE**. Details are in §4. |
| **Consequence for #64** (preregistered) | No FH cell is in FACET DISADVANTAGE and FH4 ≤ 1.25, for both N and N⁺. **#64 continues after its own amendment**: corrected equal-work semantics, the #79 + #63 (N⁺) structures, no legacy E3 facet path. |

"Fastest competitor" means the fastest equivalent-work competitor. It is Meilisearch on every cell. r = native median CPU/query ÷ fastest competitor median CPU/query. The classes use #60's bar: ≤0.75 material advantage, ≤1.25 parity, otherwise disadvantage. "Robust" means all three per-run paired ratios fall in the same class.

Raw evidence is under `artifacts/issue63/results/`. The log is `docs/experiments/ISSUE63_LOG.md`.

## 1. What was frozen

- **Code and host.** `main` at `31a82e9`, after the #57 salvage (#81). Every #63 binary was built from `b66af57`, whose Rust source is unchanged by later commits; sha256 prefixes are in the log. The host is `athos-dev`, a KVM guest with a Xeon D-1518, 4 vCPUs and 30 GB, running #79's cgroup-v2 scope envelope (3 CPUs, 6 GiB, no swap; driver on CPU 3).
- **Native FINAL.** #79 `hybrid:hybrid`, with τ_F = 919.497… and ρ_S = 0.0834 frozen and **not recalibrated**.
- **#79 revalidation first.** On `31a82e9`: the gate passed with 1,680 checks and 0 failures, and outputs and paths were identical. FH3/FH4 CPU was flagged at 1.62x / 1.49x #79's headline FINAL and kept flagged. The investigation found no code change and the same level in #79's own fidelity window, while an N0 control was not inflated. The spread is per-launch, and it is the reason Part A reports CV, min and max per arm. See `ISSUE79_LOG.md`.
- **N⁺.** It was selected from native-only microbenchmarks by the preregistered §3.4 rule and posted to #63 before Part A (commit `79bf0e2`).
  - **P0r** was adopted. It passed the correctness criterion (3,312 gate checks, 0 failures), the ≥25% reduction on an FH cell (95.7%), and the no-regression criterion (worst cell 1.044).
  - P1, P2 and P2b failed criterion 3 (worst 1.10004 / 1.185 / 1.203), on cells whose code path is unchanged. The criterion is noise-limited on this host: run-to-run spread for identical in-process work is about ±20%. The rule was applied as written anyway. P2/P2b's larger reductions are characterization only (§3).

## 2. Part A — same-window, equal-work facet confirmation (500k, N = 515,928)

### 2.1 Protocol, as run

- **Order.** A 3×3 Latin square in one window, 07:51–08:54 UTC on 2026-09-29: run1 N,N⁺ → M → S; run2 S → N⁺,N → M; run3 M → S → N,N⁺.
- **State.** Every arm was a fresh launch: native reloaded its index, and Meilisearch/Solr re-provisioned a fresh index through the frozen #77/#79 scripts.
- **Engines.**
  - Meilisearch 1.11.3: `maxValuesPerFacet=3000`, `attributesToRetrieve:["id"]`.
  - Solr 9.10.1: JSON Facet `limit:-1`, `fields:id`.
  - Typesense: excluded as dominated (≥5.5x Solr on every FH cell in #79's same-host runs).
  - ES/OpenSearch: not re-measured (#79 §2). Vespa: environmentally excluded.
- **Equal work, checked first.** In every run, one response per cell per engine was compared exactly with the independent oracle (`num_found` and every facet's complete `{value: count}` map). Hits had to be ID-only.
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

Native's P95/P99 tails are wider than Meilisearch's on FH1 FINAL. N⁺'s tails are tighter everywhere.

**Secondary cells** (same launches; not part of the facet verdict; they feed §4):

| cell | FINAL r | N⁺ r | fastest |
|---|---|---|---|
| SH1 numeric range + sort | 1.151 PARITY (median-only: 1.025 / 1.386 / 1.015) | 1.276 DISADVANTAGE (1.276 / 1.360 / 1.373) | Meili |
| filter_depth_1 (color=white) | 0.069 | 0.070 | Meili |
| filter_depth_3 (3 enum filters) | 0.098 | 0.094 | Meili |
| filter_depth_5 (4 enum filters) | 0.144 | 0.139 | Meili |

**SH1, N⁺ vs FINAL.** SH1's range constraint is never match-all, so P0r executes FINAL's exact code path there. In run 2, where N⁺ was measured first, it read 7,331 against FINAL's 7,473. In runs 1 and 3, measured second, it read 8,169/9,969 against 6,564/7,371. The N⁺-vs-FINAL SH1 gap is therefore within-launch order and noise, not P0r. It is reported as measured and not re-judged. Sort's position relative to Meilisearch (1.0–1.4 per run) is **not** a native advantage. That agrees with #79's like-for-like sensitivity (1.31).

### 2.3 Reading

- **The unresolved #79 facet question is resolved in native's favour.** Measured in one counterbalanced window with complete facet counts on every engine, native **FINAL is already ≥25% cheaper than Meilisearch on the two hard cells.** FH3, full-catalog color, is 0.70. FH4, the realistic filtered disjunctive cell, is 0.51. Low- and medium-cardinality full-catalog facets are at parity.
- **#79's preregistered PARTIAL verdict (FH3 2.86, FH4 1.28) was an artefact of unequal work plus cross-window drift, as #79 §11 suspected.** Equalizing work raised Meilisearch's FH3 cost from 8.0 ms (100 values, full documents) to 53 ms, and Solr's from 43.7 ms (top 200 returned) to 93.6 ms.
- **N⁺ makes every FH cell a material advantage (0.10–0.43).** P0r removes the per-element match-all construction, which in the server costs 12–13 ms per match-all.
- **Scope of the full-catalog win.** FH1–FH3 and FH4's self-excluded color facet are shapes with no indexable constraint on the faceted domain (clarification C1). The FH1/FH2 gains from N⁺ apply only to such requests. They do not apply to category-scoped PLPs, whose candidate set is explicit. FH4 is the realistic cell, and there FINAL is already 0.51.
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
- **Dictionary/value-loop overhead:** only on the bitmap path, 1.8–2.3 µs per value, so 6.5 ms at V = 2825.
- **Bitmap-intersection bytes:** not a large term.
- **Column gather:** 0.3–1.1 ns/candidate. It is sequential, so not a bottleneck.
- **Cache misses were not measured.** Hardware counters are unavailable (`perf_event_paranoid=4`), so the locality statements rest on working-set sizes and the u32/u64 contrast.

### 3.3 Residual attribution of FINAL's service CPU

The source is the median-CPU Part A run, using the native server's own phase timers from the same process and window. B2's decomposition only splits the facet-counting share, as a proportional model.

| component | FH3 (service 37,219 µs) | FH4 (service 37,394 µs) |
|---|---|---|
| match-all construction (P0) | 13,388 µs · **36.0%** | 12,130 µs · **32.4%** (the self-excluded color facet's second match-all) |
| base candidates | — | 33 µs · 0.1% |
| facet counting: bitmap iteration | 11,644 · 31.3% | 12,341 · 33.0% |
| facet counting: column gather | 444 · 1.2% | 470 · 1.3% |
| facet counting: counter increments | 5,999 · 16.1% | 6,358 · 17.0% |
| facet counting: output materialization | 3,986 · 10.7% | 4,225 · 11.3% |
| output assembly (48 ids) | 172 · 0.5% | 154 · 0.4% |
| HTTP parse, JSON serialization, kernel, other | 1,583 · 4.3% | 1,682 · 4.5% |

FH4's facet-counting rows include its four small facets over the 16,332-candidate color=black set. They cost about 0.8 ms hot-loop, so they fall inside the split.

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
| 5-way conjunction (4 enums + `average_rating ≥ 4`) | 4,330 µs | 150 | 6.3 MB | 12 | 4.4x | 0.144 (filter_depth_5: the 4 enums only) | **KEEP**, with the range term REFINE |
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
| logical MatchAll sentinel / dense full-catalog paths (P2/P2b) | **REFINE (not adopted)** | Correct (3,312 checks), and a further ~2x on FH3/FH4 in-process. Rejected by a noise-limited no-regression criterion. A re-test needs a protocol with enough runs to resolve 10%. |
| high-cardinality facet counting | **KEEP** (material advantage at equal work), with named residuals | Bitmap iteration (the largest term), V-proportional materialization, and counter working set. |
| low/medium-cardinality facets | **KEEP** | FINAL at parity; N⁺ at a material advantage. |
| enum bitmap filters / conjunctions | **KEEP** | Same-window r 0.07–0.14. |
| exact lookup | **KEEP (provisional)** | 229 ns; no same-window competitor cell. |
| numeric range construction | **REFINE** | Value-order insertion; the fix is named but not measured. |
| numeric sort | **PARITY, not an advantage** | SH1 1.0–1.4 against Meilisearch. |
| same-variant conjunction | **KEEP semantics / REFINE class by rule** | Correct by construction. |
| lexical residual | **DELEGATE** | Per #57. |

No single score is computed.

## 6. Adversarial review and limitations

See §7, added after the independent review.

- **One host, one dataset.** A single KVM host, WANDS replicated 12x, single-connection serving. Throughput and concurrency are out of scope (the E3 confound).
- **Excluded competitors.** Typesense, ES/OpenSearch and Vespa were not re-measured. The "fastest competitor" assumption for them rests on #77/#79 evidence.
- **Equal work versus identical output shape.** Equal work means identical facet *content*. Solr returns buckets sorted by count, Meilisearch and native by value, and native's JSON also carries a small `diag` object. Serialization differences are inside each engine's CPU.
- **Microbenchmark conditions.** They are hot-loop and single-CPU (§3.3 gap), and allocations are counted with a counting allocator, which adds a few ns per allocation to every primitive.
- **Early exposure (disclosed in the log).** Smoke-level 100k numbers were seen before N⁺ was frozen and before Part A. No rule depends on them.
