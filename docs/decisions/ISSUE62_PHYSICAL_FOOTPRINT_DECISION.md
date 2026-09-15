# Issue #62 — E2 Physical Footprint / SKU-Density Scaling

Raw evidence: `artifacts/issue62/results/*.json` (72 final-status cells: 60
`ok` measurements, 9 documented Havenask exclusions, 3 native/1M
memory-ceiling failures) and `artifacts/issue62/results/invalid/*.json` (20
preserved-but-superseded/invalidated attempts, each named with its
invalidation reason). Harness: `crates/issue62-eval/`, `scripts/issue62/`.
Config: `benchmarks/configs/issue62/container_limits.env`.

**This document was substantially revised after an independent adversarial
review found two confirmed problems with the first draft** (see "Adversarial
review findings and corrective action" below) — the numbers and verdict
below are the corrected, re-verified version, not the original.

## Verdict: REFINE — native's disk-vs-competitors comparison is not apples-to-apples; under an equalized memory ceiling, native fails to complete the 1M tier at all

Two headline claims from the first draft did not survive adversarial review:

1. **"Native has the smallest disk footprint" is not a valid disk-vs-disk
   comparison.** Native's `index_bytes` is `index.approximate_size_bytes()`
   (`crates/commerce-core/src/index/mod.rs`) — a documented **approximate
   on-heap** estimate of a subset of native's in-memory structures (bitmaps
   and ordinal/numeric vectors only; excludes raw catalog strings, `HashMap`
   overhead, allocator bookkeeping). Native persists **nothing to disk**
   during this build (`data_dir_for(Engine::Native) => None` in
   `crates/issue62-eval/src/bin/i62_measure.rs`) — there is no directory to
   `du -sb`, unlike every competitor, whose `index_bytes` is a real
   persisted-file measurement. So "native's index_bytes is Nx smaller than
   competitor Y's" was comparing a partial in-RAM structure-size estimate
   against real on-disk bytes for six other engines. This is reported below
   for reference (native's own number is internally consistent and useful
   as a lower bound on the size of its core structures) but is **not**
   claimed as a disk-footprint win.
2. **Native's 1M-tier result was measured under a different, unconstrained
   memory ceiling than every competitor.** `launch_native()` hardcoded
   `--memory=16g --memory-swap=16g` regardless of
   `benchmarks/configs/issue62/container_limits.env`, while every competitor
   script sourced that file and was capped to `I62_MEMORY=6g` from the 500k
   tier onward (after a mid-campaign reduction forced by real host memory
   instability, see below). Native's original 1M warm RSS (8.8 GiB)
   exceeded that 6g ceiling. **Fixed and re-measured**: with native's
   container now sourcing the same `I62_MEMORY`/`I62_MEMORY_SWAP` values as
   every competitor, **all 3/3 native/1M runs failed to complete** —
   `wait_native_ready` never observed a successful `/ping` within the full
   3600s budget under the 6g ceiling (`peak_build_memory_bytes: null` in
   all three, consistent with either an early OOM before the build-sampler
   took a reading or severe memory-pressure slowdown; container-name reuse
   between runs prevented a retroactive `docker inspect --format
   .State.OOMKilled` check on the exact mechanism — the functional result,
   that native cannot complete 1M under the same ceiling competitors were
   held to, is unambiguous either way).

Under the corrected, equalized-memory-ceiling harness: **native completes
100k and 500k, but is the only engine of the 7 attempted that fails to
reach the 1M tier at all under the shared 6g memory budget.** This is a
genuine, disclosed negative result for the "more efficient"/"more stable"
hypothesis at scale, not measurement noise — it is reported as-is per the
standing research-discipline rule that negative results are first-class
outputs.

**Recommendation carried into follow-up, not decided here**: investigate
why native's memory need grows so steeply with corpus size relative to its
own disk-estimate growth (100k→500k RSS roughly quadrupled while its
approximate structure-size estimate merely quadrupled in lockstep with
document count — the *absolute* RSS/doc ratio, not just its 1M failure, is
worth a dedicated look), and specifically whether native holds a
fully-materialized-in-heap representation where a competitor would use an
mmap'd, page-cache-backed one.

## Adversarial review findings and corrective action

An independent adversarial review (`ecc:rust-reviewer`, dispatched before
merge per the standing forensic-review step) was asked to try to falsify
this experiment's conclusions. It found:

- **CONFIRMED**: native's `index_bytes` is a partial on-heap estimate, not
  a real disk measurement — see verdict point 1 above. This is a
  documentation/interpretation fix, not a code fix (there is no real disk
  number to substitute; native genuinely persists nothing in this build).
- **CONFIRMED**: native's container had an unconditional 16g/16g memory
  ceiling hardcoded in `launch_native()`, never subject to the mid-campaign
  `container_limits.env` reduction that every competitor's provisioning
  script correctly sourced. **Fixed**: `launch_native()` and `base_result()`
  now both call a new `read_env_var()` helper that reads
  `I62_MEMORY`/`I62_MEMORY_SWAP`/`I62_JVM_HEAP` directly from
  `container_limits.env` at invocation time, matching what the bash
  scripts already did. All 9 native cells (3 tiers × 3 runs) were archived
  to `invalid/` with reason `SUPERSEDED_unconstrained_16g_memory_ceiling`
  and re-measured under the corrected, competitor-matching harness — see
  verdict point 2 for the result.
- **CONFIRMED**: `base_result()` hardcoded `memory_limit: "16g"` and
  `jvm_configured_heap_bytes: 8 GiB` as constants for every cell of every
  engine and tier, regardless of what was actually applied — every
  Solr/Elasticsearch/OpenSearch result file's recorded provenance metadata
  was stale/wrong even though the real `docker run --memory` flags were
  correctly env-sourced by the bash scripts. **Fixed**: both fields are now
  read live from `container_limits.env` via the same helper, so
  newly-recorded results carry accurate provenance. (Solr/ES/OpenSearch
  result files measured before this fix still carry the stale
  `memory_limit`/`jvm_configured_heap_bytes` metadata fields — their actual
  applied limits are correctly described in prose in the "JVM heap-ceiling
  confound" section below and were independently confirmed via
  `container_limits.env`'s git history, not via the stale per-file field.)
- Reviewed and found no problem: linear-fit/multiplier arithmetic matched
  the raw JSON; the Havenask SIGILL/AVX2 exclusion is real and consistent
  with the provisioning script; `cargo test -p issue62-eval` and
  `cargo clippy -p issue62-eval --all-targets --all-features -- -D
  warnings` both passed clean; cell counts reconciled correctly.

## Scope actually executed vs. preregistered

- **Engines attempted**: native, Solr 9.10.1, Elasticsearch 8.15.0,
  OpenSearch 2.19.0, Typesense 27.1, Meilisearch v1.11, Vespa (generic
  x86-64 image), Havenask (`ha3_runtime`).
- **Havenask — documented exclusion, not silent omission**: the `ha_sql`
  worker binary SIGILLs unconditionally on this host's CPU. `gdb` backtrace
  pinpoints the crash inside `ailego::internal::CpuFeatures::CpuFlags::CpuFlags()`
  on a VEX-encoded `vpxor` instruction — the official `ha3_runtime` binaries
  require baseline AVX/FMA/BMI2, which this host's CPU lacks. Verified via
  direct `gdb` inspection, not inferred from a timeout. All 9 Havenask cells
  (3 tiers × 3 runs) fail identically and immediately with this diagnosis;
  `scripts/issue62/provision_havenask.sh` fails fast with the same message
  rather than hanging.
- **Tiers measured**: 100k and 500k fully measured (3/3 valid runs) for all
  7 non-Havenask engines. 1M fully measured (3/3) for the 6 non-native
  engines; native fails 3/3 at 1M under the corrected, equalized memory
  ceiling (see verdict) — this is reported as native's measured ceiling at
  this tier, not a missing data point.
- **3M/5M tiers: not attempted this round.** The host repeatedly showed
  external memory-pressure instability during this campaign (MemTotal
  observed dropping from ~30 GiB to ~13-15 GiB mid-run on more than one
  occasion, unrelated to this experiment's own containers; see "Host
  instability" below). Given native cannot even complete 1M under the
  shared memory ceiling, and the 1M tier alone required raising
  `BUILD_TIMEOUT` to the full originally-preregistered 3600s with
  multi-hour-per-run engines under host contention, attempting 3M/5M this
  round was judged likely to produce more environmental noise than signal.
  This is a scope reduction from the full preregistered ladder, applied
  uniformly, and disclosed here rather than silently narrowed.
- **Fixed product:variant ratio**: dataset is WANDS rows replicated via
  `scripts/datasets/replicate_wands_scale.py` at multipliers 3x/12x/24x for
  100k/500k/1M (128,982 / 515,928 / 1,031,856 rows respectively) — a
  disclosed controlled-stress-catalog limitation carried over from a prior
  Phase 6B experiment: facet cardinality is fixed, only candidate-set depth
  scales. Query latency/relevance were explicitly out of scope this round
  per the E2 preregistration.

## Repetition and validity

Every reported cell has exactly 3 independent measured runs from clean
state (native/1M's 3 runs are 3 independent failure observations, not
skipped), per the campaign's repetition rule. 20 attempts across the
campaign hit a real infra/harness problem or were superseded by a harness
fix; each was preserved in `artifacts/issue62/results/invalid/` with an
accurate reason rather than silently deleted or re-run past budget. No cell was re-run because a number
looked bad, and no outlier was discarded from a reported aggregate — see
"Disclosed measurement instability" for two cases where a real outlier was
*kept* and reported rather than excluded.

## Headline results (median across n=3 valid runs; min/max/CV shown)

### Index bytes — competitors: real on-disk bytes (`du -sb`). Native: **not comparable**, see verdict point 1; shown for reference only.

| engine | 100k median (min–max, CV%) | 500k median (min–max, CV%) | 1M median (min–max, CV%) |
|---|---|---|---|
| native (approx. on-heap estimate, not disk) | 29,911,320 (0%) | 116,915,570 (0%) | **FAILED — did not complete under 6g ceiling (3/3 runs)** |
| solr | 67,543,638 (67,538,350–67,547,716, 0.01%) | 266,756,025 (266,735,545–266,935,261, 0.03%) | 532,993,500 (532,975,424–533,012,479, 0.00%) |
| elasticsearch | 74,784,282 (74,777,016–74,819,282, 0.02%) | 296,031,530 (295,974,563–296,066,567, 0.01%) | 592,763,137 (592,695,081–592,783,319, 0.01%) |
| opensearch | 75,712,855 (75,682,931–**155,291,296**, 49.56%†) | 299,216,953 (299,018,622–**602,548,240**, 47.80%†) | 597,902,598 (597,884,664–598,096,875, 0.02%) |
| typesense | 256,130,363 (253,738,787–386,982,053, 24.31%†) | 1,438,984,018 (1,023,314,813–1,474,787,338, 14.24%†) | 2,289,875,393 (1,989,370,390–2,305,250,738, 6.35%†) |
| meilisearch | 1,246,761,094 (0.01%) | 4,339,667,078 (0.00%) | 8,143,011,974 (0.00%) |
| vespa | 160,397,773 (158,956,005–161,552,797, 0.66%) | 635,642,671 (635,134,695–636,015,359, 0.06%) | 1,269,178,297 (1,268,633,044–1,270,274,925, 0.05%) |

† = disclosed measurement instability, see below. Median is still the
correct headline number per the repetition rule; the high-CV runs are kept
in the reported min/max, not discarded.

### Warm/steady-state RSS (all under the same, now-equalized memory ceiling: 6g from 500k tier onward, 16g/8g heap at 100k for JVM engines only — see JVM confound note)

| engine | 100k median | 500k median | 1M median | note |
|---|---|---|---|---|
| native | 1,115,430,912 (1,115,287,552–1,256,308,736) | 4,597,055,488 (4,406,075,392–4,970,483,712) | **did not complete (3/3 fail under 6g)** | only engine that fails to reach 1M under the shared ceiling |
| solr | 9,323,446,272‡ | 3,855,831,040 | 4,182,867,968 | |
| elasticsearch | 9,453,408,256‡ | 4,057,591,808 | 4,368,084,992 | |
| opensearch | 9,293,221,888‡ | 4,045,074,432 | 4,354,174,976 | |
| typesense | 472,174,592 | 1,805,639,680 | 2,573,029,376 | smallest at 1M |
| meilisearch | 1,736,404,992 | 5,214,502,912 | 6,327,136,256 | |
| vespa | 3,750,785,024 | 3,985,027,072 | 4,032,118,784 | |

‡ = **JVM heap-ceiling confound, disclosed**: the 100k tier for
Solr/Elasticsearch/OpenSearch was measured under the *original*
preregistered container limits (`I62_MEMORY=16g`, `I62_JVM_HEAP=8g`); the
500k/1M tiers were measured after a mid-campaign reduction to
`I62_MEMORY=6g`/`I62_JVM_HEAP=3g` (forced by real host memory instability,
see below). JVM engines aggressively use whatever heap ceiling they are
given, so their 100k-tier RSS numbers are inflated by the larger heap
budget available at that point in the campaign, not by genuinely needing
more RSS per document at smaller scale. **This makes any RSS-vs-docs trend
fit across all three tiers for Solr/ES/OpenSearch invalid** — a naive linear
fit produces a nonsensical negative per-doc slope. Only the 500k/1M pair
(both measured under the same 6g/3g ceiling) is a like-for-like RSS
comparison for these three engines, and even that pair is heap-capped, not
a measurement of unconstrained natural RSS need. Native, Typesense,
Meilisearch, and Vespa are not JVM-heap-bounded in this way. Note that
native's own 100k measurement was, before the fix described above, taken
under an unconstrained 16g ceiling like the JVM engines' 100k tier — the
re-measured 100k number above (1,115,430,912) is already the corrected,
6g-ceiling value and is directly comparable to native's own 500k number,
unlike the JVM engines' stale 100k figures.

### Build wall time / CPU time (median, seconds) — native's 1M row is N/A (did not complete)

| engine | 100k wall/cpu | 500k wall/cpu | 1M wall/cpu |
|---|---|---|---|
| native | 29.0 / 24.8 | 100.3 / 98.3 | N/A (failed) |
| solr | 119.6 / 224.4 | 194.9 / 317.5 | 387.4 / 547.9 |
| elasticsearch | 229.0 / 218.8 | 190.3 / 294.1 | 321.4 / 411.0 |
| opensearch | 88.1 / 173.8 | 177.6 / 272.0 | 324.2 / 416.1 |
| typesense | 76.2 / 106.4 | 303.3 / 561.0 | 729.7 / 1179.0 |
| meilisearch | 221.5 / 196.9 | 1000.4 / 870.3 | 2455.5 / 2098.2 |
| vespa | 472.3 / 1112.8 | 932.4 / 2603.8 | 1553.5 / 4423.3 |

Native is fastest to build at 100k/500k by a wide margin, and its CPU-time
measurement is unaffected by #74's disclosed low-CPU-session reconciliation
issue — these are minutes-scale, large-denominator measurements, exactly
the regime #74 does not apply to.

## Derived: competitor/native multipliers (500k tier only — 1M has no valid native baseline; 100k disk multiplier not meaningful given point 1 above)

| engine | 500k rss mult (vs. native) |
|---|---|
| solr | 0.84x |
| elasticsearch | 0.88x |
| opensearch | 0.88x |
| typesense | 0.39x |
| meilisearch | 1.13x |
| vespa | 0.87x |

At 500k, under an equalized memory ceiling, native's RSS is already at or
above most competitors' (all multipliers ≤ 1.0 except Meilisearch), and
this gap **becomes a hard failure to complete at all** by the 1M tier —
the 500k-to-1M trend already points the wrong way for the "more efficient"
hypothesis on memory, and 1M confirms it decisively.

## Linear fit: native's own approximate-structure-size vs. docs (100k/500k only — not a disk fit, not extrapolated to 1M since native did not complete that tier)

| engine | fixed overhead | bytes/doc (100k→500k) |
|---|---|---|
| native | -890,690 (extrapolated intercept from only 2 points; not meaningful as a real "fixed overhead") | 224.66 |

Reported for completeness; a 2-point fit has no way to validate linearity
and should not be used for capacity planning. Competitor disk-budget
projections from the first draft (Solr/Elasticsearch/OpenSearch/Typesense/
Meilisearch/Vespa, fit across all 3 tiers, disk-only, unaffected by the
memory-ceiling confound) remain valid and are reproduced below.

| engine | fixed overhead | bytes/doc | max docs @ 6 GiB disk budget | @ 12 GiB | @ 24 GiB |
|---|---|---|---|---|---|
| solr | 946,508 | 515.55 | 12,494,476 | 24,990,789 | 49,983,414 |
| elasticsearch | 495,857 | 573.78 | 11,227,261 | 22,455,387 | 44,911,638 |
| opensearch | 1,000,081 | 578.39 | 11,136,778 | 22,275,286 | 44,552,300 |
| typesense | 87,571,178 | 2,219.91 | 2,862,669 | 5,764,787 | 11,569,022 |
| meilisearch | 315,416,544 | 7,623.72 | 803,681 | 1,648,734 | 3,338,842 |
| vespa | 2,021,422 | 1,228.05 | 5,244,431 | 10,490,508 | 20,982,661 |

Native intentionally omitted from this disk-budget table — it has no real
on-disk index to project (see verdict point 1).

### RSS linear fit + projected max docs at 6/12/24 GiB RSS budget (typesense/meilisearch/vespa — 3 clean tiers each; native omitted, only 2 non-failing tiers and a hard ceiling below 1M)

| engine | fixed overhead | bytes/doc | max docs @ 6 GiB | @ 12 GiB | @ 24 GiB |
|---|---|---|---|---|---|
| typesense | 341,780,369 | 2,281.48 | 2,674,001 | 5,497,808 | 11,145,423 |
| meilisearch | 1,672,594,460 | 4,926.31 | 968,242 | 2,276,008 | 4,891,538 |
| vespa | 3,755,141,009 | 299.69 | 8,967,004 | 30,464,146 | 73,458,430 |

Solr/Elasticsearch/OpenSearch omitted — JVM heap-ceiling confound (above).
Native omitted — no valid 1M data point and its known 500k-to-1M behavior
is "fails to complete," which a linear fit cannot represent.

## Disclosed measurement instability (not fixed mid-campaign, reported honestly)

Two independent, unexplained size-instability patterns were found and
investigated but not fully root-caused; per the repetition rule they are
reported via median + min/max/CV rather than silently rerun or
cherry-picked:

1. **OpenSearch index_bytes**: a ~2x outlier appeared on one run each at
   100k (run1: 155,291,296 vs. run2/3 ≈ 75,700,000) and 500k (run3:
   602,548,240 vs. run1/2 ≈ 299,100,000). Investigated: ruled out a silent
   forcemerge failure (the script's forcemerge call uses `curl -f` under
   `set -euo pipefail`, so a real failure would abort loudly, not
   silently produce a bad number). Not otherwise explained. 1M showed no
   such outlier (CV 0.02%).
2. **Typesense index_bytes**: consistent ~6-45% spread across every tier
   (100k CV 24.31%, 500k CV 14.24%, 1M CV 6.35% — decreasing with absolute
   scale). Root-caused: `scripts/issue62/provision_typesense.sh` measures
   `du -sb /data` immediately after indexing completes, with no
   compaction/settle step analogous to Solr's `forceMerge(1)` or Vespa's
   `triggerFlush` + poll-until-stable. Typesense is RocksDB-backed;
   background compaction timing varies run to run, so raw on-disk size
   varies with how much compaction had completed at the moment of
   measurement. **Not fixed mid-campaign** — retrofitting a settle step
   would change measurement methodology after seeing results, which the
   repetition rule prohibits. Recommended as a concrete refinement for a
   future round.

## Other harness bugs found and fixed this round (minimal-repro → root-cause → regression-test → review, per protocol)

1. **Vespa nested-JSON feed-status parser bug** (`scripts/issue62/provision_vespa.sh`).
   The post-feed success check used a non-nesting-aware regex
   (`\{[^{}]*\}`) to find "the last JSON object" in `vespa-feed-client`'s
   `--benchmark` output, which contains nested objects (e.g.
   `"http.response.code.counts": {"200": N}`). The regex matched the
   innermost nested object instead of the last top-level one, which lacks
   `feeder.error.count`, producing a false `FATAL` despite the feed having
   fully succeeded (confirmed via `feeder.ok.count` == full doc count,
   `feeder.error.count: 0`, in the very data the buggy parser misread).
   Fixed with a proper `json.JSONDecoder.raw_decode` walk over top-level
   objects only. Regression-tested against the real captured buggy
   `feed.log` content from the incident before trusting further Vespa
   measurements.
2. **Native readiness-timeout too tight for 1M-tier startup**
   (`crates/issue62-eval/src/bin/i62_measure.rs`). Native's `/ping` only
   answers after the single-threaded server finishes loading the entire
   catalog into memory (established in #61) — unlike competitors, whose
   readiness poll happens *before* indexing. The originally-fixed 120s
   `READINESS_TIMEOUT` was calibrated for 100k/500k and wrongly truncated a
   live, still-loading 1M-tier process into a false `InvalidEnvironmental`
   failure at least once — verified live via `docker stats` (100% CPU,
   growing RSS, no crash) before concluding it was a timeout-too-tight bug
   rather than a real hang. Fixed by aliasing `READINESS_TIMEOUT` to
   `BUILD_TIMEOUT`. This fix is orthogonal to the 1M-tier failures reported
   in the verdict above — those are memory-ceiling failures (`peak_build_memory_bytes:
   null`, full 3600s budget exhausted) observed after this readiness-timeout
   fix was already in place, not a recurrence of the original too-tight-timeout bug.
3. **`BUILD_TIMEOUT` recalibration (1200s → 1800s → 3600s)**. Originally
   reduced from the preregistered 3600s to 1200s after an unrelated
   ~30-minute silent OpenSearch hang (no log growth, frozen container —
   root cause never fully determined, disclosed as an open anomaly). That
   cut proved too tight for legitimately slow-but-progressing engines at
   1M (Meilisearch, Vespa — both verified via log-progress inspection, not
   silent hangs), and was restored to the originally-preregistered 3600s.

## Self-inflicted methodological incident, disclosed

While backfilling invalidated cells, a manual Meilisearch/1m backfill was
briefly run **concurrently** with the main campaign's Vespa/1m attempt,
violating the preregistered one-container-at-a-time sequential resource
isolation rule (confirmed via `docker ps` showing both containers up
simultaneously and host load average 19.6-27.7 on a 4-vCPU host at that
moment). Corrected immediately: the concurrent backfill was stopped, its
orphaned container removed, and Vespa/1m was re-measured alone. A related
tooling hazard was also found and fixed: an early backfill used a bounded
(non-persistent) 1-hour `Monitor` wrapping a multi-run loop; when the cap
fired mid-run it killed the orchestrating process but left the underlying
Docker container running orphaned and unmeasured. Subsequent backfills used
a `persistent` monitor with no timeout cap.

## Host instability (disclosed, not this experiment's defect)

This shared host's available memory was independently observed to drop
from an initial ~26-30 GiB to ~13-15 GiB more than once during this
campaign, unrelated to this experiment's own container usage, with high
concurrent load average (19-29 on 4 vCPUs) observed at the same times.
This forced the `container_limits.env` memory/heap reduction described
above (contributing to the JVM RSS confound, and to native's genuine 1M
failure once equally constrained) and is the primary reason 3M/5M tiers
were not attempted this round.

## Summary table for #60

| dimension | verdict | headline evidence |
|---|---|---|
| Disk footprint vs. competitors | **NOT COMPARABLE** | native persists nothing to disk in this build; its reported number is a partial on-heap estimate, not a disk measurement (adversarial-review finding) |
| Build speed (100k/500k) | **SUPPORTED** | native fastest wall/CPU build time at 100k and 500k by a wide margin |
| Steady-state RSS at equalized memory ceiling | **REFUTED** | native is the only one of 7 engines that fails to complete the 1M tier under the same 6g ceiling every competitor was held to; already RSS-equal-or-worse-than-most at 500k |
| Tier ceiling reached | 500k (native); 1M (all 6 competitors) | native's own measured ceiling, under a corrected/equalized harness, not a host-instability artifact |
