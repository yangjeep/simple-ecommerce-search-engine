# Related-Work Positioning for the Commerce-Native Execution Thesis

## Purpose

This note translates the novelty review into paper-facing positioning. It is deliberately conservative: closely related systems should be acknowledged directly, and differences should be stated in terms of the actual object being optimized rather than terminology.

The primary research object in this project is **per-request indexed execution**, not result reuse.

## Experimental boundary: index execution, not answer caching

The core comparison is:

```text
Solr / Elasticsearch
request -> index-resident structures -> execute current request -> current result

Commerce-native
request -> semantic admission -> commerce-native index-resident structures
        -> execute current request -> current result
```

Both sides should be allowed their legitimate native/index-resident optimizations. Examples include Solr/Lucene filter paths, DocSets/bitsets, DocValues, optimized faceting, index sorting and early termination, and equivalent native commerce bitmaps, typed columns, compiled memberships, and semantic FIB structures.

The primary benchmark excludes **query-result / response memoization** on both sides. This is not because caching is unimportant; it is because it answers a different research question.

The boundary is:

> **Index-resident acceleration is part of the execution substrate. Reusing a previously materialized answer across requests is a separate, orthogonal optimization layer.**

A hosted application pays real execution cost on every request that misses or bypasses a result cache. This research asks whether that execution itself can be made much cheaper through commerce-native indexing and semantics.

No cache-vs-no-cache experiment is required for the core thesis. A cache can be composed in front of either system without changing the underlying execution comparison.

---

## 1. ROSE: robust caches for Amazon product search

**Prior work:** Chen Luo et al., *ROSE: Robust caches for Amazon product search*, The Web Conference 2022.

ROSE is relevant because it attacks product-search serving cost and latency by improving query-result reuse, including robustness to typos, misspellings, and redundant query forms. Amazon describes the motivating behavior explicitly: popular query results are stored and can be served without re-executing product retrieval.

### What ROSE optimizes

```text
request A -> expensive retrieval -> materialized answer
request B ~ A -> cache lookup -> reuse answer
```

The gain comes from avoiding repeated retrieval work across requests.

### What this project optimizes

```text
request -> current indexed state -> execute the request more cheaply
```

Every native FastPath hit is still a real execution of the request against the current index/state. The optimization is the physical representation and execution path, not answer memoization.

### Relationship

The two techniques are **orthogonal and composable**, not competing substitutes:

```text
optional result-cache layer
          |
        cache miss
          v
semantic admission
   /              \
native index     generic index
execution        execution
```

Therefore:

- keep ROSE in related work;
- do not claim superiority over caching;
- do not add caching to the primary experiment matrix;
- state clearly that a result cache can sit in front of either architecture;
- measure the underlying execution substrate without response caching so the paper isolates the system property under study.

Paper-facing distinction:

> Prior cache-based systems reduce search cost by reusing work across requests. We study the cost of executing each request against current indexed commerce state. Result caching is orthogonal and can be composed with either serving architecture.

---

## 2. Amazon category-intent shard selection

**Prior work:** Heran Lin et al., *Light feed-forward networks for shard selection in large-scale product search*, SIGIR eCom 2020.

This is probably the closest public prior work to the safe-offload story. Amazon observes that product shards correspond to categories and that queries imply category intent. A lightweight classifier selects relevant category shards so only part of the search estate executes the ordinary retrieval path. The work reports double-digit search-engine cost reductions across multiple locales without degrading customer experience and was deployed worldwide.

### Their optimization object

```text
query
-> predict category/shard intent
-> choose fewer shards
-> execute the normal retrieval engine on those shards
```

### Our optimization object

```text
query
-> correctness-aware semantic admission
-> if sufficient, execute a materially different commerce-native physical plan
-> otherwise immediately execute the unchanged mature fallback
```

The distinction is **not** simply that both systems route queries. Query routing and selective search are established prior art.

The important distinction to validate is:

> Shard selection chooses *where to run the ordinary retrieval algorithm*. Commerce-native admission chooses *whether the ordinary retrieval algorithm is needed at all for this request*.

This should remain a mandatory reference/ablation concept. If category/shard pruning plus Solr captures the same physical/economic benefit as the native path, the novelty claim must narrow.

---

## 3. Amazon implicit query parsing / query attribute recommendation

**Prior work:** Amazon's implicit-query-parsing and query-attribute-recommendation work, including *Implicit Query Parsing for Product Search* (SIGIR 2023).

Amazon already demonstrates that product-search queries contain useful implicit commerce attributes not literally present in the query and that catalog structure, knowledge graphs, and customer behavior can infer those attributes.

Conceptually:

```text
iphone 8
-> brand = Apple
-> operating system = iOS
-> product type / family information
```

### Implication for this project

Predictive fixups such as:

```text
air force 1 -> Brand = Nike
```

are **not standalone novelty**.

Issue #16 remains valuable because implicit facts may create the structural anchor required for safe native admission. Its contribution, if any, is causal and systems-level:

```text
learned implicit fact
-> stronger structural anchor
-> safe admission of a previously-fallback request
-> materially cheaper deterministic execution
```

The paper should cite implicit-query work as an enabling mechanism and evaluate whether compiling such knowledge expands the safe-offload frontier.

---

## 4. Selective search, federated routing, cascades, and QPP

There is extensive prior work on routing queries to selected collections/shards, predicting query difficulty, choosing processing depth, and cascading expensive retrieval/ranking stages.

None of these broad ideas should be claimed as new.

The paper must formalize a narrower systems distinction:

```text
traditional selective processing:
choose where/how deeply to execute a retrieval pipeline

commerce-native semantic admission:
decide whether explicit/recoverable commerce semantics are sufficient
for a different physical execution regime to produce an acceptable answer;
otherwise abstain and use the generic engine unchanged
```

This distinction only matters if the native physical plan remains materially different and materially cheaper. The >=80x multiplier floor is therefore part of novelty defense as well as performance discipline: if coverage expansion converges toward ordinary lexical retrieval, the architecture has ceased to be differentiated.

---

## 5. Havenask / IndexLib

Havenask is not prior art to dismiss; it is the strongest obvious proof that search specialization and multiple physical index types can work at serious commerce scale.

Havenask/IndexLib already contains mature machinery around inverted indexes, KV/KKV access, attribute/index structures, realtime updates, memory/segment organization, and distributed serving. Specialized indexing itself is therefore not novel.

### Why Havenask remains the performance anchor

The project should ask:

> After accounting for what a mature specialized engine already knows, what advantage remains from correctness-aware semantic admission, browse-heavy target-market specialization, and a lower-cost multi-tenant deployment model?

The intended market envelope is different:

- Havenask: Alibaba-scale marketplace/distributed serving;
- this project: North American mid-market / enterprise hosted commerce, with low per-merchant fixed cost as a first-class constraint.

This market distinction must be validated through real workload data and cost/isolation experiments rather than asserted.

---

## 6. Alibaba eCommerceSearchBench

Alibaba's eCommerceSearchBench/AIBench ecommerce-search workload is important because it provides an external production-derived systems benchmark associated with a mature marketplace-search environment.

It should become standard external validation rather than a novelty claim.

Its role is to test whether results survive outside the ESCI-derived setup and to provide a marketplace/search-heavy comparison point for the target-market workload hypothesis.

A useful paper contrast, if validated by production logs, would be:

```text
marketplace/search-heavy benchmark distribution
vs
North-American hosted-commerce browse/category/PLP-heavy distribution
```

The architecture should be allowed to differ if the workload distributions materially differ.

---

## 7. eBay Cassini and other vertical commerce-search engines

Systems such as eBay Cassini establish that ecommerce retrieval has enough domain-specific requirements to justify substantial custom search engineering.

This should be treated as motivation/background, not novelty.

The narrower research question here is:

> Which commerce request classes can safely avoid the generic retrieval regime entirely, how far can that frontier be expanded while preserving a large physical multiplier, and does the resulting substrate have better economics for a hosted multi-tenant market?

---

## Recommended paper positioning

Do not claim novelty in:

```text
commerce query parsing
implicit attribute inference
query routing
shard selection
selective search
cascades / early exit
result caching
specialized index types
vertical ecommerce search
```

The strongest current positioning is:

> Prior work has separately demonstrated commerce-aware query understanding, implicit attribute inference, selective shard search, robust query-result caching, and highly specialized ecommerce search engines. We study a different systems question: **given a mature generic fallback, how much real commerce retrieval traffic can be correctness-aware admitted into a materially cheaper physical execution regime, while keeping abstention effectively free and preserving a large per-hit multiplier?**

The empirical object is the frontier:

```text
safe traffic coverage
x relevance/correctness
x physical multiplier
x fallback tax
```

with two workload-specific targets to validate from real production traffic:

```text
free-text search -> extend native execution through P50
category / collection / PLP -> extend native execution toward P95
```

and a deployment question:

```text
can first-class commerce semantics support materially lower fixed cost
and stronger native isolation for a hosted multi-tenant market?
```

That combination, rather than any one mechanism, is the current novelty hypothesis.
