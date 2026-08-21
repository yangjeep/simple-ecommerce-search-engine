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

## P7-E03: cross-process fixed cost (H6, stated before implementation)

Every prior Phase 7 measurement (H1/H2/H4/H5) is IN-PROCESS: N tenants
packed into one running process. Every "unresolved risk" note in
`PHASE7_DECISION.md` names the same gap: a real multi-tenant deployment
might instead give each tenant its own OS process/container for fault
isolation, which would pay a per-process baseline (binary + runtime + OS
bookkeeping) that pooling avoids. This is also the first Phase 7
experiment to test `docs/WHY.md`'s own opening thesis with real numbers
rather than assumption: "pooled infrastructure can serve many tenants
far more cheaply... because idle capacity for one tenant absorbs a
burst in another" (statistical multiplexing) implicitly assumes pooling
avoids a real per-tenant fixed cost that isolation would pay — H1/H5
already showed that cost is negligible IN-PROCESS; H6 asks whether it is
also negligible ACROSS processes.

**H6**: a real, measurable per-OS-process baseline overhead exists
(Rust binary + runtime + OS process bookkeeping, independent of any
tenant data) that a one-process-per-tenant deployment model pays once
PER TENANT, while H1/H5's pooled multi-tenant process pays it once
TOTAL for the whole node. This baseline is expected to be non-negligible
relative to H1/H5's near-zero in-process marginal per-tenant cost
(~1.26 KB/product, ~0 KB fixed per near-empty tenant) — i.e., process-
per-tenant isolation should show a real, quantifiable cost that pooling
avoids, unlike the in-process finding.

**Pass/fail defined in advance**: spawn N fresh child OS processes (via
`std::process::Command`, the same compiled binary each time) that each
report their own RSS immediately at process start, before touching any
tenant data or commerce_core code at all — this isolates the pure
process/runtime baseline from any tenant-data cost. Compare that
baseline against H1/H5's established in-process marginal cost. If the
per-process baseline is orders of magnitude larger than the in-process
marginal per-tenant cost, H6 is CONFIRMED: pooling has a real, measured
economic advantage over process-per-tenant isolation. If the baseline
turns out to be comparable to or smaller than the in-process marginal
cost, H6 is FALSIFIED: process-per-tenant isolation would carry no
material fixed-cost penalty in this specific runtime/OS environment.

## P7-E03: two self-caught implementation bugs, found before trusting the first numbers

**Bug 1**: the first implementation's child-mode tenant loader called
`partition_depth1(catalog_path, 55, Order::LargestFirst)` and then
`.into_iter().find(|(n,_)| n == name)` to select one tenant. This looks
like it should only build the target tenant, but `partition_depth1`
returns an already-fully-materialized `Vec` of all 55 built `Catalog`s
-- `.find()` over an already-built `Vec` doesn't avoid constructing the
other 54, it just discards them after the fact. Result: every "single
tenant" child process reported ~100 MB regardless of whether the
requested tenant had 1 product or 16,039 -- all of them were actually
paying the cost of building all 55 tenants. Caught immediately by
comparing the three tenant sizes' reported numbers (all suspiciously
identical) before writing up any result. Fixed with a dedicated
`load_single_tenant` helper that filters raw records to the ONE target
tenant BEFORE calling `build_catalog`.

**Bug 2** (found immediately after fixing bug 1, from the still-odd
numbers that followed): even after fixing bug 1, EVERY tenant --
including the genuinely 1-product "Water Filter Pitchers" -- still
reported ~37-39 MB, an obvious remaining anomaly for what should be a
near-empty catalog per H1/H5's own finding. Root cause: `data::load_catalog`
always parses the ENTIRE shared 42,994-product `catalog.jsonl` file
before any per-tenant filtering happens, so even a 1-product tenant's
child process was paying the cost of parsing every other tenant's raw
data too -- a real, second confound with the same shape as bug 1 (doing
much more work than the one tenant actually needs), just one level
lower in the pipeline. Fixed by writing a genuinely single-tenant JSONL
file (only that tenant's raw lines, filtered from the shared file) and
pointing each child at ITS OWN small file, matching how a real
single-tenant deployment would actually be provisioned (its own data,
not a shared multi-tenant file it has to filter itself).

Both bugs were caught and fixed before any external adversarial review
was needed, using the same "does this number make sense given what I
already know" discipline established across every other Phase 6B/Phase
7 self-correction this session.

## P7-E03 result (after both fixes): H6 CONFIRMED, reproduced across 3 independent runs

| condition | run1 (KB) | run2 (KB) | run3 (KB) |
|---|---|---|---|
| bare process baseline (mean of 20 children, no tenant data) | 2,148.0 | 2,147.6 | 2,151.6 |
| Furniture (16,039 products) marginal | 56,532* / 50,964 | 50,960 | 50,964 |
| Faux Plants and Trees (5 products) marginal | 37,072* / 176 | 176 | 220 |
| Water Filter Pitchers (1 product) marginal | 37,088* / 184 | 216 | 164 |

(*first two columns show the bug-1-only and bug-1-and-2-fixed numbers
from the same initial debugging session, both superseded by run1's final
corrected figure shown after the slash; run2/run3 are independent
process runs of the fully-fixed binary.)

The bare per-process baseline (~2,148-2,152 KB, essentially identical
across all 3 runs) is **the dominant term**: it alone equals the
in-process pooled marginal cost of ~1,700 products' worth of tenant
data (using H1/H5's ~1.263 KB/product figure) — larger than the entire
marginal cost of either near-empty tenant (164-220 KB) and comparable to
a meaningful fraction of even the largest real tenant's own marginal
cost (~51 MB for 16,039 products).

**H6 CONFIRMED, reproduced across 3 independent runs**: a real,
consistent per-OS-process baseline (~2.1-2.2 MB, essentially unchanged
run to run) exists that a one-process-per-tenant deployment model would
pay once PER TENANT, while H1/H5's pooled in-process design pays it
once TOTAL for the whole node. For a hypothetical 1,000-tenant SMB
deployment, process-per-tenant isolation would pay this baseline
~1,000 times (~2.1-2.2 GB just in per-process overhead) versus once for
a pooled design — this is the first Phase 7 (or project-wide) evidence
directly supporting `docs/WHY.md`'s opening "statistical multiplexing"
thesis with real numbers rather than an assumed advantage. Small,
near-empty tenants ALSO show a modest but real per-process cost (~150-220
KB beyond the bare baseline) that doesn't appear at all in the pooled
in-process measurement (H1/H5 found ~0 KB fixed cost for near-empty
tenants there) -- plausibly the cost of spinning up JSON-parsing/ingestion
machinery fresh per process rather than sharing it across a pool,
though this specific sub-mechanism was not further isolated.

**Named limitations**: this uses `.output()`-based process spawn/exit
(short-lived child processes), not a genuinely long-running resident
server process -- real per-process overhead for a long-lived service
(additional runtime warm-up, connection handling, logging/metrics
infrastructure, etc.) is likely higher than this floor-level measurement
captures. Only 3 real tenant sizes were sampled (largest, middle,
smallest by real WANDS distribution), not a full sweep. This measures
memory only, not per-process CPU/scheduling overhead, which a real
multi-tenant capacity model would also need.

## P7-E04: long-running resident-process overhead (H7, stated before implementation)

H6 closed the in-process-vs-cross-process gap, but every unresolved-risk
note since P7-E03 named the same remaining gap: H6's child processes are
SHORT-LIVED -- spawn, (optionally) load one tenant, print RSS, exit, all
in well under a second. A real deployed service does not exit
immediately: it stays resident, keeps a worker/connection-handler thread
pool alive, and serves a sustained query stream. P7-E04 tests whether
that distinction actually matters for RSS.

**H7**: a genuinely long-running resident process's RSS, measured WHILE
it is still alive and actively behaving like a service, is materially
higher than H6's immediate spawn-and-exit snapshot -- because a real
service keeps worker threads resident (thread stacks, per-thread
allocator arenas) and performs sustained allocation/deallocation churn
serving real queries, neither of which a process that exits in
milliseconds ever exercises.

**Pass/fail defined in advance**: two resident conditions, mirroring H6's
bare/tenant split, each held alive for a fixed `RUN_DURATION` (20
seconds) with RSS sampled every 5 seconds:
- **idle-resident**: no tenant data; `WORKER_THREADS` (4, matching this
  container's real CPU count) real OS threads spawned and parked alive
  for the whole window -- a resident but idle connection-handler pool,
  something H6's children never allocated since they exited before any
  pool would exist.
- **active-resident**: one real tenant's data loaded (largest/mid/
  smallest, same three tenants H6 used for direct comparability);
  `WORKER_THREADS` threads continuously execute real structural queries
  (the same `facet_scan_once` helper P7-E01 used) against it for the
  whole window.

Compare the PEAK of the periodic RSS samples taken while the process is
still live and serving against the immediate post-load snapshot
(matching H6's `with_tenant_rss_kb` methodology exactly). A growth of
>=20% for the idle condition, or >=200 KB for any tenant condition (the
same order of magnitude as H6's own smallest-tenant marginal cost, a
principled anchor rather than an arbitrary number) is material and
CONFIRMS H7; growth below that stays close to H6's existing floor and
FALSIFIES H7 for this workload.

## P7-E04 self-caught methodology issue: the wrong RSS reading was almost used as the primary metric

The first-draft binary reported a "steady_state_rss_kb" taken AFTER the
worker threads had already been `.join()`-ed (torn down) at the end of
the window, and used THAT as the primary comparison against the
immediate post-load snapshot. Running it once produced a deeply
counterintuitive result: the largest tenant (Furniture, 16,039 products)
showed a NEGATIVE "growth" of roughly -472 to -480 KB (steady-state
LOWER than the immediate snapshot), while the two near-empty tenants
each showed +324 KB despite serving ~80-200 million trivial queries in
the same window.

Inspecting the newly-added per-sample printout (added specifically so
the growth curve's shape, not just its two endpoints, would be
auditable) resolved this immediately: the samples taken WHILE the
process was still actively serving showed a real, monotonically
increasing-then-decelerating climb (e.g. Furniture: 53,976 -> 54,008 ->
54,032 -> 54,044 KB across the 20-second window, still rising, not yet
fully plateaued) that was **materially higher** than either the
immediate post-load snapshot OR the "steady-state" reading taken after
thread teardown. The post-join reading was actively misleading: joining
the worker threads reclaims real memory (thread stacks, and very likely
per-thread allocator arena high-water marks built up from the sustained
alloc/dealloc churn of millions of `facet_scan_once` calls), so for a
tenant whose worker threads did substantial real work (Furniture), the
post-teardown number dropped BELOW even the immediate snapshot -- hiding
a real, large, in-service-only increase (~900 KB peak growth) rather than
revealing it. A real long-running service does not tear down its worker
pool mid-life, so a metric that only looks correct after teardown was
measuring the wrong thing entirely.

**Fixed** (before trusting any number, using the same "does this number
make sense" discipline as every other Phase 6B/Phase 7 self-correction):
the binary now tracks and reports the PEAK of the periodic in-service
samples as the primary H7 metric, and keeps the post-teardown reading
only as a clearly-labeled secondary data point about shutdown behavior,
explicitly excluded from the H7 verdict.

## P7-E04 result (after the fix): H7 CONFIRMED, reproduced across 3 independent runs

| condition | run1 (KB) | run2 (KB) | run3 (KB) |
|---|---|---|---|
| idle-resident: t0 mean | 2186.7 | 2186.7 | 2188.0 |
| idle-resident: peak mean | 2430.7 | 2430.7 | 2432.0 |
| idle-resident: peak growth | 244.0 | 244.0 | 244.0 |
| idle-resident: post-teardown mean (secondary) | 2558.7 | 2558.7 | 2560.0 |
| Furniture (16,039 products): with-tenant snapshot | 53,148 | 53,212 | 53,132 |
| Furniture: peak-serving | 54,044 | 54,108 | 54,032 |
| Furniture: peak growth | 896 | 896 | 900 |
| Furniture: post-teardown (secondary) | 52,668 | 52,732 | 52,656 |
| Faux Plants and Trees (5 products): peak growth | 196 | 196 | 196 |
| Water Filter Pitchers (1 product): peak growth | 196 | 196 | 196 |

Every one of these figures is essentially exact across all 3 independent
process runs -- the idle-resident peak growth is identically 244.0 KB in
all 3 runs, and both near-empty tenants show identically 196 KB peak
growth in every run. The idle-resident growth (244 KB, 11.2% of its own
t0 baseline) stays below the pre-registered 20% idle threshold on its
own, but the active-resident tenant growth clears the 200 KB threshold
decisively for the realistic case (Furniture: 896-900 KB, ~1.7% of its
~53 MB total footprint but ~42% of H6's entire per-process baseline
~2,148-2,152 KB) -- **H7 CONFIRMED** via the tenant-growth criterion,
reproduced across all 3 runs.

The per-sample trace shows two qualitatively different shapes: the
near-empty tenants' RSS jumps once (within the first 5-second sample)
and then stays perfectly flat for the rest of the window, while
Furniture's RSS keeps climbing, decelerating but not fully plateaued,
across the entire 20-second window -- consistent with real, ongoing
allocator/arena effects driven by the VOLUME of real work being churned
(Furniture's worker threads allocate and free real per-query
candidate/facet-count structures over its full 16,039-product index;
the near-empty tenants' equivalent structures are almost too small to
matter, so their growth saturates almost immediately).

**Named limitation, not resolved this pass**: the specific allocator
mechanism behind this growth (thread-local arena high-water marks from
sustained alloc/dealloc churn is the most likely candidate, given the
shape and the fact that it partially reverses on thread teardown) is a
plausible, disclosed hypothesis, not a profiled and confirmed one --
matching how H2's small cross-vs-same-tenant latency gap was handled.
The post-teardown secondary reading shows its own small but exactly
reproducible curiosity (a consistent +128 KB bump above the flat
plateau for idle/near-empty-tenant conditions in every single run,
across every child), also named as an open, unconfirmed observation
rather than asserted. Only a 20-second window at 4 worker threads was
tested; whether the still-rising Furniture curve would continue growing
materially further over minutes/hours of real sustained service, or
plateau shortly past this window, is untested.

**What this means for the economic model**: H6 established a real
per-process baseline (~2,148-2,152 KB) that isolation pays once per
tenant while pooling pays once total. H7 shows that baseline
UNDERSTATES a genuinely long-running, actively-serving process's real
footprint by a further, real, reproducible amount -- negligible for
near-empty tenants (+196 KB) but substantial for tenants doing real
sustained work (+896-900 KB for the largest real tenant, on top of its
own ~51 MB in-process data cost). This strengthens, not weakens, H6's
qualitative conclusion (pooling has a real cost advantage over
process-per-tenant isolation): the true per-process cost a
one-process-per-tenant deployment would pay is higher than H6's
short-lived floor alone suggested.

## P7-E05: extended-duration resident overhead (H8, stated before implementation)

H7's per-sample trace showed Furniture's (the largest real tenant)
in-service RSS still climbing, decelerating but not fully plateaued, at
the end of its 20-second window. Before treating H7's ~896-900 KB figure
as a stable number, P7-E05 asks the obvious next question: does that
curve actually plateau given a much longer window, or does it keep
climbing without bound (which would suggest a genuine memory leak in
this architecture's query path -- a materially more serious finding)?

**H8**: Furniture's RSS growth, extended to a much longer resident
window (180 seconds, 9x H7's 20-second window, sampled every 15
seconds), decelerates materially in the window's second half compared
to its first half -- consistent with a bounded allocator/arena
high-water-mark effect settling toward a ceiling, not an open-ended
leak. Idle-resident (already flat within H7's first 5-second sample) is
included as a cheap comparison point, not the focus of this experiment.

**Pass/fail defined in advance**: compare RSS growth (relative to the
immediate post-load snapshot) accumulated in the FIRST HALF of the
180-second window (0-90s) against growth accumulated in the SECOND HALF
(90-180s). If second-half growth is at most half of first-half growth
(or non-positive), the curve is decelerating toward a plateau --
CONFIRMED. If second-half growth is comparable to or larger than
first-half growth, the curve is not decelerating -- FALSIFIED, and would
require further investigation (e.g. profiling) before trusting any
long-running-service memory claim.

Reused the exact same `phase7_eval::resident` sampling primitives H7's
binary uses (factored out into a shared module specifically so P7-E05
would not duplicate the mechanism), just with `RUN_DURATION` raised from
20s to 180s and `SAMPLE_INTERVAL` from 5s to 15s. A regression sanity
run immediately after this refactor reproduced H7's original result
(idle growth 222.7-244.0 KB, Furniture 896-904 KB, near-empty tenants
196 KB across an ad-hoc check), confirming the refactor did not change
behavior before this new experiment was trusted.

## P7-E05 result: H8 CONFIRMED, reproduced across 3 independent runs

| condition | run1 | run2 | run3 |
|---|---|---|---|
| idle: peak growth (KB) | 244 | 244 | 244 |
| idle: first-half / second-half growth (KB) | 244 / 0 | 244 / 0 | 244 / 0 |
| Furniture: peak growth (KB, 180s window) | 1,004 | 1,004 | 1,024 |
| Furniture: first-half / second-half growth (KB) | 984 / 20 | 976 / 28 | 1,004 / 20 |
| Furniture: total queries served | 51,396 | 51,608 | 50,764 |

Idle-resident's curve is identical in shape across all 3 runs: RSS jumps
once at the first 15-second sample (matching H7's finding that idle
growth happens immediately, not gradually) and then stays PERFECTLY
flat for the remaining 165 seconds of the window (all 12 samples in
every run show the same value from t15s through t180s) -- H7's idle
finding holds unchanged over a 9x longer window, with zero further
growth.

Furniture's curve decelerates sharply and consistently across all 3
runs: roughly 98% of the total 180-second window's growth happens in
the first half (976-1,004 KB out of 1,004-1,024 KB total), with only
20-28 KB accruing in the entire second half -- a ~35-50x deceleration
ratio between the two halves. **H8 CONFIRMED** by the pre-registered
criterion (second-half growth is far below half of first-half growth)
in all 3 runs.

**Named, honestly-disclosed residual**: the deceleration is very strong
but the curve is not PERFECTLY flat by t180s -- run2's samples show a
small, real, still-positive creep in the tail (54,108 -> 54,116 ->
54,116 -> 54,116 -> 54,124 -> 54,132 -> 54,136 KB across the last 7
samples, i.e. still gaining a few KB every 15-30 seconds), and run3
shows a similar small tail creep (54,096 -> 54,096 -> 54,100 -> 54,104
-> 54,104). This residual is roughly two orders of magnitude smaller
than the initial climb and does not change the H8 verdict (it is well
within the "decelerating" criterion), but it means "fully plateaued" is
not asserted -- only "decelerating toward what looks like a bound,"
per this project's discipline against overclaiming past what was
actually measured. Whether this tiny residual creep itself eventually
stops, or continues indefinitely at a much slower rate over an even
longer window (minutes to hours), remains untested.

**What this means for the economic model**: H8 does not change H7's
qualitative or quantitative conclusion -- it strengthens confidence in
it. The ~896-1,024 KB peak growth figure for Furniture is not a
transient artifact of a too-short measurement window; the underlying
mechanism (whatever it is -- a thread-local allocator arena high-water
mark remains the leading, still-unconfirmed hypothesis) settles toward
a bound rather than growing without limit, which is exactly the
property a real capacity-planning model needs before treating H7's
figure as a stable input. Idle-resident's finding is now confirmed
completely stable over a 9x longer window with literally zero
additional growth observed.

**Named limitations, not resolved this pass**: only a single 180-second
run's worth of samples per repetition (12 samples at 15-second
intervals) was examined for deceleration shape; a finer-grained sample
rate might reveal structure this coarser one misses. Only Furniture (the
one condition that hadn't plateaued in H7) and idle were tested --
near-empty tenants, which had already plateaued within H7's first
5-second sample, were not re-tested at length (a reasonable scope
choice given they showed no sign of continued growth in H7, but
technically untested at 180s). The specific allocator mechanism remains
a disclosed, unconfirmed hypothesis, as in H7. Whether the tiny residual
tail creep continues, plateaus, or accelerates over an even longer
window (minutes to hours, matching a real production service's actual
lifetime) is untested.

## P7-E06: cold-tenant overhead under realistic background load (H9, stated before implementation)

Issue #21's Phase 7 "Experiments" list explicitly names "cold tenant
overhead" and "hot tenant saturation" as required measurements.
Nothing in H1-H8 tested this directly: H2 compared one heavily-loaded
tenant against a quiet tenant's latency, but both were otherwise
equally available, not genuinely "cold" (infrequently queried over a
long window while OTHER tenants dominate the process). P7-E01 varied
the BREADTH of other tenants touched, not the QUERY FREQUENCY of any
one tenant. P7-E06 asks directly whether infrequent access itself costs
anything in this architecture, given each tenant's `CatalogIndex` is a
fully independent, immutable structure with no shared warm-cache/LRU
state to lose -- the same mechanistic reasoning H2's isolation finding
already rests on.

**H9**: a cold tenant's (infrequently queried) own p50/p99 latency,
measured against a SAME-SIZED hot tenant's (continuously queried) own
p50/p99, within the same process under realistic multi-tenant
background load, does NOT show material degradation -- since there is
no explicit software-level cache/warm-up state for infrequent access to
lose.

**Design** (isolating query FREQUENCY from tenant SIZE): pick two
tenants of near-identical product count from the real 55-tenant
population (adjacent in a size-sorted ranking, taken from the middle of
the distribution to avoid re-testing H6/H7/H8's already-covered
largest/smallest extremes). One ("hot") is queried continuously by a
dedicated thread; the other ("cold") is queried only once every 100ms
(simulating a genuinely low-QPS tenant) by a second dedicated thread,
timing ONLY the query call itself (not the sleep) so scheduler wakeup
jitter is excluded from the measured latency by construction. Two
additional threads continuously hammer the OTHER 53 tenants
(round-robin, matching P7-E01's established background-load pattern),
so both hot and cold tenants are measured under realistic multi-tenant
CPU contention (4 threads total, matching this container's real CPU
count), not in an otherwise-idle process. Run for 30 seconds (giving
~300 cold-tenant samples), repeated 3x.

**Pass/fail defined in advance**: if the cold tenant's p99 latency is
>=2x the same-sized hot tenant's p99 (the same material-regression
threshold used throughout Phase 7, e.g. H2), H9 is FALSIFIED --
infrequent access carries a real cost. Below that, H9 is CONFIRMED.

## P7-E06 result: H9 FALSIFIED by the pre-registered ratio threshold, reproduced across 3 runs -- but the absolute magnitude is tiny

The real 55-tenant population's median-adjacent pair both happened to
be 5-product tenants: "Faux Plants and Trees" (hot) and "Ergonomic
Accessories" (cold).

| run | hot p50 (ms) | hot p99 (ms) | cold p50 (ms) | cold p99 (ms) | p50 ratio | p99 ratio |
|---|---|---|---|---|---|---|
| 1 | 0.0013 | 0.0030 | 0.0115 | 0.0391 | 8.85x | 12.83x |
| 2 | 0.0013 | 0.0028 | 0.0121 | 0.0357 | 9.31x | 12.68x |
| 3 | 0.0013 | 0.0027 | 0.0130 | 0.0350 | 10.00x | 12.88x |

**H9 is FALSIFIED by the pre-registered 2x threshold**, decisively and
reproducibly: the cold tenant's p99 latency is 12.68-12.88x the hot
tenant's across all 3 runs, and -- importantly -- the p50 ratio
(8.85-10.00x) is nearly as large as the p99 ratio. This rules out a
"rare tail-outlier" explanation (which would inflate p99 much more than
p50) in favor of a genuine, systematic shift across the ENTIRE cold-
tenant latency distribution, reproducibly across all 3 independent
runs.

**Critical context the ratio alone does not convey**: every single one
of these latencies, hot AND cold, is on the order of MICROSECONDS
(1.3-13.0 microseconds), three to four orders of magnitude below
typical real-world network/application request latencies (usually
single-digit-to-double-digit MILLISECONDS). The absolute cold-tenant
penalty here -- roughly 10-30 microseconds -- is almost certainly
negligible next to any real deployed service's actual per-request
overhead (network round-trip, serialization, connection handling, none
of which Phase 7 measures). A 12x RATIO sounds dramatic; a "costs an
extra 20 microseconds" absolute figure does not, and for a real
production system the latter framing is the practically relevant one.

**Named, disclosed-but-unconfirmed mechanism**: the leading hypothesis
is CPU cache locality, not any explicit software-level cache this
architecture manages -- H2's own finding (no material regression from
cross-tenant contention) already established there is no SOFTWARE state
for a cold tenant to lose. But a tenant queried once every 100ms while
three OTHER threads continuously touch different tenants' data very
plausibly has its own small working set evicted from L1/L2 CPU cache
between accesses, while the continuously-hammered hot tenant's data
stays resident in cache -- a real, physical, hardware-level "cold
tenant" cost that exists ORTHOGONAL to any explicit warm-cache
mechanism, and one this architecture does not currently do anything to
mitigate. This is a plausible, mechanistically consistent explanation,
not a profiled and confirmed one.

**Named limitations, not resolved this pass**: only one size-matched
tenant pair (both 5 products) was tested; whether the ratio holds,
grows, or shrinks for a larger size-matched pair is untested. Only one
cold-query interval (100ms) was tested; whether the ratio scales with
how stale the cache is (e.g., a 10ms vs. 1000ms interval) -- which would
further support the cache-locality hypothesis if confirmed -- is
untested and named as a natural follow-up. This measures latency only,
under a specific 4-thread contention pattern; it says nothing about
whether this microsecond-scale effect would be visible at all once real
network/serialization overhead dominates a genuine multi-tenant
service's total request latency.

## P7-E07: fairness + aggregate QPS under a realistic Zipfian demand mix (H10, stated before implementation)

Issue #21's Phase 7 "Experiments" list explicitly names "aggregate
QPS," "fairness under skewed tenant load," and "hot tenant saturation"
as required measurements. Nothing in H1-H9 tested a realistic, single,
shared query stream spanning all 55 real tenants at once: P7-E01 (H4)
held one tenant's own load fixed and varied only the BREADTH of other,
uniformly-touched tenants; P7-E06 (H9) used a deliberately simple,
artificial design (one dedicated hot thread spinning as fast as
possible with zero interleaving, one dedicated cold thread on a fixed
100ms interval, two background-noise threads). P7-E07 asks whether H9's
finding (a real ~9-13x cold/hot latency-ratio effect, plausibly CPU
cache locality) is a genuine architectural property that replicates
under a DIFFERENT, more realistic query-arrival pattern, or an artifact
specific to H9's fixed-interval, fully-dedicated-thread methodology.

**H10**: reusing H9's exact same-size tenant-pair selection (isolating
query FREQUENCY from tenant SIZE, so results are directly comparable to
H9), but embedding both tenants in a single shared Zipfian-weighted
(weight(rank) = 1/rank, a well-established real-world traffic-skew
model) query stream spanning all 55 real tenants at once -- with the
pair's own weights overridden to the population's max/min (~55x apart)
-- H9's cold/hot p99 ratio replicates (stays >=2x, the same
material-regression threshold used throughout Phase 7) under this
materially different, more realistic arrival pattern.

**Design**: 4 worker threads (matching this container's real CPU
count), each independently sampling a tenant from the SAME shared
weighted distribution and executing a real structural query
(`facet_scan_once`), recording latency only for the tracked hot/cold
pair (all other tenants' queries are counted toward aggregate
throughput but not individually timed). Each of the 3 repeated runs
uses the SAME per-thread RNG seed (a deterministic seed per this
project's standing discipline), so the logical query sequence is
identical across runs -- any run-to-run difference in the measured
latencies is attributable to genuine runtime/scheduling noise, not
sampling noise.

**Pass/fail defined in advance**: if this design's cold/hot p99 ratio
clears the same 2x threshold in every run, H10 REPLICATES (H9's effect
is a real architectural property, not a methodology artifact); if it
drops below 2x in any run, H10 DOES NOT REPLICATE (H9's effect may be
specific to its fixed-interval, fully-dedicated-thread design).

## P7-E07 self-caught statistical problem (first draft, before trusting any ratio)

The first-draft run used a 15-second window. The cold tenant's assigned
weight is a real ~55x smaller population share, and over 15 seconds it
received only **62-63 samples** per run. With n=62, p99 is essentially
the value of the single highest (or second-highest) observed sample --
not a robust tail-latency estimate. This showed up immediately: using
the IDENTICAL deterministic query sequence in all 3 runs (same RNG
seed), the reported p99 ratio swung wildly -- **1.53x, 1.82x, 5.50x** --
while the far more robust p50 ratio (computed from the BULK of each
much-larger hot-tenant sample, and still meaningful even for cold's
smaller sample) stayed essentially IDENTICAL across all 3 runs
(2.04x, 2.08x, 2.08x). A statistic that swings 3.6x across 3 runs of an
*identical* logical query sequence is not measuring a real per-run
difference -- it is measuring how few cold samples landed near the
tail. This was caught before any ratio was written up as a finding.

**Fixed**: `RUN_DURATION` raised from 15s to 120s (8x), targeting
several hundred cold-tenant samples -- enough for p99 to reflect actual
tail behavior rather than the identity of one lucky/unlucky sample. The
undersampled first-draft results were renamed, not deleted:
`docs/research/artifacts/p7_e07_realistic_demand_mix_run1/results_15s_undersampled_superseded.csv`
and its matching `.log`.

## P7-E07 result (after the fix): H10 does not cleanly replicate H9's magnitude, but the DIRECTION replicates in every run

| run | hot p50 (ms) | hot p99 (ms) | cold p50 (ms) | cold p99 (ms) | cold n | p99 ratio |
|---|---|---|---|---|---|---|
| 1 | 0.0024 | 0.0047 | 0.0050 | 0.0093 | 494 | 1.98x |
| 2 | 0.0024 | 0.0058 | 0.0050 | 0.0121 | 487 | 2.08x |
| 3 | 0.0024 | 0.0044 | 0.0050 | 0.0081 | 501 | 1.85x |

Cold-tenant sample counts (487-501) are now comparable to H9's own
~300-sample design, and the ratio is far more stable than the
undersampled first draft: **1.85x-2.08x** across all 3 runs, hovering
almost exactly on the pre-registered 2.0x line (1 of 3 runs, run 2,
technically clears it; runs 1 and 3 fall just under). By the letter of
the pre-registered criterion (ALL runs must clear 2.0x), **H10 does NOT
replicate** H9's finding as a clean pass.

**But this is not "no effect" -- it is a much SMALLER, still real and
directionally consistent effect.** The p50 ratio is remarkably stable:
cold p50 (0.0050ms) is IDENTICAL in all 3 runs, hot p50 (0.0024ms) is
IDENTICAL in all 3 runs, giving an exact 2.083x ratio in every single
run. Combined with the p99 ratios clustering tightly around the same
~2x value (unlike the first draft's noisy 1.53-5.50x swing), this reads
as a real, small, highly reproducible effect (cold measurably slower
than hot, consistently, at both p50 and p99, in 100% of runs) -- just
one **roughly 4-6x smaller in magnitude** than H9's originally-observed
9-13x ratio, not one that has vanished.

**Named, disclosed-but-unconfirmed hypothesis for why the magnitude
shrank**: H9's design gave the hot tenant a fully dedicated thread with
ZERO interleaving -- spinning on only that one tenant's data gives it an
artificially ideal, maximally-warm CPU cache. In this design, by
contrast, EVERY thread (including whichever one happens to serve the
hot tenant on a given iteration) is constantly interleaved with queries
to all 53 other real tenants via the same shared weighted stream --
so even the hot tenant's own cache locality is diluted relative to
H9's idealized setup, likely narrowing the observed hot/cold gap from
both ends (cold doesn't get dramatically worse, hot doesn't get to stay
dramatically better). This is a plausible, mechanistically coherent
explanation for the discrepancy between the two designs' effect sizes,
not a profiled and confirmed one.

**Aggregate throughput**: 1,320-1,345 rps across all 3 runs (tight,
consistent with the identical deterministic query sequence). As
pre-registered, this is explicitly NOT compared directly to H4/P7-E01's
aggregate throughput number -- which tenants are hot/cold and their
per-query cost dominates this figure (the same workload-mix-sensitivity
class of caveat P7-E01's own first draft had to learn the hard way), so
no cross-experiment throughput comparison is drawn here.

**What this means**: H9's cold-tenant-overhead finding is real and
replicates in DIRECTION under a materially different, more realistic
full-population query-arrival pattern -- but its MAGNITUDE was
substantially inflated by H9's specific fully-dedicated-thread design.
A realistic, shared, interleaved worker-thread pool (the architecture
this project would actually deploy) shows a smaller, ~2x effect that
sits right at this project's own material-regression threshold rather
than dramatically clearing it. Both figures (H9's ~9-13x under an
idealized design, H10's ~1.85-2.08x under a realistic one) are now
part of the honest record, neither erasing the other.

**Named limitations**: only one size-matched tenant pair (the same one
H9 used, both 5 products) was tested; only one weight ratio (~55x) was
tested; whether the ~2x effect grows, shrinks, or holds at a different
traffic-skew ratio or a different-sized pair is untested. The specific
"cache dilution from interleaving" hypothesis for why H9's and H10's
magnitudes differ is disclosed, not profiled or confirmed. Aggregate
throughput and per-tenant "hot tenant saturation" behavior were observed
only as secondary context, not independently, rigorously isolated as
their own falsifiable claims this pass.

## P7-E08: extending H4's QPS-vs-breadth finding to controlled-stress-replicated tenant counts (H11, stated before implementation)

P7-E01 (H4) confirmed that a fixed tenant's own query throughput/latency
does not meaningfully degrade as the BREADTH of other, distinct,
concurrently-touched tenants grows -- but WANDS' real partition only
reaches 55 total tenants (54 "other"). P7-E02 (H5) separately confirmed
memory scales linearly, not tenant-count-dependently, all the way to
6,500 controlled-stress-replicated tenants (real 55-tenant population
repeated end to end, `-copyN` suffixed to stay distinct, explicitly
disclosed as NOT a claim about organic tenant growth -- it isolates
tenant COUNT as a variable while holding per-tenant data/schema shape
fixed). PHASE7_DECISION.md's own "what would be built next" list named
the natural combination as still open: "extending H4 (query throughput
under breadth) to the hundreds-to-thousands tenant counts H5 already
reached for memory."

**H11**: H4's finding (a fixed tenant's own throughput/latency does not
degrade as breadth of other touched tenants grows) continues to hold
when breadth is extended via H5's controlled-stress replication
methodology far beyond WANDS' real 54-tenant ceiling, into the
hundreds-to-thousands (matching H5's memory-scale reach).

**Design**: reuses P7-E01's exact quiet/noisy-tenant methodology --
one fixed tenant ("Rugs-copy0", the first replication pass of P7-E01's
own "Rugs" checkpoint tenant, so it is guaranteed present at every
tested breadth since copy0 always covers all 55 real base tenants
first) is queried continuously for a fixed 4-second window by a
dedicated loop, while `WORKERS - 1 = 3` noisy threads round-robin
through every OTHER replicated tenant for the same window, each
building its own `CatalogIndex`. `TENANT_COUNTS = [55, 200, 500, 1000,
2000]` -- the top of that range matches the same order of magnitude H5
reached before this run's RSS crossed a 6 GB safety cap (this
process's own RSS, not system-wide, matching P7-E02's established
safety-capping convention; the container also holds Solr's JVM at
~4.8 GB resident). The new shared `phase7_eval::tenants::replicate_tenants`
helper (added this pass, not retrofitted into P7-E02 since P7-E02's
incrementally-safety-checked build loop is a different access pattern)
performs the replication upfront for each tenant count.

**Pass/fail defined in advance**: as breadth grows from n=55 to n=2000,
if the quiet tenant's own throughput does not drop more than 20% and
its p99 does not grow more than 2x (both relative to the n=55
baseline, the same material-regression bar used throughout Phase 7),
H11 is CONFIRMED (H4's finding generalizes to H5's memory-scale
tenant counts). If either threshold is crossed, H11 is FALSIFIED.

## P7-E08 result: H11 CONFIRMED, reproduced across 3 independent runs

Three independent full runs (55 -> 200 -> 500 -> 1000 -> 2000 tenants
each), raw data in `docs/research/artifacts/p7_e08_extended_breadth_run1/
results_run{1,2,3}.csv`. All 3 runs completed the full ladder without
tripping the 6 GB safety cap (peak RSS 5.48 GB at n=2000, ~91% of the
cap -- close enough to be worth naming as a limitation below, but the
run was never actually stopped short).

| n_tenants | rps (run1/2/3) | p99 ms (run1/2/3) | RSS MB |
|---|---|---|---|
| 55 | 757.0 / 760.5 / 753.8 | 1.858 / 1.879 / 1.804 | 242.9 |
| 200 | 760.3 / 761.0 / 759.8 | 1.909 / 1.833 / 1.780 | 680.0 |
| 500 | 777.8 / 744.5 / 759.3 | 1.656 / 1.832 / 1.950 | 1508.6 |
| 1000 | 719.5 / 764.8 / 758.8 | 2.120 / 1.742 / 1.833 | 2851.8 |
| 2000 | 712.8 / 688.5 / 700.3 | 1.992 / 1.982 / 1.957 | 5481.4 |

Throughput ratio at n=2000 vs. the n=55 baseline: 0.942 / 0.905 / 0.929
across the 3 runs -- a consistent 6-9% reduction, well inside the 20%
pass bar. p99 ratio at n=2000 vs. n=55: 1.072 / 1.055 / 1.085 -- a
consistent 5-8% growth, far inside the 2x pass bar. **H11 is CONFIRMED
in all 3 runs**: H4's breadth-independence finding, originally
established only up to WANDS' real 55-tenant ceiling, continues to
hold cleanly at 2,000 controlled-stress-replicated tenants -- a 36x
larger breadth, and the same order of magnitude H5 reached for memory.

As a secondary consistency check (not this experiment's primary
claim, but a useful cross-reference against H5): RSS grows linearly
with tenant count in all 3 runs, ~2.7-3.0 MB/tenant, matching H5's own
per-tenant memory-scaling finding rather than contradicting it.

**Named limitations**: a small, consistent throughput dip and p99
uptick appear specifically at n=2000 in every run (the ladder's
largest jump is between n=1000 and n=2000, not evenly spread across
the whole range) right as RSS reaches ~5.48 GB, ~91% of the 6 GB
safety cap. Because the effect is well inside the pre-registered pass
thresholds it does not change H11's verdict, but this run cannot fully
rule out memory-pressure (cache/TLB pressure, allocator fragmentation
as RSS nears the cap) as a contributing factor at n=2000, as distinct
from a pure tenant-count/breadth effect -- this joins the project's
existing disclosed-but-unconfirmed mechanism hypotheses (H7/H8's
allocator-arena growth, H9/H10's CPU-cache-locality/interleaving-
dilution effect) as a candidate for future profiling rather than a
claim made here. Only one quiet tenant ("Rugs-copy0") and one noisy-
worker-count (3) were tested; the noisy threads' own round-robin
coverage of ever-larger tenant counts was not independently, rigorously
isolated as its own claim (it is reported via `noisy_total_requests`
in the raw CSVs for transparency, not analyzed as a hypothesis here).
Query type is a single fixed facet-scan operation, the same one every
other Phase 7 QPS/throughput experiment (P7-E01, P7-E04-E07) has used.

## P7-E09: tenants per fixed hardware envelope at target SLO (H12, stated before implementation)

Issue #21 explicitly names "tenants per fixed hardware envelope at
target SLO" as a required Phase 7 "Economic output" metric. P7-E02/H5
established that a 6 GB self-process-RSS safety cap supports 6,500
controlled-stress-replicated tenants for MEMORY -- but that measurement
keeps only each tenant's `CatalogIndex` resident (H5's own loop drops
the raw `Catalog` immediately after each tenant's index is built, one
at a time). P7-E08/H11's query-serving configuration needs BOTH
`Catalog` and `CatalogIndex` resident simultaneously per tenant
(`facet_scan_once` takes both), so its real per-tenant footprint is
materially higher than H5's index-only figure would suggest -- but
H11 only tested up to 2,000 tenants, short of H5's 6,500-tenant memory
ceiling. This experiment asks the natural combination directly: what
is the actual, safely-reached tenant count where BOTH the memory
ceiling AND the query-latency SLO are confirmed simultaneously, for a
query-capable deployment on this container -- rather than assuming H5's
own (index-only) 6,500 figure carries over unchanged?

**H12**: at the largest tenant count this container's real, disclosed
hardware envelope can safely support for a QUERY-CAPABLE deployment
(both `Catalog` and `CatalogIndex` resident per tenant), the quiet
tenant's own throughput and p99 latency stay within the same
material-regression bar used throughout Phase 7 (throughput drop
<20%, latency growth <2x) relative to the n=55 real-tenant baseline.

**Design**: reuse P7-E01/P7-E08's quiet/noisy-tenant methodology,
building tenants incrementally (one `Catalog`+`CatalogIndex` pair at a
time, matching P7-E02's proven-safe pattern) with this process's real
RSS checked every 250 tenants, stopping before a self-imposed safety
cap. Latency is measured at fixed checkpoints (n=55, n=2000, matching
H11 for continuity) plus once more at whatever count is actually,
safely reached when the cap trips.

## P7-E09 first self-caught problem: a real OOM, and a wrong assumption about this container's memory ceiling

The first-draft binary reused P7-E08's exact pattern -- call
`replicate_tenants()` to build a `Vec<(String, Catalog)>` of ALL N
tenants eagerly, THEN `.iter().map(CatalogIndex::build).collect()` to
build all N indexes -- and checked this process's own RSS only ONCE,
after both Vecs were fully built. At n=6500 (the target point,
matching H5's own established ceiling), this process was OOM-killed by
the Linux cgroup out-of-memory killer while STILL BUILDING the
indexes, before its own safety check ever ran: `dmesg` showed
`Memory cgroup out of memory: Killed process ... anon-rss:13955460kB`.
Inspecting `/sys/fs/cgroup/memory/.../memory.limit_in_bytes` for this
process's actual cgroup revealed the real, enforced limit is
**14,327,726,080 bytes (13.34 GiB)** -- materially lower than the
~15 GB host-level total this project's prior safety-cap choices had
implicitly assumed from `free -h` (which reports host memory, not this
specific cgroup's own enforced ceiling). Building all N raw `Catalog`s
into one Vec via `replicate_tenants()`, then building all N
`CatalogIndex`es while STILL holding that entire raw-catalog Vec
alive, transiently roughly doubles peak memory relative to either
piece's own steady-state footprint -- exactly the kind of transient
peak P7-E02's own incremental, one-tenant-at-a-time pattern was
designed to avoid, and exactly the discipline this new binary's first
draft failed to reuse.

**Fix**: rewrote the binary to build one tenant's `Catalog` and
`CatalogIndex` at a time, immediately retiring any transient
per-tenant construction state and pushing only the final `Arc`-wrapped
pair into growing `Vec`s (mirroring P7-E02's proven-safe pattern, now
extended to also retain the raw `Catalog`, which P7-E02 itself never
needed to keep). RSS is checked every 250 tenants DURING construction,
not once after an entire batch -- so a real safety trip happens before
peak transient memory can exceed either the chosen cap or this
container's real hard limit. The safety cap was set to 9 GB, chosen
with real, deliberate margin under the newly-discovered 13.34 GiB hard
limit (not under the previously-assumed ~15 GB host figure).

## P7-E09 second self-caught problem: an unstable p99 at the very first in-process checkpoint

Running the corrected binary 3 times surfaced a second, different
issue, caught before trusting any ratio: the n=55 baseline
checkpoint's p99 varied sharply across the 3 runs (1.777 / 4.104 /
2.755 ms) while its p50 stayed tight (1.2895 / 1.2885 / 1.2858 ms) --
and this instability was specific to n=55, the very FIRST in-process
measurement taken in each run. The n=2000 and n=3500 checkpoints (each
measured only after substantially more prior construction/warm-up
work in the same process) showed tight, consistent p99s across all 3
runs (1.837-1.871 ms at n=2000; 1.812-2.030 ms at n=3500). This is a
cold-start artifact specific to being the first measurement taken
against freshly-built, never-yet-touched structures (page faults,
un-warmed allocator/cache state) -- not a general instability in the
methodology, and not evidence of any real per-run difference, since
p50 (far less sensitive to a handful of unusually slow first queries)
stayed essentially flat. Matching this project's established
precedent (P7-E07/H10 also treated p50 as the more trustworthy metric
when p99 was the unstable statistic, there for a different underlying
reason -- undersampling, not cold start), **p50 is used as the
primary metric for this experiment's SLO determination**, with raw
p99 values still reported honestly rather than discarded.

## P7-E09 result: H12 CONFIRMED, reproduced across 3 independent runs

The corrected, incremental-build binary completed cleanly in all 3
runs with no OOM: the 9 GB safety cap tripped at **exactly n=3500** in
every run (9,410 MB > 9,216 MB, deterministic since replication uses
real, ordered data with no randomness) -- raw data in
`docs/research/artifacts/p7_e09_slo_tenant_envelope_run1/
results_run{1,2,3}.csv`.

| n_tenants | rps (run1/2/3) | p50 ms (run1/2/3) | p99 ms (run1/2/3) | RSS MB |
|---|---|---|---|---|
| 55 | 762.8 / 710.8 / 752.8 | 1.290 / 1.289 / 1.286 | 1.777 / 4.104 / 2.755 | 244.1 |
| 2000 | 760.0 / 755.8 / 743.2 | 1.277 / 1.286 / 1.308 | 1.837 / 1.871 / 1.869 | 5481.1 |
| 3500 | 725.3 / 735.5 / 764.3 | 1.314 / 1.308 / 1.272 | 2.030 / 2.003 / 1.812 | 9411.0 |

p50 ratio at n=3500 vs. n=55: **0.989-1.019** across all 3 runs --
essentially flat (within ~2% noise). Throughput (rps) ratio at n=3500
vs. n=55: **0.951-1.035** -- also flat within noise. Both stay nowhere
near the pre-registered pass bar (throughput drop <20%, latency growth
<2x) -- this is a much cleaner pass than H11's own already-comfortable
margin at n=2000, not a marginal one. **H12 CONFIRMED**: at 3,500
tenants -- the real, safely-reached ceiling for a QUERY-CAPABLE
deployment (`Catalog`+`CatalogIndex` both resident) under a disclosed
9 GB self-process safety envelope on this container -- the quiet
tenant's own throughput and p50 latency are both essentially
unaffected relative to the 55-real-tenant baseline. Combined with the
9 GB memory envelope itself being empirically, safely reached (not
assumed or extrapolated), **this directly answers Issue #21's
"tenants per fixed hardware envelope at target SLO" metric for the
first time this phase**: ~3,500 tenants per this container's disclosed
9 GB query-serving envelope, at a p50-latency SLO that shows
essentially zero degradation at that count.

As a secondary cross-check, per-tenant memory footprint from n=55 to
n=3500 computes to **~2.66 MB/tenant** -- consistent with, and a
tighter-sample confirmation of, H11's own ~2.7-3.0 MB/tenant figure,
and meaningfully higher than H5's own ~0.96 MB/tenant implied by its
1.26 KB/product figure (782 products/tenant average) -- because H5's
measurement never keeps the raw `Catalog` resident, only the
`CatalogIndex`, while a real query-serving deployment needs both. This
is an important, previously-implicit distinction made explicit here:
**H5's 6,500-tenant memory ceiling describes an index-only
configuration, not a query-capable one** -- the real
query-capable-and-latency-confirmed ceiling on this container's
disclosed envelope is materially lower, at ~3,500 tenants.

**Named limitations**: the 9 GB safety cap is a deliberately
conservative, self-imposed choice with real margin under this
container's actual 13.34 GiB hard limit -- NOT a claim that 3,500 is
this container's absolute ceiling; a deployment with less reserved
margin (e.g. a dedicated node, or one not sharing headroom with other
processes) could very plausibly push higher, untested here. Only one
quiet tenant, one query type (facet scan), and one hardware envelope
(this specific container) were tested; the dollar-cost implication of
this tenant count depends on cloud pricing assumptions this document
deliberately keeps separate, per Issue #21's own instruction to keep
architecture-normalized metrics reproducible independent of price
changes. The n=55 checkpoint's own p99 instability (cold-start
artifact, not a real per-run difference) means this experiment's p99
numbers at n=55 specifically should not be over-read; p50 is the
trustworthy metric for this experiment's conclusion.
