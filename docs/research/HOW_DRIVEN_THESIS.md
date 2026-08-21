# HOW-Driven Thesis: Expand the Commerce-Native Multiplier Toward P50/P95

## Status

This document records the research consensus after Phase 2 and the first Phase 3 safe-offload experiments. It intentionally closes the broad WHY/WHAT exploration unless new evidence materially contradicts the conclusions below.

The project is now **HOW-driven**.

The question is no longer whether commerce-native specialization can have value. The evidence says it can. The question is how far that physical advantage can be extended across real commerce retrieval traffic while preserving relevance, fallback safety, and the native performance multiplier.

## What the evidence already establishes

The current evidence supports the following framing:

1. **Whole-engine replacement is the wrong target.** Phase 2 falsified the original traffic-weighted 5–10x whole-engine replacement thesis: the structural native path was extremely fast, but applied to too little traffic, while hybrid/punt execution added too much work.
2. **The physical advantage is real.** On the supported structural slice, the native path measured roughly **87–105x faster than Solr**.
3. **Transparent fallback is viable.** Phase 3 measured fallback tax as statistically indistinguishable from zero, which means rejected traffic can return immediately to the mature Solr path without recreating Phase 2's hybrid penalty.
4. **Candidate-set size is not sufficient for safe admission.** Naive lexical narrowing failed the relevance budgets and exposed a real false-coverage bug caused by guaranteed-empty candidate sets.
5. **Semantic/structural anchoring matters.** Restricting lexical narrowing to queries that already contain a structural constraint materially improved the relevance profile and produced a kept mechanism with much higher safe coverage.
6. **Mechanism accounting can be made rigorous.** The current combined mechanisms have been measured with explicit overlap checks, Pareto frontiers, confidence intervals, and preserved negative evidence.

Together these results support a stronger statement than the original project thesis:

> A commerce-native execution plane should not replace a mature generic search engine outright. It should safely absorb the structurally tractable and semantically recoverable majority of commerce retrieval traffic, preserve a large native physical multiplier, and transparently fall back for the unresolved tail.

## Research objective

The north-star research objective is:

> **Move the physical multiplier from an extreme minority slice toward the median request without violating relevance safety or collapsing the multiplier.**

Formally, the program seeks to maximize safe native coverage subject to three hard constraints:

```text
relevance degradation <= accepted budget
fallback tax ~= 0
native physical multiplier >= 80x floor
```

The project should stop treating every possible search feature as equally valuable. Every new mechanism must answer one concrete question:

> What is the next largest traffic class that can be admitted safely and still execute with the required physical advantage?

## Two percentile goals, not one

### Free-text / search traffic: target P50

Search is impressive only if the safe native slice crosses the median.

- Target: **>=50% of free-text/search retrieval traffic** admitted to native execution under the relevance budget.
- Below 50% coverage, the aggregate p50 cannot move materially regardless of how fast the native minority slice becomes.
- During this phase, p95/p99 are not the primary search optimization target. Ambiguous and unresolved tail traffic may remain on Solr permanently if that is the safe economic choice.

The key research question is therefore not how to improve tail latency for every query. It is how to extend safe, high-multiplier execution far enough to change the median.

### Category / collection / PLP traffic: target P95

Browse traffic has a different structure and should have a different target.

Category, collection, filtering, faceting, sorting, pagination, inventory gating, and related PLP operations are largely explicit structural retrieval rather than open-ended query understanding. They should therefore be expected to admit much more aggressively if the physical hypothesis holds.

- Target: **>=95% of category/collection/filter/facet retrieval traffic** handled natively where semantics are explicit.
- This traffic must be measured separately from free-text search rather than hidden inside one aggregate offload number.

The combined system should eventually report a broader metric:

```text
Native Retrieval Share =
  native free-text
+ native category/collection
+ native filter/facet
+ other native structural retrieval
------------------------------------
  all commerce retrieval requests
```

## The 80x red line

Coverage growth is not allowed to turn the native system into a second general-purpose search engine.

The existing 87–105x structural FastPath result establishes the reference physical advantage. For the HOW-driven phase:

> **80x is the red line for promoted fast-path mechanisms.**

A mechanism that increases coverage but materially collapses the native physical multiplier below this floor should be presumed REJECT unless an explicit whole-workload analysis demonstrates a stronger tradeoff and that tradeoff is reviewed separately.

This constraint is deliberate. The project is not trying to maximize coverage at any cost. It is trying to extend a proven physical multiplier over as much safe traffic as possible.

## The HOW loop for search coverage

Search coverage expansion should now operate as a directed traffic-class mining loop:

1. rank fallback/rejected populations by real traffic share;
2. characterize the missing semantic or structural fact;
3. propose the smallest mechanism that could make that population safely executable;
4. implement and test the mechanism;
5. replay the full real workload;
6. measure marginal coverage, overlap with existing mechanisms, relevance point estimate and confidence interval, admission precision, and native latency;
7. adversarially test large favorable results;
8. KEEP, REJECT, or PARK;
9. move to the next highest-volume class.

The goal is purposeful convergence, not feature accumulation.

Relevant mechanism families include:

- structurally anchored lexical refinement;
- learned semantic implication / predictive fixup;
- model/product-family to brand/category/product-type implications;
- typed residual-attribute recognition;
- entity relationships;
- catalog-derived semantic completion;
- merchant-specific compiled semantic routes.

Learned/model-derived knowledge belongs in the offline control plane. It must be proposed, replayed, validated, promoted, and compiled. The serving path remains deterministic.

## Browse/PLP as a separate physical-execution program

Category/collection/PLP work should not be treated as just another query-understanding mechanism. It is a physical-execution research track.

The relevant comparison is not against a naive generic search request. Mature Lucene/Solr/Elasticsearch systems already provide specialized non-scoring filter paths, cached DocSets/bitsets, DocValues, optimized facet methods, request caches, index sorting, and early termination.

The fair question is:

> After using the strongest native Solr/Lucene optimizations, does a first-class commerce physical layout shift the saturation frontier or degradation curve under realistic browse/PLP load?

Therefore the benchmark sequence is:

1. establish and audit the strongest fair Solr/Lucene baseline;
2. measure cold, warm, and hot-cache regimes;
3. then increase catalog/working-set size, category cardinality, filter complexity, facet count/cardinality, sort diversity, pagination depth, inventory/price churn, concurrency, and QPS;
4. compare degradation and breakpoint curves rather than only idle p50 latency.

A useful result may be that both systems are fast at low load but the commerce-native layout reaches its knee materially later.

## Havenask is the performance anchor, not the product target

Havenask should be treated as the relevant performance anchor because it already demonstrates that search specialization, multiple physical index forms, and commerce-scale production engineering can outperform generic search-engine assumptions.

The project should not claim novelty merely from using specialized indexes or avoiding generic lexical execution where semantics are known.

Instead, Havenask helps define the bar:

> Can a commerce-native system achieve comparable specialization benefits in a much lower-cost deployment envelope, while adding safe semantic admission, transparent generic fallback, and native multi-tenant isolation?

The target market and deployment problem are different.

Havenask is designed for Alibaba-scale distributed search. This project is aimed at **North American mid-market and enterprise ecommerce workloads**, where the economically important question is often how much useful specialization can be delivered without requiring hyperscale cluster complexity or a large fixed per-merchant serving footprint.

## eCommerceSearchBench becomes standard practice

Alibaba's `eCommerceSearchBench` should become part of the standard research workflow.

It serves several purposes:

- an external workload beyond the original ESCI corpus;
- a reality check against a commerce-search benchmark associated with a mature specialized system;
- a way to compare latency, QPS, and resource behavior under a less project-specific workload;
- protection against overfitting the architecture to one query/catalog distribution.

The benchmark should be preserved as faithfully as practical. Differences in deployment scale or workload semantics must be documented rather than normalized away. Claims should be made only where the deployment envelopes are comparable.

## Low-cost native multi-tenancy is part of the thesis

The product thesis is not simply that a native path is faster.

It is:

> **First-class commerce semantics may enable a lower-cost multi-tenant serving architecture with simpler isolation boundaries and a lower per-merchant fixed cost than a general-purpose search cluster.**

This needs its own architecture and experiments.

Questions include:

- tenant-local semantic FIB / compiled context;
- tenant-specific category, collection, merchandising, and learned rules;
- shared immutable structures where safe;
- separation of high-churn inventory and price state;
- noisy-neighbor isolation;
- per-tenant memory and index accounting;
- cache isolation and pollution;
- concurrency fairness;
- shard/partition strategy versus a shared process;
- tenant-specific rule promotion and withdrawal;
- cost per tenant and deployment complexity.

This is where the project's target market can diverge materially from Havenask even if some physical primitives are similar.

## Real production traffic closes the workload-model gap

Real production request data is available and should replace scenario-only workload assumptions as soon as it can be safely derived.

The important unit is **backend retrieval requests**, not sessions.

At minimum characterize:

```text
free-text search
category render
collection render
facet/filter refinement
sort
pagination
SKU/exact lookup
recommendation/candidate retrieval where applicable
other retrieval
```

Also characterize hot-versus-long-tail request distribution, filter/facet combinations, inventory/price update frequency, concurrency/QPS distribution, and tenant skew where permitted.

Raw production data does not need to be committed. A sanitized/reproducible derived workload is sufficient for the benchmark harness.

## What no longer needs to be debated unless new data contradicts it

The project should not repeatedly reopen the following questions:

- whether a generic lexical engine should be rebuilt from scratch;
- whether every query must execute natively;
- whether fallback is a design failure;
- whether a tiny 100x fast-path slice is enough to change whole-workload economics;
- whether candidate-set size alone is an adequate admission rule;
- whether mature Solr/ES baselines can be treated as naive filter implementations.

These questions have either been experimentally answered or incorporated into the stronger framing above.

## Paper-level articulation

The current paper-level thesis is:

> Mature generic search engines are effective safety-net systems for the unresolved tail, but a large fraction of commerce retrieval contains enough explicit or recoverable structure to admit a substantially simpler execution path. The research problem is therefore not whole-engine replacement; it is how to safely extend a large commerce-native physical multiplier across enough real traffic to change the median request, while preserving transparent fallback and realistic relevance guarantees.

A second systems contribution may emerge from browse/PLP traffic:

> Generic search engines frequently execute high-volume requests that are semantically browse/filter operations. Even after applying mature Lucene/Solr optimizations, first-class commerce physical representations may shift the saturation and update-cost frontier for these workloads.

A third product/systems contribution may emerge from deployment economics:

> The same commerce-native semantics may support a low-cost native multi-tenant isolation model better matched to North American ecommerce deployment scales than hyperscale commerce-search architectures.

## Success envelope

A strong end state would show:

- free-text safe native coverage crossing **P50 / >=50%**;
- category/collection/PLP native handling approaching **P95 / >=95%**;
- promoted native fast paths preserving **>=80x** physical advantage;
- transparent fallback remaining statistically negligible;
- real production traffic mix showing material whole-workload native retrieval share;
- external validation using `eCommerceSearchBench` and Havenask as the performance anchor;
- a defensible low-cost multi-tenant architecture whose economics fit the target market.

If search stalls below 50% but the structural boundary is clear and browse/PLP economics remain strong, that is still a publishable bounded result. If mature Solr/Havenask optimizations erase the expected advantage, that boundary is also a valid result and should be preserved.
