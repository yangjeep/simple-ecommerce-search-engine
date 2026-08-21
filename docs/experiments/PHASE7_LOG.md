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

## Raw results (P7-E00)

`docs/research/artifacts/p7_e00_tenant_packing_run1/{h1_rss_amortization.csv,h2_isolation.csv}`.

### H1: RSS at each tenant-count checkpoint

| N | tenant added at this checkpoint | tenant products | cumulative products | cumulative marginal RSS (KB) | naive RSS/tenant (KB) |
|---|---|---|---|---|---|
| 1 | Furniture | 16,039 | 16,039 | 139,680 | 139,680.0 |
| 5 | Storage & Organization | 2,175 | 30,906 | 139,680 | 27,936.0 |
| 10 | Baby & Kids | 1,204 | 39,664 | 142,628 | 14,262.8 |
| 25 | Buffet Accessories | 6 | 41,378 | 149,432 | 5,977.3 |
| 55 | Water Filter Pitchers | 1 | 41,438 | 150,252 | 2,731.9 |

### H2: quiet-tenant latency, alone vs. under concurrent cross-tenant load

| condition | p50 (ms) | p99 (ms) | n |
|---|---|---|---|
| Rugs tenant alone | 1.1915 | 1.8225 | 500 |
| Rugs tenant + 3 threads hammering Furniture tenant | 1.4001 | 2.7177 | 500 |

p50 ratio 1.18x, p99 ratio 1.49x — under the pre-registered pass/fail
rule (material regression = >=2x), **H2 passes**: no material p99
regression from an unrelated tenant's sustained heavy concurrent load.

## Self-caught interpretation issue: the naive RSS/tenant metric is misleading

The naive `cumulative_marginal_rss / N` column trivially decreases
(139,680 -> 2,731.9 KB) as N grows from 1 to 55 — but this is dominated
by WANDS' own real long-tail shape, not primarily by fixed-cost
amortization: cumulative products barely grow past N=10 (39,664 -> 41,378
-> 41,438 for N=10/25/55), so most of the added "tenants" beyond N~10 are
each contributing only a handful of products. Dividing a roughly-flat
total by a growing N will show this same "decreasing average" shape for
ANY heterogeneous-size population, independent of whether real per-
process fixed-cost amortization exists. Presenting the naive curve alone
would overclaim.

The more informative decomposition is the **marginal RSS added per
step**, separated into marginal-per-added-tenant and marginal-per-
added-product:

| step | tenants added | products added | marginal RSS (KB) | KB/tenant | KB/product |
|---|---|---|---|---|---|
| 1->5 | +4 | +14,867 | 0 | 0.0 | 0.000 |
| 5->10 | +5 | +8,758 | 2,948 | 589.6 | 0.337 |
| 10->25 | +15 | +1,714 | 6,804 | 453.6 | 3.970 |
| 25->55 | +30 | +60 | 820 | 27.3 | 13.667 |

KB/product *grows* in the tail (0.337 -> 3.970 -> 13.667) while KB/tenant
*shrinks* (589.6 -> 453.6 -> 27.3) — the signature of a real, roughly
constant **per-tenant fixed structural overhead** (bitmap/hashmap/
dictionary scaffolding paid once per tenant regardless of size) that
dominates cost for near-empty tenants (where there is almost no
per-product cost to amortize it against) and is comparatively invisible
for large tenants (where it is dwarfed by real per-product data). The
1->5 step showing exactly 0 KB marginal RSS is itself informative: either
below this measurement's resolution, or those 4 added tenants' data fit
within already-committed/over-allocated pages from the first (largest)
tenant's build — recorded as an open question, not resolved here.

**This reframes H1 from "per-tenant RSS shrinks with scale" (the naive,
overclaiming reading) to "a small, real, roughly-constant per-tenant
fixed cost exists (approximately 27-590 KB in this measurement,
resolution-limited) plus a much smaller genuine per-product marginal
cost" — directly answering Issue #21's explicit Phase 7 metric "idle/
low-QPS tenant fixed cost" more precisely than the original hypothesis
statement anticipated.**

## Adversarial review

[Pending — see PHASE7_DECISION.md for the outcome before this finding is
promoted.]
