# Architecture

This page describes what exists in `main` now. Experimental treatments that have not been promoted are called out explicitly.

## End-to-end shape

```mermaid
flowchart TB
    subgraph Ingestion[Offline ingestion / compilation]
        C[Catalog] --> P[Deterministic profiling]
        P --> X[Compressed semantic problems]
        X --> M[Model / heuristic proposals]
        M --> V[Deterministic validation / canonicalization]
        V --> B[Compiled context + physical indexes]
    end

    subgraph Query[Serving]
        Q[Raw query] --> IR[Commerce IR compiler]
        IR --> PL[Planner]
        PL --> SI[Structural index execution]
        PL --> LD[Lexical delegate]
        SI --> RK[Ranking / top-K]
        LD --> RK
        O[Mutable availability overlay] --> SI
    end

    B --> IR
    B --> PL
    B --> SI
```

The project enforces one architectural rule throughout: **semantic flexibility is resolved before or around compilation; query serving should stay deterministic and context-light.**

## 1. `commerce-core`

`crates/commerce-core` is the engine. Evaluation crates do not become dependencies of it.

Its major responsibilities are:

- `domain` — Product/Variant and typed attribute concepts;
- `ir` — query compilation into typed Commerce IR;
- `index` — bitmap/range/facet/identifier physical structures;
- `plan` — native/delegate composition;
- `admission` — conservative routing decisions;
- `control_plane` — offline proposal/replay/promotion primitives;
- `state` — mutable availability overlay.

## 2. Query compilation and ambiguity

The compiler resolves known commerce phrases into structural constraints and preserves residual lexical text. Ambiguous meaning is not supposed to become a hard filter merely because one interpretation exists.

Historical experiments found several real compiler-resolution defects; the current baseline includes the corrections that survived RED-test/adversarial-review cycles. One typed-ambiguity performance question remains isolated in Issue #51.

## 3. Physical indexes

The serving path uses specialized structures rather than a universal document schema.

### Bitmaps and typed structural constraints

Enum-like values and typed IDs use compact IDs / Roaring bitmaps. Variant-safe matching is handled by typed constraints rather than ad-hoc string filtering.

### Numeric/range

Numeric constraints use typed numeric structures rather than lexical token matching.

### Faceting

The code contains both scan-style and ordinal/dictionary counting families. Phase 6D is important because it changed the interpretation of earlier facet results: the old crossover was primarily a property of the naive scan algorithm, not a fundamental limit of commerce-native faceting. The ordinal method beat Solr across every tested WANDS scale-ladder checkpoint for the color case, while typed-ID facets still showed a small-candidate crossover because ordinal counting has a fixed dictionary-reset cost.

There is not yet a universal cost-based runtime chooser that selects every physical implementation optimally from measured cardinality.

### Identifier dictionary

Issue #42 promoted `IdentifierClassifier` / `IdentifierDictionary` after the dedicated primitive outperformed variant-level lexical indexing on the measured exact-lookup/adversarial workload. Classification is statistics-based, not field-name-based.

## 4. Native + lexical execution

`commerce_core::plan::LexicalDelegate` keeps mature lexical retrieval outside the structural engine.

The planner can execute:

- **FastPath** — native-only structural execution;
- **Hybrid** — structural narrowing plus lexical ranking;
- **Punt** — lexical backend first, followed by native verification where required.

Issue #42 added an optional compiled residual-token policy so residual words can be classified as required/preferred/contextual/unknown instead of always acting as a hard veto. Existing call sites can still pass `None` and preserve prior behavior.

The concrete lexical/search baselines live in evaluation crates. `commerce-core` deliberately does not depend on Solr, Elasticsearch or OpenSearch.

## 5. Dynamic merchant schema compilation

Issue #38 tested the hot-path cost of compiling a merchant-discovered schema into physical structures.

The naive generic tuple-key design was measurably slower because it allocated strings during lookup. A redesigned per-field compiled map removed that cost: the successful treatment matched the hand-coded path's allocation count and met the preregistered serving-overhead gate.

The architectural conclusion is narrow but important: **merchant schema variability does not require runtime-generic serving.** Ingestion can discover/compile a field into a concrete physical operator before queries arrive.

## 6. Learned semantic proposals

Issue #42's E2b provided the first actual model-assisted feature-discovery evidence. The model produced useful semantic descriptors but failed the raw repeated-stability gate and lacked real Product/Variant/relationship-rich external validation.

Issue #45 then tested deterministic canonicalization. It established two useful things:

1. raw model wording/primitive choices should not own the installed schema;
2. deterministic rules can absorb a large part of proposal instability without unsafe promotion.

The E2c canonicalizer is still an experimental/evaluation boundary, not a production service. The active Issue #47 tests adaptive consensus and model capability/cost before any productionization decision.

## 7. Mutable commerce state

`CommerceStateOverlay` keeps variant availability separate from immutable semantic/index state and composes through query execution. This avoids rebuilding semantic structures for every OOS event.

Known limitations are intentionally tracked separately:

- #11 — coarse `RwLock` concurrency;
- #12 — no restart durability/replay.

## 8. Multi-tenant findings vs. multi-tenant product code

Phase 7/8 evaluation crates measured packing/pooling, noisy-neighbor and correlated-burst behavior. Those results are evidence about the target operating model, **not proof that `commerce-core` contains a production tenant scheduler or isolation subsystem**.

The important measured boundary is that pooled in-process native querying behaved well in steady state, while index rebuilds and a shared lexical backend created real cross-tenant tail-latency interference; correlated bursts amplified those known gaps.

## 9. What is still experimental or absent

- E2c/E2d adaptive learned compilation is not installed as production control-plane behavior.
- No real Product/Variant/relationship-rich external dataset has yet closed the learned-schema external-validity gap.
- No generic cost-based planner covers every measured operator crossover.
- No production Solr/Elasticsearch/OpenSearch adapter lifecycle or service API exists.
- No durable mutable-state log/snapshot mechanism.
- No distributed serving / HA / sharding / replication.

That boundary is deliberate. The repository adds product machinery only when a falsifiable experiment shows it is needed.
