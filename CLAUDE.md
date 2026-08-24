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

The active learned-control-plane experiment is **GitHub Issue #47**: adaptive semantic consensus and proposal-model capability/cost frontier.

The clean baseline for that experiment is recorded in the issue itself. Start new research branches from current `main`; do not continue historical stacked branches.

Issue #51 is a separate R1b serving-contract follow-up. Do not silently fold it into #47.

Other open issues are independent backlog unless the user explicitly asks to work them.

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
