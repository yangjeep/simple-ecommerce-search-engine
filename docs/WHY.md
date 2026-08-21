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

## What this project is not

This repository does not market itself as a universally faster search
engine, a Havenask/Elasticsearch clone, or a general-purpose document
search system. It is not pursuing distributed systems work, generic query
DSLs, authentication/tenancy/HA, or cluster coordination during the
current research epic (see `docs/WHAT.md` for the explicit non-goals). Any
claim in this repository that a mechanism is fast should be read as
"faster within the measured, disclosed boundary," not "faster in general."
