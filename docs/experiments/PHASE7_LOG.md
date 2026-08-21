# Phase 7 Experiment Log — Issue #21 Phase 7: Multi-Tenant SMB/Mid-Market Serving Economics

## Governing context

Phases 0–6B established a single-tenant, single-process physical result:
commerce-native structural execution is often orders of magnitude
cheaper than a generic engine for filter/pagination/concurrency, with
real, measured, operator-specific breakpoints for facet/sort/numeric-
range operators. None of that answers Issue #21's actual product
question, stated in the epic's own research question:

> Can a commerce-native forwarding/execution plane materially improve
> the economics and predictability of multi-tenant SMB/mid-market
> commerce retrieval under heterogeneous steady-state traffic... while
> preserving mature search relevance through transparent backend
> delegation?

Phase 7's goal (Issue #21): test whether commerce specialization reduces
per-tenant fixed cost and increases safe tenant packing density, without
sacrificing per-tenant latency predictability or isolation. Every prior
phase measured one catalog at a time in one process; Phase 7 is the
first phase in this project's history to build and measure more than one
tenant's index in the same process at once.

## Why this is unblocked (unlike Retailrocket/H&M/Amazon Reviews 2023/Havenask)

Phase 7 does not require any new external dataset or engine. It requires
a *multi-tenant workload shape* layered over catalogs already acquired
and validated (WANDS, real, checksummed, already in this repository's
`dataset_cache/`). Issue #21 explicitly permits this: "Public/synthetic
workloads must remain reproducible and clearly labeled" when
production-derived tenant distributions are unavailable (they are,
here — this project has no real multi-tenant SaaS traffic to draw on).

## Tenant model for the first pass (P7-E00)

Rather than fabricate arbitrary tenant boundaries over one catalog, this
experiment uses WANDS' own real category structure: each of the 55 real,
distinct `category_depth_1` values (Rugs, Lighting, Furniture, ...)
becomes one independent tenant's full catalog. This is not an arbitrary
partition — it is a realistic model of one real SMB pattern (a
specialty retailer whose whole catalog is one category vertical), and
it inherits a REAL, non-fabricated heterogeneous size distribution
directly from Phase 6A/6B's own findings (2,002 to 16,039 products per
depth-1 node, a genuine long-tail shape, not something invented for this
experiment).

Ceiling: 55 tenants using this real partition. Tenant counts beyond 55
would require either finer real partitioning (depth-2/depth-3, more
nodes but much smaller each) or controlled-stress replication (Phase
6B's disclosed methodology) — named as a follow-up if the real-partition
ceiling turns out to bound the finding before a breakpoint is found.

## Falsifiable hypotheses (stated before implementation)

**H1 (packing density / fixed-cost amortization)**: per-tenant memory
overhead is NOT constant — a fixed per-process cost (binary, allocator
arenas, shared runtime state) amortizes across tenants, so marginal
RSS-per-tenant should *decrease* as tenant count grows from 1 toward 55.
**Pass/fail**: compare RSS-per-tenant at N=1 vs N=55; a decrease of any
material size (>10%) supports amortization; a flat or increasing curve
falsifies it for this architecture.

**H2 (tenant isolation / no noisy-neighbor interference)**: one tenant
issuing a sustained heavy query load should not measurably degrade
another, unrelated tenant's own query latency, since each tenant's
`CatalogIndex` is a structurally independent, immutable, per-tenant data
structure with no shared mutable state between tenants in the current
architecture. **Pass/fail**: measure a quiet tenant's p50/p99 latency
alone vs. while a different tenant is under sustained concurrent load; a
material p99 regression (>2x) falsifies isolation; no material change
supports it.

**H3 (packing ceiling)**: there exists some tenant count N (bounded by
this container's ~15GB RAM budget) beyond which the packing stops being
cheap — record the tested bound honestly rather than extrapolating past
it, per this project's standing discipline.

## Measurement plan (defined before running)

For tenant counts N in {1, 5, 10, 25, 55} (real depth-1 partitions,
largest-catalogs-first so N=1 is the single largest real tenant,
"Furniture," and each successive N adds the next-largest remaining real
category):

- build all N tenants' `CatalogIndex` in one process, recording total
  and marginal RSS and build time;
- run a fixed-QPS-per-tenant concurrent workload against all N tenants
  simultaneously (structural filter queries, reusing the depth-1
  category-membership query already validated in Phase 6A/6B), recording
  aggregate and per-tenant p50/p99;
- run the H2 isolation check: hold N-1 tenants idle, drive one tenant at
  a sustained high QPS, measure a different (quiet) tenant's own p50/p99
  in isolation vs. concurrently with the loaded tenant.

No new commerce_core mechanism is introduced for this first pass — each
tenant is a plain, independent `CatalogIndex` over its own `Catalog`,
reusing existing physical indexes unchanged. This deliberately tests the
current architecture's OUT-OF-THE-BOX multi-tenant packing behavior
before considering any tenant-aware optimization.

## First-draft results and self-caught interpretation issue

The first implementation measured only RSS, baseline captured BEFORE
partitioning (bundling the one-time whole-catalog parse into N=1's
number), largest-tenant-first only, one run. The naive
`cumulative_marginal_rss / N` column trivially decreased (139,680 ->
2,731.9 KB) as N grew — but per-step decomposition showed this was
dominated by WANDS' long-tail shape (cumulative products barely grow
past N=10), not obviously fixed-cost amortization. The first draft
tentatively read the per-step KB/tenant-shrinking-while-KB/product-
growing pattern as "a real per-tenant fixed cost (~27-590 KB)."

## Adversarial review found this was not resolved, and possibly wrong

A 3-lens adversarial review (confound analysis, statistical rigor,
honesty/scope) found: (1) a single, untrialed RSS snapshot series cannot
distinguish real per-tenant cost from allocator/page-granularity noise;
(2) tenants were always built largest-first, so every "per-tenant cost"
estimate was confounded with build ORDER (the exact confound Phase 6B's
reversed-order check existed to rule out, not run here); (3)
`CatalogIndex::approximate_size_bytes()` — a deterministic, allocator-
noise-free instrument already implemented in commerce_core — was never
used, despite being the cheapest, highest-value check available; (4) H2
had no same-tenant control, so it could not distinguish "tenant boundary
isolation" from "generic CPU/memory contention"; (5) leftover state
(H1's `partitions`, 53 of H2's built-but-unused tenants) stayed
needlessly resident during H2's timing window.

## Fixes applied, then re-measured (not just re-worded)

`crates/phase7-eval/src/tenants.rs` and `p7_e00_tenant_packing.rs` were
revised: RSS baseline now captured AFTER partitioning; each tenant's
`approximate_size_bytes()` recorded alongside RSS; an `order` CLI arg
(`forward`/`reversed`) added; H1's intermediate state explicitly dropped
before H2; H2 now includes a same-tenant control condition; and — found
while making these fixes — tenants now get independently-interned
`CategoryId`/`ProductTypeId`/`BrandId` spaces (raw records are grouped
by `category_depth_1` BEFORE `catalog_ingest::build_catalog` runs,
instead of after), matching how real independent tenants would each
bootstrap their own schema rather than sharing one canonicalized ID
space from a single whole-catalog ingestion pass.

The revised ladder was then run 3x forward and 3x reversed (order
control) plus 3x total for H2 (same/cross-tenant conditions each run).
Raw: `docs/research/artifacts/p7_e00_tenant_packing_run1/{forward,reversed}_run{1,2,3}.log`,
`h1_forward_run{2,3}.csv`, `h1_reversed_run{1,2,3}.csv`, `h2_isolation.csv`.

## Corrected H1 result: the original "per-tenant fixed cost" claim is FALSIFIED

The deterministic `approximate_size_bytes()` metric (order-invariant by
construction — confirmed identical, 9,843,378 bytes at N=55, in both
forward and reversed runs) shows per-tenant fixed cost is genuinely
small: a single-product tenant costs 1,292 bytes total. But the decisive
result is the **reversed-order (smallest-tenant-first) RSS run**:

| N (reversed order) | tenant added | products | cumulative RSS marginal (KB) |
|---|---|---|---|
| 1 | Bath Rugs & Mats | 1 | 68-108 (run-to-run) |
| 5 | Hooks | 1 | 68-108 (UNCHANGED from N=1) |
| 10 | Physical Education Equipment | 1 | 68-108 (UNCHANGED) |
| 25 | Early Education | 3 | 68-108 (UNCHANGED) |
| 55 | Furniture (the single large tenant, built LAST) | 16,039 | 37,432-37,476 |

Building 54 near-empty tenants first costs **essentially zero**
additional RSS (flat at 68-108 KB total across all of N=1 through N=25);
essentially all real memory cost appears the moment the ONE large tenant
(Furniture) is built, regardless of whether it is built first (forward
order) or last (reversed order). This directly contradicts the
forward-order run's apparent "27-590 KB per-tenant cost in the tail":
that pattern was an **allocator/build-order artifact** — building the
large tenant first inflates the process's heap/arena footprint
immediately, and that inflated baseline does not shrink for later tiny
tenants, making them look like they "cost" something when the
deterministic metric and the reversed-order control both show they cost
close to nothing. Cross-checking magnitudes confirms this: in the
forward-order N=10->N=55 range, RSS grew ~7,644 KB while
`approximate_size_bytes()` grew only ~718 KB for the identical set of
added tenants — a ~10.6x disproportion only explicable by allocator/page
effects, not real structural cost.

**Corrected finding**: in this architecture, per-tenant fixed memory
overhead is negligible; total memory cost is overwhelmingly driven by
aggregate PRODUCT COUNT, not tenant COUNT. This answers Issue #21's
"idle/low-QPS tenant fixed cost" metric more directly and more
favorably than the first draft's hedged "27-590 KB" estimate: an
idle/near-empty tenant costs close to nothing to keep resident.
Cross-run reproducibility was tight for both forward (RSS within ~0.3%
across 3 runs at every checkpoint) and reversed (RSS within the
tiny-tenant range consistently flat across 3 runs) orders, so this is
not a one-off result.

**Named limitation, not fixed this pass**: this experiment measures
memory only; it says nothing about per-tenant CPU/connection/process
overhead in a real multi-process or multi-container deployment model,
which could have a materially different (larger) fixed cost than the
in-process `CatalogIndex` construction measured here.

## Corrected H2 result: PASS, robust across 3 runs, with a small consistent same-vs-cross difference

| run | cross-tenant p99 ratio | same-tenant-control p99 ratio |
|---|---|---|
| 1 | 1.31x | 0.85x |
| 2 | 1.43x | 1.15x |
| 3 | 1.34x | 1.16x |

All 6 ratios (3 runs x 2 conditions) stay well below the pre-registered
2.0x material-regression threshold — **H2 passes robustly across
repeated runs**, not just a single lucky draw. The cross-tenant ratio is
consistently, if modestly, higher than the same-tenant-control ratio in
all 3 runs (mean 1.36x vs 1.05x) — a real, reproducible, but small
difference, not dramatic enough to claim a specific tenant-boundary
mechanism with confidence. Named as an open question (a plausible
unproven hypothesis: accessing a different tenant's data may cost
slightly more in cache locality than repeatedly touching the same
tenant's already-warm data even under contention) rather than asserted.
**The core claim — no material p99 regression from an unrelated
tenant's sustained heavy concurrent load — holds robustly.**

## H3 (packing ceiling): not tested this pass

Stated as a falsifiable hypothesis before implementation but never
actually exercised: the binary hard-stops at N=55 (WANDS' real
`category_depth_1` cardinality) at ~50 MB peak marginal RSS, nowhere
near this container's ~15 GB budget. The real-category partition simply
ran out of distinct categories before any resource ceiling was
approached. Recorded as explicitly untested rather than silently
dropped — a genuine packing-ceiling test needs either finer real
partitioning (depth-2/depth-3, more but smaller real categories) or
controlled-stress replication (Phase 6B's disclosed methodology, applied
to tenant count rather than catalog size) as a named follow-up.

## P7-E01: QPS-scaling across tenant count (falsifiable hypothesis, stated before implementation)

P7-E00 tested memory (H1) and two-tenant isolation (H2), but not Issue
#21's explicit Phase 7 metric "tenants/node at fixed latency SLO" —
sustained concurrent query throughput spread across MANY tenants at
once, not just two. Every prior concurrency measurement in this project
(Phase 5's `p5e03_concurrency_sweep`, Phase 6A's `p6a_e01_concurrency_sweep`)
hammered a SINGLE shared catalog from multiple threads; P7-E01 asks a
genuinely new question: does spreading concurrent query load across many
DISTINCT tenants' independent data structures (poor cross-tenant cache
locality — worker threads jump between totally different catalogs each
query, unlike hammering one warm catalog repeatedly) degrade aggregate
throughput compared to a single-tenant baseline, at a FIXED worker count
(4, matching this container's real CPU count)?

**H4 (tenant-count-invariant throughput)**: at a fixed worker count,
aggregate QPS and p99 latency do not degrade materially as tenant count N
grows from 1 to 55 with query load spread uniformly across all N
tenants — since each query still touches only one tenant's independent,
unshared `CatalogIndex`, this predicts throughput is governed by CPU
parallelism, not by how many distinct tenants' data is resident/touched.

**Pass/fail defined in advance**: compare aggregate QPS at N=1 (single
tenant, all load on it) against aggregate QPS at N=55 (same total worker
count, load spread across all 55 tenants) at fixed 4-worker concurrency.
A material aggregate QPS drop (>20%) or p99 growth (>2x) as N grows
falsifies H4 and reveals a real cross-tenant data-locality cost; flat
throughput within that margin supports it.

## P7-E01 self-caught confound (first draft, before any repeat run)

The first implementation measured aggregate throughput while spreading
ALL query load uniformly at random across all N tenants (matching the
originally-stated plan). Result: throughput appeared to grow ~19.8x from
N=1 (238.5 rps) to N=55 (4,719.2 rps). Before treating this as a finding,
inspection found an obvious confound: `load_depth1_tenants` orders
tenants largest-first, so as N grows the tenant population increasingly
includes WANDS' many near-empty long-tail categories; with UNIFORM
random tenant selection, most query volume shifts onto these
progressively cheaper (near-empty) tenants as N grows, so the "increase"
mostly reflects average per-query cost falling, not a real tenant-count
effect. This is a workload-mix artifact, not a scaling result, and was
corrected before promotion.

## P7-E01 corrected design and result

Redesigned: one dedicated worker repeatedly queries a FIXED tenant
("Rugs", matching H2's own checkpoint), holding its own query completely
constant; the remaining 3 workers continuously round-robin through the
`n-1` OTHER tenants (not randomly weighted, guaranteeing genuine coverage
of all of them). `n` (the number of other tenants concurrently touched)
is the only thing that varies; Rugs' own workload is fixed throughout, so
any change in ITS throughput/latency isolates the effect of touching
more distinct tenants, not workload-mix drift.

Run 3x independently (`docs/research/artifacts/p7_e01_qps_scaling_run1/results_run{1,2,3}.csv`):

| n (other tenants touched) | run1 rps | run2 rps | run3 rps | run1 p99 (ms) | run2 p99 (ms) | run3 p99 (ms) |
|---|---|---|---|---|---|---|
| 1 | 789.2 | 728.2 | 719.0 | 1.85 | 1.83 | 1.96 |
| 4 | 776.0 | 719.5 | 784.5 | 2.06 | 1.89 | 1.94 |
| 9 | 794.5 | 736.2 | 795.8 | 1.85 | 1.82 | 1.82 |
| 24 | 784.0 | 808.8 | 786.0 | 1.95 | 1.91 | 1.94 |
| 54 | 816.2 | 794.0 | 694.0 | 1.71 | 2.02 | 2.16 |

Quiet tenant throughput stays in a 694-816 rps band across all 15
measurements (3 runs x 5 breadth levels) with no monotonic trend as
breadth grows from 1 to 54 — variation across the whole table (~15%
peak-to-trough) is consistent with ordinary run-to-run noise, not a
breadth-dependent effect. p99 latency likewise shows no trend (1.71-2.16
ms band throughout).

**H4 CONFIRMED, robust across 3 independent runs**: touching many
distinct tenants' data concurrently (up to WANDS' real ceiling of 54
other tenants) does not measurably degrade a fixed tenant's own query
throughput or p99 latency, well within the pre-registered pass margins
(20% aggregate/2x p99). Consistent with the architecture's per-tenant
`CatalogIndex` instances being fully independent, immutable structures
with no shared mutable state to contend over.

**Named limitation**: only tested up to WANDS' real 54-other-tenant
ceiling (the same real-category-cardinality bound H3 already named);
whether this holds at materially larger tenant counts (hundreds to
thousands, the scale Issue #21's Phase 7 ultimately asks about) is
untested and would need the same finer-partition or controlled-stress
extension named for H3.

## P7-E02: packing ceiling via controlled-stress tenant-count replication (H5, stated before implementation)

H3 was named as a hypothesis in P7-E00 but never tested: the real
`category_depth_1` partition tops out at 55 tenants at ~50 MB RSS,
nowhere near this container's ~15 GB budget (currently ~10 GB
available, Solr's JVM already resident at ~4.8 GB RSS). P7-E02 uses the
same controlled-stress replication discipline Phase 6B established for
catalog SIZE, applied instead to TENANT COUNT: the real 55-tenant
population is replicated K times, with each copy's tenant names
suffixed (e.g. `Rugs-copy3`) to keep them distinct, to reach tenant
counts in the hundreds to thousands. **This is explicitly not a claim
about organic tenant growth** — real independent SaaS tenants would not
be K copies of the same 55 real categories; this isolates tenant COUNT
as a variable, holding per-tenant data/schema shape fixed, the same
disclosure discipline `replicate_wands_scale.py` used for catalog size.

**H5**: given H1's finding that per-tenant fixed memory overhead is
negligible and total memory cost tracks aggregate PRODUCT count, RSS
should scale roughly linearly with total product count as the 55-tenant
population is replicated K times (i.e., with K itself, since each
replica carries the same real product count) — not degrade faster than
linear as raw tenant COUNT grows into the hundreds/thousands.

**Pass/fail defined in advance**: track RSS per replication step; if it
stays within roughly linear bounds of K (say, within 2x of the linear
prediction) up to a safety-capped ceiling (this process's own RSS kept
below ~6 GB, leaving headroom under the container's real ~15 GB budget
so a real OOM is never risked), H5 holds and the achieved tenant count
is recorded as a real, non-extrapolated tested bound — not evidence
about tenant counts beyond it, per this project's "record the tested
bound rather than extrapolating" discipline. If RSS grows materially
faster than linear before the safety cap, H5 is falsified and that
becomes the reported (still tested, not extrapolated) ceiling.

## P7-E02 result: H5 CONFIRMED, cleanly linear all the way to the safety cap

Run twice independently (`docs/research/artifacts/p7_e02_packing_ceiling_run1/results_run{1,2}.csv`),
reaching the same point both times: **6,500 tenants (118x the real
55-tenant population), 4,929,348 total products**, before the 6 GB
self-imposed safety cap was reached (RSS ~6,223 MB both runs, within
0.01% of each other).

| tenants | products | RSS (KB) | KB/product |
|---|---|---|---|
| 100 | 82,866 | 104,448 | 1.260 |
| 1,000 | 785,548 | 991,060 | 1.262 |
| 3,000 | 2,279,055 | 2,877,140 | 1.263 |
| 6,000 | 4,547,648 | 5,752,436 | 1.265 |
| 6,500 | 4,929,348 | 6,223,372 | 1.263 |

KB/product stays at 1.260-1.265 across the entire range — a ~0.4%
spread over a 65x range in tenant count and 59x range in product count.
This is essentially perfectly linear, with no sign whatsoever of
super-linear degradation as tenant count grows into the thousands.
**H5 CONFIRMED**: packing cost in this architecture is governed by total
product count, not tenant count, holding cleanly from WANDS' real
55-tenant scale all the way to 6,500 tenants.

**What this is and is not evidence of**: 6,500 tenants / ~50 MB-per-1000-
tenants-worth-of-real-data-volume is a **self-imposed safety-capped
bound**, chosen to avoid risking a real OOM in this shared container (15
GB total, ~10 GB available at the start of this experiment, Solr's own
JVM already resident at ~4.8 GB) — it is NOT a discovered architectural
or hardware ceiling. The real ceiling on this hardware, or on a
dedicated/larger machine, is very likely materially higher; this
experiment deliberately stopped short of finding it, per this project's
"record the tested bound rather than extrapolating" discipline, rather
than push toward an actual OOM in a shared, multi-purpose container.
Build time also scaled close to linearly (a mild super-linear component:
~66x build-time growth for a 65x tenant-count increase), consistent with
the same mild super-linear build-cost signal P7-E00/H1 already found and
attributed to real per-item construction cost, not to tenant count
specifically.

H3 (the original packing-ceiling hypothesis from P7-E00) is now answered
by proxy: no non-linear packing-cost wall was found anywhere between the
real 55-tenant scale and this experiment's self-imposed 6,500-tenant
safety bound.
