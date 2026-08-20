# Commerce-Native Search Engine

Experimental Rust search engine exploring a **semantic forwarding plane + learned control plane** for ecommerce retrieval.

This repository intentionally starts from a narrower world model than a generic document search engine. Products, variants, product types, brands, categories, prices, inventory, availability, and typed commerce attributes are first-class concepts. Known commerce semantics should compile into deterministic structural retrieval; lexical search handles residual uncertainty; model-assisted reasoning belongs primarily in an offline control plane that learns and validates semantic routes before compiling them into the fast path.

## Status

Research prototype / architecture experiment. The previous C/GTrie implementation remains in git history but is not the target architecture.

The active experiment is tracked in **GitHub Issue #2**. Gates 0-7 have initial evidence recorded in [`docs/experiments/LOG.md`](docs/experiments/LOG.md) (E000-E007) and [`docs/adr/`](docs/adr/); the resulting decision is [`SCALE_UP_DECISION.md`](SCALE_UP_DECISION.md).

## Core questions

The project is intended to measure, not assume:

- What fraction of realistic ecommerce queries can be resolved structurally without general model inference?
- What should the typed Commerce IR contain?
- Which commerce concepts must be first-class versus extensible attributes?
- Can product/variant-aware structural retrieval be both more correct and cheaper than generic document matching?
- What semantic FIB representation gives a useful memory/latency tradeoff?
- Can catalog profiling + semantic fuzzing produce a useful cold-start context without one LLM call per SKU?
- Can unresolved queries safely teach a versioned fast path through replay and promotion gates?
- At what scale does a single-node immutable/mmap-oriented serving model stop being the right default?

## Initial scope

The prototype should remain deliberately narrow:

- Rust
- single node
- read-heavy serving
- Product / Variant aware
- typed attributes
- canonical IDs and aliases
- bitmap-based structural filtering
- numeric/range filtering
- minimal lexical postings
- facets
- top-K ranking
- versioned compiled semantic context
- deterministic test and benchmark fixtures

Distributed coordination, generic document DSL compatibility, production multi-tenancy, HA, and elaborate UI are non-goals until measurements justify them.

## Autonomous experiment workflow

Long-running coding sessions must follow [`CLAUDE.md`](CLAUDE.md) and [`docs/EXPERIMENT_LOOP.md`](docs/EXPERIMENT_LOOP.md).

The project advances through measured experiment gates rather than a feature roadmap. Failed hypotheses are expected artifacts and must remain in the experiment log.

## Target decision

The current epic ends with `SCALE_UP_DECISION.md` containing one of:

- **PROCEED** — evidence supports scaling the architecture and workload;
- **REVISE** — core idea remains useful but a measured assumption needs redesign;
- **STOP** — the commerce-native specialization does not provide enough advantage to justify further scale-up.

Success is a defensible decision backed by reproducible data, not a large codebase.

**Current decision: PROCEED** to the next round of experiments (an external baseline, a larger/real catalog, larger scale tiers) — not to production. See [`SCALE_UP_DECISION.md`](SCALE_UP_DECISION.md) for the full evidence, unresolved risks, and what should explicitly not be built yet.
