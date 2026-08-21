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
| Phase 7 (Issue #21 Phase 7) | Does commerce specialization reduce per-tenant fixed cost and increase safe tenant packing density while preserving isolation — first pass, single-process and cross-process, real WANDS category partitions as tenants? | **PROCEED** — [`PHASE7_DECISION.md`](PHASE7_DECISION.md) |

**The active epic is [Issue #21](https://github.com/yangjeep/simple-ecommerce-search-engine/issues/21)** (Phases 6–9): cross-dataset/cross-engine validation, multi-tenant SMB/mid-market economics, correlated-burst (BFCM) elasticity, and an integrated, falsifiable system. See `docs/WHY.md` for why the project reframed around this after Phases 2–5.

## Core questions

The project is intended to measure, not assume. The original Gate-era questions (below) are largely answered — see `docs/WHY.md` for the falsified/narrowed results — and Issue #21 now asks the multi-tenant/multi-dataset/burst-elasticity versions of the same questions:

- What fraction of realistic ecommerce queries can be resolved structurally without a model call in the hot path? (Phases 2–4: a real but small slice — see `docs/WHY.md`.)
- Does that advantage hold for browse/PLP-style structural traffic, and where does it break down by cardinality? (Phase 5: yes for filter/pagination/concurrency, no for facet/large-sort past a measured breakpoint.)
- Does the result generalize across independent datasets/verticals and against a second specialized engine (Havenask)? (Phase 6A: filter/pagination/concurrency robustly reproduce on WANDS; facet's crossover threshold shifts but is mechanistically explained. Phase 6B: that explanation substantially holds under a controlled scale ladder, with one narrower, cause-unconfirmed exception; Havenask and every other named Phase 6 dataset remain blocked from this environment.)
- Does commerce specialization reduce per-tenant fixed cost and increase safe tenant packing density? (Phase 7 first pass: yes for in-process memory, confirmed cleanly from 55 real tenants up to 6,500 controlled-stress-replicated tenants — per-tenant fixed cost is negligible, tracked to aggregate product count, not tenant count, after two self-caught/adversarially-corrected first-draft overclaims; pairwise isolation and throughput-under-breadth both hold robustly; a real, measured per-process baseline confirms pooling has a genuine cost advantage over one-process-per-tenant isolation — the first real evidence for this project's own statistical-multiplexing thesis; a follow-on long-running-resident-process measurement found that advantage is even larger than the short-lived-process floor alone suggested, reproduced exactly across repeated runs; a further follow-on extending that resident window 9x confirmed the growth decelerates toward a plateau rather than climbing without bound, so the earlier figure is a stable input, not a measurement-window artifact; a first economic cost-per-tenant model combines all of this into an explicit pooled-vs-isolated deployment cost formula; a cold-tenant-overhead measurement (an Issue #21-named metric untested until now) found a real, reproducible ~9-13x latency-ratio effect between an infrequently- and continuously-queried same-sized tenant, but at an absolute scale (tens of microseconds) almost certainly negligible next to real-world request latency; a replication check embedding the same tenants in a realistic full-population Zipfian demand mix (also testing Issue #21's aggregate-QPS/fairness/hot-tenant-saturation metrics) confirmed the effect's direction in every run but found its magnitude was ~4-6x smaller under a realistic shared/interleaved design than the original idealized one; a follow-on extended the fixed-tenant throughput-under-breadth finding from WANDS' real 54-other-tenant ceiling to 2,000 controlled-stress-replicated tenants (36x larger), confirmed cleanly across 3 runs with only a small, honestly-disclosed dip near the top of the range possibly tied to RSS approaching the safety cap; a final follow-on combined the memory model with latency evidence to directly answer "tenants per fixed hardware envelope at target SLO" for the first time, discovering this container's real memory limit directly (after a first-draft OOM at the memory-only ceiling) and safely reaching ~3,500 query-capable tenants under a disclosed envelope with throughput/latency essentially unaffected there — materially lower than, but a genuinely query-capable, number versus the earlier memory-only ceiling; combining Phase 3/4's promoted admission-rate evidence with Phase 7's tenant model now also answers "backend requests avoided" — ~58,000-62,000 backend requests avoided per million real queries per tenant — closing the last of Issue #21's 7 named economic-output metrics; a final measurement answered "CPU/query and CPU/tenant" for the first time (every prior experiment tracked wall-clock only), finding CPU cost per query does NOT scale linearly like memory does — sub-linear at tiny tenant sizes, then measurably super-linear for the largest real tenant (Furniture's actual cost is 3.81x higher than a linear extrapolation from smaller tenants predicts), reproduced across 3 runs; a final measurement tested mutation instead of query load for the first time — unlike pure query load (confirmed safe), a co-located tenant undergoing repeated catalog-index rebuilds measurably degrades a quiet tenant's own tail latency (p99 4.00-6.70x, reproduced across 3 runs, even though typical-case latency barely moves) — a genuine, actionable isolation gap this pass names honestly, with a mitigation explicitly left as necessary future work rather than assumed away.)
- What happens to this architecture when a correlated retail event (BFCM) breaks normal statistical multiplexing? (Phase 8, not yet run.)
- Is the smallest coherent integrated system (native + Solr, native + Havenask) actually better than operating either backend directly for the target market? (Phase 9, not yet run.)

## Scope

See `docs/WHAT.md` for the current evidence-backed system boundary and explicit non-goals. In short: single-node, single-process, no multi-tenancy/auth/HA/distributed coordination yet, no production polish, no LLM call in the query hot path, no generic query DSL — all deliberate, and re-evaluated only when a phase's measurements justify changing them.

## Autonomous experiment workflow

Long-running coding sessions must follow [`CLAUDE.md`](CLAUDE.md) and [`docs/EXPERIMENT_LOOP.md`](docs/EXPERIMENT_LOOP.md).

The project advances through measured experiment gates rather than a feature roadmap. Failed and narrowed hypotheses are expected, permanent artifacts — see `docs/experiments/` for the full per-experiment logs behind every decision document above, and `docs/adr/` for architectural decisions.

## Reproducing results

Every headline number in a `PHASE*_DECISION.md` traces to raw artifacts under `docs/research/artifacts/`, generated by the experiment binaries in `crates/phase*-eval/src/bin/`. See `docs/architecture/README.md` for the crate map, and the relevant phase's experiment log (`docs/experiments/PHASE*_LOG.md`) for the exact command used to produce each result.
