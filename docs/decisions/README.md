# Research decisions

These files are the compact verdicts for major research checkpoints. Read these when you want the conclusion; read `../experiments/` when you want the full protocol, raw-history narrative, methodology corrections and reruns.

## Chronology

| Checkpoint | Decision / significance |
|---|---|
| [`SCALE_UP_DECISION.md`](SCALE_UP_DECISION.md) | Initial typed-domain / Commerce IR / physical-index prototype: proceed to real-data falsification. |
| [`ROUND1_DECISION_TREE.md`](ROUND1_DECISION_TREE.md) | Real ESCI + Solr evidence: **narrow the product** rather than rebuild a general search engine. |
| [`PHASE2_DECISION.md`](PHASE2_DECISION.md) | Whole-engine replacement thesis: **STOP**. Mature lexical ranking remains delegated. |
| [`PHASE3_DECISION.md`](PHASE3_DECISION.md) | Safe admission frontier: narrow support for selective structural traffic. |
| [`PHASE4_DECISION.md`](PHASE4_DECISION.md) | Offline learned implication rules: useful only with strict validation/promotion gates. |
| [`PHASE5_DECISION.md`](PHASE5_DECISION.md) | Browse/PLP operators expose real cardinality-dependent win/loss regions. |
| [`PHASE6A_DECISION.md`](PHASE6A_DECISION.md) | WANDS cross-dataset validation. |
| [`PHASE6B_DECISION.md`](PHASE6B_DECISION.md) | Controlled WANDS scale ladder; candidate size and attribute complexity characterized. |
| [`PHASE6C_DECISION.md`](PHASE6C_DECISION.md) | Direct Lucene baseline corrected the naive facet-algorithm interpretation. |
| [`PHASE6D_DECISION.md`](PHASE6D_DECISION.md) | Ordinal faceting closes the earlier color-facet crossover; also finds its own small-candidate typed-ID limit. |
| [`PHASE6E_DECISION.md`](PHASE6E_DECISION.md) | Embedded Elasticsearch/OpenSearch baselines become runnable; Havenask remains blocked in the measured environment. |
| [`PHASE7_DECISION.md`](PHASE7_DECISION.md) | Multi-tenant pooling economics: promising, with concrete rebuild/backend isolation gaps. |
| [`PHASE8_DECISION.md`](PHASE8_DECISION.md) | Correlated burst tests: pure query bursts hold; known isolation gaps become more reliable/worse under burst. |
| [`PHASE8_FEASIBILITY.md`](PHASE8_FEASIBILITY.md) | What Phase 8 could and could not validly test in the available environment. |
| [`PHASE9_DECISION.md`](PHASE9_DECISION.md) | Integrated Phase 9 evidence and corrected query-resolution baseline. |
| [`ISSUE38_DECISION.md`](ISSUE38_DECISION.md) | Dynamic compiled merchant schema: hot-path overhead can be removed; unseen/mixed synthetic generalization succeeds within its stated scope. |
| [`ISSUE42_DECISION.md`](ISSUE42_DECISION.md) | R2 residual policy and R3 identifier primitive GO; R1 and model-assisted E2b remain REVISE. |
| [`ISSUE45_DECISION.md`](ISSUE45_DECISION.md) | Deterministic semantic canonicalization substantially reduces LLM instability but remains **REVISE** under the stricter single-proposal reading. |
| [`ISSUE43_DECISION.md`](ISSUE43_DECISION.md) | Phase 9 reproducibility re-audit: published numbers **CONFIRMED** byte-identical against the Tantivy determinism fix; unrelated Solr-JVM-warmup confound found and disclosed as a new open thread. |
| [`ISSUE55_H3_DECISION.md`](ISSUE55_H3_DECISION.md) | Variant-scoped conjunction correctness **CONFIRMED** on real Product/Variant data (Magento configurable products) for the first time, closing the external-validity gap `ISSUE47_DECISION.md` named; FastPath only, disclosed scope boundary. |
| [`ISSUE51_DECISION.md`](ISSUE51_DECISION.md) | Precomputed corroboration registry (Treatment E): correctness-preserving, but **REVISE** at R1's own 5-product fixture; a disclosed scaling diagnostic shows a 492x asymptotic advantage over the query-time scan at realistic catalog sizes, naming a concrete next step. |
| [`ISSUE55_RANK_SCALING_DECISION.md`](ISSUE55_RANK_SCALING_DECISION.md) | `execute_ranked`'s full sort replaced with a proven-equivalent partial top-K selection (adopted); **REFINE** — real but modest gain, not the dominant driver of P9-E06's native-vs-Solr gap on real WANDS data, which localizes instead to Solr JVM variance and a newly found `score_text_relevance` tokenization cost. |
| [`ISSUE55_TEXT_TOKEN_CACHE_DECISION.md`](ISSUE55_TEXT_TOKEN_CACHE_DECISION.md) | `score_text_relevance`'s per-query tokenization precomputed at index-build time (adopted, 43-59% synthetic cost reduction); **KEEP, and reverses Phase 9's H3 verdict** — on the same real WANDS data that originally found native slower (0.42x-0.60x), native is now consistently 4.6x-8.2x **faster** than Solr-restricted. `PHASE9_DECISION.md` corrected via dated addendum. |

## How to read old numbers

Decision records are intentionally not rewritten to make the research look linear. When an experiment later discovered a methodology defect, the repository preserves the old result and records the correction. A later decision may therefore supersede an earlier interpretation without deleting it.

For current project status, always prefer the root [`README.md`](../../README.md) and [`../architecture/README.md`](../architecture/README.md) over an old phase narrative.
