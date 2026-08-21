# Novelty Check: Commerce-Native Safe Offload Research Program

## Status

This is a living prior-art / novelty-risk document. It is intentionally conservative. A component that exists in prior work must not be presented as novel merely because this repository implements it differently.

Current conclusion: **most individual ingredients are known; the strongest potential novelty is in the systems composition, target workload, and evaluation objective.**

The paper should avoid claims such as "we introduce ecommerce query parsing", "we introduce semantic query routing", "we introduce specialized commerce indexes", or "we introduce implicit attribute inference".

## Primary benchmark boundary: execution, not answer reuse

The core performance comparison is between two systems that both execute the current request from their indexes:

```text
Solr / Elasticsearch:
request -> index structures -> execute -> current result

Commerce-native:
request -> semantic admission -> commerce-native index structures -> execute -> current result
```

The primary comparison should **not** use query-result / response caching on either side.

Index-resident acceleration is allowed and required for fairness. This includes, for example:

- Solr filter/query execution paths, cached DocSets/bitsets where they are part of normal index execution, DocValues, facet optimizations, index sorting, early termination, and other native Lucene/Solr machinery;
- commerce-native bitmaps, typed columns, compiled category/collection membership, semantic FIB structures, precomputed index metadata, and other index-resident physical structures.

The boundary is:

> **Index-resident acceleration is part of the execution substrate; answer memoization is outside the primary comparison.**

A result cache can be placed in front of either system and is therefore orthogonal to the main research question. Cache literature such as ROSE is still useful related work because it also studies how to make common ecommerce requests cheaper, but it solves a different problem: reuse across requests rather than cheaper execution of each request.

The paper should state this distinction directly rather than treating caching as a competing baseline:

> Prior cache-based systems reduce cost by reusing work across requests. We study the cost of executing each request against current indexed commerce state. Result caching is orthogonal and can be composed with either serving architecture.

## Novelty matrix

| Project idea | Closest known prior art | Novelty status | Implication |
|---|---|---|---|
| Typed ecommerce query understanding / extracting brand, category, attributes | Extensive ecommerce query parsing and attribute extraction literature | **Not novel** | Treat as prerequisite / mechanism only |
| Implicit semantic fixup such as `air force 1 -> brand=nike` | Amazon Query Attribute Recommendation (RecSys 2022); Implicit Query Parsing for Product Search (SIGIR 2023) | **Not novel as a concept** | #16 is a coverage mechanism inside the larger system, not a standalone contribution |
| Offline model/LLM knowledge compiled into online retrieval state | Query rewriting, distillation, attribute-recommendation and knowledge-graph systems | **Weak standalone novelty** | Contribution must come from admission + serving economics, not from using an LLM offline |
| Query routing / selecting shards or engines | Federated search, selective search, query routing literature | **Not novel** | Routing language is architectural vocabulary only |
| Ecommerce-specific category-intent shard selection | Amazon `Light Feed-Forward Networks for Shard Selection in Large-scale Product Search` (SIGIR eCom 2020) | **Very close prior art** | Mandatory related-work comparison and ablation target |
| Use structure to reduce retrieval search space | Selective search and structured semantic-search literature | **Not novel broadly** | Background, not contribution |
| Multi-stage retrieval / cascades / early exits | Mature IR literature | **Not novel** | Need to distinguish correctness-aware admission to a different physical path |
| Cache frequent/easy product-search queries | Amazon ROSE and related cache systems | **Orthogonal prior art** | Discuss as reuse-across-requests; do not make it the primary comparator |
| Commerce-specific search engine because ecommerce differs from web search | eBay Cassini and other production systems | **Not novel** | Do not claim vertical search itself as new |
| Multiple specialized physical index types | Havenask / IndexLib; KV/KKV, inverted, bitmap/attribute, realtime machinery | **Not novel** | Havenask is the physical-performance anchor |
| End-to-end ecommerce benchmark from production-derived data | Alibaba eCommerceSearchBench / AIBench | **Not novel** | Incorporate as external standard validation |
| Browse/category/PLP as a first-class optimization target | Common in production systems, less common as the central research target | **Potentially differentiating** | Strong only if production logs establish the target-market traffic distribution and native execution survives strong Solr/Havenask baselines |
| Safe asymmetric architecture: specialized fast path, immediate mature fallback on abstention | Related to cascades, selective search, QPP, federated routing | **Composition may be novel; primitive is not** | Requires crisp distinction and empirical evidence |
| `coverage × relevance safety × physical multiplier × fallback tax` frontier | No directly matching ecommerce systems formulation found in current review | **Promising novelty candidate** | Likely central paper contribution if deeper review continues to hold |
| Extend an ~80–100x physical execution advantage toward search P50 | Related to selective processing/cascades, but no directly matching ecommerce formulation found | **Promising novelty candidate** | Requires strong result and explicit prior-art positioning |
| Search target P50 + browse target P95 based on target-market request distribution | No directly matching formulation found | **Potential workload/system novelty** | Depends on production-log validation |
| Low-cost commerce-native multi-tenancy | Multi-tenant search exists broadly | **Potential systems novelty** | Needs actual isolation/noisy-neighbor/cost-per-tenant experiments |
| Separate slow semantic state from high-churn operational state | Generic realtime/partial update systems exist; Havenask has mature realtime machinery | **Likely not novel alone** | Contributes only if commerce-specific layout changes update/search coexistence or tenant economics materially |

## Closest prior work that must be discussed explicitly

### 1. Amazon category-intent shard selection (SIGIR eCom 2020)

Amazon's work predicts product-category intent and searches only relevant category shards, reducing infrastructure cost while preserving customer experience.

The similarity is real:

```text
commerce semantics
  -> lightweight decision
  -> execute less retrieval work
  -> preserve quality
  -> reduce serving cost
```

The distinction that must survive full-paper review is:

```text
Category-shard selection:
query -> select subset of category shards -> run the normal retrieval engine there

Our hypothesis:
query -> correctness-aware semantic admission
      -> execute a materially different commerce-native physical plan
      -> otherwise immediately execute the unchanged mature fallback
```

The paper should directly compare these ideas with an ablation such as:

```text
category/shard pruning + Solr
vs
commerce-native physical execution + fallback
```

If ordinary shard/category pruning captures the same benefit, the novelty claim must narrow.

### 2. Amazon Query Attribute Recommendation / Implicit Query Parsing

Amazon has already published systems that infer implicit attributes not literally present in the query using catalog structure, knowledge graphs, and customer behavior.

Therefore mappings conceptually like:

```text
iphone 8 -> brand=apple
           operating_system=ios
```

are established prior art.

Our learned semantic implication work (#16) is a mechanism for increasing **safe native coverage**. Its research value is measured by whether inferred structure enables a cheaper, correctness-safe physical execution path, not by the inference mechanism alone.

### 3. Selective search / federated query routing / cascades

Selecting collections, shards, or processing paths is a mature research area. These systems generally answer variants of:

> Where, or how deeply, should the ordinary retrieval pipeline run?

Our intended distinction is:

> Is the request semantically constrained enough that a **different physical execution regime** is sufficient, allowing the generic retrieval engine to be skipped entirely for that request?

This distinction must be formalized through the admission contract and validated by ablation, not asserted by terminology.

### 4. Cache-based head-query acceleration (e.g. ROSE)

Cache-based product-search systems are relevant background because they also reduce the cost of common/head requests. However, they optimize **reuse across requests**.

Our primary performance question is about **execution cost per real request** with answer caching excluded from both sides.

The two techniques are orthogonal and composable:

```text
optional result cache
       |
      miss
       v
semantic admission
   /          \
native       Solr
index exec   index exec
```

Therefore ROSE should be discussed as a neighboring optimization layer, not as the primary baseline. The paper should not claim superiority over caching, and should not include a cache-vs-no-cache experiment as evidence for the core execution thesis.

### 5. Havenask / IndexLib

Havenask already demonstrates extensive search specialization, multiple physical index forms, realtime update machinery, and hyperscale production engineering. It is the strongest obvious physical-performance anchor and wheel-reinvention check.

The research question is not whether specialized indexes can work; Havenask already answers that.

Our differentiated target is a lower-cost deployment envelope for North American mid-market / enterprise commerce, with safe semantic admission, transparent generic fallback, browse-heavy workload optimization where supported by production data, and native multi-tenant isolation.

### 6. Alibaba eCommerceSearchBench

`eCommerceSearchBench` provides a production-derived ecommerce systems workload and should become standard external validation. It protects against overfitting to ESCI and provides a marketplace/search-heavy comparison point.

It should be preserved faithfully where practical, with deployment-envelope differences documented rather than normalized away.

### 7. eBay Cassini / production vertical-search architectures

Production systems such as Cassini already establish that ecommerce search has domain-specific requirements and can justify custom engineering. That observation is background.

Our contribution must be narrower: which commerce traffic classes can safely use a materially cheaper physical plan, how that frontier grows, and whether the resulting serving model fits a lower-cost multi-tenant market.

## Current strongest novelty hypothesis

The strongest paper-level claim currently appears to be:

> **Given a mature generic ecommerce-search fallback, how much real commerce retrieval traffic can be correctness-aware admitted into a materially cheaper specialized physical execution plane, while keeping abstention effectively free and preserving a large per-hit physical multiplier?**

The potential contribution is the combination of:

1. an asymmetric contract where the specialized system can abstain and the generic engine remains the safety net;
2. explicit semantic/correctness admission rather than generic query-difficulty or topic-shard routing;
3. a different physical execution path, not merely fewer copies of the same retrieval engine;
4. a measured **coverage × relevance × physical-multiplier × fallback-tax** frontier;
5. explicit mechanism-overlap accounting and confidence-certified operating points;
6. extension from free-text search to browse/category/PLP traffic;
7. target-market workload characterization;
8. low-fixed-cost native multi-tenancy if experimentally supported.

This remains a hypothesis pending deeper related-work review.

## Target-market workload hypothesis

A potentially important distinction from marketplace systems such as Alibaba/Havenask is the dominant user journey.

The hypothesis to validate from real production request logs is:

- large marketplace environments are relatively search/recommendation driven;
- North American independent/DTC/merchant sites may be substantially more browse/category/collection/PLP driven, with site search acting as a supplementary path for many merchants.

If supported, the workload difference can justify a different specialization target:

```text
search: expand safe native coverage toward P50
browse/category/PLP: drive native handling toward P95
```

This would make workload characterization part of the systems contribution rather than product-market commentary.

## High-risk novelty questions before submission

### A. Is safe offload only selective search under different terminology?

Mandatory defense: compare category/shard pruning + Solr against commerce-native execution + fallback.

### B. Does the native physical path remain materially different from mature Lucene/Havenask execution?

Mandatory defense: strongest fair Solr baseline, Havenask archaeology/eCommerceSearchBench, and profiling/breakpoint analysis.

### C. Does query-performance-prediction / cascade literature already formalize the same admission problem?

Continue deep review of QPP, selective query processing, cascades, and vertical/federated search before freezing the novelty statement.

### D. Is browse/PLP specialization genuinely differentiated?

The novelty cannot be that category pages exist. It must come from production-log workload characterization, a physical/saturation advantage after strongest mature baselines, and/or multi-tenant economic consequences.

### E. Is native multi-tenancy more than standard tenant filtering?

Benchmark against realistic shared-index/tenant-filter patterns. Novelty requires isolation/cost advantages beyond adding `tenant_id` to a generic index.

## Immediate experimental implications

1. Keep #16, but do not present implicit semantic inference as new.
2. Add Amazon 2020 category-intent shard selection as a mandatory search ablation/reference.
3. Treat result caching as orthogonal; exclude answer caching from the primary execution comparison on both sides.
4. Allow all legitimate **index-resident** optimizations in Solr/ES/Havenask and the native engine.
5. Use Havenask/eCommerceSearchBench as standard external specialization/workload anchors.
6. Prioritize real production request-log characterization.
7. Make multi-tenant isolation and cost concrete or remove it from claimed contributions.
8. Continue related-work review around QPP, selective processing, cascades, routing, and vertical search.

## Current verdict

**The project is not obviously duplicative, but the novelty is narrower and more systems-oriented than the original architecture language suggested.**

Known ingredients:

```text
commerce query parsing
implicit attributes
query routing
selective search
cascades / early exit
specialized indexes
realtime updates
vertical search engines
```

Potentially novel contribution:

```text
correctness-aware semantic admission
+ materially different commerce-native physical execution
+ transparent mature fallback
+ explicit safe-coverage / relevance / multiplier frontier
+ browse-heavy target-market workload characterization
+ low-fixed-cost native multi-tenancy
```

The paper should win on **problem formulation + end-to-end systems evidence + workload characterization**, not on claiming invention of individual building blocks.
