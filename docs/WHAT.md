# What the repository contains

This is a **research engine plus evaluation harnesses**, not a deployable search service.

The easiest way to understand the boundary is to separate `commerce-core` from everything used to test it.

## Core system

### Typed commerce domain and IR

`crates/commerce-core` models products, variants, product types, categories, identifiers, prices and typed attributes directly. Queries compile into Commerce IR with resolved structural constraints, lexical residuals and explicit ambiguity.

### Structural indexes

`CatalogIndex` contains the specialized physical structures the research is about:

- Roaring bitmaps for structural equality/set membership;
- numeric/range structures;
- ordinal/dictionary facet structures;
- identifier dictionary/classifier;
- minimal lexical postings used by specific experiments/paths.

The code does not assume every operator should use one representation. Phase 6D's facet results are a concrete example: ordinal counting is excellent for some shapes but has its own fixed-cost crossover for already-cheap typed-ID facets.

### Planning and backend delegation

`commerce_core::plan` composes native retrieval with a `LexicalDelegate` and produces FastPath / Hybrid / Punt-style outcomes. Residual lexical tokens can be governed by the compiled policy added in Issue #42 rather than acting as an unconditional hard veto.

General relevance ranking remains delegated. The project is intentionally not rebuilding a full lexical scoring stack.

### Identifier lookup

Issue #42 added a dedicated identifier classifier/dictionary. The classifier uses measured field behavior rather than trusting names such as `sku` or `part_number`.

### Mutable availability state

`CommerceStateOverlay` provides a real variant-availability/OOS mutation path separate from immutable catalog semantics. It is intentionally incomplete: durability/replay and finer-grained concurrency are still tracked separately (#12 and #11).

### Offline semantic learning / compilation

Model-assisted work is outside the query hot path. Existing control-plane and Issue #42/#45 evaluation code follows the same direction:

1. profile/compress catalog evidence;
2. propose semantic descriptors;
3. canonicalize and validate deterministically;
4. compile only accepted semantics into serving structures;
5. preserve abstention and provenance.

Issue #45's canonicalizer remains experimental evidence, not a production online-learning service.

## Evaluation infrastructure

Everything named `*-eval` is research infrastructure, not production dependency surface. These crates own dataset adapters, benchmark binaries, relevance judgments, Solr/Tantivy/Elasticsearch/OpenSearch comparison harnesses, synthetic catalogs and adversarial fixtures.

`bench-harness`, `benchmarks/`, `artifacts/` and `scripts/` exist to keep the experiments reproducible and to preserve corrected/superseded results.

## What is deliberately not here

- A public HTTP search API.
- Authentication or tenant authorization.
- A generic JSON document model or Elasticsearch-compatible DSL.
- Query-time model inference.
- A home-grown replacement for mature lexical ranking.
- Distributed consensus, sharding, replication, HA or multi-region serving.
- Production-grade catalog/state durability.
- A universal hand-maintained ontology for every commerce vertical.

## Current research boundary

The current mainline question is no longer “can an LLM infer a merchant schema?” The experiments already show useful semantic signal and also show why raw model output cannot be installed directly.

The active question (#47) is whether **adaptive consensus + deterministic compilation lets cheaper models handle most semantic problems while escalating only the hard ones**.

A separate R1b follow-up (#51) asks whether typed ambiguity can be corroborated using precomputed ingestion-time state rather than an expensive query-time catalog scan.

For current architecture details, see [`architecture/README.md`](architecture/README.md). For the chronological research verdicts, see [`decisions/`](decisions/).
