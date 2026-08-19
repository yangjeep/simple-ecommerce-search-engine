# CLAUDE.md

## Mission

This repository is an experimental **Rust-based commerce-native search engine**.

The architecture is a **semantic forwarding plane + learned control plane**:
- deterministic, low-latency serving path;
- commerce entities and product/variant semantics are first-class;
- structural retrieval is primary where semantics are known;
- lexical retrieval handles residual uncertainty;
- LLM/model reasoning belongs in an offline/slow control plane, not the default hot path;
- learned semantic knowledge must be validated, versioned, and compiled into the fast path.

The active execution plan is GitHub Issue #2.

## Primary objective

Prove or disprove the architecture with measurements. Do not optimize for feature count, API completeness, or resemblance to Elasticsearch.

The strongest outcome is a defensible `SCALE_UP_DECISION.md` backed by reproducible experiments, even if the decision is REVISE or STOP.

## Autonomy contract

You are expected to continue working without waiting for routine human confirmation.

For each experiment loop:
1. Read Issue #2, this file, `docs/EXPERIMENT_LOOP.md`, current experiment log, and relevant code/tests.
2. State a falsifiable hypothesis in the experiment log.
3. Define the measurement and pass/fail interpretation before implementation.
4. Add or modify a failing test/benchmark first where practical.
5. Implement the smallest experiment that can answer the question.
6. Run formatting, linting, unit tests, integration tests, regression tests, and relevant benchmarks.
7. Record raw results, environment, interpretation, limitations, and next action.
8. Commit a coherent checkpoint.
9. Continue to the next highest-value unanswered hypothesis.

Do not stop merely because one experiment succeeds. Continue until the scale-up stop condition in Issue #2 is reached or the next meaningful experiment requires materially larger infrastructure/data/product scope.

## Hard rules

- Rust is the implementation language for the active engine.
- Do not preserve old C architecture for compatibility. Preserve git history only.
- No LLM/model call in the default query hot path.
- No test may require a real model API key. Model-provider code must have deterministic fixtures/mocks.
- Product/variant correctness is non-negotiable. Cross-variant false matches are bugs.
- Prefer typed domain concepts over generic JSON/document abstractions.
- Preserve ambiguity explicitly when confidence is insufficient.
- Keep benchmark inputs deterministic and versioned.
- Never improve benchmark numbers by weakening correctness or silently changing the workload.
- Record failed experiments. Do not erase evidence because an approach was abandoned.
- Avoid distributed systems work until the single-node thesis has been measured.
- Avoid production polish, UI work, generic query DSLs, auth, tenancy, HA, and cluster coordination during this epic.

## Engineering quality gate

Before every checkpoint commit, run at minimum:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo build --workspace --release
```

Run all relevant benchmark/replay commands for the experiment as well.

If these commands do not yet exist because Gate 0 is incomplete, creating a clean equivalent is the first task.

## Architecture bias

Start narrow and explicit:
- Product
- Variant
- ProductType
- Brand
- Category
- Price
- Inventory / Availability
- typed attributes
- Commerce IR
- compiled semantic context / FIB
- specialized physical indexes

Likely physical primitives include compact IDs, bitmaps, typed columns/range structures, minimal postings, dense ranking feature arrays, and immutable/mmap-friendly bundles. These are hypotheses, not dogma: benchmark alternatives when the tradeoff matters.

## Decision discipline

Create/update ADRs when an architectural choice materially affects semantics, index representation, query planning, benchmark validity, or future scale.

Prefer evidence such as:
- correctness fixtures;
- query coverage;
- latency percentiles;
- memory/RSS;
- index size;
- CPU-normalized throughput;
- facet latency;
- build time;
- relevance metrics;
- replay regressions.

Do not claim an architectural win from microbenchmarks alone when end-to-end evidence is available.

## Working tree / commits

Keep commits small enough to explain but large enough to represent a complete experimental checkpoint. Use commit messages that describe the hypothesis/result, not just the files changed.

Do not rewrite published history. Do not force-push shared branches.

## End state

When the epic reaches a stop condition, produce `SCALE_UP_DECISION.md` containing:
- PROCEED / REVISE / STOP;
- architecture tested;
- datasets/workloads;
- measured results;
- failed experiments;
- unresolved risks;
- what would be built next if scaling up;
- what should explicitly not be built yet.
