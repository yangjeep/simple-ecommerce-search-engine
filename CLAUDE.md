# CLAUDE.md

## Mission

This repository is an experimental Rust implementation of **commerce-native hybrid retrieval**.

The core research question is:

> **Can an ecommerce search engine be faster, more flexible, more accurate, and more stable at the same time?**

Interpret those goals concretely:

- **faster** — materially lower CPU/latency on meaningful ecommerce workload classes;
- **more flexible** — unseen merchant schemas/verticals should not require bespoke serving code;
- **more accurate** — specialization must preserve or improve correctness/relevance;
- **more stable** — deterministic installed semantics and predictable serving despite messy catalogs and stochastic model proposals.

The current architectural hypothesis is intentionally narrow:

- use deterministic, typed commerce structures where they measurably reduce work;
- delegate open-ended lexical retrieval/ranking to a mature backend rather than rebuilding a general search engine;
- learn merchant-specific semantics offline, over compressed catalog problems rather than per-SKU calls;
- treat model output as a proposal, never production truth;
- validate/canonicalize/compile accepted semantics before serving;
- keep normal query serving model-free, deterministic and cheap;
- preserve Product/Variant correctness and explicit ambiguity.

Read [`README.md`](README.md) and [`docs/README.md`](docs/README.md) before starting broad work.

## Current research

The active experiment is **GitHub Issue #55**, an architecture-falsification loop running on PR #56: close out semantic-promotion-lifecycle/comparator-fairness/documentation hygiene (A1-A4), then recover public datasets previously degraded by network restrictions (B). Issue #55 is an orchestrator over prior work, not a replacement for it.

**GitHub Issue #57** is the stage gate immediately after #55/#56 close: one frozen, full-matrix end-to-end benchmark (native vs. Solr vs. Elasticsearch vs. Havenask) across every recovered dataset. Do not begin #57's measured matrix before #55/#56 are closed.

Issue #47 (adaptive semantic consensus / proposal-model capability-cost frontier) is **closed**: both Phase A (adaptive consensus controller) and Phase B (capability/cost frontier) concluded REVISE — the controller missed its own efficiency bar, and a cheap-model cascade consumed *more* tokens than the strong-model baseline it was meant to reduce, not fewer. No architecture GO resulted. Do not describe #47 as active or reopen it without a new, explicitly scoped follow-up issue.

Issue #35 (generalize the specialization methodology across unseen verticals) reached its own stated "at least three materially different verticals" bar (real ESCI electronics/automotive/beauty slices, `docs/decisions/README.md`'s `ISSUE35_*_DECISION.md` rows); its evidence feeds Issue #55/#57 rather than remaining a separate open thread.

Issue #51 is a separate R1b serving-contract follow-up. Do not silently fold it into #47 or #55.

Start new research branches from current `main`; do not continue historical stacked branches. Other open issues are independent backlog unless the user explicitly asks to work them.

## Research discipline

For nontrivial experiments:

1. Read the issue, relevant decision record under `docs/decisions/`, protocol/log under `docs/experiments/`, and affected code.
2. State a falsifiable hypothesis and competing explanations.
3. Preregister treatments, metrics, thresholds, splits and stop conditions before held-out results where the issue requires it.
4. Keep generators/proposal models separate from evaluators/oracles.
5. Add RED correctness/regression tests before production fixes where practical.
6. Implement the smallest experiment that can answer the question.
7. Run targeted tests, then the full quality gate.
8. Preserve raw outputs, failures, model/provider/version/settings, seeds, manifests and superseded numbers.
9. Run a fresh adversarial review that tries to falsify favorable conclusions.
10. Record GO / REVISE / STOP without changing thresholds after seeing the answer.

Negative results are first-class outputs. Do not turn a failed gate into a feature roadmap.

See [`docs/EXPERIMENT_LOOP.md`](docs/EXPERIMENT_LOOP.md) for the durable experiment-loop guidance.

## Hard rules

- Rust is the implementation language for the engine.
- No LLM/model call in the normal query hot path.
- No test or CI path may require a live model API key; live outputs used by tests must be frozen artifacts.
- Product/Variant correctness is non-negotiable. Cross-variant false matches are bugs.
- Prefer typed commerce concepts over generic JSON/document abstractions.
- Preserve ambiguity/abstention when evidence is insufficient.
- LLMs may propose semantic meaning; deterministic code owns safety, installed semantics and physical representation.
- Never improve benchmark numbers by weakening correctness, changing the workload silently, or dropping failed cases.
- Preserve corrected and superseded evidence rather than rewriting history.
- Do not rebuild generic lexical ranking unless an experiment demonstrates a differentiated need.
- Avoid distributed systems, production UI/auth and compatibility-DSL work until measured evidence requires them.

## Engineering quality gate

Before a checkpoint/PR is considered complete:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo build --workspace --release
```

Run the relevant benchmark/replay/evaluation commands as well.

## Repository boundaries

- `crates/commerce-core/` — engine/product code.
- `*-eval` crates — experiment/evaluation code, not product dependencies.
- `docs/architecture/` — current implementation description.
- `docs/decisions/` — phase/issue verdicts.
- `docs/experiments/` — protocols and append-only logs.
- `docs/research/` — exploratory analysis/prior art/paper/economic work.
- `docs/adr/` — durable architecture decisions.
- `benchmarks/` / `artifacts/` — reproducibility metadata and archived evidence.

Do not create a new documentation category for a single issue.

## Architecture bias

The serving plane should remain small and concrete: typed Commerce IR, compact IDs, bitmap/range/identifier structures, measured facet implementations, deterministic planning, mature lexical delegation and top-K/ranking composition.

Merchant/category diversity should be absorbed by ingestion-time profiling, semantic problem compression, validated descriptors and physical compilation — not by a universal runtime schema or vertical-specific serving branches.

Treat this as a hypothesis to falsify, not a requirement to preserve.

## Decision discipline

Create/update ADRs when a choice materially changes serving semantics, physical representation, planner contracts or durable architecture.

Research verdicts belong in `docs/decisions/`; detailed measurement/correction history belongs in `docs/experiments/`.

Do not claim a system-level win from a microbenchmark when end-to-end evidence is available.
