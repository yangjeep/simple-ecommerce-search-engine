# Novelty Check: Commerce-Native Safe Offload Research Program

## Status

This is a living prior-art / novelty-risk document. It is intentionally conservative. A component that exists in prior work must not be presented as novel merely because this repository implements it differently.

Current conclusion: **most individual ingredients are known; the strongest potential novelty is in the systems composition, target workload, and evaluation objective.**

The paper should therefore avoid claims such as "we introduce ecommerce query parsing", "we introduce semantic query routing", "we introduce specialized commerce indexes", or "we introduce implicit attribute inference".

## Novelty matrix

| Project idea | Closest known prior art | Novelty status | Implication |
|---|---|---|---|
| Typed ecommerce query understanding / extracting brand, category, attributes | Extensive ecommerce query parsing and attribute extraction literature; catalog-knowledge-graph methods | **Not novel** | Treat as prerequisite / mechanism only |
| Implicit semantic fixup such as `iphone 8 -> brand=apple` or `air force 1 -> brand=nike` | Amazon Query Attribute Recommendation (RecSys 2022) and Implicit Query Parsing for Product Search (SIGIR 2023) infer implicit attributes from attribute relations + behavior | **Not novel as a concept** | #16 must be framed as a coverage mechanism inside the larger system, not a standalone contribution |
| Offline model/LLM knowledge used to improve online retrieval without expensive model calls on every query | Query rewriting, distillation, synthetic-data pipelines, attribute-recommendation systems | **Weak standalone novelty** | Any claim must focus on compiled deterministic admission/route behavior and measured serving economics |
| Query routing / selecting a specialized engine | Web query routing / federated search since at least the 1990s/2000s | **Not novel** | "Routing" alone is not contribution |
| Selective search: search only a subset of corpus/shards | Large selective-search literature; MICO; distributed selective search | **Not novel** | Need to distinguish specialized physical execution from ordinary shard pruning |
| Ecommerce-specific selective execution based on category intent | Amazon `Light Feed-Forward Networks for Shard Selection in Large-scale Product Search` (SIGIR eCom 2020): category-intent prediction, selective shard search, double-digit cost reduction without customer-experience degradation | **Very close prior art** | This is a major novelty risk. Our paper must explain why routing to a cheaper physical execution plane with transparent generic fallback and a coverage/relevance/multiplier frontier is a different problem than category-shard selection |
| Use structure to reduce semantic product-search search space | Amazon `Embracing Structure in Data for Billion-Scale Semantic Product Search` partitions a query-product interaction graph and searches relevant partitions | **Not novel broadly** | "Structure reduces search work" is background, not contribution |
| Multi-stage retrieval / cascades / early exits | Mature IR literature and production systems | **Not novel** | Our value must be in the admission contract, physical path, and workload economics |
| Cache frequent/easy product-search queries instead of running expensive pipeline | Amazon ROSE (WWW 2022) explicitly covers most traffic with near-constant-time cache lookup and avoids expensive models | **Close conceptual neighbor** | Median/tail economics alone are not novel. Need to distinguish semantic execution from result/query caching and show behavior under catalog/state changes |
| Commerce-specific search engine built from scratch because ecommerce differs from web | eBay Cassini architecture (SIGIR eCom 2017) | **Not novel** | Do not claim that ecommerce warrants a custom engine as novel |
| Multiple specialized physical index types for commerce search | Havenask / IndexLib; KV/KKV, inverted, attribute/bitmap, realtime update machinery | **Not novel** | Havenask is the anchor and wheel-reinvention check |
| Realistic end-to-end ecommerce search benchmark driven by production data | Alibaba eCommerceSearchBench / AIBench ecommerce-search benchmark | **Not novel** | Incorporate as standard external validation rather than inventing another synthetic benchmark in isolation |
| Browse/category/PLP as a first-class physical workload | Common in production search systems, but relatively underrepresented as a paper-level specialization target compared with query search | **Potentially differentiating, not yet established novel** | Stronger if production logs prove browse-heavy North-American merchant traffic differs materially from marketplace benchmark distributions |
| Safe asymmetric architecture: specialized fast path, immediate generic fallback on abstention | Related to cascades, selective search, fallback, QPP, federated routing | **Composition may be novel; primitive is not** | Need a crisp formal distinction and related-work section |
| Optimize a `coverage × relevance safety × physical multiplier` frontier, with fallback tax near zero | No directly matching ecommerce systems paper found in current review | **Promising novelty candidate** | Likely central paper contribution if literature review continues to hold |
| Extend an ~80–100x physical advantage from minority traffic toward P50 while keeping generic tail unchanged | Related to cache/early-exit/cascade systems, but no directly matching ecommerce semantic-offload formulation found | **Promising novelty candidate** | Requires strong empirical result; wording must acknowledge neighboring cache/cascade work |
| Search target P50 + browse target P95 based on real target-market request distribution | No directly matching formulation found; depends on production data | **Potential novel workload/system framing** | Becomes strong only if production-log characterization supports it |
| Low-cost commerce-native multi-tenancy as the deployment objective, rather than Alibaba-scale distributed search | Multi-tenant search exists broadly; no directly matching commerce-semantic isolation/cost study found in current review | **Potential systems novelty** | Requires actual isolation, noisy-neighbor, memory, and cost-per-tenant experiments |
| Separate slow semantic state from high-churn inventory/price/availability overlays | Generic realtime/partial update systems exist; Havenask has mature realtime machinery | **Likely not novel alone** | Could contribute only if measured commerce-specific layout materially changes update/search coexistence or tenant economics |

## Closest prior work that must be discussed explicitly

### 1. Amazon category-intent shard selection (SIGIR eCom 2020)

This is the closest prior work to the semantic-offload story found so far.

It observes that ecommerce product shards correspond to categories and that queries imply category intent. A lightweight model predicts relevant shards, so only those shards are searched. The system is evaluated in terms of infrastructure cost and relevance/customer-experience impact and reports double-digit cost reduction without customer-experience degradation.

Why this is close:

```text
commerce semantics
  -> lightweight routing decision
  -> execute less retrieval work
  -> preserve quality
  -> reduce serving cost
```

How our research must differ if it is to be novel:

```text
Amazon shard selection:
query -> select subset of category shards -> run the normal retrieval engine there

our hypothesis:
query -> correctness-aware semantic admission
      -> execute a materially different commerce-native physical plan
      -> otherwise immediately execute the unchanged mature fallback
```

The primary measured object is also different: not shard-recall/cost alone, but the **safe-offload frontier** under explicit relevance budgets, fallback tax, per-hit physical multiplier, overlap between mechanisms, and the point where the multiplier reaches median traffic.

This distinction must survive a full-paper reading, not just abstract comparison.

### 2. Amazon Query Attribute Recommendation / Implicit Query Parsing

Amazon has already published systems that infer implicit attributes not literally present in the query, using query parsing, intent classification, attribute-relation graphs, behavior data, and catalog knowledge.

Therefore examples like:

```text
iphone 8 -> brand=apple
           operating_system=ios
```

are established prior art.

Our learned semantic implication issue (#16) is only publishable as a **mechanism contributing to safe offload coverage**, not as the paper's core novelty.

### 3. Selective search / federated query routing

Routing queries to selected collections, shards, or specialized engines is decades old. Modern selective-search systems learn partitions and route queries to a subset to reduce latency/computation while maintaining effectiveness.

Therefore terms such as "semantic router", "forwarding plane", and "query routing" are useful architectural vocabulary but cannot themselves carry novelty.

### 4. ROSE / frequent-query caches

ROSE is important because it already makes a distributional systems argument: serving all product-search traffic through expensive models is unnecessary; much traffic can be covered by near-constant-time cache behavior, with long-tail handling remaining elsewhere.

This is conceptually close to "make the median cheap while leaving hard tail expensive".

Our distinction must be that the native path is **semantic/physical execution over live commerce state**, not cached result reuse. Tests involving inventory/price churn, category/filter composition, unseen-but-structurally-equivalent requests, and multi-tenant state are therefore important novelty defenses.

### 5. Havenask / eCommerceSearchBench

Havenask already demonstrates extensive search-system specialization and multiple physical index types. Alibaba's eCommerceSearchBench is an end-to-end workload driven by real Taobao user logs and production-derived data, including personalized recommendation/search planning components.

We should treat both as prior-art/performance anchors, not as evidence that specialization itself is new.

## Current strongest novelty hypothesis

The current strongest paper-level claim is **not** any individual parser, route, index, or cache mechanism.

It is the following systems problem and empirical program:

> **Given a mature generic ecommerce-search fallback, how much real commerce retrieval traffic can be correctness-aware admitted into a materially cheaper specialized physical execution plane, while keeping abstention effectively free and preserving a large per-hit physical multiplier?**

The potential contribution is the combination of:

1. an asymmetric contract where the specialized system is allowed to abstain and the generic engine remains the safety net;
2. explicit semantic/correctness admission rather than generic query-difficulty or topic-shard routing;
3. a different physical execution path, not merely fewer copies of the same retrieval engine;
4. a measured **coverage × relevance × physical-multiplier × fallback-tax** frontier;
5. rigorous accounting of disjoint/overlapping admission mechanisms and confidence-certified operating points;
6. extension from free-text search to structurally dominant browse/PLP traffic;
7. target-market workload characterization and low-cost native multi-tenancy as deployment constraints.

This combination appears differentiated in the current search, but **must still be treated as a hypothesis pending deeper related-work review**.

## High-risk novelty questions to answer before paper submission

### A. Is safe-offload just selective search with different terminology?

We need to show that ordinary selective search chooses *where* to run essentially the same retrieval algorithm, whereas our system chooses *whether a different physical execution semantics is sufficient* and otherwise delegates unchanged to the general engine.

Ablation candidate:

```text
category/shard pruning + Solr
vs
commerce-native physical execution + fallback
```

This would directly quantify the distinction from Amazon's 2020 work.

### B. Is the P50 story just caching?

Compare against a competent hot-query/result cache where realistic. Demonstrate which gains survive:

- unseen requests sharing the same structural semantics;
- filter/facet combinations;
- dynamic inventory/price changes;
- long-tail categories;
- tenant-specific state.

If a normal cache gets the same economics, narrow the claim.

### C. Does query performance prediction / cascade literature already formalize the same safety problem?

Review QPP/selective-query-processing literature in detail. Current evidence shows related decision problems, including deciding when to trigger special processing or fallback, but not the exact ecommerce specialized-physical-execution frontier.

### D. Is browse/PLP specialization genuinely a research gap?

Production engines obviously execute browse/filter/facet traffic. The novelty cannot be "category pages exist". The research gap would need to be one of:

- target-market workload characterization showing this class dominates a merchant segment;
- a new physical/saturation advantage after strongest Solr/Havenask-style optimization;
- a multi-tenant cost/isolation consequence.

### E. Is native multi-tenancy sufficiently different from standard shared-index tenant filtering?

Benchmark against realistic tenant-filtered Solr/ES patterns and existing multi-tenant search architectures. Novelty requires more than adding `tenant_id` to a bitmap.

## Immediate experimental implications of the novelty check

1. Keep #16, but **do not** present implicit semantic inference as new.
2. Add Amazon 2020 shard selection as a mandatory baseline/ablation concept for the search-coverage paper story.
3. Add a competent query/result-cache baseline for high-frequency/head traffic where applicable, motivated by ROSE.
4. Use Havenask/eCommerceSearchBench as external specialization/workload anchors.
5. Prioritize production-log workload characterization because it can establish a target-market difference that existing marketplace literature does not answer.
6. Make multi-tenant cost/isolation experiments concrete; otherwise remove multi-tenancy from the claimed contributions.
7. Continue searching related work around selective query processing, QPP, cascades, and vertical/federated search before freezing the paper's novelty statement.

## Current verdict

**The project is not obviously duplicative, but the novelty is narrower and more systems-oriented than the original architecture language suggested.**

Known / prior-art ingredients:

```text
commerce query parsing
implicit attributes
query routing
selective search
cascades / early exit
specialized indexes
realtime updates
head-query caches
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

The paper should win on **problem formulation + end-to-end systems evidence + workload characterization**, not on claiming invention of its individual building blocks.
