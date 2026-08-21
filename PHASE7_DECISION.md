# Phase 7 Decision (Issue #21 Phase 7) — P7-E00 + P7-E01 first pass

**Decision: PROCEED**, with one hypothesis falsified (in the good
direction), one hypothesis confirmed with a small, honestly-disclosed
open question, and one hypothesis confirmed cleanly across repeated
runs.

This is the first phase in this project's history to build and measure
more than one tenant's index in the same process. It does not require
any of the currently-blocked external resources (Retailrocket, H&M,
Amazon Reviews 2023, Havenask) — it is built entirely over the real
WANDS catalog already validated in Phase 6A/6B.

## Recap: what P7-E00/P7-E01 were asked to answer

Issue #21's Phase 7 goal: can commerce specialization reduce per-tenant
fixed cost and increase safe tenant packing density while preserving
predictable latency and isolation? Four falsifiable hypotheses were
stated before implementation (`docs/experiments/PHASE7_LOG.md`):

- **H1**: per-tenant memory overhead amortizes as tenant count grows.
- **H2**: one tenant's heavy concurrent load doesn't measurably degrade
  another tenant's own latency.
- **H3**: there exists a packing ceiling on this hardware, to be recorded
  honestly rather than extrapolated past.
- **H4** (P7-E01): a fixed tenant's own query throughput/latency does not
  degrade as the BREADTH of other, distinct, concurrently-touched tenants
  grows, at fixed worker concurrency — Issue #21's explicit "tenants/node
  at fixed latency SLO" metric.

## Process note: this project's adversarial-review discipline caught a real problem here too

The first-draft implementation measured only RSS, from a single
untrialed process run, always building the largest tenant first, with
no same-tenant control for H2. A 3-lens adversarial review (mirroring
the process that caught a real overclaim in Phase 6B) found this was not
sufficient to support the claimed "~27-590 KB real per-tenant fixed
cost," identified a build-order confound never tested, pointed at a
deterministic instrument (`CatalogIndex::approximate_size_bytes()`)
already in the codebase and never used, and found H2 could not
distinguish tenant-boundary isolation from generic CPU contention.

Rather than just softening language, the methodology was fixed and
re-measured: RSS baseline moved to after partitioning; the deterministic
size instrument added; a reversed-build-order control added; H1's
leftover state dropped before H2; a same-tenant control condition added
to H2; and — found while making these fixes — tenants were given
independently-interned ID spaces instead of sharing one canonicalized
space from a single whole-catalog ingestion pass.

## Architecture tested

Plain, independent `commerce_core::index::CatalogIndex` instances, one
per tenant, over independently-schema'd `Catalog`s (per the fix above).
No new commerce_core mechanism — this deliberately tests the current
architecture's out-of-the-box multi-tenant behavior before considering
any tenant-aware optimization.

## Tenant model

Each of WANDS' 55 real, distinct `category_depth_1` values (Rugs,
Lighting, Furniture, ...) is treated as one independent tenant's full
catalog — a realistic SMB pattern (a specialty single-category
retailer), inheriting WANDS' own real, non-fabricated long-tail size
distribution (1 to 16,039 products per category) rather than an
arbitrary synthetic split.

## Measured results

**H1 — FALSIFIED as originally stated, replaced with a stronger, more
favorable finding.** The claimed "~27-590 KB real per-tenant fixed cost"
does not survive a reversed-build-order control: building 54 near-empty
tenants first costs essentially zero additional RSS (flat at 68-108 KB
total, 3 independent runs), and essentially all real memory cost appears
the moment the ONE large tenant (Furniture, 16,039 products) is built —
regardless of whether it is built first or last. The original
forward-order "per-tenant cost in the tail" was an allocator/build-order
artifact: RSS grew ~7,644 KB across a tenant range where the
deterministic `approximate_size_bytes()` metric (order-invariant,
confirmed identical across forward/reversed runs) grew only ~718 bytes
— an ~10.6x disproportion explicable only by allocator/page effects, not
real structural cost. **Corrected finding**: per-tenant fixed memory
overhead is negligible in this architecture; total memory cost is driven
almost entirely by aggregate product count, not tenant count. This
answers Issue #21's "idle/low-QPS tenant fixed cost" metric more
favorably than the withdrawn first-draft estimate: an idle tenant costs
close to nothing to keep resident. Reproducible across 3 forward + 3
reversed independent process runs (RSS within ~0.3% at every forward
checkpoint; reversed-order tiny-tenant range consistently flat).

**Named limitation**: this measures in-process memory only, not
per-tenant CPU/connection/process overhead in a real multi-process
deployment, which could carry a materially different fixed cost.

**H2 — PASS, robust across 3 independent runs.** Quiet tenant's own p99
degrades by 1.31x-1.43x under 3 threads of sustained unrelated-tenant
load (cross-tenant), and by 0.85x-1.16x under the same load applied to
its own data (same-tenant control) — all 6 measurements (3 runs x 2
conditions) stay comfortably below the pre-registered 2.0x
material-regression threshold. The cross-tenant ratio is consistently,
if modestly, higher than the same-tenant-control ratio in all 3 runs
(mean 1.36x vs 1.05x) — a real, reproducible, but small difference of
unconfirmed cause (a cache-locality hypothesis is named, not asserted).
**The core claim holds robustly: no material p99 regression from an
unrelated tenant's sustained heavy concurrent load.**

**H3 — not tested.** The real `category_depth_1` partition tops out at
55 real tenants, reaching only ~50 MB peak marginal RSS — nowhere near
this container's ~15 GB budget. The packing ceiling was never actually
approached; this is recorded honestly as untested rather than
extrapolated, per this project's standing discipline.

**H4 — CONFIRMED, robust across 3 independent runs (P7-E01).** A fixed
tenant's ("Rugs") own query throughput and p99 latency (694-816 rps,
1.71-2.16ms p99 across all 15 measurements — 3 runs x 5 breadth levels)
show no monotonic trend as the number of OTHER, distinct, concurrently-
touched tenants grows from 1 to 54 (WANDS' real ceiling), at fixed
4-worker concurrency. This directly answers Issue #21's "tenants/node at
fixed latency SLO" metric for the memory/CPU-contention dimension: this
architecture's per-tenant `CatalogIndex` instances are independent,
immutable structures with no shared mutable state to contend over, and
that independence appears to hold in practice, not just in principle.
**Named limitation**: only tested up to WANDS' real 54-other-tenant
ceiling; whether this holds at the hundreds-to-thousands scale Issue #21
ultimately asks about is untested.

**P7-E01 self-caught confound (documented for the process record)**: the
first-draft design spread ALL query load uniformly at random across all
N tenants and found throughput apparently increasing ~19.8x from N=1 to
N=55 — before promotion, inspection found this was a workload-mix
artifact (uniform random selection over an increasingly long-tail
population shifts most load onto progressively cheaper near-empty
tenants as N grows), not a real tenant-count effect. Caught and corrected
before any external review was needed, by holding one tenant's own query
completely fixed and varying only the breadth of other tenants touched.

Full tables, raw CSVs/logs: `docs/experiments/PHASE7_LOG.md`,
`docs/research/artifacts/p7_e00_tenant_packing_run1/`,
`docs/research/artifacts/p7_e01_qps_scaling_run1/`.

## Failed / fixed experiments (preserved, not erased)

The first-draft H1 methodology (single-run RSS, baseline before
partitioning, largest-first only, no deterministic cross-check) is
preserved in `docs/experiments/PHASE7_LOG.md`'s "First-draft results and
self-caught interpretation issue" section, alongside exactly what the
adversarial review found wrong with it and what was fixed — per this
project's "record failed experiments, do not erase evidence" rule. The
P7-E01 first-draft workload-mix confound is likewise preserved in the
log rather than silently rewritten.

## Unresolved risks

1. **In-process memory is not the whole cost story.** A real multi-
   tenant deployment likely isolates tenants across processes/containers
   for fault isolation, which would introduce real per-tenant fixed
   costs (process overhead, connection pools, OS scheduling) this
   single-process measurement cannot see. Named as the most important
   follow-up before any economic model is built on top of H1's result.
2. **The small, consistent cross-tenant-vs-same-tenant latency
   difference (H2) has no confirmed mechanism.** A cache-locality
   hypothesis is plausible but unverified; would need profiling to
   confirm.
3. **H3 was never actually tested.** The real category partition's
   natural ceiling (55) was reached long before any hardware resource
   limit. A genuine packing-ceiling test needs finer real partitioning
   or controlled-stress replication of tenant COUNT (distinct from Phase
   6B's replication of catalog SIZE).
4. Only one tenant model (real category-based partitions of one real
   catalog) has been tested. Real SaaS tenants would have completely
   independent catalogs (not partitions of the same source), likely with
   more genuinely-independent schema/vocabulary divergence than this
   experiment's fix (independent ID-interning per tenant) fully
   captures.

## What would be built next if scaling up

A cross-process or cross-container tenant-isolation measurement to
capture the per-tenant fixed cost this single-process design cannot see;
a genuine packing-ceiling test (finer real partitions or controlled-
stress tenant-count replication) to actually exercise H3 and to test H4
at tenant counts beyond WANDS' real 55-category ceiling; an aggregate
throughput-under-realistic-load experiment (P7-E01 tested breadth of
touched tenants at fixed per-tenant demand, not aggregate QPS at a
realistic multi-tenant demand mix, which Issue #21's "per-tenant and
aggregate QPS" metric also asks for).

## What should explicitly not be built yet

No tenant-aware planner/admission changes based on this single pass —
H1's favorable result and H2's pass are encouraging but rest on one
tenant model (real category partitions of one catalog) and one machine
configuration; no economic cost model (Issue #21's Phase 7 "economic
output" section) should be built until the cross-process fixed-cost gap
above is closed, since that is very likely to dominate any real
per-tenant cost estimate.

## What this decision does and does not claim

**Does claim**: in this single-process architecture, per-tenant memory
overhead is negligible and total memory cost tracks aggregate product
count, not tenant count (H1, corrected and adversarially validated); one
tenant's heavy concurrent load does not cause material latency
regression for another tenant sharing the same process (H2, confirmed
across repeated runs); a fixed tenant's own throughput/latency does not
degrade as the breadth of other, distinct, concurrently-touched tenants
grows up to WANDS' real ceiling (H4, confirmed across repeated runs); the
specific numeric first-draft "27-590 KB per-tenant fixed cost" estimate
and the first-draft P7-E01 "~19.8x throughput increase" are both
withdrawn — the former a build-order/allocator artifact, the latter a
workload-mix artifact — neither a real property of the architecture.

**Does not claim**: that this generalizes to a real multi-process/multi-
container SaaS deployment (the in-process measurement cannot see
per-process fixed costs); that the small cross-vs-same-tenant latency
difference in H2 is understood; that a packing ceiling has been found
(H3 untested, and H4 only tested up to the same real ceiling); that
aggregate QPS under a realistic multi-tenant demand mix or economic
cost-per-tenant questions (both explicitly named in Issue #21's Phase 7)
have been answered — this is a first pass on memory, pairwise isolation,
and fixed-tenant throughput-under-breadth only.

**Decision: PROCEED** to the next Phase 7 sub-experiment (a cross-process
fixed-cost measurement, and/or a genuine packing-ceiling test beyond
WANDS' real 55-category limit) without changing the underlying
commerce-native mechanism. The favorable, adversarially-corrected H1
result and the robust H2/H4 results are real evidence in favor of the
architecture's packing-density potential, but are explicitly a floor on
the claim (single-process, one tenant model, one real ceiling), not a
ceiling on what remains to be tested.
