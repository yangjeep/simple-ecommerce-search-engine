# Phase 7 Decision (Issue #21 Phase 7) — P7-E00 through P7-E06 first pass

**Decision: PROCEED**, with two hypotheses falsified (one in the good
direction, one revealing a real but practically tiny effect) and six
hypotheses confirmed — one with a small, honestly-disclosed open
question, five cleanly across repeated runs. The most important new
finding (H6) is the first real, measured evidence for this project's own
opening "statistical multiplexing" thesis: pooling tenants in one
process has a real, quantifiable cost advantage over process-per-tenant
isolation. A follow-on finding (H7) shows that advantage is even larger
than H6 alone suggested: a genuinely long-running, actively-serving
process costs more than H6's short-lived snapshot captured. A further
follow-on (H8) confirms H7's figure is a stable, decelerating-toward-a-
plateau measurement rather than a transient artifact of too short a
window. A final follow-on (H9) is Phase 7's first direct test of Issue
#21's explicitly-named "cold tenant overhead" metric: it found a real,
reproducible ~9-13x latency-ratio effect between an infrequently-queried
tenant and a same-sized continuously-queried one — technically
falsifying the stated hypothesis — but at an absolute scale (tens of
microseconds) almost certainly negligible next to any real deployed
service's actual request latency, a distinction this document is
explicit about rather than leading with the more dramatic ratio alone.

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
- **H7** (P7-E04): a genuinely long-running resident process's RSS,
  measured while it is still alive and actively serving, is materially
  higher than H6's immediate spawn-and-exit snapshot — closing the gap
  every prior Phase 7 unresolved-risk note named between H6's
  short-lived-process floor and a real deployed service's actual cost.
- **H8** (P7-E05): H7's still-rising RSS curve for the largest real
  tenant decelerates toward a plateau given a much longer (9x) resident
  window, rather than growing without bound — distinguishing a bounded
  allocator/arena warm-up from a genuine leak, and confirming H7's
  figure is a stable input rather than a transient artifact of too
  short a measurement window.
- **H9** (P7-E06): a cold (infrequently-queried) tenant's own p50/p99
  latency, measured against a same-sized hot (continuously-queried)
  tenant's own p50/p99 under realistic multi-tenant background load,
  does not show material degradation — Issue #21's explicitly-named
  "cold tenant overhead" metric, untested by any prior Phase 7
  hypothesis.

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

**H7 — CONFIRMED, reproduced across 3 independent runs (P7-E04).**
Closes the gap H6's own limitations section named: a genuinely
long-running resident process (worker threads held alive for a fixed
20-second window, sampled every 5 seconds, rather than exiting
immediately) shows PEAK in-service RSS materially higher than H6's
immediate spawn-and-exit snapshot. Idle-resident (no tenant data, 4
parked worker threads) grows by an exactly-reproducible **244.0 KB**
(11.2% of its own baseline) in all 3 runs — below the pre-registered 20%
idle threshold on its own. Active-resident (one real tenant, 4 worker
threads continuously serving real structural queries) clears the 200 KB
material threshold decisively for the realistic case: the largest real
tenant (Furniture, 16,039 products) shows peak growth of **896-900 KB**
across all 3 runs — comparable to ~42% of H6's entire per-process
baseline, on top of Furniture's own ~53 MB in-process footprint — while
near-empty tenants show a smaller, also exactly-reproducible **196 KB**
peak growth in every run. The per-sample trace shows Furniture's RSS
still climbing (decelerating, not yet fully plateaued) across the whole
20-second window, while near-empty tenants' RSS jumps once in the first
5 seconds and then stays perfectly flat.

**P7-E04 self-caught methodology issue (documented for the process
record)**: the first-draft binary used RSS measured AFTER the resident
worker threads had already been joined (torn down) as its primary
metric, and got a deeply counterintuitive result — the largest tenant
showed NEGATIVE "growth" (-472 to -480 KB) while near-empty tenants
showed positive growth despite doing far less real work. Inspecting the
newly-added per-sample printout (added specifically to make the growth
curve's shape auditable, not just its endpoints) showed the post-join
reading was actively misleading: joining worker threads reclaims real
memory (thread stacks, likely allocator-arena high-water marks from
sustained churn), so it understates or reverses the true in-service
peak for any tenant whose threads did substantial real work. Fixed by
using the PEAK of the in-service periodic samples as the primary metric,
keeping the post-teardown reading only as a labeled secondary datum
about shutdown behavior — excluded from the H7 verdict, since a real
long-running service never tears down its worker pool mid-life.

**Named limitations**: the specific allocator mechanism behind the
growth is a plausible, disclosed hypothesis (thread-local arena
high-water marks from sustained alloc/dealloc churn), not profiled and
confirmed. A secondary, exactly-reproducible +128 KB post-teardown bump
(present in every idle/near-empty-tenant run) is named as an open,
unconfirmed observation. Only a 20-second window at 4 worker threads was
tested; whether Furniture's still-rising curve would keep growing over a
much longer real service lifetime, or plateau shortly past this window,
is untested.

**What this means for the economic model**: H7 does not weaken H6's
qualitative conclusion — it strengthens it. The true per-process cost a
one-process-per-tenant deployment would pay is higher than H6's
short-lived floor alone suggested: negligible additional cost for
near-empty tenants (+196 KB) but a real, substantial addition for
tenants under genuine sustained load (+896-900 KB for the largest real
tenant). Pooling avoids paying this too.

**H8 — CONFIRMED, reproduced across 3 independent runs (P7-E05).**
Answers the question H7's own limitations section raised: extending the
resident window 9x (20s -> 180s, sampled every 15s instead of 5s) shows
Furniture's RSS growth decelerates sharply rather than continuing at a
similar rate. Roughly 98% of the full window's growth happens in the
FIRST half (976-1,004 KB out of a 1,004-1,024 KB total across 3 runs),
with only 20-28 KB accruing in the entire second half — a ~35-50x
deceleration between halves, in every run, clearing the pre-registered
"second-half growth at most half of first-half growth" bar decisively.
Idle-resident's finding is now confirmed completely stable over the same
9x longer window: RSS jumps once at the first 15-second sample and then
stays perfectly flat for the remaining 165 seconds, identically across
all 3 runs (zero further growth).

**Named, honestly-disclosed residual**: the deceleration is very strong
but not perfectly flat by t180s — 2 of the 3 runs show a small,
still-positive tail creep (a few KB every 15-30 seconds in the last
30-60 seconds of the window), roughly two orders of magnitude smaller
than the initial climb. This does not change the H8 verdict but means
"fully plateaued" is not claimed — only "decelerating toward what looks
like a bound." Whether this tiny residual eventually stops is untested.

**What this means for the economic model**: H8 does not change H7's
qualitative or quantitative conclusion — it strengthens confidence that
H7's ~896-1,024 KB peak-growth figure for the largest real tenant is a
stable input for a capacity model, not a transient artifact of too short
a measurement window.

**H9 — FALSIFIED by the pre-registered ratio threshold, reproduced
across 3 runs, but at a practically tiny absolute scale (P7-E06).**
Issue #21 explicitly names "cold tenant overhead" as a required Phase 7
measurement; nothing in H1-H8 tested it directly. Two SIZE-MATCHED real
tenants (both 5 products, isolating query FREQUENCY from tenant size)
were measured under realistic 4-thread background contention: one
("hot") queried continuously, the other ("cold") queried once every
100ms. Cold tenant p99 latency was **12.68-12.88x** the hot tenant's
across all 3 runs, decisively clearing the pre-registered 2x threshold
— and the p50 ratio (8.85-10.00x) was nearly as large, ruling out a
rare-tail-outlier explanation in favor of a genuine, systematic shift
across the whole cold-tenant latency distribution.

**Critical context**: every latency involved, hot and cold alike, is on
the order of MICROSECONDS (1.3-13.0 microseconds) — three to four
orders of magnitude below typical real-world network/application
request latency. The absolute cold-tenant penalty (~10-30 microseconds)
is almost certainly negligible next to any real deployed service's
actual per-request overhead, none of which Phase 7 measures. The
leading, disclosed-but-unconfirmed mechanism is CPU cache locality (not
any explicit software-level cache this architecture manages) — a real,
physical, hardware-level "cold tenant" cost orthogonal to the "no shared
software state to lose" reasoning H2's isolation finding already
established.

**Named limitations**: only one size-matched tenant pair (both 5
products) and one cold-query interval (100ms) were tested; whether the
ratio holds at other tenant sizes, or scales with cache staleness, is
untested and named as a natural follow-up.

Full tables, raw CSVs/logs: `docs/experiments/PHASE7_LOG.md`,
`docs/research/artifacts/p7_e00_tenant_packing_run1/`,
`docs/research/artifacts/p7_e01_qps_scaling_run1/`,
`docs/research/artifacts/p7_e02_packing_ceiling_run1/`,
`docs/research/artifacts/p7_e03_cross_process_run1/`,
`docs/research/artifacts/p7_e04_long_running_run1/`,
`docs/research/artifacts/p7_e05_extended_duration_run1/`,
`docs/research/artifacts/p7_e06_cold_tenant_overhead_run1/`.

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
implementation bugs" section. P7-E04's first-draft primary metric (RSS
measured after worker-thread teardown, which produced a nonsensical
negative "growth" for the largest tenant) is preserved the same way in
`PHASE7_LOG.md`'s "P7-E04 self-caught methodology issue" section, along
with the superseded raw CSVs/logs
(`docs/research/artifacts/p7_e04_long_running_run1/*_postjoin_metric_superseded.*`),
renamed rather than deleted.

## Unresolved risks

1. **The small, consistent cross-tenant-vs-same-tenant latency
   difference (H2) has no confirmed mechanism.** A cache-locality
   hypothesis is plausible but unverified; would need profiling to
   confirm.
2. **H3's real-category ceiling (55) never reached any hardware limit,
   but H5's controlled-stress replication answered the same question by
   proxy up to a self-imposed 6 GB safety cap (6,500 tenants).** The
   actual hardware/architectural ceiling (if one exists at all before
   available RAM runs out) remains genuinely untested beyond that point
   — this experiment deliberately stopped short of finding it rather
   than risk a real OOM in a shared container.
3. Only one tenant model (real category-based partitions of one real
   catalog) has been tested. Real SaaS tenants would have completely
   independent catalogs (not partitions of the same source), likely with
   more genuinely-independent schema/vocabulary divergence than this
   experiment's fix (independent ID-interning per tenant) fully
   captures.
4. **H6/H7/H8 measure memory only.** A real capacity model would also
   need per-process CPU/scheduling overhead, which this experiment did
   not attempt.
5. **H8 confirms H7's growth decelerates sharply over a 9x longer
   window, but a small, real residual tail creep persists in 2 of 3
   runs.** The specific allocator mechanism behind both H7's growth and
   this residual (thread-local arena high-water marks from sustained
   alloc/dealloc churn is the leading, disclosed, but unconfirmed
   hypothesis) has not been profiled. Whether the residual eventually
   stops, continues at a much slower rate, or behaves differently over a
   real service's minutes-to-hours lifetime (vs. this experiment's
   180-second window) is untested.
6. **H9's cold-tenant-overhead mechanism (CPU cache locality) is
   disclosed, not profiled or confirmed.** Only one size-matched tenant
   pair (both 5 products) and one cold-query interval (100ms) were
   tested; whether the ~9-13x ratio holds at other tenant sizes, or
   scales with how stale the cache is, is untested.

## What would be built next if scaling up

**A first-pass economic cost-per-tenant model is now done** — see
`docs/research/PHASE7_ECONOMIC_MODEL.md`, which combines H1/H5's
in-process marginal cost, H6's per-process floor, and H7/H8's
long-running active-serving overhead into an explicit pooled-vs-isolated
deployment cost formula, with worked examples at the real 55-tenant
scale and up to 6,500 controlled-stress-replicated tenants. It addresses
4 of Issue #21's 7 required "Economic output" metrics well, one
partially (a memory-only "cost per million requests" proxy, shown to be
highly window-length-sensitive), and names 2 as explicit, undelivered
gaps (tenants-per-envelope-at-SLO; backend requests avoided) rather than
silently omitting them.

**Cold-tenant overhead (Issue #21's explicit metric) is now measured**
(H9, P7-E06) — a real, reproducible ~9-13x latency-ratio effect between
an infrequently-queried tenant and a same-sized continuously-queried
one, at a practically tiny absolute scale (tens of microseconds),
plausibly attributable to CPU cache locality rather than any explicit
software-level cache this architecture manages.

Still to build: an aggregate throughput-under-realistic-load experiment
(P7-E01 tested breadth of touched tenants at fixed per-tenant demand,
not aggregate QPS at a realistic multi-tenant demand mix, which Issue
#21's "per-tenant and aggregate QPS" metric also asks for); extending H4
(query throughput under breadth) to the hundreds-to-thousands tenant
counts H5 already reached for memory; combining Phase 7's memory model
with H2/H4's latency/isolation evidence to produce the still-missing
"tenants per envelope at target SLO" metric; combining Phase 3/4's
admission-rate evidence with a multi-tenant request-volume model to
produce "backend requests avoided"; profiling to identify the specific
allocator mechanism behind H7/H8's growth pattern and residual tail
creep, and the CPU-cache-locality hypothesis behind H9's cold-tenant
effect, if a real deployment's memory/latency budget needs tighter
precision than "decelerates toward roughly a known bound" / "plausibly
cache locality."

## What should explicitly not be built yet

No tenant-aware planner/admission changes based on this pass —
H1/H4/H5/H6/H7/H8's favorable and now-quantified results and H2's pass
are encouraging but rest on one tenant model (real category partitions,
or controlled-stress replicas of them), one machine configuration, and
(for H7/H8) only a 180-second resident window rather than a real
service's full lifetime; a full economic cost model can now reasonably
proceed given H8's confirmation that H7's figure is stable rather than
still-climbing, but should still disclose that a real service's much
longer lifetime (minutes to hours vs. this experiment's 180 seconds)
remains untested.

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
and documented alongside the fix. A genuinely long-running, actively-
serving resident process costs MORE than H6's short-lived snapshot alone
suggested: peak in-service RSS grows by an exactly-reproducible 244 KB
(idle, 3 runs) and by 196-900 KB (active, scaling with real per-tenant
query volume/data size, 3 runs) beyond the immediate post-load snapshot
(H7, reproduced across 3 runs) — strengthening, not weakening, H6's
pooling-advantage conclusion. A third self-caught methodology issue (the
first-draft H7 binary used a post-thread-teardown RSS reading as its
primary metric, which actively inverted the sign of the effect for the
largest tenant) is withdrawn and documented alongside the fix. Extending
the resident window 9x (20s -> 180s) shows that growth decelerates
sharply toward what looks like a bound rather than continuing at a
similar rate — roughly 98% of the total growth happens in the first half
of the window, in all 3 runs (H8) — confirming H7's figure is a stable
input, not a transient artifact of too short a measurement window; idle-
resident's finding is now confirmed completely stable with zero further
growth over the same 9x longer window.

A cold (infrequently-queried) tenant's own latency IS measurably worse
than a same-sized hot (continuously-queried) tenant's — 12.68-12.88x at
p99, 8.85-10.00x at p50, reproduced across 3 runs (H9, technically
falsifying the stated no-material-degradation hypothesis) — but the
absolute magnitude (~10-30 microseconds on top of 1.3-13.0-microsecond
baseline latencies) is almost certainly negligible next to any real
deployed service's actual request latency, and this document is explicit
about that distinction rather than leading with the more dramatic ratio
alone. A first-pass economic cost-per-tenant model
(`docs/research/PHASE7_ECONOMIC_MODEL.md`) combines H1/H5/H6/H7/H8 into
an explicit pooled-vs-isolated deployment cost formula, addressing 4 of
Issue #21's 7 required "Economic output" metrics well, one partially,
and naming 2 as explicit undelivered gaps.

**Does not claim**: that the small cross-vs-same-tenant latency
difference in H2 is understood; that 6,500 tenants is a discovered
hardware or architectural ceiling (it is a self-imposed safety bound —
the real ceiling is very likely materially higher and was deliberately
not pursued); that H4's no-degradation finding holds at the
hundreds-to-thousands tenant counts H5 reached for memory (H4 itself was
only tested up to WANDS' real 54-other-tenant ceiling); that H8's
180-second resident window represents a real service's full-lifetime
steady state (a small, real residual tail creep persists in 2 of 3 runs,
roughly two orders of magnitude smaller than the initial climb, and
whether it fully stops over a much longer real lifetime is untested);
that the specific allocator mechanism behind H7/H8's growth is confirmed
(a thread-local-arena hypothesis is named, not verified); that H9's
cache-locality mechanism is confirmed (named, not profiled), or that its
~9-13x ratio generalizes beyond the one size-matched tenant pair and one
cold-query interval tested; that aggregate QPS under a realistic
multi-tenant demand mix, "tenants per envelope at target SLO," or
"backend requests avoided" (all explicitly named in Issue #21's Phase 7)
have been answered — this is a first pass on memory (including at
scale), pairwise isolation, fixed-tenant throughput-under-breadth,
process-baseline floors (short-lived, and a longer-resident window that
decelerates toward but has not been proven to fully reach a bound), a
first cold-vs-hot latency comparison, and a first economic-model
synthesis only.

**Decision: PROCEED** to the next Phase 7 sub-experiment (aggregate QPS
under a realistic multi-tenant demand mix; combining H2/H4 with the
economic model for an SLO-conditioned tenant count; or combining Phase
3/4's admission-rate evidence for "backend requests avoided") without
changing the underlying commerce-native mechanism. The favorable,
adversarially-corrected H1 result, its clean confirmation at scale via
H5, the robust H2/H4 results, H6/H7/H8's real, reproduced,
now-stability-confirmed measurement of the pooling advantage this
project's own thesis assumed, the first economic cost-per-tenant model,
and H9's honestly-scaled cold-tenant finding are real evidence in favor
of the architecture's packing-density and latency-predictability
potential, but are explicitly a floor on the claim (single-process,
short-lived-process, or 180-second-resident-process measurements, one
tenant model, one self-imposed safety bound, one size-matched
hot/cold pair), not a ceiling on what remains to be tested.
