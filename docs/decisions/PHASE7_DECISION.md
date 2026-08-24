# Phase 7 Decision (Issue #21 Phase 7) — Terminal Decision (P7-E00 through P7-E12)

**Decision: PROCEED** — Issue #21's Phase 7 required "Experiments" list
(15 falsifiable hypotheses, H1-H15) and all 7 "Economic output" metrics
are now fully addressed (6 delivered well, 1 partial). This is the
terminal decision for the Phase 7 measurement campaign: no further
required experiment remains untested, so this document closes the
campaign rather than opening another sub-experiment. See the "Measured
results" quick-scan table below for every hypothesis at a glance, and
"What this decision does and does not claim" for the exact boundary of
what is asserted.

**The headline, in three parts:**

1. **The core packing/pooling thesis holds robustly.** H1/H5 confirm
   per-tenant memory overhead is negligible and total memory tracks
   aggregate product count (not tenant count) cleanly from 55 real
   tenants to 6,500 controlled-stress-replicated tenants. H2/H4/H11
   confirm the native in-process query path shows no material
   cross-tenant degradation, holding cleanly up to 2,000
   controlled-stress-replicated tenants (36x WANDS' real ceiling). H6 is
   this project's first real, MEASURED evidence for its own opening
   "statistical multiplexing" thesis (`docs/WHY.md`): pooling tenants in
   one process has a real, quantified cost advantage over
   process-per-tenant isolation, and H7/H8 show that advantage is even
   larger and more stable than the first snapshot suggested. H12 combines
   the memory and latency evidence into a concrete, empirically-reached
   answer for Issue #21's "tenants per fixed hardware envelope at target
   SLO": ~3,500 query-capable tenants under a disclosed 9 GB envelope on
   this container, discovering this container's real 13.34 GiB memory
   limit directly along the way (after a self-caught first-draft OOM).
   The economic model (`docs/research/PHASE7_ECONOMIC_MODEL.md`) now
   answers all 7 of Issue #21's named "Economic output" metrics.

2. **Two nuanced, honestly-scaled findings temper the picture without
   overturning it.** H9 found a real ~9-13x cold-tenant latency-ratio
   effect, but at an absolute scale (microseconds) almost certainly
   negligible next to real request latency — Issue #21's "cold tenant
   overhead" metric, answered with the magnitude kept in view rather
   than led with alone. H10's replication check found the DIRECTION of
   H9's effect holds under a more realistic demand mix, but the
   MAGNITUDE shrinks ~4-6x, pointing at H9's idealized dedicated-thread
   design (not a general property) as the likely cause. H13 found CPU
   cost per query does NOT scale linearly with tenant size the way
   memory does — sub-linear at tiny tenant sizes, then measurably
   super-linear for the largest real tenant — a genuinely new
   capacity-planning nuance no memory-focused measurement could have
   surfaced.

3. **Two real, unmitigated isolation gaps were found and are not
   smoothed over.** H14: a co-located tenant undergoing repeated index
   REBUILDS (this architecture's only mutation path, since tenant
   bundles are immutable) degrades a low-churn tenant's own p99 latency
   by 4.00-6.70x — a real risk pure query-load testing (H2) could never
   have surfaced. H15: sharing one Solr instance across tenants degrades
   a quiet tenant's own p99 latency by 2.16-2.48x under ordinary query
   load — the native path is safe, but the shared lexical backend is
   not. Neither gap has a designed or tested mitigation; both are named
   explicitly as necessary future work in "What should explicitly not be
   built yet" below, not assumed away.

This is the first phase in this project's history to build and measure
more than one tenant's index in the same process, the first to spawn
real separate OS processes to test the pooling-vs-isolation thesis with
numbers, and the first to touch an external lexical backend (Solr) from
inside the multi-tenant harness. It does not require any of the
currently-blocked external resources (Retailrocket, H&M, Amazon Reviews
2023, Havenask) — it is built entirely over the real WANDS catalog
already validated in Phase 6A/6B, plus the Solr installation and cores
already present in this environment from that same work.

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
- **H10** (P7-E07): H9's cold/hot p99 ratio replicates (clears the same
  2x threshold) when the same size-matched pair is embedded in a single
  shared, Zipfian-weighted query stream spanning all 55 real tenants at
  once, rather than H9's simpler dedicated-thread design — a direct test
  of whether H9's finding is a general architectural property or an
  artifact of its specific methodology, and Issue #21's explicitly-named
  "aggregate QPS," "fairness under skewed tenant load," and "hot tenant
  saturation" metrics, untested by any prior Phase 7 hypothesis.
- **H11** (P7-E08): H4's finding (a fixed tenant's own throughput/latency
  does not degrade as breadth of other touched tenants grows) continues
  to hold when breadth is extended via H5's controlled-stress
  replication methodology far beyond WANDS' real 54-tenant ceiling, into
  the hundreds-to-thousands (matching H5's memory-scale reach) — closing
  a gap this document itself had previously named as still open.
- **H12** (P7-E09): at the largest tenant count this container's real,
  disclosed hardware envelope can safely support for a QUERY-CAPABLE
  deployment (both `Catalog` and `CatalogIndex` resident per tenant,
  unlike H5's index-only measurement), the quiet tenant's own throughput
  and latency stay within Phase 7's material-regression bar relative to
  the n=55 real-tenant baseline — directly answering Issue #21's
  "tenants per fixed hardware envelope at target SLO" metric.
- **H13** (P7-E10): this architecture's per-query CPU cost (user+system
  time) for a facet-scan operation scales roughly linearly with tenant
  product count, mirroring H1/H5's clean linear memory-scaling finding
  — directly answering Issue #21's "CPU/query and CPU/tenant" metric,
  untested by any prior Phase 7 hypothesis (all of which measured
  wall-clock only).
- **H14** (P7-E11): a high-churn tenant (one whose `CatalogIndex` is
  repeatedly rebuilt and hot-swapped in a shared process) does not
  measurably degrade a separate low-churn tenant's own query
  latency/throughput, matching H2's own established finding for pure
  QUERY load — directly answering Issue #21's "high-churn tenant impact
  on low-churn tenants" metric, untested by any prior Phase 7 hypothesis
  (all of which used static, once-built catalogs).
- **H15** (P7-E12): one tenant's heavy Solr query load does not
  materially degrade another co-located tenant's own Solr query
  latency, when both tenants' lexical-fallback traffic shares the same
  Solr instance — the Solr-side analog of H2's native-path isolation
  finding, directly answering Issue #21's "lexical-backend contention"
  metric, the last item on Issue #21's required "Experiments" list.

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

Quick-scan summary (full detail, raw data, and named limitations for
every row below):

| Hyp. | Question | Verdict | Headline number |
|---|---|---|---|
| H1 | Per-tenant memory fixed cost | FALSIFIED (favorably) | Negligible; cost tracks aggregate product count, not tenant count |
| H2 | Cross-tenant query-load isolation (native, in-process) | CONFIRMED | p99 ratio 1.31-1.43x cross-tenant vs. 0.85-1.16x same-tenant control |
| H3 | Packing ceiling (real 55-tenant partition) | Not directly reached; answered by proxy via H5 | Real partition tops out at ~50 MB, far under any limit |
| H4 | Fixed-tenant throughput under breadth (native) | CONFIRMED | No trend from 1 to 54 other tenants, 694-816 rps / 1.71-2.16ms p99 |
| H5 | Memory scaling with controlled-stress tenant replication | CONFIRMED | Linear to 6,500 tenants / 4.93M products, 1.2558-1.2881 KB/product |
| H6 | Per-OS-process floor vs. in-process pooling | CONFIRMED | ~2,144-2,152 KB per process, paid once by pooling vs. N times isolated |
| H7 | Long-running resident-process overhead | CONFIRMED | +244 KB idle, +196-900 KB active beyond H6's spawn-exit floor |
| H8 | Does H7's growth plateau over 9x longer window | CONFIRMED | ~98% of growth in first half; small residual tail creep in 2/3 runs |
| H9 | Cold-tenant overhead (simple dedicated-thread design) | FALSIFIED (tiny absolute scale) | 12.68-12.88x p99 ratio, but only 1.3-13.0 microseconds absolute |
| H10 | Does H9 replicate under a realistic Zipfian demand mix | Direction replicates; magnitude does not | 1.85-2.08x, ~4-6x smaller than H9's idealized design |
| H11 | H4 extended to memory-scale tenant counts (2,000) | CONFIRMED | Throughput drop 6-9%, p99 growth 5-8%, both inside the pass bar |
| H12 | Tenants per fixed hardware envelope at target SLO | CONFIRMED | ~3,500 query-capable tenants under a disclosed 9 GB envelope |
| H13 | CPU-seconds/query scaling with tenant size | FALSIFIED | Sub-linear at small n, super-linear at large n; Furniture 3.81x over a linear fit |
| H14 | High-churn (index-rebuild) tenant impact on low-churn tenant | FALSIFIED (material) | p99 degrades 4.00-6.70x; a real, unmitigated isolation gap |
| H15 | Lexical-backend (shared Solr) contention | FALSIFIED (material) | p99 degrades 2.16-2.48x; a real, unmitigated isolation gap |

Plus a synthesis, not a new hypothesis: the economic cost-per-tenant
model (`docs/research/PHASE7_ECONOMIC_MODEL.md`), now addressing all 7
of Issue #21's named "Economic output" metrics (6 delivered, 1
partial), including the "backend requests avoided" combination with
Phase 3/4's admission-rate evidence.

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

**H10 — magnitude does NOT cleanly replicate H9, but the direction
does, in every run (P7-E07).** Tests whether H9's ~9-13x effect is a
general architectural property or an artifact of H9's specific
fully-dedicated-thread design: the same size-matched pair was embedded
in a single shared Zipfian-weighted (weight ∝ 1/rank) query stream
spanning all 55 real tenants at once, with the pair's weights
overridden to the population's max/min (~55x apart).

A first-draft 15-second run self-caught a real statistical problem
before any ratio was trusted: the cold tenant received only 62-63
samples, and with n=62 the reported p99 (essentially the value of the
single highest sample) swung 1.53x-5.50x across 3 runs using an
*identical* deterministic query sequence — clear evidence the statistic
was unstable, not that the effect itself varied. Fixed by raising the
run duration 8x (15s → 120s), yielding 487-501 cold samples per run,
comparable to H9's own ~300-sample design.

With adequate sampling, the ratio stabilizes to **1.85x-2.08x** across
all 3 runs — hovering almost exactly on the pre-registered 2.0x
threshold (1 of 3 runs technically clears it). By the letter of the
pre-registered "all runs must clear 2.0x" criterion, H10 does **not**
replicate H9 as a clean pass. But this is not "no effect": the p50
ratio is an exact, identical 2.083x in every single run, and the p99
ratios cluster tightly around that same value (unlike the first
draft's noisy swing) — a real, small, highly reproducible effect,
**roughly 4-6x smaller** than H9's originally-observed 9-13x ratio, not
one that vanished. Aggregate throughput (1,320-1,345 rps, tight across
runs) is explicitly NOT compared to H4/P7-E01's number, since which
tenants are hot/cold and their per-query cost dominates it — the same
workload-mix-sensitivity caveat P7-E01's own first draft had to learn.

**Named, disclosed-but-unconfirmed hypothesis for the magnitude gap**:
H9's hot tenant ran on a fully dedicated thread with zero interleaving
(an artificially ideal, maximally cache-warm setup); here, every thread
constantly interleaves hot/cold queries with all 53 other real tenants
via the same shared stream, likely diluting even the hot tenant's own
cache locality and narrowing the gap from both ends. Plausible, not
profiled or confirmed.

**What this means**: H9's underlying finding is real and its direction
replicates under a materially more realistic full-population
query-arrival pattern, but a realistic, shared, interleaved worker-pool
architecture — the one this project would actually deploy — shows a
substantially smaller effect (~2x, right at the material-regression
line) than H9's idealized dedicated-thread design (~9-13x). Both
figures are now part of the honest record.

**H11 — CONFIRMED, reproduced across 3 independent runs (P7-E08).**
Directly closes this document's own previously-named gap: does H4's
breadth-independence finding, only tested up to WANDS' real
54-other-tenant ceiling, also hold at the much larger tenant counts H5
reached for memory? Using the exact same quiet/noisy-tenant
methodology as H4, with breadth extended via H5's controlled-stress
tenant-count replication (`-copyN`-suffixed repeats of the real
55-tenant population, holding per-tenant data/schema shape fixed), the
quiet tenant's own throughput and p99 latency were measured at 5
breadth levels: 55, 200, 500, 1,000, and 2,000 total tenants.

Throughput ratio at n=2,000 vs. the n=55 baseline: **0.905-0.942**
across all 3 runs (a consistent 6-9% reduction). p99 ratio at n=2,000
vs. n=55: **1.055-1.085** (a consistent 5-8% growth). Both stay
comfortably inside the pre-registered pass bar (throughput drop <20%,
p99 growth <2x) in every run. **H4's finding generalizes cleanly to
2,000 tenants — a 36x larger breadth than WANDS' real ceiling, and the
same order of magnitude H5 reached for memory.** As a secondary
cross-check (not this experiment's primary claim), RSS grew linearly
with tenant count in all 3 runs at ~2.7-3.0 MB/tenant, consistent with
H5's own per-tenant memory-scaling finding rather than contradicting
it.

**Named, honestly-disclosed limitation**: a small, consistent
throughput dip and p99 uptick appear specifically at n=2,000 in every
run — the ladder's largest single-step change, not evenly spread across
the whole range — right as RSS reached ~5.48 GB, ~91% of this run's 6
GB safety cap. Because the effect stays well inside the pass
thresholds it does not change the H11 verdict, but this run cannot
fully rule out memory-pressure effects (cache/TLB pressure, allocator
fragmentation as RSS nears the cap) as a contributing factor at n=2,000,
as distinct from a pure tenant-count/breadth effect. This joins H7/H8's
allocator-arena hypothesis and H9/H10's cache-locality/interleaving-
dilution hypothesis as a disclosed, unconfirmed mechanism candidate for
future profiling.

**H12 — CONFIRMED, reproduced across 3 independent runs (P7-E09).**
Directly answers Issue #21's "tenants per fixed hardware envelope at
target SLO" metric for the first time this phase. H5's own 6,500-tenant
memory ceiling measures an INDEX-ONLY configuration (the raw `Catalog`
is dropped immediately after each tenant's index is built); a real
query-serving deployment needs both `Catalog` and `CatalogIndex`
resident per tenant, a materially higher real per-tenant footprint. A
first-draft binary reused P7-E08's eager "build all N catalogs, then
build all N indexes" pattern and was OOM-killed at n=6,500 by this
container's real cgroup memory limit — **14,327,726,080 bytes
(13.34 GiB)**, read directly from `/sys/fs/cgroup/memory/.../
memory.limit_in_bytes`, materially lower than the ~15 GB host-level
figure `free -h` reports and this project's prior safety-cap choices
had implicitly assumed. Fixed by rebuilding incrementally (one
tenant's `Catalog`+`CatalogIndex` at a time, mirroring H5/P7-E02's own
proven-safe pattern), checking real RSS every 250 tenants during
construction rather than once after a whole batch.

The corrected binary safely built up to **exactly n=3,500** in all 3
runs (a 9 GB safety cap, chosen with real margin under the 13.34 GiB
hard limit, tripped at precisely the same point every time —
deterministic, since replication uses real, ordered data with no
randomness). A second self-caught issue surfaced here too: the very
first in-process latency checkpoint (n=55) showed an unstable p99
across runs (1.777/4.104/2.755 ms) while its p50 stayed tight
(1.290/1.289/1.286 ms) — a cold-start artifact specific to being the
first measurement taken against freshly-built structures, not a real
per-run difference (later checkpoints' p99s were tight across runs).
p50 is used as the primary metric accordingly.

p50 ratio at n=3,500 vs. n=55: **0.989-1.019** across all 3 runs —
essentially flat. Throughput ratio: **0.951-1.035** — also flat. Both
stay nowhere near the pre-registered pass bar (throughput drop <20%,
latency growth <2x). **H12 CONFIRMED**: ~3,500 tenants is the real,
empirically-reached answer to "tenants per fixed hardware envelope at
target SLO" for this container's disclosed 9 GB query-serving envelope
— materially lower than H5's own 6,500-tenant figure, because that
figure describes a configuration that cannot actually serve queries.
Per-tenant memory footprint computed to ~2.66 MB/tenant, consistent
with H11's own ~2.7-3.0 MB/tenant figure and about 2.8x H5's
index-only ~0.96 MB/tenant implied figure — confirming the
"index-only vs. query-capable" distinction with an independent number.

**Named limitations**: the 9 GB cap is a deliberately conservative,
self-imposed choice with real margin under the 13.34 GiB hard limit,
not a claim about this container's absolute ceiling — a deployment
with less reserved margin could plausibly push higher, untested here.
Only one quiet tenant, one query type, and one hardware envelope (this
container) were tested; dollar-cost implications are deliberately kept
separate from this architecture-normalized tenant count, per Issue
#21's own instruction.

**H13 — FALSIFIED, reproduced across 3 independent runs (P7-E10).**
Answers Issue #21's "CPU/query and CPU/tenant" metric for the first
time this phase — every prior Phase 7 experiment measured wall-clock
only. Reusing H6/H7/H8/H9's exact 3 real tenant sizes
(largest/middle/smallest), CPU time was measured single-threaded, with
no concurrent noisy load (CPU-time accounting via `/proc/self/stat` is
process-wide, so concurrent load would contaminate the signal).

A first sanity check passed cleanly: the CPU/wall-clock ratio was
0.997-1.002 for every tenant in every run — essentially exactly 1.0,
as expected for a single-threaded, CPU-bound, uncontended loop with no
I/O inside `facet_scan_once`, validating the `/proc/self/stat`-based
CPU-time method before trusting any comparison built on it.

H13 itself (linear CPU scaling with product count) does NOT hold:
from 1 to 5 products (5.0x), CPU/query grows only 2.74x — sub-linear,
consistent with a fixed per-query overhead dominating at tiny sizes
(the same shape H1's "near-empty tenants show flat marginal memory
cost" finding has, now shown for CPU). From 5 to 16,039 products
(3,207.8x), CPU/query grows 9,694.6x — clearly super-linear. A
straight-line fit through the two small points predicts ~2,767
us/query for Furniture; the actual measured cost is **10,534.77
us/query — 3.81x higher**, reproduced consistently (under 1% spread
across all 3 runs). **Unlike memory (H1/H5's clean linear scaling),
CPU cost per query is NOT well-described by a single linear law across
this size range** — a capacity model estimating CPU cost purely from
aggregate product count would underestimate the largest real tenant's
true cost.

**Illustrative CPU/tenant figure** (matching the "per million requests"
convention already used elsewhere in Phase 7's economic model, for
comparability): ~0.40 CPU-seconds per million queries for a near-empty
tenant, ~1.09 for a 5-product tenant, and **~10,535 CPU-seconds
(~2.93 CPU-hours) per million queries for Furniture** — an illustrative
rate, not combined with any real measured per-tenant request volume
(Phase 7 has never measured one, the same disclosure the
backend-requests-avoided synthesis made).

**Named, disclosed-but-unconfirmed mechanism hypothesis**:
`facet_counts_by_scan` groups candidates by their `color` value; cost
may depend on the number of DISTINCT color values touched, not just
candidate count, and a large diverse catalog like Furniture plausibly
has far more distinct colors than a tiny niche category — compounding
into faster-than-linear growth. Disclosed, not profiled or confirmed,
joining H7/H8's allocator-arena hypothesis and H9/H10's
cache-locality/interleaving hypothesis as a candidate for future
profiling.

**Named limitations**: only 3 real tenant sizes were tested (the exact
scaling shape between the sub-linear small-n region and the
super-linear large-n region is unknown); only one query type (facet
scan) was tested — other structural operators may scale differently.

**H14 — FALSIFIED, reproduced across 3 independent runs, a genuine
material effect (P7-E11).** Reuses H2's exact quiet-tenant methodology
(same reps, duration, and quiet tenant "Rugs") so this result is
directly comparable to H2's own null finding for pure query load. A
separate "high-churn" tenant (Furniture, the largest real tenant) has
its `CatalogIndex` continuously rebuilt and hot-swapped by a dedicated
thread with no sleep between rebuilds — real allocation/deallocation
churn, simulating this architecture's only mutation path (immutable
tenant bundles must be rebuilt, not updated in place).

| Run | Baseline p50/p99 (ms) | Under-churn p50/p99 (ms) | p50 ratio | p99 ratio |
|---|---|---|---|---|
| 1 | 1.524 / 1.908 | 1.745 / 12.793 | 1.15x | **6.70x** |
| 2 | 1.376 / 2.127 | 1.568 / 10.746 | 1.14x | **5.05x** |
| 3 | 1.346 / 1.925 | 1.575 / 7.702 | 1.17x | **4.00x** |

p50 shows only a mild, consistent ~14-17% slowdown — well inside the
pass bar. **p99 shows a real, reproducible 4.00-6.70x degradation in
every run**, decisively clearing the 2x threshold. Unlike this
project's several prior cases where an unstable p99 turned out to be a
measurement artifact (P7-E07's undersampling, P7-E09's cold-start
effect), this elevation is large, consistent in direction across all 3
runs, and has a mechanistically coherent explanation — **this is a
genuine falsification, not a technicality like H9's negligible-scale
one.**

A striking secondary finding: exactly 5 full index rebuilds completed
in the 5-second window in EVERY run — a single `CatalogIndex::build()`
call for Furniture (16,039 products) costs almost exactly **1 second**
of wall time, ~100x the cost of a single query against that same
tenant (H13's own ~10.5ms CPU/query figure). **A query-only noisy
neighbor (H2) and a rebuild-churning noisy neighbor (H14) are NOT the
same risk**: real commerce catalogs churn (price/inventory updates),
and this architecture's immutable-bundle model means every such update
pays a real, substantial rebuild cost that can materially degrade a
co-located tenant's own tail latency while in progress — a genuine
isolation gap none of H2/H4/H9/H10/H11 could have surfaced, since none
of them ever mutated a tenant's data.

**Named, plausible (not profiled) mechanism**: with only 4 real CPU
cores in this container and one thread continuously CPU-bound for
~1-second stretches, some quiet-tenant queries are plausibly scheduled
onto the same core as the churn thread, or contend for shared
caches/memory bandwidth/allocator locks during the rebuild's own heavy
allocation activity — consistent with p50 barely moving while p99
moves sharply.

**Named limitations**: only 5 discrete rebuild events occur per
5-second run — a small sample of "rebuild windows," likely why the p99
ratio itself varies more between runs (4.00x-6.70x) than p50 does. Only
one churn tenant (Furniture, the largest) and one quiet tenant (Rugs)
were tested; whether smaller tenants would show a proportionally
smaller effect, and whether many small tenants churning concurrently
would compound the effect, are both untested.

**H15 — FALSIFIED, reproduced cleanly across 3 independent runs, the
last item on Issue #21's required "Experiments" list (P7-E12).**
Feasibility was verified directly rather than assumed out of scope:
Solr 9.10.1 is already installed in this container with 5 real
WANDS-derived cores from Phase 6A/6B's scale-ladder work, so no
materially larger infrastructure was needed. Reuses H2's exact
quiet/noisy methodology, issuing real HTTP queries to Solr instead of
in-process `facet_scan_once`: `wands_bench` (42,994 docs) is the quiet
tenant, `wands_bench_20x` (859,880 docs, Phase 6B's largest rung) is
the noisy tenant.

A first-draft binary (no warm-up phase) gave an ambiguous, borderline
result: p50 ratio 1.29-1.30x (consistent) but p99 ratio 1.32x / 2.04x /
2.13x, straddling the 2.0x threshold. Inspecting the raw numbers found
run 1's baseline p99 (8.98ms) was more than 2x runs 2-3's (3.85ms /
3.57ms) — a real JVM/query-cache/connection-pool cold-start artifact in
the very first invocation, the same class of issue P7-E09 already
learned to guard against. **Fixed** with a 500ms warm-up phase before
the baseline measurement.

| Run | Baseline p50/p99 (ms) | Cross-tenant p50/p99 (ms) | p50 ratio | p99 ratio |
|---|---|---|---|---|
| 1 | 2.790 / 3.904 | 3.989 / 9.676 | 1.43x | **2.48x** |
| 2 | 2.702 / 3.624 | 3.301 / 7.818 | 1.22x | **2.16x** |
| 3 | 2.728 / 3.574 | 3.348 / 7.836 | 1.23x | **2.19x** |

Baseline p99 is now tight (3.57-3.90ms) across all 3 runs, confirming
the fix worked. p50 ratio (1.22-1.43x) stays comfortably inside the
pass bar. **p99 ratio (2.16-2.48x) clears the 2.0x threshold in every
run** — a clean, unambiguous falsification. **Like H14, and unlike H2:
a shared Solr instance is NOT safely isolated under noisy-neighbor
query load in this design.** This is Phase 7's THIRD distinct
isolation-gap finding: the native in-process QUERY path (H2, H4, H11)
is confirmed safe, but both the native REBUILD path (H14) and the
shared LEXICAL BACKEND path (H15) show real, material cross-tenant
degradation.

**Named, plausible (not profiled) mechanism**: Solr's own thread pool,
JVM garbage collection, and shared Lucene segment-merge/cache resources
are genuinely shared across cores in one JVM — unlike the native
path's fully independent, immutable per-tenant structures with no
shared mutable state.

**Named limitations**: only 2 cores and one query type (`rows=24` page
browse) were tested; only one noisy-worker count (3) was tested; this
container's Solr uses default, out-of-the-box configuration with no
per-core resource limits or dedicated hardware isolation a production
deployment might apply to mitigate this — untested here.

Full tables, raw CSVs/logs: `docs/experiments/PHASE7_LOG.md`,
`docs/research/artifacts/p7_e00_tenant_packing_run1/`,
`docs/research/artifacts/p7_e01_qps_scaling_run1/`,
`docs/research/artifacts/p7_e02_packing_ceiling_run1/`,
`docs/research/artifacts/p7_e03_cross_process_run1/`,
`docs/research/artifacts/p7_e04_long_running_run1/`,
`docs/research/artifacts/p7_e05_extended_duration_run1/`,
`docs/research/artifacts/p7_e06_cold_tenant_overhead_run1/`,
`docs/research/artifacts/p7_e07_realistic_demand_mix_run1/`,
`docs/research/artifacts/p7_e08_extended_breadth_run1/`,
`docs/research/artifacts/p7_e09_slo_tenant_envelope_run1/`,
`docs/research/artifacts/p7_e10_cpu_per_query_run1/`,
`docs/research/artifacts/p7_e11_high_churn_impact_run1/`,
`docs/research/artifacts/p7_e12_lexical_backend_contention_run1/`.

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
renamed rather than deleted. P7-E07's first-draft undersampled run (15s,
only 62-63 cold samples, producing a p99 ratio that swung 1.53x-5.50x
across 3 runs of an *identical* deterministic query sequence — clear
evidence of an unstable statistic, not a real per-run difference) is
preserved the same way in `PHASE7_LOG.md`'s "P7-E07 self-caught
statistical problem" section, with the superseded raw CSV/log
(`docs/research/artifacts/p7_e07_realistic_demand_mix_run1/*_15s_undersampled_superseded.*`)
renamed rather than deleted. P7-E09's first-draft OOM (an eager
build-then-check pattern that got this process killed by this
container's real 13.34 GiB cgroup memory limit) and its second
self-caught issue (an unstable p99 at the very first in-process
checkpoint, a cold-start artifact) are both documented in
`PHASE7_LOG.md`'s "P7-E09" sections. P7-E12's first-draft borderline
result (a JVM/connection-pool cold-start artifact inflating run 1's
baseline p99, making the verdict look ambiguous) is documented in
`PHASE7_LOG.md`'s "P7-E12 self-caught problem" section, with the
superseded raw CSV/log
(`docs/research/artifacts/p7_e12_lexical_backend_contention_run1/*_no_warmup_superseded.*`)
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
   than risk a real OOM in a shared container. H12/P7-E09 later
   discovered this container's actual enforced memory ceiling directly
   (13.34 GiB, read from this process's own cgroup) after a first-draft
   OOM at n=6,500 — materially lower than the ~15 GB host-level figure
   `free -h` reports and every prior Phase 7 safety-cap choice had
   implicitly assumed; H5's own 6 GB cap stayed comfortably under this
   real limit, so its result is unaffected, but future safety caps in
   this project should be grounded in the real cgroup limit, not
   `free -h`.
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
7. **H10 shows H9's effect size is design-sensitive (~9-13x idealized
   dedicated-thread design vs. ~2x realistic shared/interleaved design),
   but the "cache dilution from interleaving" explanation for WHY is
   disclosed, not profiled or confirmed.** Only one weight ratio (~55x)
   and one size-matched pair were tested under the realistic design;
   whether the ~2x effect grows, shrinks, or holds at a different
   traffic skew or tenant size is untested.
8. **H11's small n=2,000 throughput dip/p99 uptick coincides with RSS
   reaching ~91% of this run's 6 GB safety cap, and memory-pressure
   effects cannot be fully ruled out as a contributing factor there,
   distinct from a pure breadth effect.** The effect is well inside the
   pass thresholds and does not change H11's verdict, but whether it
   would grow, shrink, or disappear at a higher safety cap (a larger
   machine) or a different quiet tenant/noisy-worker-count is untested.
9. **H12's 3,500-tenant envelope is specific to this container's real
   13.34 GiB memory ceiling and a deliberately conservative 9 GB
   self-imposed cap.** Whether a larger machine, or a deployment
   willing to reserve less headroom, would show the same, a
   proportionally larger, or a qualitatively different tenant ceiling
   is untested; only one quiet tenant and one query type were tested at
   this envelope.
10. **H13's super-linear CPU-cost growth at the large end (Furniture,
    16,039 products) has a disclosed, unconfirmed mechanism hypothesis
    (distinct-facet-value cardinality growing with tenant size), not a
    profiled or confirmed one.** Only 3 real tenant sizes were tested;
    the exact scaling shape between the sub-linear small-n region and
    the super-linear large-n region — and whether other structural
    operators (range filters, category membership, sort) show the same
    shape — is untested.
11. **H14 is a real, actionable gap, not just an unconfirmed mechanism
    question: a co-located tenant undergoing index rebuilds materially
    degrades another tenant's own tail latency (4.00-6.70x p99),
    something this architecture's current design does nothing to
    prevent.** No mitigation (e.g. rate-limiting concurrent rebuilds,
    running rebuilds on a separate core/cgroup, background-thread
    deprioritization) has been designed or tested. Only one churn
    tenant size (the largest) and one quiet tenant were tested; the
    proposed CPU-core-contention mechanism is plausible, not profiled.
12. **H15 is likewise a real, actionable gap: a shared Solr instance
    does not safely isolate tenants under noisy-neighbor query load
    (2.16-2.48x p99 degradation), and no mitigation has been designed
    or tested** (per-core resource limits, request-rate limiting,
    dedicated JVMs/containers per tenant tier). Only 2 cores, one
    query type, one noisy-worker count, and this container's default
    out-of-the-box Solr configuration were tested; a production
    deployment with tuned resource isolation might show a materially
    different result, untested here.

## What would be built next if scaling up

**A first-pass economic cost-per-tenant model is now done** — see
`docs/research/PHASE7_ECONOMIC_MODEL.md`, which combines H1/H5's
in-process marginal cost, H6's per-process floor, and H7/H8's
long-running active-serving overhead into an explicit pooled-vs-isolated
deployment cost formula, with worked examples at the real 55-tenant
scale and up to 6,500 controlled-stress-replicated tenants. It addresses
4 of Issue #21's 7 required "Economic output" metrics well, one
partially (a memory-only "cost per million requests" proxy, shown to be
highly window-length-sensitive). **Tenants per fixed hardware envelope
at target SLO is now also delivered** (H12, P7-E09, below), and
**"backend requests avoided" is now delivered too** — combining Phase
3/4's promoted admission-rate evidence (5.80% clean-in-budget / 6.18%
stacked-but-marginally-over-budget) with Phase 7's real 55-tenant
population, added directly to `PHASE7_ECONOMIC_MODEL.md`'s own
"Backend requests avoided" section. All 7 of Issue #21's required
economic-output metrics now have a delivered or partial status; none
remain silently undelivered.

**Cold-tenant overhead (Issue #21's explicit metric) is now measured**
(H9, P7-E06) — a real, reproducible ~9-13x latency-ratio effect between
an infrequently-queried tenant and a same-sized continuously-queried
one, at a practically tiny absolute scale (tens of microseconds),
plausibly attributable to CPU cache locality rather than any explicit
software-level cache this architecture manages.

**Aggregate QPS under a realistic demand mix, fairness under skewed
load, and hot-tenant behavior (Issue #21's explicit metrics) are now
measured** (H10, P7-E07) — a single Zipfian-weighted query stream
spanning all 55 real tenants at once, replicating H9's finding's
DIRECTION but showing its MAGNITUDE is highly design-sensitive (~9-13x
under an idealized dedicated-thread design vs. ~2x under a realistic
shared/interleaved one). Aggregate throughput itself (1,320-1,345 rps)
is measured but explicitly not compared across experiments, since
per-query cost varies by exactly which tenants are hot/cold — the same
workload-mix caveat P7-E01's first draft had to learn.

**H4's breadth-independence finding is now extended to H5's memory-scale
tenant counts** (H11, P7-E08) — confirmed cleanly at 2,000
controlled-stress-replicated tenants (36x WANDS' real ceiling), with a
small, honestly-disclosed dip right at the top of the tested range
possibly (not confirmed) related to RSS approaching this run's safety
cap.

**Tenants per fixed hardware envelope at target SLO (Issue #21's
explicit economic-output metric) is now measured** (H12, P7-E09) —
~3,500 query-capable tenants on this container's disclosed 9 GB
envelope, with quiet-tenant throughput/p50 latency essentially
unaffected there. This also surfaced and corrected two real
first-draft problems: an OOM caused by an eager build-then-check
pattern (fixed by building incrementally with RSS checks during
construction, mirroring H5's own proven-safe pattern), and an unstable
p99 at the very first in-process latency checkpoint (a cold-start
artifact; p50 was used as the primary metric). It also made explicit an
important distinction that had been implicit until now: H5's own
6,500-tenant memory ceiling describes an index-only configuration, not
a query-capable one — the real query-capable ceiling on this
container's disclosed envelope is materially lower.

**Backend requests avoided (Issue #21's last remaining economic-output
metric) is now also delivered** — combining Phase 3/4's promoted,
already-existing admission-rate evidence (P3-E16/E17's 5.80%
clean-in-budget coverage; P4-E01/E02's stacked 6.18% marginally-over-
budget coverage) with Phase 7's real 55-tenant population:
~58,019-61,804 backend requests avoided per million real queries per
tenant, ~3.19-3.40 million per 55M queries in aggregate under an
illustrative even-traffic-split assumption, added to
`PHASE7_ECONOMIC_MODEL.md`. This closes the last of Issue #21's 7
named "Economic output" metrics without silently leaving any
undelivered.

**CPU/query and CPU/tenant (Issue #21's explicit required-experiments
metric) is now measured** (H13, P7-E10) — reusing H6/H7/H8/H9's exact 3
real tenant sizes, single-threaded with no concurrent noisy load to
keep the process-wide `/proc/self/stat` CPU-time reading uncontaminated.
Unlike memory's clean linear scaling (H1/H5), CPU cost per query is
sub-linear from 1 to 5 products then super-linear from 5 to 16,039
products — Furniture's measured cost is 3.81x higher than a
straight-line extrapolation from the two small tenants would predict,
reproduced across 3 runs. A CPU/wall ratio of 0.997-1.002 in every
measurement validated the CPU-time reading method itself before
trusting this comparison.

**High-churn tenant impact on low-churn tenants (Issue #21's last
required-experiments metric besides lexical-backend contention) is now
measured** (H14, P7-E11) — and unlike H2's pure-query-load null result,
this one is a genuine, material falsification: a co-located tenant
undergoing repeated `CatalogIndex` rebuilds (this architecture's only
mutation path) degrades a low-churn tenant's own p99 latency by
4.00-6.70x, reproduced across 3 runs, even though p50 barely moves
(1.14-1.17x). A secondary finding: a single rebuild of the largest real
tenant (16,039 products) costs almost exactly 1 second of wall time —
~100x the cost of a single query against it. This is a genuine,
actionable architectural gap, not a disclosed-but-tiny effect like H9's.

**Lexical-backend contention (Issue #21's last remaining
required-experiments metric) is now measured** (H15, P7-E12) — feasible
directly (Solr was already installed with real WANDS cores from Phase
6A/6B), so tested rather than deferred. Like H14, this is a genuine
falsification: sharing one Solr instance across tenants degrades a
quiet tenant's own p99 by 2.16-2.48x, reproduced across 3 runs after a
self-caught JVM/connection-pool cold-start artifact in the first draft
was fixed with a warm-up phase. **This completes Issue #21's required
"Experiments" list — every named item is now tested.**

Still to build: profiling to identify the specific allocator mechanism
behind H7/H8's growth pattern and residual tail creep, the
CPU-cache-locality hypothesis behind H9's cold-tenant effect, the
"cache dilution from interleaving" hypothesis behind H10's smaller
magnitude, the memory-pressure-vs-safety-cap hypothesis behind H11's
n=2,000 dip, whether H12's 3,500-tenant envelope would look materially
different on a larger machine or with less reserved headroom, the
distinct-facet-value-cardinality hypothesis behind H13's super-linear
CPU growth at the large end, the CPU-core-contention hypothesis behind
H14's rebuild-induced p99 spike, and the shared-JVM-resource hypothesis
behind H15's Solr contention, if a real deployment's memory/
latency/CPU budget needs tighter precision than "decelerates toward
roughly a known bound" / "plausibly cache locality" / "plausibly facet
cardinality" / "plausibly core contention" / "plausibly shared JVM
resources." Designing and testing mitigations for both H14's and H15's
isolation gaps (rate-limiting concurrent rebuilds or Solr requests,
isolating rebuild/backend work to a separate core/cgroup/JVM,
background-thread deprioritization, per-tenant Solr resource limits)
are the two genuinely new, highest-priority items this pass surfaces.

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

No production deployment of this architecture should assume safe
isolation under real catalog churn without first addressing H14's
finding: a co-located tenant's index rebuild measurably degrades
another tenant's own tail latency in this pass's design. This pass
deliberately does not attempt a mitigation (rebuild rate-limiting,
core/cgroup isolation for rebuild work, background-thread
deprioritization) — that is future work, not something to build on
faith that the problem is small, given H14's own measured 4.00-6.70x
p99 effect.

Likewise, no production deployment should assume a shared Solr backend
safely isolates tenants under real noisy-neighbor query load without
first addressing H15's finding (2.16-2.48x p99 degradation from a
co-located tenant's ordinary query traffic, not even a rebuild). No
mitigation (per-core resource limits, request-rate limiting, dedicated
JVMs/containers per tenant tier) has been designed or tested here.

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
and naming 2 as explicit undelivered gaps. Embedding H9's same
size-matched pair in a single realistic, Zipfian-weighted query stream
spanning all 55 real tenants at once (H10) replicates the DIRECTION of
H9's finding in every run (cold measurably slower than hot, at both p50
and p99) but shows the MAGNITUDE is highly design-sensitive: ~1.85-2.08x
under this realistic shared/interleaved design vs. H9's ~9-13x under an
idealized fully-dedicated-thread design — a real, reproducible, and
smaller effect, not a vanished one, most likely because H9's dedicated
thread gave the hot tenant an artificially ideal, uninterrupted cache
advantage a realistic shared worker pool does not provide. H4's own
breadth-independence finding, previously only tested up to WANDS' real
54-other-tenant ceiling, is now confirmed to extend cleanly to 2,000
controlled-stress-replicated tenants — a 36x larger breadth, matching
the order of magnitude H5 reached for memory (H11, reproduced across 3
runs): quiet-tenant throughput drops only 6-9% and p99 grows only 5-8%
across that entire 36x range, both far inside the pre-registered pass
bar, with only a small, honestly-disclosed dip at the very top of the
tested range coinciding with RSS nearing this run's safety cap.
Combining H5's memory-scaling model with H4/H11's latency-independence
evidence, directly answering Issue #21's "tenants per fixed hardware
envelope at target SLO" metric for the first time this phase (H12,
P7-E09, reproduced across 3 runs): a query-capable tenant population
(both `Catalog` and `CatalogIndex` resident, unlike H5's index-only
measurement) was built incrementally on this container, discovering
its real 13.34 GiB cgroup memory limit directly (after a first-draft
OOM at the originally-assumed n=6,500) and safely reaching exactly
3,500 tenants under a disclosed, conservative 9 GB envelope — with
quiet-tenant p50 latency and throughput both essentially flat (within
~2-4%) relative to the 55-tenant baseline. This also made explicit an
important distinction that had been implicit until now: H5's own
6,500-tenant figure describes an index-only configuration that cannot
actually serve queries, not a query-capable ceiling. CPU/query and
CPU/tenant (Issue #21's remaining required-experiments metric) is
measured for the first time this phase (H13, P7-E10, reproduced across
3 runs): unlike memory's clean linear scaling, CPU cost per facet-scan
query is sub-linear from 1 to 5 products (a fixed per-query overhead
dominates at tiny sizes) then super-linear from 5 to 16,039 products —
Furniture's measured CPU cost is 3.81x higher than a straight-line
extrapolation from the two small tenants would predict. A CPU/wall
ratio of 0.997-1.002 in every measurement validated the underlying
`/proc/self/stat`-based CPU-time reading method before this comparison
was trusted. Finally, and in contrast to H2's own null result for pure
query load: a co-located tenant undergoing repeated `CatalogIndex`
rebuilds (this architecture's only mutation path, since tenant bundles
are immutable) DOES materially degrade a separate low-churn tenant's
own tail latency (H14, P7-E11, reproduced across 3 runs) — p99 grows
4.00-6.70x while p50 barely moves (1.14-1.17x), and a single rebuild of
the largest real tenant costs almost exactly 1 second of wall time,
~100x a single query's own cost. This is a genuine, actionable
isolation gap this pass's other query-focused experiments could not
have surfaced. Finally, sharing a single Solr instance across tenants
also shows real cross-tenant degradation under ordinary query load, not
just rebuilds (H15, P7-E12, reproduced across 3 runs after a
self-caught JVM/connection-pool cold-start artifact was fixed with a
warm-up phase): p99 degrades 2.16-2.48x while p50 stays modest
(1.22-1.43x). This completes Issue #21's required "Experiments" list —
every named item has now been tested at least once.

**Does not claim**: that the small cross-vs-same-tenant latency
difference in H2 is understood; that 6,500 tenants (H5) or 3,500
tenants (H12) are this container's absolute hardware ceilings — both
are self-imposed, conservative safety bounds chosen with real margin
under the container's actual, now-directly-measured 13.34 GiB cgroup
memory limit (discovered via H12, not assumed), and a deployment
willing to reserve less headroom could plausibly push either figure
higher, untested here; that H8's 180-second resident window represents a real
service's full-lifetime steady state (a small, real residual tail creep
persists in 2 of 3 runs, roughly two orders of magnitude smaller than
the initial climb, and whether it fully stops over a much longer real
lifetime is untested); that the specific allocator mechanism behind
H7/H8's growth is confirmed (a thread-local-arena hypothesis is named,
not verified); that H9's cache-locality mechanism is confirmed (named,
not profiled), or that either its ~9-13x or H10's ~2x ratio generalizes
beyond the one size-matched tenant pair, one cold-query interval, and
one traffic-skew ratio tested; that H10's "cache dilution from
interleaving" explanation for the magnitude gap between H9 and H10 is
confirmed (disclosed, not profiled); that H4's own throughput/latency
finding has been re-tested under a realistic skewed demand mix (H10
measured a DIFFERENT size-matched pair's latency under skew, not H4's
own breadth-at-fixed-demand claim); that H11's small n=2,000
throughput dip/p99 uptick has a confirmed cause (a memory-pressure
hypothesis tied to RSS nearing this run's safety cap is disclosed, not
profiled), or that H11's result generalizes beyond the one quiet
tenant, one noisy-worker-count, and one query type (facet scan) tested;
that H12's 3,500-tenant figure generalizes beyond this specific
container, this one 9 GB self-imposed envelope, one quiet tenant, and
one query type, or that a larger/less-headroom-reserved machine would
show the same, a proportionally larger, or a qualitatively different
ceiling (untested); that the "backend requests avoided" figure
(58,019-61,804 per million queries) reflects a same-dataset
measurement — it combines Phase 3/4's ESCI-corpus admission rate with
Phase 7's WANDS-based tenant population, a disclosed cross-dataset
combination, not an admission-rate measurement run against WANDS'
own real queries; or that the illustrative even-split-across-55-
tenants traffic assumption reflects any measured per-tenant traffic
distribution (Phase 7 never measured one); that H13's distinct-facet-
value-cardinality hypothesis for the super-linear CPU growth is
confirmed (disclosed, not profiled), or that its scaling shape
generalizes beyond the 3 tenant sizes and one query type (facet scan)
tested; that H14's CPU-core-contention explanation for its p99 spike is
confirmed (plausible, not profiled), that its 4.00-6.70x effect
generalizes beyond the one churn tenant (Furniture, the largest), one
quiet tenant (Rugs), and one rebuild frequency tested, or that any
mitigation for this isolation gap has been designed, implemented, or
validated (explicitly not attempted this pass); that H15's shared-JVM-
resource explanation is confirmed (plausible, not profiled), that its
2.16-2.48x effect generalizes beyond the 2 cores, one query type, and
one noisy-worker count tested, that this container's default Solr
configuration represents a tuned production deployment, or that any
mitigation for this gap has been designed, implemented, or validated —
this is a first pass on memory (including at scale), pairwise isolation,
fixed-tenant throughput-under-breadth (now including at memory-scale
tenant counts), process-baseline floors (short-lived, and a
longer-resident window that decelerates toward but has not been proven
to fully reach a bound), a first cold-vs-hot latency comparison under
two different designs, a first economic-model synthesis (now
addressing all 7 of Issue #21's named "Economic output" metrics, six
well and one partially), a first CPU-cost-per-query measurement, a
first churn-vs-query isolation comparison, and a first lexical-backend
contention measurement only.

**Decision: PROCEED.** Issue #21's Phase 7 required "Experiments" list
is now fully tested (every named item, H1 through H15) and all 7
"Economic output" metrics are addressed (6 delivered, 1 partial) — this
document is the terminal decision for the Phase 7 measurement campaign;
its remaining open items are mitigation design for H14/H15's two real
isolation gaps and mechanism profiling for several disclosed-but-
unconfirmed hypotheses, both follow-on engineering work rather than
unanswered required measurements. The favorable, adversarially-corrected
H1 result, its clean
confirmation at scale via H5, the robust H2/H4 results (H4 now itself
confirmed at memory-scale tenant counts via H11), H6/H7/H8's real,
reproduced, now-stability-confirmed measurement of the pooling
advantage this project's own thesis assumed, the first economic
cost-per-tenant model (now addressing all 7 of Issue #21's named
economic-output metrics), H9's honestly-scaled cold-tenant finding,
H10's honest replication check (confirming the direction, correcting
the magnitude), H12's real, empirically-reached tenants-per-envelope
figure (which also directly discovered this container's actual hard
memory limit rather than continuing to assume a host-level figure),
the backend-requests-avoided synthesis combining Phase 3/4 with Phase
7's tenant model, and H13's first real CPU-cost measurement (revealing
memory and CPU do NOT scale the same way, a genuinely new finding) are
real evidence in favor of the architecture's packing-density and
latency-predictability potential — but H14's and H15's genuine,
material falsifications (a co-located tenant's index rebuild degrades
another tenant's own p99 by 4.00-6.70x; a co-located tenant's shared-
Solr query load degrades another tenant's own p99 by 2.16-2.48x) are
equally real, honestly-recorded LIMITS on that potential, not smoothed
over or minimized: this pass explicitly does not claim safe isolation
under real catalog churn or under a shared lexical backend, and names
designing/testing mitigations for both as necessary future work before
any such claim could be made. All of the above are explicitly a floor
on the claim (single-process, short-lived-process, or
180-second-resident-process measurements, one tenant model, one
self-imposed safety bound, one size-matched hot/cold pair, one
traffic-skew ratio, one hardware envelope, one cross-dataset admission-
rate combination, one churn scenario, one Solr configuration), not a
ceiling on what remains to be tested.
