# Issue #64 — Infra E4 (amended): facet-heavy commerce workload economics

**Question** (#64 amendment 1; original body preserved on the issue). For facet-heavy PLP requests over **category-scoped** candidate sets, at equal logical work, how does native's CPU/query compare with the fastest mature engine? How does the comparison change with the number of facet groups, facet cardinality, candidate-set size, self-exclusion and multi-select? Where are the breakpoints?

**Answer (preregistered verdict): KEEP.** All 32 cells of the realistic, category-scoped grid are **MATERIAL FACET ADVANTAGE**, and all 32 are robust. r ranges from **0.068 to 0.458** of the fastest equal-work competitor's CPU/query. Every engine's complete facet counts, hit counts and `num_found` were checked exactly against an independent oracle after every arm of every run: 9 of 9 arms EQUIVALENT on all 44 cells.

| what | result |
|---|---|
| native arm | N⁺ = #79 hybrid facet/sort planner (τ_F, ρ_S frozen) + #63 P0r. On category-scoped sets it runs exactly #79 FINAL's path. |
| competitors | Meilisearch 1.11.3 (`maxValuesPerFacet=3000`, ID-only) and Solr 9.10.1 (`limit:-1`), both equal-work. In the realistic grid R, Solr is fastest on 21 cells and Meilisearch on 11. Across all 38 non-baseline cells, Solr is fastest on 21 and Meilisearch on 17. |
| scopes | S0 full catalog (reference, not in R); S1 Furniture 192k; S2 Décor & Pillows 55k; S3 Flooring, Walls & Ceiling 14k; S4 Bookcases 3k; S5 4'×6' Area Rugs 600 |
| facet groups | k ∈ {1, 3, 5, 8, K_max ≤ 11}, plus a color-only cell per scope and, on S2/S3, single-select and multi-select disjunctive cells |
| total-CPU class | MATERIAL everywhere, including S0 (0.10–0.40) |
| **facet-only CPU** (cell − same-scope k=0) | native is 0.12–0.53 of the cheapest competitor's facet-only CPU on S0–S4. At S5 the competitors' facet-only CPU is within noise, so there the advantage is the per-request floor |
| **per-request floor** (k=0) | native 370–438 µs; Meilisearch 5,977–6,587 µs; Solr 5,508–20,134 µs |
| **incremental CPU per added facet** | native below both competitors on S0–S3 and S5. **At S4 (3k docs), Solr's marginal cost per facet (138 µs) is below native's (188 µs)**, which is the one breakpoint |
| economics | cores per 1,000 QPS (= CPU ms/query) at S3, k=5: native **4.3**, fastest competitor 18.6. At S1, k=10: 31.7 against 69.3 |
| memory (not gated) | RSS native 4.41 GB, Meilisearch 5.55 GB, Solr 3.85 GB (3 GiB JVM heap). **E2's memory REFINE (#62) stands.** Native uses more memory than Solr on this catalog. |

## 1. What was frozen

- **Code.** `main` at `160ee09`, then branch `i64/facet-economics`. The binaries were built at `f20e4de`, with the gate rebuilt at `a7c60be`; sha256 prefixes are in the log.
- **Host.** #79's `athos-dev` cgroup-v2 envelope (KVM, 3 CPUs, 6 GiB, swap 0; driver on CPU 3), as in #63.
- **Workload.** The scopes, facet order, ladder and disjunctive/multi-select values were fixed in the amendment, from catalog composition only. The cell list is pinned by a unit test.
- **Multi-select** (OR within an attribute) is implemented in the experiment executor and the oracle only. The native IR has **no `EnumAny` constraint**, so production serving cannot express multi-select today. That capability gap is a finding. This experiment makes no product change.
- **Correctness first.** The gate ran 9,984 candidate checks, 6,864 of them on #64 cells, with 0 failures. Its first run surfaced a bug in the gate's own retrieval recomputation (§6), not in the executor.

## 2. Headline grid (500k; CPU/query µs, median of 3 counterbalanced runs)

The class is r against the fastest equal-work competitor under #60's bar. Every class shown is robust (3/3 per-run ratios in the same class). The complete per-cell table, with per-run values, latency and CV, is in `artifacts/issue64/results/confirm_report.md`.

| scope (size) | k=1 | k=3 | k=5 | k=8 | K_max | color |
|---|---|---|---|---|---|---|
| S0 full (515,928), reference | 0.10 | 0.38 | 0.34 | 0.29 | 0.40 (k=11) | 0.39 |
| S1 Furniture (192,468) | 0.11 | 0.33 | 0.29 | 0.44 | 0.46 (k=10) | 0.27 |
| S2 Décor & Pillows (55,344) | 0.26 | 0.26 | 0.34 | 0.32 | 0.37 (k=10) | 0.19 |
| S3 Flooring, Walls & Ceiling (14,472) | 0.10 | 0.14 | 0.23 | 0.21 | 0.19 (k=9) | 0.12 |
| S4 Bookcases (3,024) | 0.08 | 0.10 | 0.16 | 0.17 (k=8 = K_max) | — | 0.08 |
| S5 4'×6' Area Rugs (600) | 0.08 | 0.07 | 0.12 | 0.11 (k=8 = K_max) | — | 0.07 |

**Disjunctive, k=5:**

| cell | S2 | S3 |
|---|---|---|
| single-select style, self-excluded | 0.28 | 0.11 |
| multi-select style (2 values), self-excluded | 0.25 | 0.11 |

**Absolute CPU/query, for scale:**

| cell | native | Meilisearch | Solr |
|---|---|---|---|
| S1 k=10 | 31.7 ms | 69.3 ms | 129.5 ms |
| S3 k=5 | 4.3 ms | 34.5 ms | 18.6 ms |
| S5 k=5 | 0.70 ms | 20.0 ms | 5.8 ms |

## 3. What the advantage is made of

- **The per-request floor is part of it, and at small scopes it is all of it.** With no facet (k=0), native costs 0.4 ms and the competitors 5.5–20 ms. That floor is the full request at the shared HTTP boundary (#61 R2.1's contract): request parsing, retrieval of 48 IDs plus `num_found`, and serialization. At S5 (600 docs) every engine's facet work is small, so the class comes from the floor. Solr's facet-only CPU there is indistinguishable from zero (−1.4 to +0.7 ms).
- **Facet work itself is cheaper natively on S0–S4.** Facet-only CPU is 0.12–0.53 of the cheapest competitor's. For example, at S2 k=10 it is 22.2 ms against 55.7 ms (Meilisearch), and at S1 k=10, 31.3 ms against 63.2 ms (Meilisearch).
- **Marginal cost per added facet.**

| scope | native µs/facet | Meilisearch µs/facet | Solr µs/facet |
|---|---|---|---|
| S0 | 5,137 | 13,396 | 22,014 |
| S1 | 3,371 | 6,263 | 11,649 |
| S2 | 2,075 | 5,079 | 6,346 |
| S3 | 732 | 4,048 | 2,624 |
| S4 | 188 | 1,766 | **138** |
| S5 | 59 | 2,395 | 188 |

  At S4 Solr's *marginal* facet cost is below native's, but native still wins S4's total CPU through the floor (0.38 against 5.5 ms). This is the **breakpoint for planner use**: for small explicit candidate sets, the native facet loop's advantage over Solr's per-facet counting disappears. Native keeps the request.
- **Meilisearch's disjunctive cost.** It issues one extra request per self-excluded facet (2 backend requests on the disjunctive cells), which is inherent to its API. Its multi-select cost is included.
- **Cardinality.** Color-only cells (V = 2825) are 0.07–0.39. The high-cardinality facet is not a weak region once the candidate set is explicit. #63's full-catalog color cost came from match-all.

## 4. Consequence and implications

- **Facet-heavy PLP economics: KEEP.** On WANDS-shaped catalogs, for category-scoped facet requests with 1–10 facet groups, single- or multi-select self-exclusion, and candidate sets from 600 to 192k, native needs **2.2x to 15x fewer CPU-cores per unit of query throughput** than the fastest mature engine. The work is equal and checked exactly, and it was measured in one counterbalanced window.
- **Not claimed.** A memory or total-cost advantage: RSS is higher than Solr's, and E2's REFINE stands. Throughput under concurrency: the single-connection confound is out of scope. Any dataset other than WANDS.
- **Product gap.** Multi-select needs an IR constraint (`EnumAny` or equivalent) before production serving can offer it. That is an ADR-level change for a future issue, and was not made here.
- **Next in #60's queue:** #65, realistic mixed workload and max sustainable QPS/core. #64 does not run it.

## 5. Limitations

- **One host and one dataset**: a KVM guest (±15% host-probe drift between arms) and WANDS replicated 12x. The Latin square spreads drift across arms. The classes carry a wide margin: the worst R cell is 0.458 against the 0.75 bar.
- **Single-connection serving.** Per-request floors are single-connection floors. Under concurrency, JVM and Meilisearch floors may amortize differently.
- **Scope selection** depends on catalog composition. S1 has low attribute coverage (0.19, disclosed), so its facets are sparse.
- **Competitor configuration.** The configurations are the ones #63 checked: Meilisearch's documented facet cap, and Solr `limit:-1` with count sort, where index sort did not help (#63 §7). Other Solr facet methods remain untried.
- **k=0 subtraction.** The facet-only figures subtract two medians taken from separate batches, so they are noisy at small scopes. S5's negative Solr values illustrate that.

## 6. Correctness incident, and review

- **Gate bug.** The first gate run reported 312 candidate failures, all on the multi-select cells, all with `candidates_ok = false` while `num_found` and facets were correct. The gate recomputes base retrieval itself, and that recomputation ignored `any_filters`. After the gate was fixed, it reported 0 failures. The executor was never wrong: its `num_found` and facets had matched the oracle.
- **Adversarial review:** see §7, added after the independent review.
