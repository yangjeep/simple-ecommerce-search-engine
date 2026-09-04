# Issue #61 Preregistered Protocol — Infra E1: fair baseline + reproducible resource benchmark harness

Committed **before** any measured run, per this repository's governance
(`docs/EXPERIMENT_LOOP.md`) and Issue #60's own merge-gated execution rule
("preregister hypothesis, baseline, protocol, metrics, and KEEP/REJECT/REFINE
gates before held-out measurement").

Parent epic: [#60](https://github.com/yangjeep/simple-ecommerce-search-engine/issues/60).
This issue: [#61](https://github.com/yangjeep/simple-ecommerce-search-engine/issues/61).

---

## 0. What this is testing, and what it deliberately is not

Issue #60 asks how much **infrastructure** commerce-native execution can
remove at equivalent semantics and SLO. Every downstream experiment (#62–#70)
compares resource numbers against a mature-engine baseline and applies a
**>=25% materiality bar**.

That bar is only meaningful if the measuring instrument can *resolve* a 25%
effect. This issue measures the instrument, not the architecture.

Concretely, E1 asks:

> On this specific 4-vCPU QEMU-virtualised host, with no hardware performance
> counters and no root, can repeated native-vs-Solr runs produce resource
> numbers whose run-to-run uncertainty is small enough that a true 25%
> difference could not be confused with noise — and do both engines
> demonstrably answer the same question?

**This protocol registers no architecture claim.** E1 computes no native-vs-Solr
verdict. Any ratio that appears in E1's artifacts is descriptive context for
calibrating batch sizes and is explicitly forbidden from being cited as a
campaign result. The campaign's first architecture claim is E2 (#62).

A "the instrument is not good enough" outcome is a **successful** E1, not a
failure. It is recorded as `FIX MEASUREMENT` and it blocks #62 until resolved.

---

## 1. Hypothesis

**H0 (the instrument is adequate).** With (a) uniform cgroup v2 accounting
applied identically to every engine, (b) engines executed serially and never
co-resident, (c) steal-time-contaminated repetitions excluded by a
preregistered rule, and (d) n=10 repetitions per cell, every gated cell reaches
a relative 95% bootstrap CI half-width of <=7.5% for CPU/query and warm p50
latency, and <=2% for resident memory and index bytes.

**H1 (virtualization noise dominates).** Steal time, absent CPU-frequency
pinning, and shared-host interference push at least one gated cell past those
bounds. The honest verdict is then `FIX MEASUREMENT`: enumerate the noise
source, do **not** begin #62, and do **not** relax the thresholds.

H1 is a real possibility on a QEMU guest and is preregistered as an
acceptable, publishable outcome.

---

## 2. Baseline — and why it is not a straw man

Issue #61 states: *"Use competent Solr/Elasticsearch-style configuration; do
not create a straw-man baseline."* A resource win against a crippled baseline
is invalid, and the campaign's adversarial-review step is instructed to hunt
for exactly that.

### 2.1 Frozen engine set

| Engine | Status in E1 | Version / provenance |
|---|---|---|
| Native `commerce-core` | **PRIMARY** | this repository @ baseline SHA below |
| Apache Solr | **PRIMARY** | `solr:9.10.1`, digest `sha256:1f055b0260d3efb177b12d6a46e9ef510fb4d2616473a91f8f4d099384aa176a` |
| Elasticsearch | **SECONDARY, time-boxed** | `8.15.0` via Docker; see §2.4 |
| Havenask | **DEFERRED** | remains #57's scope; runtime-confirmed only, never benchmark-confirmed |
| OpenSearch | **EXCLUDED** | the historical route was an embedded Java test node; Java is absent on this host, and for E1's purpose it duplicates Elasticsearch |

Baseline commit SHA: `c6953063e2641ceada1992d1372766a2e6ad63cd`.

Every exclusion above is recorded here rather than left silent. Deferral is a
scope decision, not a measured finding.

### 2.2 Solr competence bar

Solr 9.10.1 is the same major/minor version this repository's own prior
comparator work used (`ISSUE55_COMPARATOR_CENTRALIZATION_DECISION.md`,
`ISSUE35_SOLR_HARNESS_HARDENING_DECISION.md`), so E1 does not silently change
the comparator identity while changing the measurement method.

The configuration must satisfy all of the following, and any deviation from
vendor defaults must be enumerated in `ISSUE61_LOG.md`:

- structured commerce attributes indexed as `StrField` with `docValues=true`
  (exact match, faceting, sorting) — never as analyzed `TextField`;
- lexical fields as analyzed `TextField`; `edismax` with an explicit `qf`;
- hard constraints issued in **filter context** as `fq`, not folded into `q`;
- `filterCache` / `queryResultCache` / `documentCache` explicitly sized and
  recorded, not left implicit;
- indexing completed and committed before measurement; `forceMerge(1)` applied
  to the read-only corpus;
- heap explicitly set (`SOLR_HEAP`), sized for the corpus, with the remainder
  of container memory left to the OS page cache backing `MMapDirectory`.

`forceMerge(1)` favours Lucene. That direction is **conservative against
native** and is retained deliberately.

### 2.3 Structural translation parity

Native structural constraints are translated to Solr `fq` clauses through the
already-hardened `crates/comparator-eval` translator (`translate.rs`), not a
fresh per-experiment client. This repository has twice shipped fairness bugs
from bespoke comparator clients — a transport failure scored as a
native-favouring `NDCG=0.0`, and a stale query builder silently omitting a
whole constraint arm. Reusing the centralized, tested translator is a direct
mitigation of a *known, twice-observed* defect class in this codebase.

### 2.4 Elasticsearch time-box

Elasticsearch requires a new Rust adapter and a new translator arm; neither
exists. It is included on a strict time-box. If its smoke test does not pass
inside that box, the verdict `DEFERRED-TO-PRE-E2` is written into
`ISSUE61_LOG.md` and E1's gate is computed on native + Solr only.

**Elasticsearch never blocks or alters E1's gate.** This is preregistered here
so that dropping it later cannot be mistaken for post-hoc convenience.

---

## 3. Dataset and workload

### 3.1 Provenance (frozen)

All datasets are re-acquired from canonical upstreams via the repository's
existing pinned fetch scripts and verified against committed checksums. The
raw bytes remain gitignored; the scripts plus checksums are the reproducible
artifact.

| Dataset | Source | Pin | Verification result |
|---|---|---|---|
| WANDS | `github.com/wayfair/WANDS` | commit `3b74dcf4ba29ab8ff3e6a50b5b09fc627cb882b5` | `sha256sum -c wands_checksums.sha256` — `product.csv: OK`, `query.csv: OK`, `label.csv: OK` |
| ESCI (electronics slice) | `huggingface.co/datasets/tasksource/esci` | revision `45c948250c2116f1e535bac67b92501c695307a4` | `train0000.parquet` = `bd6e1217eef98968103d9731ae52e5e1e640b8af8810956ad144cb13481bf3b9` — matches committed manifest |

Row counts as re-acquired on this host:

- WANDS — 42,994 products, 480 queries, 233,448 relevance judgments. These
  match `docs/research/artifacts/p6a_dataset_acquisition/manifest.json`
  exactly, so the corpus is byte-identical to the one prior WANDS evidence was
  measured on.
- ESCI-electronics — 2,075 products, 600 queries, of which 490/600 carry at
  least one non-`Irrelevant` judgment; judgment distribution
  `Exact 1406 / Substitute 422 / Complement 33 / Irrelevant 281`.

### 3.2 Measured vs. frozen-only

- **Measured in E1**: WANDS (primary), ESCI-electronics (independent second
  dataset, guards against a WANDS-specific stability artifact).
- **Frozen-only in E1** (fetched, checksummed, manifested, *not* measured):
  ESCI-automotive, ESCI-beauty, Magento configurable, Retailrocket.

Retailrocket carries no relevance judgments and therefore cannot participate
in E1's semantic-equivalence audit at all; it is a #62+ input.

### 3.3 Workload freeze

Workload manifests are generated deterministically and checksummed to
`benchmarks/workloads/i61_wands_480.jsonl` and
`benchmarks/workloads/i61_esci_electronics.jsonl`, each record carrying:
query id, query text, the compiled structural constraints, the admission
class from `commerce_core::admission::admit`, top-K, and a qrels reference.

**Regression anchor**: the WANDS routing split must reproduce P9-E02's already
published 21 structural-routed / 459 punt-routed division. A different split
means the workload changed, and any comparison to prior evidence is void.

---

## 4. Treatments

**None.** E1 is a single-arm stability and equivalence study. The "treatments"
are the measurement regimes (cold/warm) and the engines, which are measured
*against themselves across repetitions*, not against each other.

---

## 5. Correctness / semantic-equivalence requirements

Performance numbers are invalid until semantics reconcile. This gate is
evaluated **before** any performance interpretation.

1. **Corpus parity** — indexed document count identical per dataset per engine.
2. **Structural subset** — per-query result **ID-set equality**, order-insensitive,
   native vs. engine. Target 100%. Every mismatch is named by `query_id` and
   root-caused. A frozen known-difference list (e.g. tokenizer punctuation
   edge cases, a class this repository has already documented) is permitted
   **only if enumerated per `query_id` before the gate is evaluated**.
3. **Text / hybrid subset** — no identical-ranking requirement; different
   rankers is the premise of the architecture. Equivalence here means: same
   query text, same `qf` field set, same `rows`, and relevance computed by
   identical code against identical qrels.
4. **Failure handling** — transport, query, and parse failures are
   **excluded and counted**, never scored. This is the
   `crates/comparator-eval` contract. Because Solr runs on the same host under
   our control, any non-zero failure count is a real infrastructure defect and
   **aborts the run** before any number is printed.

A report containing excluded engine failures **does not pass**. Silence is not
agreement.

---

## 6. Metrics

### 6.1 Gated (primary)

| Metric | Definition | Applies to |
|---|---|---|
| `cpu_us_per_query` | Δ cgroup `cpu.stat usage_usec` / queries | all engines |
| `latency_p50_us` (warm) | client-observed per-query p50 | all engines |
| `serving_rss_bytes` | cgroup `memory.current` and `memory.peak`, post-warm | all engines |
| `index_serialized_bytes` | see §6.3 | all engines |

### 6.2 Descriptive (never gated in E1)

p95/p99; QPS/core proxy (`1e6 / p50_us`, per ADR 0007); cold index-build wall
time; end-to-end client latency **explicitly labelled** as including HTTP
transport for containerized engines.

### 6.3 Index bytes — two disclosed definitions, never mixed

1. `index_serialized_bytes` — native: `CatalogIndex::approximate_size_bytes()`
   (already defined by ADR 0007 as serialized Roaring bitmaps plus flat
   ordinal/numeric/price vectors); Solr/ES: `du -sb` of the core index
   directory after final commit and `forceMerge(1)`.
   Disclosed asymmetry: the native figure excludes allocator and `HashMap`/
   `String` overhead; the Lucene figure includes stored fields and docValues.
   The defensible equivalence is that each side carries exactly what it needs
   to answer the identical frozen workload — not that the two byte-counting
   methods are identical.
2. `serving_rss_bytes` — uniform cgroup accounting for **all** engines.
   **This is the metric the campaign's >=25% RSS materiality bar uses.**

### 6.4 Published calibrations

- **timer floor** — clock resolution and `Instant::now()` overhead; any
  measurement below `100x` the floor is reported as a bound, never as a ratio;
- **per-engine transport floor** — CPU and latency for a request that performs
  essentially no retrieval work (match-nothing, `rows=0`), isolating the
  HTTP + JVM per-request tax; both raw and floor-subtracted CPU are reported;
- **cgroup-vs-getrusage agreement** — the native engine is measured both ways;
  disagreement >2% fails calibration and blocks measurement.

---

## 7. The process-boundary confounder, and its frozen resolution

`commerce-core` is an **in-process Rust library**. Solr and Elasticsearch are
**HTTP servers inside JVMs inside containers**. Comparing a direct function
call against an HTTP round trip is precisely the "process-boundary
differences" confounder Issue #60 forbids, and it would inflate any native
advantage.

E1 freezes this resolution:

1. **Uniform containment.** Every engine — *including the native benchmark
   binary* — runs inside a Docker container with identical limits. CPU is
   attributed from that container's own `cpu.stat`. Neither side gets a
   privileged accounting boundary.
2. **Transport floor published.** The per-request HTTP/JVM tax is measured
   directly (§6.4) so a reader can see how much of any gap is protocol
   overhead rather than retrieval work. Both raw and floor-subtracted CPU
   are reported; neither is suppressed.
3. **Timer floor respected.** Native structural primitives can execute in
   microseconds. Batch sizing guarantees every measured interval clears both
   the timer floor and cgroup's 1 µs accounting granularity.
4. **End-to-end latency published separately**, carrying an explicit
   transport-asymmetry label, and never used as a gated metric in E1.

This is a *mitigation*, not an elimination. The residual asymmetry —
native pays no serialization cost — is a real limitation and is restated in
§12.

---

## 8. Resource limits and isolation

Single source of truth: `benchmarks/configs/issue61/container_limits.env`.

```
--cpus=3  --cpuset-cpus=0-2  --memory=6g  --memory-swap=6g
```

- `--memory-swap` is pinned **equal to** `--memory`. The host has 4 GiB of
  swap enabled; without this, a memory-pressured engine is silently rescued by
  swap and every RSS number becomes meaningless.
- The benchmark driver runs on the host pinned to the remaining core
  (`taskset -c 3`) so the driver never competes with the engine under test.
- The native benchmark binary asserts `VmSwap == 0` and aborts otherwise.
- **Engines are executed strictly serially and are never co-resident.** On a
  4-vCPU host, co-residency would make every number a contention measurement.

---

## 9. Warm/cold protocol and repetition policy

### 9.1 Regimes

- **Cold** — freshly started container/process, index built, first full
  workload pass. This is a *cold process*, not a cold page cache: dropping the
  host page cache requires root, which is unavailable. **Disclosed as a
  limitation, not silently labelled "cold".**
- **Warm** — three full workload warm-up passes, then measurement. The same
  warm-up policy applies to every engine, while acknowledging that JIT makes
  the JVM engines more sensitive to early iterations than the native binary.

### 9.2 Repetitions

n=10 independent repetitions per
(engine x dataset x query-class x regime) cell.

Engine order is round-robin across repetitions using
`bench_harness::round_robin_schedule` with a frozen seed, so a monotonic drift
in host conditions cannot be absorbed entirely by whichever engine happened to
run first.

### 9.3 Steal-time exclusion rule (preregistered)

A repetition is **excluded** if `/proc/stat` steal time over that repetition
exceeds **1.0%** of elapsed CPU. At most **2 exclusions per cell** are
tolerated; a third means the cell **FAILS as environment-unstable**.

Exclusions are logged with their measured steal percentage. Excluding a
repetition without recording it is prohibited.

### 9.4 Batch sizing

Every timed interval must satisfy both:
- `Δ cpu_usage_usec >= 1000` (i.e. >=1000x cgroup's 1 µs granularity);
- measured wall interval `>= 100x` the timer floor.

Where a single query is too cheap, queries are executed in a batch of M and
per-query values are derived from the batch. M is recorded per cell.

---

## 10. Decision criteria — fixed now, before any data

**KEEP** — all gated cells pass §11's statistic **and** the §5 equivalence
audit passes for every compared path. #62 may begin.

**FIX MEASUREMENT** (this issue's REJECT-equivalent) — any gated cell fails, or
equivalence fails. Enumerate the noise source or the semantic defect. Do not
start #62. Do not relax the thresholds.

**REFINE** — equivalence passes and exactly one dataset's cells pass. Freeze
E1's scope to the passing dataset and name the other as explicit pre-#62 work.

Changing any threshold after observing data requires a **new numbered protocol
revision** in this file, dated, with the original preserved.

---

## 11. Gate statistic

For each gated metric in each cell, over the n=10 (minus exclusions)
repetition means:

- **percentile bootstrap 95% CI of the mean** —
  `bench_harness::bootstrap_ci_mean`, 10,000 resamples, `alpha=0.05`,
  `seed=61`;
- **relative half-width** = `(ci_high - ci_low) / (2 * mean)`;
- **PASS** iff relative half-width `<= 7.5%` for `cpu_us_per_query` and warm
  `latency_p50_us`, and `<= 2%` for `serving_rss_bytes` and
  `index_serialized_bytes`;
- **secondary check**: coefficient of variation `<= 10%`;
- a cell whose mean is `0.0` **FAILS** (relative half-width is defined as
  infinite) rather than producing `NaN`.

**Power justification, stated before measurement.** At n=10 with CV <=10%, a
Welch t-test has power >0.95 to detect a 25% difference at `alpha=0.05`.
Equivalently: two means separated by a true 25% gap cannot have overlapping
+-7.5% confidence intervals. The 7.5% bound is therefore not arbitrary — it is
the precision required to make the campaign's own 25% materiality bar
decidable.

Cold build time is reported with a CV only and is **not** gated.

---

## 12. Known confounders and how each is handled

| # | Confounder | Handling |
|---|---|---|
| C1 | Process-boundary / HTTP asymmetry | uniform containerized cgroup CPU + published transport floor + separately-labelled E2E latency (§7). Residual asymmetry disclosed, not eliminated. |
| C2 | Steal time on a shared QEMU host | measured per repetition; preregistered 1.0% exclusion rule; >2 exclusions fails the cell (§9.3) |
| C3 | No hardware performance counters (`perf_event_paranoid=4`, no root) | **no cycles/instructions claim will be made anywhere in this campaign.** #63's "cycles/query where practical" degrades to CPU-time accounting. Disclosed, not silently skipped. |
| C4 | No CPU-frequency pinning (virtualized, no `cpufreq`) | disclosed; partially mitigated by round-robin engine ordering (§9.2) |
| C5 | Host page cache cannot be dropped (no root) | "cold" is scoped to **cold process**, explicitly (§9.1) |
| C6 | JVM warm-up / JIT | uniform warm-up policy + published transport floor; JVM sensitivity acknowledged |
| C7 | `forceMerge(1)` favours Lucene | retained deliberately; direction is conservative against native |
| C8 | Index-bytes definitional asymmetry | two separate disclosed metrics, never mixed (§6.3) |
| C9 | Host page cache shared across serial engine runs | round-robin ordering; engines never co-resident |
| C10 | 4 vCPU ceiling | #66's planned 16/8/4/2-core sweep is **impossible on this host**; it must become 4/3/2/1 via cgroup `cpu.max`. Recorded here because it changes a downstream issue's stated protocol and must be surfaced in #60 before #66 runs. |
| C11 | Swap enabled (4 GiB) | `--memory-swap == --memory`; native asserts `VmSwap == 0` (§8) |

---

## 13. Stop conditions

- Any Solr/ES transport, query, or parse failure during a measured run —
  **abort before printing any number** and investigate.
- Steal-time exclusions exceeding the §9.3 budget in any cell — stop, record
  `FIX MEASUREMENT`.
- Calibration disagreement between cgroup and `getrusage` accounting >2% —
  stop; the instrument disagrees with itself.
- WANDS routing split differing from 21/459 — stop; the workload is not the
  workload prior evidence was measured on.

---

## 14. Traceability

- Baseline SHA: `c6953063e2641ceada1992d1372766a2e6ad63cd`
- Branch: `i61/baseline-freeze`
- Host: Ubuntu 26.04 LTS, Linux 7.0.0-30-generic x86_64, 4 vCPU
  (QEMU Virtual CPU version 2.5+), 15 GiB RAM, cgroup v2, Docker 29.7.2
- Rust: `cargo 1.98.0 (797e8a9bc 2026-08-05)` / `rustc 1.98.0 (88d9e12ae 2026-08-18)`
- Append-only log: `docs/experiments/ISSUE61_LOG.md`
- Decision record: `docs/decisions/ISSUE61_DECISION.md`
- Benchmark manifest: `benchmarks/manifests/i61_e1_baseline_freeze.yaml`
- Result manifest: `artifacts/manifests/i61_e1.json`
- Raw artifacts: `docs/research/artifacts/i61_e1_baseline_run1/`
