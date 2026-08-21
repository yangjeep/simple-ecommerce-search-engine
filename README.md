# Commerce-Native Search Engine

Experimental Rust search engine exploring a **semantic forwarding plane + learned control plane** for multi-tenant ecommerce retrieval.

This repository intentionally starts from a narrower world model than a generic document search engine. Products, variants, product types, brands, categories, prices, inventory, availability, and typed commerce attributes are first-class concepts. Known commerce semantics compile into deterministic structural retrieval; a mature lexical engine (Solr today; Havenask as a planned second anchor) handles residual/open-ended relevance; model-assisted reasoning belongs in an offline control plane that proposes and validates semantic routes before compiling them into the fast path — never in the query hot path.

Start here, in order:

- **[`docs/WHY.md`](docs/WHY.md)** — the real problem this project exists to test, and which hypotheses have been falsified or narrowed so far.
- **[`docs/WHAT.md`](docs/WHAT.md)** — the evidence-backed product/system boundary, and explicit non-goals.
- **[`docs/architecture/README.md`](docs/architecture/README.md)** — how the system actually works today (as opposed to what a future phase targets).

## Status

Research prototype / architecture experiment, not a deployable service. The previous C/GTrie implementation remains in git history but is not the target architecture.

The project has moved through several falsification rounds, each ending in a written decision document:

| Round | Question | Decision |
|---|---|---|
| Gates 0–7 (Issue #2) | Bootstrap: typed domain, Commerce IR, physical indexes, control plane, first benchmark | PROCEED — [`SCALE_UP_DECISION.md`](SCALE_UP_DECISION.md) |
| Round 1 (Issue #5) | Real catalog (1.2M products) + external Solr baseline + adversarial workloads | [`ROUND1_DECISION_TREE.md`](ROUND1_DECISION_TREE.md) |
| Phase 2 (Issue #6) | Whole-engine 5–10x QPS/$ replacement thesis | **STOP** — [`PHASE2_DECISION.md`](PHASE2_DECISION.md) |
| Phase 3 (Issue #14) | Safe fast-path admission frontier over Solr | **NARROW SUPPORT** — [`PHASE3_DECISION.md`](PHASE3_DECISION.md) |
| Phase 4 (Issue #16) | Learned semantic implication rules | **NARROW SUPPORT** — [`PHASE4_DECISION.md`](PHASE4_DECISION.md) |
| Phase 5 (Issue #17) | Browse/PLP as a commerce-native workload vs. a fair Solr baseline | **REVISE / NARROW BUT PUBLISHABLE** — [`PHASE5_DECISION.md`](PHASE5_DECISION.md) |
| Phase 6A (Issue #23) | Do Phase 5's PLP breakpoints reproduce on an independent, genuinely hierarchical dataset (WANDS, substituted for the unreachable Amazon Reviews 2023)? | **PROCEED** — [`PHASE6A_DECISION.md`](PHASE6A_DECISION.md) |
| Phase 6B (Issue #21 Phase 6) | Is Phase 6A's facet-crossover shift explained by attribute complexity alone, or does candidate-set size independently matter — via a controlled-stress WANDS scale ladder (Retailrocket/H&M/Amazon Reviews 2023/Havenask all confirmed blocked)? | **PROCEED** — [`PHASE6B_DECISION.md`](PHASE6B_DECISION.md) |
| Phase 6C (Issue #21 Phase 6, retroactive audit after Phase 8) | Was cross-engine validation actually completed, given Solr was the only baseline through Phase 8 — live re-check of Havenask/Elasticsearch/OpenSearch/Retailrocket/H&M/Amazon Reviews 2023, plus a new raw-Apache-Lucene-direct baseline where Maven Central proved reachable? | **PROCEED** — [`PHASE6C_DECISION.md`](PHASE6C_DECISION.md) |
| Phase 7 (Issue #21 Phase 7) | Does commerce specialization reduce per-tenant fixed cost and increase safe tenant packing density while preserving isolation — terminal decision, 15 hypotheses (H1-H15), single-process, cross-process, and lexical-backend, real WANDS category partitions as tenants? | **PROCEED** — [`PHASE7_DECISION.md`](PHASE7_DECISION.md) |
| Phase 8 (Issue #21 Phase 8) | Does Phase 7's steady-state multi-tenant isolation hold under a correlated retail-burst regime (BFCM elasticity) — first pass, partially supported in this environment (see [`PHASE8_FEASIBILITY.md`](PHASE8_FEASIBILITY.md))? | **PROCEED (first pass, two burst-amplified gaps + one cross-subsystem interaction confirmed)** — [`PHASE8_DECISION.md`](PHASE8_DECISION.md) |

**The active epic is [Issue #21](https://github.com/yangjeep/simple-ecommerce-search-engine/issues/21)** (Phases 6–9): cross-dataset/cross-engine validation, multi-tenant SMB/mid-market economics, correlated-burst (BFCM) elasticity, and an integrated, falsifiable system. See `docs/WHY.md` for why the project reframed around this after Phases 2–5.

## Core questions

The project is intended to measure, not assume. The original Gate-era questions (below) are largely answered — see `docs/WHY.md` for the falsified/narrowed results — and Issue #21 now asks the multi-tenant/multi-dataset/burst-elasticity versions of the same questions:

- What fraction of realistic ecommerce queries can be resolved structurally without a model call in the hot path? (Phases 2–4: a real but small slice — see `docs/WHY.md`.)
- Does that advantage hold for browse/PLP-style structural traffic, and where does it break down by cardinality? (Phase 5: yes for filter/pagination/concurrency, no for facet/large-sort past a measured breakpoint.)
- Does the result generalize across independent datasets/verticals and against a second specialized engine (Havenask)? (Phase 6A: filter/pagination/concurrency robustly reproduce on WANDS; facet's crossover threshold shifts but is mechanistically explained. Phase 6B: that explanation substantially holds under a controlled scale ladder, with one narrower, cause-unconfirmed exception; Havenask and every other named Phase 6 dataset remain blocked from this environment. Phase 6C, a retroactive audit after Phase 8 found the "revisit before Phase 7" instruction Phase 6B itself gave had been skipped: live re-checked, Havenask/Elasticsearch/OpenSearch/Retailrocket/H&M/Amazon Reviews 2023 are all still genuinely blocked — but Maven Central is reachable, so raw Apache Lucene (the shared core under Solr/ES/OpenSearch) was benchmarked directly for the first time. Result: Solr's own facet implementation frequently **beats** a correct, direct Lucene scan (slower in 5 of 7 real checkpoints, up to 3.3x-4.0x) — falsifying the idea that Solr's wrapper overhead was masking the native-vs-generic-engine gap, and sharpening the facet-crossover finding into a claim about facet algorithms, not serving-layer cost. See [`PHASE6C_DECISION.md`](PHASE6C_DECISION.md).)
- Does commerce specialization reduce per-tenant fixed cost and increase safe tenant packing density? (Phase 7 terminal decision, 15 hypotheses: **yes for the core thesis** — per-tenant memory overhead is negligible and tracks aggregate product count cleanly to 6,500 tenants; the native in-process query path shows no material cross-tenant degradation to 2,000 tenants; pooling has a real, measured cost advantage over process-per-tenant isolation (this project's own "statistical multiplexing" thesis, confirmed rather than assumed); a full economic cost model now answers all 7 of Issue #21's named "Economic output" metrics, including a concrete "~3,500 query-capable tenants per disclosed 9GB envelope" figure. **But two real, unmitigated isolation gaps were found and are not smoothed over**: a co-located tenant's index REBUILD (this architecture's only mutation path) degrades another tenant's own p99 latency 4.00-6.70x, and sharing one Solr instance across tenants degrades a quiet tenant's own p99 latency 2.16-2.48x under ordinary query load — both reproduced across 3 runs, neither mitigated in this pass. See [`PHASE7_DECISION.md`](PHASE7_DECISION.md) for the full 15-hypothesis quick-scan table and every named limitation.)
- Does Phase 7's steady-state multi-tenant isolation hold under a correlated retail-burst regime? (Phase 8 first pass: **mixed — yes for pure query-load bursts, no for burst combined with either of Phase 7's known real isolation gaps.** H16: a fixed group of 10 (of 55) real tenants had their traffic weight multiplied 10x mid-experiment, simulating a correlated sale/promotion event; a separate, unrelated "bystander" tenant's own p50/p99 stayed essentially flat (0.95x-1.03x) across 3 independent runs even as the burst group's own throughput grew ~10x and aggregate throughput rose ~40%. H17: but a correlated burst materially worsens Phase 7's own H14 rebuild-churn isolation gap — not just in magnitude (median 3.62x amplification across 10 runs) but in kind: an idle system's churn-driven tail-latency hit is an intermittent coincidence (~30% of measurement windows), while the same churn under burst produces a material (>=2x) degradation in effectively every measurement window (10/10 runs). H18: the same pattern recurs for Phase 7's other known gap — a correlated burst (more tenants' traffic joining a shared Solr instance) materially worsens H15's shared-Solr-contention gap too (median 1.80x amplification, tightly reproduced across all 10 runs; >=2x-degradation hit rate rises from 5/10 to 10/10 runs). H19: running H14's and H15's mechanisms SIMULTANEOUSLY (not just each under its own burst) compounds them asymmetrically — combined native-churn + shared-Solr load degrades the native tenant's own latency by 2.11x-3.37x in every one of 20 measured runs (more reliably than either mechanism alone), but does not measurably worsen the Solr-side contention beyond what Solr noise alone already causes. This is honestly scoped: only single-node infrastructure was used, since real multi-node scale-out and admission/backpressure control remain genuinely out of reach in this environment — see [`PHASE8_FEASIBILITY.md`](PHASE8_FEASIBILITY.md) for the full item-by-item feasibility assessment and [`PHASE8_DECISION.md`](PHASE8_DECISION.md) for all four results.)
- Is the smallest coherent integrated system (native + Solr, native + Havenask) actually better than operating either backend directly for the target market? (Phase 9, not yet run.)

## Scope

See `docs/WHAT.md` for the current evidence-backed system boundary and explicit non-goals. In short: single-node, single-process, no multi-tenancy/auth/HA/distributed coordination yet, no production polish, no LLM call in the query hot path, no generic query DSL — all deliberate, and re-evaluated only when a phase's measurements justify changing them.

## Autonomous experiment workflow

Long-running coding sessions must follow [`CLAUDE.md`](CLAUDE.md) and [`docs/EXPERIMENT_LOOP.md`](docs/EXPERIMENT_LOOP.md).

The project advances through measured experiment gates rather than a feature roadmap. Failed and narrowed hypotheses are expected, permanent artifacts — see `docs/experiments/` for the full per-experiment logs behind every decision document above, and `docs/adr/` for architectural decisions.

## Reproducing results

Every headline number in a `PHASE*_DECISION.md` traces to raw artifacts under `docs/research/artifacts/`, generated by the experiment binaries in `crates/phase*-eval/src/bin/`. See `docs/architecture/README.md` for the crate map, and the relevant phase's experiment log (`docs/experiments/PHASE*_LOG.md`) for the exact command used to produce each result.
