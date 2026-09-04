# Issue #61 Preregistered Protocol — Infra E1: fair baseline + reproducible resource benchmark harness

Committed **before** any measured run, per this repository's governance
(`docs/EXPERIMENT_LOOP.md`) and Issue #60's own merge-gated execution rule
("preregister hypothesis, baseline, protocol, metrics, and KEEP/REJECT/REFINE
gates before held-out measurement").

Parent epic: [#60](https://github.com/yangjeep/simple-ecommerce-search-engine/issues/60).
This issue: [#61](https://github.com/yangjeep/simple-ecommerce-search-engine/issues/61).

> **REVISION STATUS.** Everything below is **Revision 1**, preserved verbatim.
> An adversarial protocol review run *before any measurement* found six
> confirmed defects that would have made Revision 1's numbers invalid.
> **§15 (Revision 2) supersedes Revision 1 wherever the two conflict.**
> Revision 1 is not deleted, per this repository's discipline of preserving the
> original attempt alongside the correction. No measurement was ever taken
> under Revision 1.

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

---

# 15. Revision 2 — corrections required by adversarial protocol review

**Status: supersedes Revision 1 wherever the two conflict. Written before any
measured run; no data existed when these thresholds were changed.**

An adversarial review was commissioned specifically to falsify Revision 1
before data collection. It returned **BREAKS** and named six defects. Each was
then independently verified against the actual code rather than accepted on
assertion. All six reproduced. This section records the correction.

The defects are recorded here, not quietly fixed, because a protocol that
silently improves between drafts is indistinguishable from one tuned to a
desired answer.

## R2.0 The six confirmed defects

| # | Defect | Verified evidence | Why it invalidates results |
|---|---|---|---|
| D1 | Solr's exact structured filters were emitted as **regular expressions** | `comparator-eval/src/translate.rs` emits `format!("{field}:/{}/", case_insensitive_field_regex(name))` for `Brand`/`ProductType`/`Category` | Forces a regex automaton over the term dictionary instead of an O(1) term lookup. **The baseline was a straw man.** Any native CPU win would have been partly manufactured. |
| D2 | Malformed documents silently dropped | `solr.rs`: `docs.iter().filter_map(\|d\| d["id"].as_str()...)` still returns `Success` | A shortened result set with no error — the "failure becomes a favourable number" class this crate exists to prevent |
| D3 | `numFound` discarded | `EngineComparator::search` returns "at most `rows` document ids" | Full candidate-set equality is impossible; top-K equality can pass while filters differ |
| D4 | `queryResultCache` sized 4096 against a **480-query** workload | `provision_solr.sh` + `wc -l dataset_cache/wands/query.csv` = 481 | Three warm-up passes would cache *every* result. "Warm Solr" would have measured a hash lookup, not retrieval, against a native engine that has no whole-query result cache |
| D5 | `CgroupSnapshot::delta_since` used `saturating_sub` | `issue61-eval/src/cgroup.rs` | A counter reset or wrong-cgroup read silently becomes **0 CPU** — a favourable number |
| D6 | Protocol contradicted itself on latency | §6.1 gates `latency_p50_us`; §7.4 says end-to-end latency is "never used as a gated metric in E1" | The gate was undefined |

D4 was this protocol's own error, introduced by its own provisioning script. It
is recorded with the same weight as the inherited ones.

## R2.1 Measurement boundary — one boundary, no subtraction

Revision 1's transport-floor subtraction is **withdrawn**.

The review's argument is accepted: Revision 1's accounting was neither
engine-only nor total-system. Native's query loop ran inside its measured
cgroup, while Solr's request construction, encoding, socket handling and JSON
parsing ran in the host driver and escaped Solr's `cpu.stat`. Worse,
subtracting a point-estimate floor `F` from measured CPU `Q` is not valid:
`Var(Q-F) = Var(Q) + Var(F) - 2Cov(Q,F)`, so a point estimate understates
uncertainty, and `Q = engine work + fixed floor` is not an established model —
protocol cost varies with hit count, body size, cache state, GC and JIT. A
match-nothing `rows=0` probe can also short-circuit to `MatchNoDocsQuery` and
skip nearly all index work, so it *under*-estimates the tax it claims to
measure.

**Revision 2 requires:**

1. The native engine is placed behind a **minimal HTTP endpoint** returning the
   same response contract as Solr (`id` list + total match count). Both arms
   are then driven by the **same external client over a persistent connection**.
2. The primary metric is **raw server-side container CPU**, measured at an
   identical boundary on both sides. No adjusted, corrected or floor-subtracted
   CPU comparison is published.
3. No-op probes are retained as **diagnostics only**, published beside the raw
   numbers, never subtracted from them.
4. Because both arms now share one boundary, **end-to-end p50 becomes legitimately
   gateable**, which resolves D6. §6.1 stands; §7.4's blanket prohibition is
   superseded.

## R2.2 Baseline competence — corrections to Solr

- **Exact filters, not regex.** Structured string constraints are issued as
  exact term queries against a lowercased companion field populated at index
  time. Semantics are identical to Revision 1's case-insensitive regex; the
  automaton is gone. Implemented as an **opt-in** translator mode so the five
  existing evaluation binaries and their published numbers are untouched.
- **`queryResultCache` disabled (size 0).** It memoizes whole result lists and
  the workload is a fixed 480 queries.
- **`filterCache` retained and generously sized.** It caches filter-context
  bitsets and is the closest Solr analogue to the native engine's precomputed
  Roaring bitmaps. Disabling it would be a straw man in the opposite direction.
  Hit/miss/eviction counters are published with every result.
- **`documentCache` retained** — both engines materialize documents.
- Exact heap, GC, cache sizes, schema and connection settings are frozen and
  checksummed **before** the first measured run.

## R2.3 Equivalence — full candidate sets, not top-K

Revision 1's top-K ID-set equality is **insufficient** and is replaced.

Two different candidate sets can share a top-K, and identical candidate sets
can differ at the K boundary through score ties. `numFound` equality alone is
also insufficient, since two different sets can share a cardinality.

**Revision 2 requires:** outside the timed path, retrieve the **complete**
structural candidate set from both engines and compare a canonical sorted-ID
digest **and** the total count; emit both directional set differences on
mismatch; audit every hybrid query's structural prefilter independently of
ranking; treat any document lacking a valid string id as a hard `ParseError`,
never a short success.

The Revision 1 phrase "enumerated per `query_id` before the gate is evaluated"
was a **post-hoc loophole** — it permitted observing mismatches and then adding
them to the allowed list. Any known-difference manifest must now be frozen and
checksummed **before execution**.

Carried-forward limitation: WANDS and ESCI both map one product to one variant,
so product-ID equality **cannot** validate same-variant conjunction semantics.
E1 makes no same-variant claim.

## R2.4 Statistics — paired blocks, Student-t, and a ratio decision rule

Revision 1's gate is **withdrawn**. Three independent problems:

1. **The percentile bootstrap is anti-conservative at n=10.** At CV=10%, its
   half-width is ≈5.88% against a correct Student-t half-width of
   `t(9,.975)·0.10/√10` ≈ 7.15% — about 18% too narrow, giving ≈90.4% actual
   coverage for a nominal 95% interval. Resampling cannot add information the
   sample does not contain. After Revision 1's permitted two exclusions (n=8)
   coverage falls to ≈89.1%.
2. **Non-overlapping intervals do not establish a ≥25% saving.** With A=1.00,
   B=0.75 and both at ±7.5%, the compatible ratio spans
   `0.75·0.925/1.075 = 0.645` to `0.75·1.075/0.925 = 0.872` — savings anywhere
   from **12.8% to 35.5%**. A 25% point estimate is *not* decisively above a
   25% bar. Revision 1's power justification was arithmetically true but
   answered a Welch test the protocol never runs.
3. **Repetitions are not independent.** Page cache, engine caches and host
   drift induce serial dependence, so an IID bootstrap over raw repetitions is
   unjustified.

**Revision 2 requires:**

- **≥30 randomized paired blocks.** Within a block both engines run under the
  same host conditions; the block yields one paired ratio. The block, not the
  repetition, is the unit of analysis.
- **Student-t intervals**, computed on the **log** scale for ratios (ratios are
  multiplicative and right-skewed) and exponentiated back.
- **The decision is made on the ratio directly**: a ≥25% saving is claimed only
  when the **upper** 95% bound on `treatment/baseline` is ≤ 0.75. Otherwise the
  verdict is `Inconclusive` — never a pass.
- **A known-effect calibration arm is mandatory.** A deliberately injected
  effect (5 workload passes vs 4, true ratio 1.25) must be recovered: the
  observed interval must contain 1.25 **and** exclude 1.0. Repeatability alone
  cannot detect systematic accounting bias — an instrument that cannot recover
  an effect it was told to expect cannot be trusted on an unknown one.
  **A missing calibration is `FIX MEASUREMENT`, never a pass.**
- **Index bytes are an exact artifact measurement**, not a bootstrapped
  statistic. Revision 1's 2% bound was near-vacuous for a deterministic native
  estimate.
- Cells with fewer than 30 blocks are marked `underpowered` and force
  `FIX MEASUREMENT`.

## R2.5 Steal time — no post-run exclusion in the primary analysis

Revision 1's exclusion rule is **withdrawn from the primary gate**.

Preregistering an exclusion does not remove its selection bias, and here the
bias is fatal: virtualization noise is exactly the phenomenon H1 is meant to
detect, and the rule deletes precisely the repetitions that demonstrate it.
Because cgroup CPU counts time actually scheduled, steal inflates *wall
latency* more than CPU, so excluding high-steal repetitions suppresses latency
variance and makes the instrument look more stable than it is.

**Revision 2 requires:** every scheduled block enters the primary analysis.
Clean-only results are published as a **sensitivity analysis** that may never
rescue a failed all-data gate. Host conditions are screened *before* a block
runs; a rejected block is rerun **whole**, and every rejection is recorded.
Excessive rejection is itself `FIX MEASUREMENT`. Steal is measured on the
assigned CPUs, not the aggregate line, and `throttled_usec`, CPU PSI, OOM and
swap counters are recorded alongside.

## R2.6 Resource definitions

- `serving_rss_bytes` is renamed **`cgroup_memory_footprint_bytes`**. In
  cgroup v2 this includes anonymous memory, charged page cache, sockets and
  kernel memory. It remains the right cross-engine metric — it is the memory
  the container actually needs — but calling it "RSS" was inaccurate.
- `memory.peak` is **cumulative since cgroup creation** and may include
  indexing, force-merge and startup. Serving memory is therefore sampled
  across the serving window; the lifetime peak is reported separately and never
  used as serving memory.
- `memory.stat` components are recorded so the anonymous/page-cache split is
  visible rather than inferred.
- Native `approximate_size_bytes()` **excludes** the `Catalog`, `HashMap`
  buckets, location maps and allocator overhead, while Lucene's `du -sb` is
  actual persisted storage. These are **not** comparable. E1 therefore reports
  each engine's index bytes descriptively and makes **no cross-engine index-byte
  claim**; `cgroup_memory_footprint_bytes` carries the memory comparison.

## R2.7 Counter integrity

`CgroupSnapshot::delta_since` returns an error on any backwards counter
movement instead of saturating to zero. A wrong-cgroup read or counter reset
must be a loud failure, because saturating produced a *favourable* zero-CPU
measurement.

## R2.8 Narrowed scope of a KEEP verdict

A Revision 2 `KEEP` authorizes **only** what it calibrated: mean CPU/query and
p50 latency at the standardized HTTP boundary, on the two frozen datasets.

It explicitly does **not** authorize p95/p99 claims, concurrent-load claims, or
minimum-resource-envelope claims. #65 (tails, load) and #66 (envelope) each
require their own calibration before their measured matrices begin. Revision 1
implied a broader licence than its evidence could support.

`QPS/core = 1e6/p50` (ADR 0007) is **not used** in this campaign: median
latency is not CPU time and does not imply sustainable single-core throughput
for a multithreaded engine. Throughput per core, where needed, is measured
throughput divided by measured CPU.

---

# 16. Revision 2.1 — operational constants, frozen before measurement

**Status: additive to Revision 2. Written and committed before any measured
artifact exists.**

Revision 2 fixed the *method* but left seven operational quantities
unspecified. An unspecified constant is a degree of freedom, and a degree of
freedom that survives into the measurement phase is a place where a result can
be tuned after the fact without anyone being able to prove it. This section
closes them. Every value below is frozen; changing one after data exists
requires a numbered Revision 3 with the original preserved.

## 16.1 Block structure

| Constant | Value |
|---|---|
| Blocks per gated cell | **30** |
| Warm-up passes per engine session | **3** |
| Measured passes per engine session | **2** |
| Engine order within a block | randomized per block, seed **61** |
| Engine sessions | restarted every block; **never co-resident** |

A *block* is two adjacent single-engine sessions (boot → 3 warm-up passes → 2
measured passes → cgroup/memory snapshots → teardown) in randomized order. The
block, not the pass, is the unit of analysis.

Two measured passes give within-block averaging. One WANDS pass costs on the
order of 5–15 s of CPU per engine, which is ≥10⁶× cgroup's 1 µs accounting
granularity and far above the timer floor, so §9.4's batching requirement is
satisfied without additional batching. Per-query client latencies are recorded
individually — **p50 is never derived by dividing a batch time by N.**

## 16.2 Calibration arm

| Constant | Value |
|---|---|
| Injected effect | **5 measured passes vs 4**, true CPU ratio **1.25** |
| Measured quantity | **block-total container CPU** (`Δ cpu.stat usage_usec`) |
| Engines calibrated | **both** native and Solr |
| Dataset | WANDS |
| Blocks | 30 per engine |
| Pass condition | 95% Student-t CI on the log-ratio **contains 1.25 AND excludes 1.0** |

Two decisions here are load-bearing.

**The calibration metric is block-total CPU, not per-query CPU.** Running five
passes instead of four multiplies the *total* work in the measured interval by
1.25, but leaves *per-query* cost at approximately 1.0. Calibrating a per-query
metric against an expected ratio of 1.25 would be a category error and would
fail for the wrong reason.

**Both engines are calibrated, not just native.** Calibrating only the quiet
single-threaded Rust binary and then asserting the instrument is sound would be
exactly the shortcut an adversarial reviewer should attack: the JVM, with JIT
and GC, is the noisy case the instrument actually has to survive. The extra
machine time is the price of the claim.

No additional point-estimate tolerance is defined. The interval rule above *is*
the test; inventing a supplementary tolerance after seeing the observed ratio
would be threshold-tampering.

**Disclosed limitation:** p50 latency has no injectable known effect under this
design — adding passes does not change per-query latency. Latency credibility
therefore rests on three other checks rather than on calibration: the shared
HTTP boundary (R2.1), the measured timer floor (§6.4), and the
cgroup-vs-`getrusage` agreement check (≤2%, §13). This is stated here so the
decision record cannot later imply latency was calibrated when it was not.

## 16.3 Candidate-set digest

```
digest = SHA-256( join(sort(unique(ids)), "\n") )   # UTF-8, no trailing newline
```

Compared together with `numFound`. A digest match with a count mismatch, or
vice versa, is a failure. On mismatch both directional set differences are
emitted.

Retrieved **outside the timed path**, in a separate audit stage that runs
before any measured block, so retrieval cost cannot contaminate measurement by
construction. Solr uses `cursorMark` pagination (`sort=id asc`, `fl=id`,
`rows=5000`) with the terminal invariant `collected.len() == numFound`, else a
hard error. Native uses its full candidate set directly.

Scope: the structural WANDS queries, every hybrid query's structural prefilter
audited independently of ranking, and the same for ESCI. Free-text disjunction
sets are **not** audited — unbounded and not required by R2.3.

## 16.4 Steal-time pre-screen (replaces Revision 1's post-run exclusion)

| Constant | Value |
|---|---|
| Probe | 5 s, measured on the engine's assigned CPUs (`cpuset 0-2`), not the aggregate line |
| Reject-block threshold | steal > **1.0%** |
| Action on reject | rerun the **whole block**; log the measured steal % |
| "Excessive" rejection | > **20%** of scheduled blocks ⇒ `FIX MEASUREMENT` (environment) |

Every block that *runs* enters the primary analysis. Screening happens before a
block, never after it — that is the difference between controlling conditions
and deleting inconvenient data.

## 16.5 Memory sampling

`memory.current` sampled every **500 ms** during measured passes. The
**per-block median** is the gated statistic; the per-block max is reported
alongside. Lifetime `memory.peak` is recorded but is **never** used as serving
memory, because it is cumulative since cgroup creation and includes indexing,
force-merge and startup.

## 16.6 Enumerated cells

**Gated** (warm regime only): `cpu_us_per_query` and `latency_p50_us` per
engine × dataset; `cgroup_memory_footprint_bytes` at the footprint bound; index
bytes as an exact artifact **per engine** with no cross-engine claim.

**Descriptive, never gated:** the cold regime (n=5 per engine × dataset), cold
build time, p95/p99, Solr cache hit/miss counters, the no-op transport probe.

## 16.7 Standing clauses restated

- Revision 1 §10's **REFINE** clause remains operative under Revision 2: if
  equivalence passes and exactly one dataset's cells pass, E1's scope freezes to
  the passing dataset and the other is named as explicit pre-#62 work. REFINE is
  an *outcome*, not a pre-hoc option — neither dataset may be dropped before
  data exists.
- ESCI-electronics **remains a measured dataset**. Its known shape (no
  product_type, no category, no price, flat products) is disclosed, and E1 makes
  no same-variant claim on it. A corpus with weak structural signal is still a
  valid *stability* corpus, and E1 gates stability, not effect size.
- Seed **61** everywhere a seed is required.

## 16.8 Stop rule if calibration fails

1. Root-cause first. If a concrete instrument defect is found and fixed, the
   **entire** measured campaign — calibration and blocks — reruns from scratch.
   Superseded data is preserved, never overwritten.
2. At most **two** such fix-and-rerun cycles.
3. After two failed cycles, or if no defect can be identified, the verdict is
   `FIX MEASUREMENT`, and **the pull request still merges.**

The third point is deliberate. Issue #61's own gate text makes
`FIX MEASUREMENT` a legitimate terminal outcome, and the deliverables — the
harness, the protocol, the equivalence audit, the raw negative evidence — are
exactly the "negative results are first-class outputs" case in `CLAUDE.md`.
What a failed calibration blocks is **#62**, not the merge. The decision record
then names the enumerated defect as explicit pre-#62 work.

## 16.9 The frozen workload carries the per-engine request contract

Hands-on QA of the harness, before any measurement, found that the benchmark
driver sent `q` and `rows` to **both** engines and nothing else — so Solr
received no `fq`, no `defType`, no `qf` and no `fl=id`, answering an
unconstrained question over every stored field while native applied its
structural constraints and returned ids only.

This is the **third** occurrence of the same defect class in this repository
(`ISSUE55_PAIRED_COMPARATOR_DECISION.md`,
`ISSUE55_ROUTING_OUTCOME_REPLICATION_DECISION.md`). Both earlier occurrences
were found only after numbers had been published.

The recurrence is the important part. Issue #55 A3 centralized the translator
into `comparator-eval` to stop this, and that did eliminate the *translation*
defect — but not the *call-site* defect. A newly written binary can still
simply fail to call the shared translator, and nothing in the type system or
the test suite notices, because the resulting request is perfectly valid; it
just asks a different question.

Revision 2.1 therefore removes the opportunity rather than relying on the
author remembering:

1. `i61_workload_freeze` emits, for every query, the **fully translated,
   per-engine request** — native's `q`, and Solr's `q` + `fq[]` + explicit
   `params` (`defType`, `qf`, `fl`, `rows`) — into the checksummed workload
   artifact.
2. If translation reports **any** unresolvable constraint, the freeze **fails
   and exits non-zero**. A partially translated `fq` sent to Solr while native
   enforces the full constraint set is the defect itself, so a partial `fq` is
   never emitted.
3. `i61_bench` **replays** those requests verbatim and synthesizes no engine
   parameter of its own. A record missing its per-engine block is a hard error,
   never a fall-back to a bare `q`.

The consequence is that the comparator contract becomes a frozen, checksummed
**artifact** committed before measurement, rather than behaviour reconstructed
at run time by whichever binary happens to be driving. It is auditable by
reading a file, and a reviewer can diff what each engine was actually asked
without reading any Rust.

## 16.10 Disclosed response-payload asymmetry

Solr's response body carries two scalars the native endpoint does not emit
(`numFoundExact`, `start`):

```
native: {"response":{"docs":[{"id":..}],"numFound":..},"responseHeader":{"status":..}}
solr:   {"response":{"docs":[{"id":..}],"numFound":..,"numFoundExact":..,"start":..},
         "responseHeader":{"status":..}}
```

This is a small amount of serialization work Solr does and native does not. It
is **not** corrected — matching it would mean adding dead fields to the native
endpoint purely to equalize a benchmark, which is the kind of
architecture-for-benchmark change `CLAUDE.md` forbids. It is disclosed here so
it appears in the record rather than being discovered by a reviewer, and it
biases *against* native's measured advantage rather than for it.

## 16.11 E1-only lexical-equivalence profile

Issue #61 calibrates the measuring instrument and makes no architecture or
native-vs-Solr effect claim. For E1 only, Solr residual lexical matching is
forced to the native endpoint's conjunctive candidate-generation contract.

Every frozen Solr request carries these explicit values:

- `defType=edismax`;
- WANDS `qf=title description`;
- ESCI-electronics `qf=title description bullet_point`;
- `q.op=AND`;
- `mm=100%`;
- `mm.autoRelax=false`;
- `tie=0.0`;
- `sow=true`;
- `lowercaseOperators=false`;
- `ps=0`, `ps2=0`, `ps3=0`, and `qs=0`;
- `rows=10`;
- `fl=id`;
- `sort=id asc`; and
- `wt=json`.

No `pf`, `pf2`, `pf3`, `bq`, `bf`, `boost`, or `q.alt` parameter is emitted.
The phrase-slop values above are inert because phrase boost fields are absent;
they are explicit only to close request-handler/default degrees of freedom.

Residual query text is emitted as literal eDisMax input. Backslash and every
Lucene query-language metacharacter (`+ - && || ! ( ) { } [ ] ^ " ~ * ? : /`)
is backslash-escaped before HTTP form encoding. Fielded queries, boosts,
grouping, unary operators, and unescaped quote syntax are therefore not
permitted in the generated request. Empty residual text uses `*:*` only when
the compiled query carries at least one structural filter.

Before any timed block, the audit retrieves the complete result set for every
query from both engines. It compares `numFound` and the §16.3 sorted,
unique-ID digest and emits both directional differences on mismatch. This
supersedes §16.3's exclusion of free-text candidate sets for E1. Any mismatch
is `FIX MEASUREMENT`; it cannot become a post-hoc known difference.

The complete effective Solr configset is archived and checksummed: effective
managed schema, `solrconfig.xml`, config overlay, request handlers, stopwords,
synonyms, protected-word/mapping/language resources, and any loaded plugin.
Every field named by a frozen `fq`, `qf`, `fl`, or `sort` is validated against
that live configset before the audit runs.

Gated stability cells are engine × dataset × each non-empty admission class
(`FastPath`, `Hybrid`, `Punt`) plus the all-workload aggregate. Class-specific
CPU cells run as separately measured frozen sub-workloads and must satisfy the
timer/cgroup floor. CPU/query remains the primary metric; candidate count,
`numFound`, returned rows and zero-hit rate are diagnostics, never divisors used
to normalize away candidate-pruning work.

E1's forced-AND profile and its CPU values are calibration artifacts only.
They are forbidden as effect-size baselines for Issues #63 or #65. Those
issues must freeze their own production comparator settings and correctness/
relevance non-inferiority gates before measurement.

## 16.12 Audit-order and ambiguity-only corrections

The first workload regeneration under §16.11 happened before any timed data
and exposed two remaining contract defects.

First, `sort=score desc,id asc` was inconsistent with §16.3's full-set
cursor audit and needlessly retained scorer work in an experiment whose only
semantic gate is candidate-set equality. E1 therefore freezes `sort=id asc`.
This correction was committed before workload regeneration completed.

Second, WANDS query IDs `7` (`driftwood mirror`) and `160` (`marble`) compile
to explicit ambiguity with no hard constraint and no residual lexical term.
The current native endpoint consequently executes its unconstrained candidate
set. For E1 parity, any non-empty source query with no executable hard or
residual term freezes Solr `q=*:*` with an empty `fq` list. A truly empty or
whitespace-only source query remains a hard generation error. The complete-set
audit must return the full corpus on both arms for these two records; otherwise
the equivalence gate fails.

## 16.13 Dataset-specific Solr contract gate

Before any correctness audit or timed request, provisioning must validate the
live core against a dataset-specific frozen schema and config snapshot. WANDS
and ESCI-electronics have different lexical and structured fields, so one
generic snapshot cannot validate both.

Validation is fail-closed and requires exact semantic JSON equality after one
documented normalization: Solr's top-level `config.znodeVersion` is managed
metadata that changes when the config API writes a new version, so it is
omitted from frozen snapshots and removed from the live config before
comparison. No other schema or config key is ignored. The gate separately
asserts the E1 unique key, lexical fields, lowercase exact-filter analyzer,
copy fields, and cache settings so a defective frozen snapshot cannot bless a
straw baseline.

The four normalized snapshots and their SHA-256 values are frozen in
`benchmarks/configs/issue61/solr_frozen.sha256`; provisioning verifies that
manifest before reading the snapshots. Any later change requires a numbered
protocol correction before another provision or measurement.

# Revision 3 — native-compatible lexical analyzers

**Frozen 2026-09-04 after the first complete-set audit failed and before any
corrected audit or timed measurement.** Revision 2.1 and its failed artifact
remain preserved. All Revision 2 clauses remain binding except where this
revision explicitly replaces the lexical field analyzer.

## 17.1 Trigger and competing explanations

The first ESCI-electronics complete-set audit matched 542/600 queries and
failed all remaining 58. Every mismatch was Punt; every structural/hybrid
query matched. The primary hypothesis is therefore that Solr `text_general`
does not reproduce native's asymmetric catalog/query tokenization.

Competing explanations remain live until the corrected audit passes: omitted
lexical fields or content during indexing, query-parser escape behavior,
Unicode classification differences, and source-ID mapping defects. Any
remaining mismatch is evidence for one of these alternatives and remains
`FIX MEASUREMENT`; it cannot be added to the known-difference manifest.

## 17.2 Frozen analyzer treatment

The WANDS `title`/`description` and ESCI-electronics `title`/`description`/
`bullet_point` fields use a new `native_lexical` Solr field type:

- index analyzer: PatternTokenizerFactory with pattern
  `[^\\p{L}\\p{N}]+`, then LowerCaseFilterFactory;
- query analyzer: WhitespaceTokenizerFactory, then LowerCaseFilterFactory;
- no stopword, synonym, stemming, delimiter, folding or n-gram filter.

This mirrors the current native endpoint rather than improving it: catalog
text is split on every non-alphanumeric character, while compiled residual
query terms retain punctuation inside whitespace-delimited tokens. The frozen
literal eDisMax escaping and every other §16.11 request parameter remain
unchanged.

Provisioning creates the field type, replaces only those named lexical fields,
reindexes the complete corpus, force-merges, captures new dataset-specific
schema/config snapshots and passes §16.13. The failed Revision 2.1 core is not
reused.

## 17.3 Corrected-audit gate

The corrected audit reruns from scratch for all 480 WANDS and all 600
ESCI-electronics queries. The preregistered pass condition is exact: every
query must match both `numFound` and the §16.3 digest, with zero native or Solr
failures. Any mismatch on either dataset blocks timing. No threshold is relaxed
and no dataset, admission class or punctuation case may be excluded.

# Revision 4 — one explicit lexical token stream

**Frozen 2026-09-04 after the Revision 3 complete-set audit failed and after
untimed diagnostic toggles, but before regenerating either workload or running
another authoritative audit or timed measurement.** Revision 3's failed result
and both rejected diagnostic treatments remain preserved in the append-only
log. All prior clauses remain binding except where this revision explicitly
replaces native residual lookup and Solr `q` construction.

## 18.1 Trigger and corrected mechanism

Revision 3 reached 595/600 on ESCI-electronics. Its five misses exposed a
pre-existing native endpoint defect: `CommerceQuery::residual_lexical` entries
may contain multiple words, but `CatalogIndex::lexical_and_candidates` accepts
the individual tokens produced by public `commerce_core::index::tokenize`.
Passing residual entries directly therefore looked up impossible phrase keys.

Tokenizing only native residuals was rejected at 581/600. Changing Solr's query
analyzer to PatternTokenizer as well was rejected at 596/600. The final four
misses occurred because eDisMax kept punctuation-split subtokens inside one
original whitespace clause and required those subtokens in one `qf` field;
native postings are deliberately attribute-agnostic across title and every
Text attribute. An untimed four-query toggle that made every token a top-level
Solr clause matched 4/4 exactly.

## 18.2 Frozen treatment

Both engines consume one deterministic token stream derived from the compiled
residual entries:

1. apply `commerce_core::index::tokenize` independently to every
   `residual_lexical` entry;
2. preserve entry order and token order, including duplicates;
3. perform no second compiler stopword pass — punctuation splitting may expose
   a token such as `to`, and that token remains part of the request;
4. native passes the resulting token vector to
   `lexical_and_candidates`;
5. Solr joins the same tokens with one ASCII space and applies the existing
   literal eDisMax escaping before freezing `solr.q`.

Revision 3's `native_lexical` index PatternTokenizer and query
WhitespaceTokenizer remain frozen. Because each Solr whitespace chunk is now
one native token, eDisMax constructs one top-level cross-field DisMax clause
per token. No field, parser parameter, filter, ranking parameter or corpus
changes.

## 18.3 Workload and evidence replacement

Regenerate both JSONL workloads from the unchanged source query files and
catalogs. Native requests, admission classes and structural constraints must
remain unchanged; only Solr `q` values affected by residual token normalization
may differ. Record new SHA-256 values and retain the superseded workload hashes
in the log rather than rewriting history.

The authoritative audit then reruns from clean provisioned cores for all 600
ESCI-electronics and all 480 WANDS queries. The pass condition remains exact
count-plus-digest equality for every query with zero engine failures. Any
remaining mismatch blocks timing and may not be whitelisted.
