# Phase 7 Decision (Issue #21 Phase 7) — P7-E00 through P7-E03 first pass

**Decision: PROCEED**, with one hypothesis falsified (in the good
direction) and four hypotheses confirmed — one with a small,
honestly-disclosed open question, three cleanly across repeated runs.
The most important new finding (H6) is the first real, measured evidence
for this project's own opening "statistical multiplexing" thesis: pooling
tenants in one process has a real, quantifiable cost advantage over
process-per-tenant isolation.

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
- **H5** (P7-E02): given H1's finding, RSS should scale linearly with
  total product count (not tenant count) as the real 55-tenant
  population is replicated into the hundreds/thousands via Phase 6B's
  controlled-stress methodology applied to tenant count — testing H3
  by proxy, since the real category partition alone could not reach any
  resource ceiling.
- **H6** (P7-E03): a real per-OS-process baseline overhead exists that a
  one-process-per-tenant deployment model pays once PER TENANT, unlike
  H1/H5's pooled in-process design which pays it once TOTAL — the first
  direct test of `docs/WHY.md`'s own "statistical multiplexing" thesis
  with real numbers.

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

**H3 — not directly tested by real category partitions, but answered by
proxy via H5/P7-E02 below.** The real `category_depth_1` partition tops
out at 55 real tenants, reaching only ~50 MB peak marginal RSS —
nowhere near this container's ~15 GB budget. No real-category packing
ceiling was ever approached.

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

**H5 — CONFIRMED, cleanly linear, reproduced across 2 independent runs
(P7-E02).** The real 55-tenant population was replicated via
controlled-stress tenant-count replication (distinct synthetic names per
copy, same disclosure discipline as Phase 6B's catalog-size replication)
up to a self-imposed 6 GB RSS safety cap, reached at **6,500 tenants
(118x the real population) and 4,929,348 total products** in both runs
(RSS within 0.01% of each other: ~6,223 MB). KB/product stayed at
1.260-1.265 across the ENTIRE range from 100 to 6,500 tenants (100 to
4.93M products) — a ~0.4% spread over a 65x range in tenant count. No
sign of super-linear degradation at any point. **This answers H3 by
proxy**: packing cost in this architecture is governed by total product
count, not tenant count, holding cleanly two orders of magnitude beyond
WANDS' real 55-tenant ceiling. **Named limitation, stated precisely**:
6,500 tenants is a self-imposed safety-capped bound chosen to avoid
risking a real OOM in this shared container (not a discovered
architectural or hardware ceiling); the real ceiling on this or a larger
machine is very likely materially higher, deliberately not pursued here.

**H6 — CONFIRMED, reproduced across 3 independent runs (P7-E03).**
Spawning fresh, isolated OS processes (via `std::process::Command`) that
each report their own RSS before touching any tenant data found a real,
consistent per-process baseline of **~2,144-2,152 KB** across all 3
runs — essentially unchanged run to run. That baseline alone equals the
in-process pooled marginal cost of ~1,700 products' worth of tenant data
(H1/H5's ~1.263 KB/product), and is comparable to or larger than the
marginal cost of adding real, individual tenants (near-empty tenants:
~150-220 KB; the largest real tenant, Furniture, 16,039 products: ~51 MB).
**For a hypothetical 1,000-tenant SMB deployment, process-per-tenant
isolation would pay this baseline ~1,000 times (~2.1-2.2 GB in pure
per-process overhead) versus once for a pooled design.** This is the
first real, measured evidence anywhere in this project for `docs/WHY.md`'s
opening "statistical multiplexing" thesis, rather than an assumed
advantage.

**Two self-caught bugs, found and fixed before trusting the first
numbers** (documented in full in `PHASE7_LOG.md`): (1) the child
process's tenant loader called a helper that materializes ALL 55
tenants' built catalogs before selecting one, so every "single tenant"
measurement was actually paying the cost of building all 55 — caught
because all three tested tenant sizes reported suspiciously identical
~100 MB; (2) even after fixing that, `data::load_catalog` always parses
the entire shared 42,994-product file regardless of which tenant is
requested, so a genuinely 1-product tenant still reported ~37 MB —
caught for the same reason (the number still made no sense given H1/H5's
own finding). Fixed by writing each child a genuinely single-tenant data
file, matching how a real single-tenant deployment would actually be
provisioned.

**Named limitations**: uses short-lived `.output()`-based child
processes, not a genuinely long-running resident server (real service
overhead — connection handling, logging/metrics, warm caches — is likely
higher than this floor-level measurement); only 3 real tenant sizes
sampled (largest/middle/smallest), not a full sweep; measures memory
only, not per-process CPU/scheduling overhead a real capacity model
would also need.

Full tables, raw CSVs/logs: `docs/experiments/PHASE7_LOG.md`,
`docs/research/artifacts/p7_e00_tenant_packing_run1/`,
`docs/research/artifacts/p7_e01_qps_scaling_run1/`,
`docs/research/artifacts/p7_e02_packing_ceiling_run1/`,
`docs/research/artifacts/p7_e03_cross_process_run1/`.

## Failed / fixed experiments (preserved, not erased)

The first-draft H1 methodology (single-run RSS, baseline before
partitioning, largest-first only, no deterministic cross-check) is
preserved in `docs/experiments/PHASE7_LOG.md`'s "First-draft results and
self-caught interpretation issue" section, alongside exactly what the
adversarial review found wrong with it and what was fixed — per this
project's "record failed experiments, do not erase evidence" rule. The
P7-E01 first-draft workload-mix confound is likewise preserved in the
log rather than silently rewritten. P7-E03's two self-caught
implementation bugs (materializing all 55 tenants before selecting one;
parsing the entire shared file regardless of which tenant was requested)
are documented the same way in `PHASE7_LOG.md`'s "P7-E03: two self-caught
implementation bugs" section.

## Unresolved risks

1. **Cross-process overhead is now measured, but only at the
   short-lived-process floor.** H6 establishes a real ~2.1-2.2 MB
   per-process baseline via short-lived `.output()`-based child
   processes; a genuinely long-running resident server process (with
   connection handling, logging/metrics, warm caches, etc.) very likely
   costs more than this floor-level measurement captures. Quantifying
   that gap is the natural next step.
2. **The small, consistent cross-tenant-vs-same-tenant latency
   difference (H2) has no confirmed mechanism.** A cache-locality
   hypothesis is plausible but unverified; would need profiling to
   confirm.
3. **H3's real-category ceiling (55) never reached any hardware limit,
   but H5's controlled-stress replication answered the same question by
   proxy up to a self-imposed 6 GB safety cap (6,500 tenants).** The
   actual hardware/architectural ceiling (if one exists at all before
   available RAM runs out) remains genuinely untested beyond that point
   — this experiment deliberately stopped short of finding it rather
   than risk a real OOM in a shared container.
4. Only one tenant model (real category-based partitions of one real
   catalog) has been tested. Real SaaS tenants would have completely
   independent catalogs (not partitions of the same source), likely with
   more genuinely-independent schema/vocabulary divergence than this
   experiment's fix (independent ID-interning per tenant) fully
   captures.
5. **H6 measures memory only.** A real capacity model would also need
   per-process CPU/scheduling overhead, which this experiment did not
   attempt.

## What would be built next if scaling up

A long-running-resident-process measurement to close the gap H6's
short-lived-process floor leaves open (item 1 above); an aggregate
throughput-under-realistic-load experiment (P7-E01 tested breadth of
touched tenants at fixed per-tenant demand, not aggregate QPS at a
realistic multi-tenant demand mix, which Issue #21's "per-tenant and
aggregate QPS" metric also asks for); extending H4 (query throughput
under breadth) to the hundreds-to-thousands tenant counts H5 already
reached for memory; a full economic cost model (Issue #21's Phase 7
"economic output" section) combining H1/H5's negligible in-process
marginal cost with H6's now-measured per-process baseline to produce a
real cost-per-tenant-at-scale estimate for the first time.

## What should explicitly not be built yet

No tenant-aware planner/admission changes based on this pass —
H1/H4/H5/H6's favorable and now-quantified results and H2's pass are
encouraging but rest on one tenant model (real category partitions, or
controlled-stress replicas of them), one machine configuration, and (for
H6) a short-lived-process floor rather than a genuinely resident
server's real cost; a full economic cost model should wait for the
long-running-process measurement named above, since real service
overhead may be materially larger than H6's floor.

## What this decision does and does not claim

**Does claim**: in this single-process architecture, per-tenant memory
overhead is negligible and total memory cost tracks aggregate product
count, not tenant count, confirmed cleanly from the real 55-tenant scale
up to 6,500 tenants / ~4.93M products (H1 + H5, corrected and
adversarially/independently validated); one tenant's heavy concurrent
load does not cause material latency regression for another tenant
sharing the same process (H2, confirmed across repeated runs); a fixed
tenant's own throughput/latency does not degrade as the breadth of
other, distinct, concurrently-touched tenants grows up to WANDS' real
ceiling (H4, confirmed across repeated runs); the specific numeric
first-draft "27-590 KB per-tenant fixed cost" estimate and the
first-draft P7-E01 "~19.8x throughput increase" are both withdrawn — the
former a build-order/allocator artifact, the latter a workload-mix
artifact — neither a real property of the architecture.

A real, consistent per-OS-process baseline (~2.1-2.2 MB) exists that a
one-process-per-tenant deployment pays once per tenant while H1/H5's
pooled design pays once total — the first measured (not assumed)
evidence for `docs/WHY.md`'s statistical-multiplexing thesis (H6,
reproduced across 3 runs); a second self-caught pair of implementation
bugs (materializing all 55 tenants before selecting one; parsing the
whole shared file regardless of which tenant was requested) is withdrawn
and documented alongside the fix.

**Does not claim**: that H6's short-lived-process measurement represents
a genuinely long-running resident server's real cost (very likely
higher — connection handling, logging/metrics, warm caches not
captured); that the small cross-vs-same-tenant latency difference in H2
is understood; that 6,500 tenants is a discovered hardware or
architectural ceiling (it is a self-imposed safety bound — the real
ceiling is very likely materially higher and was deliberately not
pursued); that H4's no-degradation finding holds at the
hundreds-to-thousands tenant counts H5 reached for memory (H4 itself was
only tested up to WANDS' real 54-other-tenant ceiling); that aggregate
QPS under a realistic multi-tenant demand mix or a full economic
cost-per-tenant model (both explicitly named in Issue #21's Phase 7)
have been answered — this is a first pass on memory (including at
scale), pairwise isolation, fixed-tenant throughput-under-breadth, and a
process-baseline floor only.

**Decision: PROCEED** to the next Phase 7 sub-experiment (quantifying a
genuinely long-running resident process's real overhead beyond H6's
floor, then combining it with H1/H5's in-process result into a first
real economic cost-per-tenant model) without changing the underlying
commerce-native mechanism. The favorable, adversarially-corrected H1
result, its clean confirmation at scale via H5, the robust H2/H4
results, and H6's first real measurement of the pooling advantage this
project's own thesis assumed are real evidence in favor of the
architecture's packing-density potential, but are explicitly a floor on
the claim (single-process or short-lived-process measurements, one
tenant model, one self-imposed safety bound), not a ceiling on what
remains to be tested.
