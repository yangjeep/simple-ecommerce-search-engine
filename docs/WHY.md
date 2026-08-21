# Why this project exists

## The real problem

This project is not "build a faster search engine." It is a production-derived
multi-tenant commerce serving problem:

- Many merchants (tenants) share infrastructure, with wildly heterogeneous
  catalog sizes and query-per-second (QPS) rates — a hobby storefront and a
  mid-market retailer do not look alike on any axis.
- Each tenant has its own daily/weekly/seasonal traffic cycle, and those
  cycles are mostly *uncorrelated* day to day. Under normal operation this
  means **statistical multiplexing** works: pooled infrastructure can serve
  many tenants far more cheaply than dedicating a full search cluster to
  each one, because idle capacity for one tenant absorbs a burst in another.
- A few times a year (Black Friday/Cyber Monday and similar retail events),
  that assumption breaks: demand becomes **strongly correlated across most
  tenants at once**. Statistical multiplexing stops helping exactly when it
  is needed most.
- A large share of real commerce traffic is not open-ended free-text search
  at all — it is category pages, collection pages, filter/facet refinement,
  sort, and pagination (PLP: product-listing-page traffic). This traffic is
  structurally different from search: the semantics (which product, which
  attribute, which constraint) are already explicit in the request, not
  something that has to be inferred from free text.
- Price, inventory, and availability are high-churn commerce state layered
  on top of a comparatively stable catalog — a serving architecture has to
  treat these differently from the catalog itself.
- Tenant isolation and predictable tails matter: one merchant's traffic
  spike must not degrade another's p99, and a serving substrate that
  achieves high aggregate throughput by sacrificing one tenant's latency is
  not a win.
- Fixed per-tenant serving cost has to stay low enough for SMB/mid-market
  economics — this is explicitly not an Alibaba-scale, hyperscale-only
  design target.
- Relevance quality is not a knob to trade away for serving cost. A cheaper
  system that returns worse results is not a win either.

Generic, mature search engines (Solr/Elasticsearch/Lucene-family systems)
are excellent at open-ended lexical relevance, but they are generic: they
carry no first-class notion of "this is a Product," "this is a Variant,"
"these two rows are the same SKU in different sizes," or "this filter
combination is a structurally exact set operation, not a ranked-retrieval
problem." That genericity is a cost when the workload is heavily
commerce-shaped.

## Why a commerce-native execution plane, specifically

The working hypothesis, stated precisely so it can be falsified: a system
that treats products, variants, brands, categories, and typed attributes as
first-class — rather than generic tokens/documents — can execute the
*structurally resolvable* slice of commerce retrieval traffic (exact
filters, faceting, sort, pagination, variant-safe lookups) far more cheaply
than a generic engine, while leaving the *genuinely open-ended lexical*
slice (free-text relevance ranking) to the mature engine that already does
it well. The architecture is therefore a **semantic forwarding plane**, not
a replacement: a deterministic, low-latency native path in front of an
unmodified Solr (or equivalent) fallback, with LLM/model reasoning
confined to an offline control plane that proposes and validates rules,
never called in the serving hot path.

## What has actually been measured (Phases 0–5)

This is not a design document written in advance of evidence — the sections
below record what five phases of falsification-oriented experimentation on
a real 1.2M-product ESCI catalog and 22,458-query corpus actually found.
Every claim here traces to a decision document and an experiment log; see
`docs/architecture/` for the current implementation and
`docs/experiments/` / `PHASE*_DECISION.md` for the full evidence.

### Falsified: whole-engine 5–10x replacement (Phase 2 — STOP)

The original framing asked whether a commerce-native engine could replace
Solr outright at 5–10x better QPS/$ across the whole workload. It could
not: the true structural fast path *is* dramatically faster in isolation
(87–105x), but the fraction of real traffic that can be safely and fully
resolved structurally is small, and the naive hybrid/fallback path that
handled the rest made the traffic-weighted system *slower* than Solr, not
faster. See `PHASE2_DECISION.md`. This is the single most important
falsified hypothesis in the project's history, and it is the reason every
later phase is framed as "safe offload over a mature fallback," never as
"replace Solr."

### Narrowed: safe admission mechanisms recover real but small coverage (Phases 3–4 — NARROW SUPPORT)

Reframed around the Phase 2 finding, Phase 3 built a
`query -> cheap admission check -> native FastPath OR unmodified Solr
request` architecture and searched for every safe way to grow the fraction
of traffic the native path could handle without violating a strict
relevance-degradation budget. Three independently-verified, disjoint
admission mechanisms were found and kept; the best combined operating
point recovers **5.80% coverage at 1.98% relative degradation** within a
2% budget, with fallback tax statistically indistinguishable from zero and
no p95/p99 regression. Phase 4 added a learned-but-compiled semantic
implication mechanism (e.g. "air force 1" implies Brand=Nike), built via an
offline propose → replay → promote pipeline with zero model calls in the
serving path; it is the first mechanism in the whole campaign with a
**measured zero false-positive rate**, but it recovers only a further
0.38% of traffic. Both phases explicitly judged their own mining loop
exhausted: 99.18% of rejected traffic falls into two well-characterized
classes (unresolved lexical residual, ambiguity) that this admission-rule
family cannot safely recover further without a materially different
mechanism. **Neither phase reached the ≥50% P50-shift target** a
search-coverage win would need — see `PHASE3_DECISION.md`,
`PHASE4_DECISION.md`.

### Narrowed and scope-bounded: browse/PLP structural execution (Phase 5 — REVISE / NARROW BUT PUBLISHABLE)

Phase 5 asked whether category/collection/PLP-style structural retrieval —
the traffic class this project's own thesis says should be the *easiest*
win, since its semantics are already explicit — clears an even stronger bar
(a hard 80x physical-multiplier floor against a fairly-tuned Solr
baseline). A hard, evidence-backed scope boundary surfaced immediately:
this project's only real dataset (Amazon ESCI) has **no category,
collection, hierarchy, price, or inventory data at all** — confirmed by
inspecting the raw parquet schema, the full catalog export, the ingestion
code's hardcoded sentinels, and the live Solr schema. Genuine
category/PLP-page rendering is architecturally untestable here. Phase 5
benchmarked the one real, non-fabricated structural slice this catalog
does support (brand/color filter, facet, sort, pagination) and found the
80x floor holds for filtering, pagination, and concurrency (thousands-to-
tens-of-thousands-fold advantages) but **fails for faceting and
large-result-set sorting**, in both cases because of measurable,
root-caused algorithmic scaling problems in the current native
implementation, not an inherent property of structural execution. See
`PHASE5_DECISION.md`.

### The pattern across all three narrowed phases

Every phase that reached a narrow verdict did so by finding a *real*,
measurable advantage confined to a *smaller* slice of traffic than hoped,
never by finding no advantage at all. The physical multiplier itself
(dozens to thousands of times faster) has held up every time it was
actually measurable; the limiting factor has consistently been *semantic
resolvability* (how much of real traffic can be proven safe to route
natively) and, in Phase 5, *algorithmic scaling* of specific native
operators — not the core thesis that commerce-native structural execution
is fundamentally cheaper where semantics are known.

## Why the project continues (Issue #21, Phases 6–9)

The evidence so far is confined to one dataset (Amazon ESCI), one baseline
engine (Solr), and single-tenant, single-node measurement. Issue #21 exists
to test whether the narrowed-but-real result generalizes: across
independent datasets and verticals (Phase 6), across the actual
multi-tenant SMB/mid-market economics this project is meant to serve
(Phase 7), across the correlated-burst regime where statistical
multiplexing breaks down (Phase 8), and as an integrated, falsifiable
system rather than a sum of microbenchmarks (Phase 9). This document will
be updated as that evidence changes the thesis — it is a record of what is
currently believed and why, not a fixed pitch.

**First cross-dataset result (Phase 6A — PROCEED, `PHASE6A_DECISION.md`)**:
Issue #23's named dataset (Amazon Reviews 2023) turned out to be
unreachable from this project's network environment; WANDS (an
independent, real, genuinely hierarchical commerce vertical) was
substituted, with its own real trade-offs disclosed (no price field, a
much smaller product ceiling). Phase 5's structural filter, subtree-
browse, pagination, and concurrency advantages reproduced almost exactly
in order of magnitude. Facet and sort reproduced the same qualitative
breakpoint shape, but the facet crossover shifted to a substantially
lower real candidate count — explained, not just observed, by WANDS'
richer per-product attribute data. Nothing from Phase 5 was falsified;
one specific number (the facet-crossover candidate count) had to be
narrowed from "a fixed threshold" to "dependent on per-candidate
attribute-map complexity."

**Scale-ladder follow-on (Phase 6B — PROCEED, `PHASE6B_DECISION.md`)**:
with Retailrocket, H&M, Amazon Reviews 2023, and Havenask all confirmed
blocked from this environment (documented on Issue #21), Phase 6B tested
Phase 6A's attribute-complexity explanation using a controlled-stress
scale ladder (the real WANDS catalog replicated 2x–20x, holding facet
cardinality and per-candidate attribute complexity fixed while scaling
only candidate-set size) — a fallback Issue #23 itself explicitly
anticipated and authorized. This project's own adversarial-review
discipline caught a real problem in the first-draft analysis (a "genuine
super-linear scaling" claim that did not survive checking all measured
checkpoints, self-corrected before promotion — see `PHASE6B_LOG.md`):
Phase 6A's explanation substantially holds in aggregate, with one real,
noise-robust, but narrower and cause-unconfirmed exception at a specific
candidate range. A genuinely new operator (numeric-range filtering,
untestable in Phase 6A for lack of a price field) showed its own
distinct, materially higher crossover point.

**A repaired evidence-chain gap, found by a whole-campaign audit after
Phase 8 (Phase 6C — PROCEED, `PHASE6C_DECISION.md`)**: Phase 6B's own
decision document said its blocked-engine survey "should be revisited
before Phase 7." It was not — Phase 7 and Phase 8 both proceeded with
Solr as the only lexical-backend evidence, five phases deep. A fresh
audit found this and closed it late rather than leaving it silent.
Live re-verification, done in this session rather than trusted from
memory, found Havenask, Retailrocket, H&M, and Amazon Reviews 2023 all
still blocked, unchanged. Elasticsearch and OpenSearch were tested for
the first time in this project's history and are also blocked — their
official distributions are unreachable, and OpenSearch's own
from-source build hits a second, independent blocker (its bundled JDK
provider is also unreachable) before dependency resolution even
completes. But one path proved open: Maven Central is fully reachable,
meaning Apache Lucene itself — the shared retrieval core underlying
Solr, Elasticsearch, and OpenSearch alike — could be benchmarked
directly for the first time, with no server, no Docker, and no
distribution blocker of any kind. A standalone Java harness indexed the
same real WANDS catalog Phase 6A/6B already used, measured the same
operation classes at the same real category checkpoints, and
cross-checked every filter and range count against the same
still-running Solr instance before trusting any timing — all counts
matched exactly, in three repeated runs. The first pass (P6C-E00)
reversed the question it was built to answer: for faceting, raw Lucene
direct measured via a hand-rolled, per-candidate scan was *slower* than
Solr's own wrapped facet API in five of seven real checkpoints, by as
much as three-to-four-fold. This project's own adversarial-review
discipline did not let that surprising result stand: a self-directed
follow-up (P6C-E01) asked whether the finding was about Lucene itself
or about one naive implementation, and re-measured using Lucene's own
dedicated, purpose-built facet module
(`SortedSetDocValuesFacetCounts`) instead of the hand-rolled scan. The
result substantially reverses: Lucene's own specialized mechanism beats
Solr in five of seven checkpoints (up to three-fold), trailing by a
much smaller margin (roughly 1.1x-1.3x, not three-to-four-fold) in the
remaining two. Stripping away Solr's HTTP and schema layer did not
reveal a uniformly faster or slower engine underneath — it revealed
that the earlier "Solr beats raw Lucene" finding was really "Solr beats
a naive per-candidate scan," and that a specialized, ordinal-based
counting mechanism closes most (not all) of that gap. This sharpens,
rather than undermines, the facet-crossover finding this project has
now measured four separate times: the crossover is substantially,
though evidently not entirely, a property of naive per-candidate
facet-scanning specifically — this project's own `facet_counts_by_scan`
included — not of generic-engine faceting versus commerce-native
faceting in general, and it surfaces a genuine, previously-untested
candidate fix: whether commerce-native's own architecture could adopt
an equivalent ordinal-based counting approach.

**The facet crossover closes (Phase 6D — PROCEED, `PHASE6D_DECISION.md`)**:
Phase 6D built exactly the candidate fix Phase 6C surfaced. A new
`facet_counts_ordinal` method on `CatalogIndex` — a per-attribute value
dictionary plus a flat per-variant-ordinal column, the same
architectural family as Lucene's own module and Solr's own
`facet.field` — was correctness-gated two ways before any timing claim
was trusted: an exact match against the existing `facet_counts_by_scan`
across every edge case, and 21 exact top-50-facet matches (7 real
checkpoints × 3 runs) against Solr's own live response. The result is
more decisive than Phase 6C's own Lucene-module finding: the ordinal
method beats Solr at every single one of the 7 real checkpoints, by
5.2x to 69.8x, with no exceptions — where Lucene's own equivalent
module still trailed Solr at the two largest checkpoints. It also beats
commerce-native's own existing scan method by 23.5x-89.3x. The margin
is larger than Lucene's own because commerce-native's naive baseline was
paying a more expensive per-candidate cost to begin with (a full
attribute-map clone on every iteration, not just a plain ordinal
lookup) — so there was more room for the fix to help. The facet
crossover this project characterized four times over (Phase 5, 6A, 6B,
Phase 6C) is now confirmed to have been a property of naive
per-candidate scanning specifically, not an inherent ceiling on
commerce-native's own architecture. Extended across Phase 6B's own
2x-20x controlled-stress scale ladder (P6D-E01, up to 320,780
candidates), the margin over Solr holds — no exceptions across all 35
checkpoint x tier combinations tested — but narrows, not grows, at the
largest candidate counts, converging toward roughly 2.5x-3x rather than
widening further; its margin over commerce-native's own scan method, by
contrast, grows sharply with scale instead, consistent with the scan
method's per-candidate allocation cost getting relatively worse as
candidate count grows. **But the technique is not a universal, free
win — its own adversarial follow-up (P6D-E02) found the real boundary.**
Extended to the dedicated `brand`/`category`/`product_type` facets,
whose existing naive baselines never paid `color`'s attribute-map-clone
cost, the ordinal method has its own genuine crossover: 1.9x-5.2x
*slower* than the existing scan at small candidate counts, and faster
only past a real threshold. The mechanism is the same one that made
`color`'s result so large, cutting the other way: the ordinal method
trades a fixed, dictionary-size-proportional per-call cost for
per-candidate savings, and when the replaced baseline was never
expensive, that fixed cost can dominate. This does not contradict the
`color` result — it tells a future implementer exactly when to reach
for this technique and when not to, which is a more useful outcome than
an unqualified "always faster" claim would have been. Real limitations
remain (the crossover point is bracketed, not pinpointed; `brand`/
`category` have only unit-test coverage; no query-serving path yet
wired to prefer either strategy; organic growth beyond the replication
ladder untested), but the core question this whole four-phase thread
asked is answered, with its real boundary now characterized rather than
assumed away.

**The campaign's own adversarial-review discipline then caught a real
gap in its own instrumentation (P6D-E03).** `approximate_size_bytes` —
the memory-size metric this whole project has used since Phase 2,
referenced across 21 files including `SCALE_UP_DECISION.md` and
`PHASE7_DECISION.md` — had silently omitted every structure Phase 6D
added. Fixed, with a new `approximate_ordinal_facet_bytes()` accessor
and a correctness test guarding the omission from recurring. The real
measured number on the WANDS catalog — 2,876,248 bytes, 26.2% of the
whole index, 66.90 bytes/product — is about 16.7x the earlier ~172 KB
analytical estimate: still small next to Phase 7's own per-tenant costs
in absolute terms, but a materially larger share of the index than the
unmeasured estimate implied, and a reminder that an "estimated, not
measured" caveat is itself a real, actionable gap this loop is meant to
close, not a permanent asterisk.

**First multi-tenant result (Phase 7 — terminal decision: PROCEED, `PHASE7_DECISION.md`)**:
the first phase in this project's history to build and measure more
than one tenant's index in one process, using WANDS' real category
structure as a realistic SMB tenant model (each category becomes one
specialty retailer's catalog). This project's adversarial-review
discipline caught a real problem here too — a first-draft claim of a
small but real "~27-590 KB per-tenant fixed cost" did not survive a
reversed-build-order control, and was corrected (not just softened) to a
stronger, more favorable finding: per-tenant memory overhead is
negligible in this architecture, and total memory cost tracks aggregate
product count rather than tenant count. A second self-caught issue in a
follow-on QPS-scaling experiment (an apparent throughput increase that
turned out to be a workload-mix artifact, not a real tenant-count
effect) was corrected the same way before any external review was
needed. The corrected findings then reproduced cleanly at much larger
scale: a controlled-stress replication of the real tenant population up
to 6,500 tenants (~4.93M products) showed per-product memory cost
staying within 0.4% across a 65x range, with no sign of degradation
before a self-imposed safety cap. Cross-tenant latency isolation held
robustly across repeated runs, both for pairwise contention and for
breadth of concurrently-touched tenants. A follow-on measurement then
spawned actual separate OS processes to test this project's own opening
claim directly: pooling avoids a real, measured per-process baseline
(~2.1-2.2 MB, reproduced across 3 runs) that a one-process-per-tenant
deployment would pay once per tenant — the first real evidence for the
statistical-multiplexing thesis this document opens with, rather than an
assumed advantage. A further follow-on measurement then held that same
kind of process alive and actively serving real queries for a sustained
window (rather than exiting immediately) and found the true cost is
even larger: peak resident RSS grows by an exactly-reproducible 244 KB
for an idle process and by 196-900 KB for one serving real query
traffic (scaling with the tenant's own data size), reproduced across 3
runs — strengthening, not weakening, the pooling-advantage finding. A
further follow-on extended that same resident window 9x (20s -> 180s)
to check whether the largest real tenant's still-rising RSS curve was a
stable measurement or an artifact of too short a window: growth
decelerates sharply toward what looks like a bound (roughly 98% of the
total growth happens in the first half of the window, in all 3 runs),
confirming the earlier figure is a stable input rather than a
measurement-window artifact. A first economic cost-per-tenant model
then combined all of these findings into an explicit pooled-vs-isolated
deployment cost formula, addressing most of Issue #21's required
"economic output" metrics and naming the rest as explicit gaps rather
than silently omitting them. A final measurement tested Issue #21's
explicitly-named "cold tenant overhead" metric directly for the first
time: a real, reproducible ~9-13x latency-ratio effect exists between an
infrequently-queried tenant and a same-sized continuously-queried one,
plausibly attributable to CPU cache locality rather than any explicit
software-level cache this architecture manages — but at an absolute
scale (tens of microseconds) almost certainly negligible next to any
real deployed service's actual request latency, a distinction stated
explicitly rather than leading with the more dramatic ratio alone. A
final measurement then embedded that same pair in a single, realistic,
Zipfian-weighted query stream spanning all 55 real tenants at once
(testing this document's own opening thesis in a different, more
production-like arrival pattern, and Issue #21's "aggregate QPS,"
"fairness under skewed tenant load," and "hot tenant saturation"
metrics): the DIRECTION of the cold-tenant effect replicated cleanly in
every run, but its MAGNITUDE shrank roughly 4-6x under this realistic,
shared/interleaved design compared to the original idealized one —
pointing at the earlier measurement's fully-dedicated-thread setup, not
a general architectural property, as the likely source of its larger
observed ratio. A further measurement closed a gap this project's own
decision document had previously named as still open: whether the
fixed-tenant breadth-independence finding (originally tested only up to
WANDS' real 54-other-tenant ceiling) also holds at the much larger
tenant counts the memory-scaling replication reached. It does —
confirmed cleanly at 2,000 controlled-stress-replicated tenants (36x
the original ceiling), with quiet-tenant throughput dropping only 6-9%
and p99 growing only 5-8% across that entire range, reproduced across 3
runs — with a small, honestly-disclosed dip at the very top of the
tested range coinciding with resident memory approaching this run's
self-imposed safety cap, named as an unconfirmed candidate mechanism
rather than a resolved one. A final measurement combined the memory
model with this latency evidence to directly answer this project's
"tenants per fixed hardware envelope at target SLO" metric for the
first time: building a query-capable tenant population (needing both
the raw catalog and its index resident together, unlike the earlier
memory-scaling measurement, which only ever kept the index resident)
first hit a real out-of-memory kill at the scale the memory-only
measurement had reached, which led to discovering this container's
actual enforced memory limit directly from its own control group
rather than continuing to assume the host-level total — materially
lower than assumed. Rebuilding incrementally with real memory checked
during construction (rather than only after a whole batch was built)
reached a real, safely-confirmed ceiling of about 3,500 query-capable
tenants under a disclosed, conservative memory envelope, with
quiet-tenant throughput and latency both essentially unaffected there
— a materially lower, but now genuinely query-capable, number than the
earlier memory-only ceiling. A further synthesis combined Phase 3/4's
own already-promoted admission-rate evidence with this multi-tenant
population to answer the remaining named economic output directly:
tens of thousands of backend requests avoided per million real queries
per tenant, closing the final gap in this document's own required
economic-output list without leaving anything silently undelivered. A
last measurement then turned to CPU cost, a dimension every prior
Phase 7 experiment had left entirely unmeasured (all of them tracked
wall-clock time only): unlike memory, which scales cleanly and linearly
with product count, CPU cost per query does not — it is dominated by a
fixed per-query overhead at tiny tenant sizes, then grows measurably
faster than linearly for the largest real tenant, a real, reproduced,
and previously invisible finding this project's memory-focused
measurements alone could never have surfaced. A final measurement
tested mutation rather than query load for the first time in this
project's history: does a tenant undergoing frequent catalog updates
degrade a quiet neighbor sharing the same process, distinct from the
already-confirmed finding that pure query load does not? Here the
answer is genuinely different — a co-located tenant whose index is
being rebuilt (this architecture's only update path, since tenant
bundles are immutable) measurably degrades a quiet tenant's own tail
latency, reproduced consistently, even though the typical-case latency
barely moves. This is a real, actionable limitation this pass names
honestly rather than minimizes, and designing a mitigation for it is
named explicitly as necessary future work. A twelfth and final Phase 7
measurement tested the one remaining required experiment from this
epic's own opening ask: does a shared, mature lexical backend show the
same kind of noisy-neighbor risk under real multi-tenant load? Solr was
already installed in this environment with real WANDS-derived cores
from an earlier phase, so this was tested directly rather than deferred
as out of scope. The answer is the same shape as the rebuild finding:
sharing one Solr instance across tenants measurably degrades a quiet
tenant's own tail latency under ordinary query load, reproduced
consistently. Between the native in-process path (confirmed safe under
query load) and these two real limitations (rebuild churn, and a
shared lexical backend), this phase closes out Issue #21's full
required measurement list with an honest, mixed picture rather than a
uniformly favorable one — exactly the kind of result this project's own
falsification discipline exists to surface.

**First Phase 8 result (correlated retail burst / BFCM elasticity —
first pass, `PHASE8_DECISION.md`)**: before attempting Phase 8's full
required-measurement list, a feasibility check against this
environment's real constraints (`PHASE8_FEASIBILITY.md`) found roughly
two-thirds of it testable now by extending Phase 7's own validated
methodologies with a burst multiplier, and named the remainder
(request-admission/backpressure control, true multi-node
redistribution, and a direct comparison against generic cluster/shard
scale-out) as genuinely out of reach without new product surface or
infrastructure this epic has deliberately not yet built — consistent
with this project's own sequencing (single-node thesis first). The
first Phase 8 measurement directly tested Phase 8's own stated thesis:
does a correlated demand burst hitting a SUBSET of tenants (not all of
them) leave an unrelated tenant's own latency untouched? A group of 10
real tenants (of 55) had their traffic weight multiplied tenfold
mid-experiment, simulating a sudden, correlated sale event; a separate,
tracked tenant's own p50/p99 stayed essentially flat across 3
independent runs even as the bursting group's own throughput grew
roughly tenfold and the whole population's aggregate throughput rose
by around 40%. The steady-state isolation properties this project
already measured for the native path extend cleanly to this
correlated-burst regime, at least for query load — a real, positive
result, honestly scoped to what this single-node environment can
actually test.

**Second Phase 8 result — burst amplifies the known rebuild-churn gap
(`PHASE8_DECISION.md`)**: the first Phase 8 result's own most important
named follow-up was whether a correlated burst makes either of Phase
7's two real isolation gaps worse. Testing that directly for the
rebuild-churn gap (the one this architecture's own mutation model makes
unavoidable — a full index rebuild is the only path to reflect updated
catalog state), the answer is yes, and more sharply than a simple
ratio conveys. Three conditions were measured in the same run: a quiet
tenant queried alone, the same tenant queried while a co-located tenant
is continuously rebuilt (reproducing Phase 7's own H14 exactly), and a
third condition adding realistic background query load across the rest
of the tenant population, including the rebuilding tenant itself (a
shopper population plausibly browsing the same sale item while it
churns). A first pass of 3 runs gave a result too noisy to trust — a
self-caught methodology issue, fixed by widening to 10 runs and
reporting the median rather than any single run. The result: under an
otherwise-idle system, the rebuild-driven tail-latency hit is an
intermittent coincidence, showing up in roughly 3 of 10 measurement
windows; under the same rebuild load with realistic background
traffic, it showed up in all 10. Burst does not make Phase 7's known
gap merely bigger — it makes it dependable. No mitigation for this
exists yet in this codebase; it is named explicitly as necessary future
work rather than smoothed over.

**Third Phase 8 result -- burst amplifies the shared-Solr-contention
gap too (`PHASE8_DECISION.md`)**: the symmetric question for Phase 7's
other known real isolation gap -- sharing one Solr instance across
tenants -- got the same answer. Reusing that experiment's exact
harness, three conditions were measured in the same run: one tenant's
queries alone, the same tenant while a second tenant hammers a much
larger shared core (reproducing the original finding exactly), and a
third condition adding several more tenants' traffic to other cores on
the same shared instance, modeling more merchants joining the backend
during a sale rather than one merchant's load simply tripling. Having
just learned from the rebuild-churn result that a handful of repeated
runs isn't enough to trust a noisy tail statistic, this measurement
started directly with ten repeats and a median-based verdict rather
than re-learning that lesson the hard way -- and the result came back
tight from the first pass, unlike the rebuild-churn case: median
amplification of roughly 1.8x, with every individual run agreeing. The
same qualitative shift recurs: under lighter load the degradation event
showed up about half the time; with more tenants sharing the backend,
it showed up every single time. Both of Phase 7's two known real
isolation gaps now get measurably worse, not better, under a correlated
burst, and neither has a designed mitigation yet.

**Fourth Phase 8 result -- the two gaps compound with each other, on
one side (`PHASE8_DECISION.md`)**: with both known gaps individually
confirmed to worsen under burst, the natural next question was whether
running both mechanisms at once -- catalog mutation and shared-backend
contention happening simultaneously, exactly the kind of thing a real
BFCM event would produce -- makes things worse than either alone. The
answer turned out asymmetric. Combining a rebuild-churning tenant with
a Solr instance under noisy load, and measuring both an affected native
tenant and an affected Solr core at the same time, the native side
degraded materially in every single one of twenty measured runs across
two independent passes -- a far more reliable trigger than either
mechanism alone ever produced. The Solr side, however, did not get
measurably worse from adding native-side churn on top of its own
already-known contention. Getting to that result also surfaced a
genuine measurement subtlety worth naming plainly: the planned
ratio-based statistic came out inflated because the "churn alone"
condition in this particular design happened to under-measure its own
effect, traced to how the measurement window's length depends on which
of two concurrently-measured signals finishes first. Rather than either
trusting the inflated ratio or discarding the result, the write-up
leans on the more direct, denominator-free comparison instead, and
says plainly why. Two of Phase 7's isolation gaps are now not only
worse individually under burst, but interact with each other on at
least one side, and no mitigation for either exists yet.

## What this project is not

This repository does not market itself as a universally faster search
engine, a Havenask/Elasticsearch clone, or a general-purpose document
search system. It is not pursuing distributed systems work, generic query
DSLs, authentication/tenancy/HA, or cluster coordination during the
current research epic (see `docs/WHAT.md` for the explicit non-goals). Any
claim in this repository that a mechanism is fast should be read as
"faster within the measured, disclosed boundary," not "faster in general."
