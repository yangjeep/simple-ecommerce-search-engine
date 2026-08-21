# Deep Novelty Audit: SMB Serving, Theory Boundary, and Research Value

## Status

This document records a deeper novelty audit focused on the project's most defensible research contributions, especially the low-cost multi-tenant / SMB-serving thesis. It is intentionally conservative. The purpose is to identify what is genuinely research-worthy, what is prior art, and what evidence is still required before making a publication claim.

## Executive conclusion

The project should **not** claim novelty for any of the following individually:

- ecommerce query parsing;
- implicit attribute inference;
- query routing / selective search;
- category-intent shard selection;
- multi-stage retrieval / cascades;
- vertical search engines;
- bitmap/filter execution;
- multiple physical index types;
- generic multi-tenancy;
- shared-index tenant filtering;
- realtime updates;
- result/query caching.

The strongest potential novelty remains a **systems formulation and end-to-end empirical result**:

> Given a mature generic search engine as a transparent fallback, can a commerce-native system correctness-admit real requests into a materially different, substantially cheaper physical execution regime; extend that regime far enough to change the median search request and the P95 browse/PLP request; and do so in a low-fixed-cost multi-tenant deployment model that is economically suitable for hosted independent merchants rather than hyperscale marketplaces?

This appears materially different from the closest public prior work reviewed so far, but the claim is only defensible if the experiments prove the distinctions below.

---

## 1. The most important theory boundary

The project is **not** proposing another routing system whose purpose is merely to choose where to run a normal retrieval pipeline.

Closest prior work such as category-intent shard selection asks:

```text
query
-> select relevant subset of shards/collections
-> run the ordinary retrieval engine on that subset
```

The project's stronger claim is:

```text
request
-> correctness-aware semantic admission
   -> sufficient semantics: execute a different commerce-native physical plan
   -> insufficient semantics: abstain and immediately execute the unchanged mature fallback
```

The research object is therefore the **boundary between physical execution regimes**, not only routing accuracy.

The core measured frontier is:

```text
safe coverage
x relevance/correctness safety
x per-hit physical multiplier
x fallback tax
```

and the key systems question is how far the specialized regime can be extended before one of those constraints breaks.

No directly matching ecommerce systems formulation was found in the current review.

---

## 2. Why the SMB / hosted-commerce part may be genuinely differentiating

### Existing multi-tenant search practice

Public Elasticsearch guidance makes the generic multi-tenant tradeoff clear:

- index/shard-per-tenant gives isolation but has a fixed per-index/per-shard resource cost;
- many small tenants create oversharding and metadata/heap overhead;
- shared indices reduce fixed overhead but require tenant filtering/routing and expose noisy-neighbor / cache-warmup / shared-working-set problems;
- small tenants are commonly consolidated into shared indices, while larger tenants are split into dedicated indices/shards;
- Elastic explicitly documents that many small indices consume more CPU/memory and that shared-tenant configurations can suffer noisy-neighbor behavior.

Algolia similarly recommends avoiding one index per store/tenant at high tenant counts and instead consolidating tenants into shared indices with a tenant/store attribute filter. Meilisearch exposes tenant tokens that apply tenant filters to a shared index.

These are mature engineering patterns. **Shared-index multi-tenancy itself is not novel.**

### The potentially new question

The gap is narrower:

> Can **commerce semantics themselves** create a better multi-tenant physical/isolation model than generic document-search tenancy?

Potential examples to test:

- tenant-local Semantic FIB / compiled rules with tiny fixed footprints;
- tenant-local category/collection membership structures;
- tenant-local high-churn inventory/price overlays separated from slow-changing catalog state;
- shared runtime/process and common code while retaining hard tenant-addressable state boundaries;
- predictable per-tenant memory accounting;
- selective promotion of large/noisy tenants to separate partitions without requiring a full generic cluster per merchant;
- lower cold/warm fixed cost for mostly-idle merchants;
- reduced cross-tenant cache pollution because common commerce operations execute from tenant-scoped first-class structures rather than a generic shared field/search abstraction.

This is not established novelty yet. It becomes research novelty only if the project measures an actual difference in:

```text
fixed bytes / tenant
incremental bytes / SKU / variant
idle-tenant cost
cold-tenant first-request latency
noisy-neighbor interference
QPS fairness
p50/p95 under skew
update/search coexistence
cost per active tenant
cost per mostly-idle tenant
```

against competent generic-search tenancy patterns.

### Closest public evidence supports the problem, not our solution

Elastic's own documentation provides useful motivation:

- every index and shard has resource overhead;
- too many small shards are inefficient and can destabilize a cluster;
- shared-index tenancy is a common response when many tenants are small;
- shared shards can suffer noisy neighbors;
- historical hosted-Elastic guidance explicitly describes a freemium multi-tenant case where index-per-tenant fixed shard memory made idle customers economically untenable and shared-index cache warming then created a second scaling problem.

This is excellent **problem evidence**, but it does not establish that our commerce-native design solves it. That must be measured.

---

## 3. Market/workload novelty: marketplace search vs independent-merchant browse

A second possible differentiator is workload, not mechanism.

Alibaba's eCommerceSearchBench is explicitly driven by Taobao production data and real user logs and models an ecommerce search system with personalized recommendation. Havenask is a large-scale distributed search engine used by Taobao/Tmall and other Alibaba businesses.

North-American hosted commerce platforms expose a visibly different storefront execution surface. For example, BigCommerce's storefront API uses the same backend search capability for:

- category-page product retrieval;
- category filters;
- facets;
- textual search;
- sorting;
- product listing pages.

Elastic's own ecommerce category-page documentation explicitly describes category pages as exploration over a pre-filtered subset, where facets are the primary product-finding tool rather than a search box.

The current hypothesis is:

> Marketplace/hyperscale systems are more heavily optimized around search/recommendation-driven discovery, while many independent/DTC hosted merchants have a much larger browse/category/collection/PLP request share and use site search as a supplementary path.

This is plausible but **must not be claimed as fact until production request logs validate it**.

If real logs establish the difference, it becomes a meaningful systems contribution because it explains why the target optimization objective differs from Havenask's likely center of gravity:

```text
free-text search -> drive native coverage across P50
browse/category/PLP -> drive native handling toward P95
```

This would make workload characterization a causal part of the architecture argument rather than market commentary.

---

## 4. Closest prior work and how to position against it

### Amazon category-intent shard selection

Known contribution:

```text
infer category intent
-> select fewer category shards
-> reduce search infrastructure cost
-> preserve relevance/customer experience
```

Our necessary distinction:

```text
infer/resolve enough commerce semantics
-> decide whether generic retrieval can be skipped entirely
-> execute a different physical regime
-> otherwise abstain with near-zero fallback tax
```

Required ablation:

```text
category/shard pruning + Solr
vs
commerce-native physical execution + Solr fallback
```

If the ordinary pruning approach captures the same cost/latency benefit, our claim must narrow.

### Amazon implicit query parsing / query attribute recommendation

Implicit semantics such as `iphone -> Apple` or analogous product-line-to-brand inference are prior art.

Our value is not the inference itself. It is whether validated inferred structure moves requests onto a cheaper physical execution path while respecting a correctness budget and preserving the multiplier.

### ROSE and cache-based product-search acceleration

ROSE optimizes **reuse across requests** via robust cache lookup.

Our primary experiments optimize **execution of each real request against current indexed state**.

Primary experimental boundary:

```text
result/query-response cache disabled on both systems
all legitimate index-resident optimizations enabled
```

Thus:

```text
ROSE/cache: avoid work by reusing an answer
our system: perform the current request's work more cheaply
```

The approaches are orthogonal and composable. ROSE belongs in related work, not the main experimental matrix.

### Havenask / IndexLib

Havenask already proves that specialized index types, realtime machinery, and vertical search engineering can work at extreme scale. Specialized indexing itself is not our novelty.

Our question is whether a different deployment objective creates a new architecture point:

```text
Havenask: hyperscale distributed marketplace/search engine
ours: low-fixed-cost hosted commerce execution substrate
      + correctness-aware offload
      + transparent mature fallback
      + browse-heavy optimization where workload data supports it
      + native multi-tenant isolation/economics
```

### Multi-tenant search / hosted search engines

Elastic, Algolia, Meilisearch and broader SaaS/search literature already support multi-tenancy through combinations of:

- index/collection-per-tenant;
- shared indices with tenant filters;
- filtered aliases/routing;
- tenant-scoped tokens/authorization;
- partitioning large tenants away from small tenants.

The project must therefore avoid claiming generic multi-tenant search as new.

The possible contribution is **semantic/physical tenancy economics**, if first-class commerce structures produce a materially different fixed-cost and interference frontier.

---

## 5. Research value: how the paper should articulate the contribution

The industrial value is straightforward: cheaper hosted commerce retrieval, better median latency, lower fallback load, and a path toward agentic/AEO serving.

The research value needs a different articulation.

### Research value #1: a new optimization objective

Traditional retrieval work commonly optimizes ranking quality, latency, shard selection, cascade depth, or cache hit rate.

This project studies a joint boundary:

```text
How much traffic can move to a different physical execution regime
before relevance safety or physical advantage fails?
```

That produces an explicit frontier rather than a single speedup number:

```text
coverage x correctness/relevance x multiplier x fallback tax
```

The frontier itself is a potentially reusable systems abstraction for other vertical domains where a subset of requests can be compiled into specialized execution.

### Research value #2: negative-result-driven architecture revision

The project began with a stronger whole-engine replacement hypothesis and falsified it. The negative result exposed the correct architecture:

```text
whole-engine replacement -> fails
specialized structural execution -> large win
hybrid/punt -> expensive
transparent fallback -> near-zero tax
safe semantic admission -> expands useful coverage
```

This causal chain is scientifically valuable because the final architecture is an empirical consequence, not a design preference presented after the fact.

### Research value #3: identifying semantic anchoring as an execution-safety variable

Current Phase 3 evidence shows:

- candidate-set size alone is insufficient;
- naive lexical narrowing fails relevance budgets;
- structurally anchored lexical narrowing is substantially safer and increases coverage.

If this pattern survives broader data, the research contribution is not simply 'parse structure', but:

> Structural semantic evidence can act as a correctness boundary for switching physical execution regimes.

This connects query semantics to systems execution in a measurable way.

### Research value #4: workload-dependent specialization

If production logs confirm that hosted independent-commerce traffic is materially more browse/PLP-heavy than marketplace-search benchmarks, the paper can show that **domain specialization is not one universal engine design**; it depends on the request distribution of the market being served.

That is a stronger claim than 'category pages are common'. It becomes:

```text
different commerce workload distribution
-> different target percentiles
-> different physical primitives
-> different isolation/cost architecture
```

### Research value #5: specialization under a small-tenant economic constraint

Most public commerce-search systems research comes from large marketplaces where the system serves one enormous logical commerce environment.

Hosted commerce introduces another systems axis:

```text
many merchants
heterogeneous catalog sizes
many mostly-idle tenants
traffic skew
merchant-specific rules/state
strict data isolation
small per-tenant revenue budget
```

If we can show that commerce-native first-class structures materially reduce the per-tenant fixed cost or noisy-neighbor penalty relative to generic search tenancy, this is a genuine systems contribution rather than only a product feature.

---

## 6. Strongest current paper articulation

A defensible current version is:

> Mature search engines are effective general-purpose fallbacks, but commerce requests are heterogeneous: some contain enough explicit or recoverable structure to admit a much simpler physical execution regime, while others require general retrieval. We study the systems boundary between these regimes. Instead of replacing the generic engine, we measure how far a correctness-aware commerce-native execution plane can expand across real traffic while preserving near-zero abstention cost, bounded relevance loss, and a large per-hit physical multiplier. We further ask whether the resulting specialization can support a lower-fixed-cost multi-tenant deployment envelope appropriate for hosted independent merchants, whose browse/search workload distribution may differ materially from hyperscale marketplace search.

This formulation intentionally avoids claiming novelty for query parsing, routing, caching, or specialized indexes individually.

---

## 7. What evidence would make this genuinely strong research

### Core search result

- safe native search coverage crosses ~50%;
- promoted fast paths preserve the >=80x multiplier floor;
- fallback tax remains statistically negligible;
- CI-certified relevance/correctness budget holds;
- category/shard-pruning + Solr ablation does not explain away the benefit.

### Browse result

- real production logs establish a meaningful browse/category/PLP request share;
- native handling approaches ~95% for structurally explicit browse traffic;
- the advantage survives strongest fair Solr filter/DocValues/facet/index-sort optimizations;
- breakpoint/saturation curves show where the physical layout matters.

### SMB/multi-tenant result

- realistic tenant-size and traffic-skew distribution;
- bytes and resident memory per tenant;
- idle-tenant fixed cost;
- noisy-neighbor curves;
- cold-tenant first request;
- tenant-local update/search coexistence;
- cost-per-tenant comparison against one or more competent generic-search tenancy patterns.

### External validity

- eCommerceSearchBench becomes a standard secondary workload;
- Havenask is used as the specialized-engine performance/architecture anchor;
- at least one sanitized real production workload is used for target-market characterization.

---

## 8. Current novelty verdict

### High confidence: not novel individually

```text
query parsing
implicit attributes
selective search/routing
category shard selection
vertical search
specialized indexes
caching
shared-index multi-tenancy
realtime updates
```

### Promising and currently not found in directly matching public work

```text
correctness-aware semantic admission
+ switch to materially different physical execution
+ transparent mature fallback
+ coverage/relevance/multiplier/fallback frontier
```

### Potentially strong if production data and experiments support it

```text
browse-heavy hosted-merchant workload as a different specialization target
+ search-P50 / browse-P95 optimization objective
```

### Potentially strongest SMB-specific systems novelty

```text
commerce semantics as a mechanism for lowering multi-tenant fixed cost
and shifting the isolation/noisy-neighbor frontier
```

No directly matching public paper was found in this review that studies that SMB/hosted-commerce question end to end.

This is not proof that none exists. Continue the novelty audit as experiments mature, especially before paper submission.