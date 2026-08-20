# ADR 0001: Rust workspace baseline replaces the C/GTrie implementation

## Status

Accepted (Gate 0, Issue #2).

## Context

The repository previously contained a C implementation (GTrie index, index
writer, CMake/Docker build) that predates the commerce-native search engine
thesis in Issue #2. `CLAUDE.md` requires Rust as the implementation language
for the active engine and explicitly forbids preserving the old C
architecture for compatibility; only its git history is kept.

Gate 0 requires: a Rust workspace as the active product, CI running fmt /
clippy / test / release build, deterministic fixtures, a benchmark harness,
and this decision log.

## Decision

- Delete the C sources, headers, tests, CMake/Docker build files, and the
  old CMake GitHub Actions workflow. History remains reachable via git log;
  nothing is rewritten.
- Add a Cargo workspace at the repository root with a single member crate,
  `crates/commerce-core`, rather than a monolithic root crate or an
  immediately-multi-crate layout (`core`/`cli`/`bench`/`controlplane`
  split). One crate is the smallest unit that satisfies "Rust workspace"
  literally while avoiding premature crate boundaries before Gate 2-5 shape
  is known (CLAUDE.md: don't design for hypothetical future requirements).
  The workspace manifest exists specifically so later gates (e.g. a Gate 5
  offline control-plane binary, or a Gate 7 bench/report CLI) can add
  members without restructuring the root.
- Criterion benchmarks and integration tests live in `commerce-core`'s
  standard `benches/` and `tests/` directories rather than a separate
  crate; this is the idiomatic Cargo layout and needs no extra workspace
  member.
- Deterministic fixtures are plain Rust functions in `src/fixtures.rs`
  (typed domain builders), not JSON/YAML data files, per CLAUDE.md's
  preference for typed domain concepts over generic document
  abstractions and so fixtures fail to compile (rather than silently
  drift) when the domain model changes.
- CI is a single `rust-ci.yml` workflow running, in order: `cargo fmt --all
  -- --check`, `cargo clippy --workspace --all-targets --all-features -- -D
  warnings`, `cargo test --workspace --all-features`, `cargo build
  --workspace --release` — the same commands mandated by CLAUDE.md's
  engineering quality gate, so local and CI runs cannot diverge.

## Consequences

- No production code path exists yet; this ADR only establishes the
  scaffold the rest of the epic builds on.
- Adding a second crate later (e.g. splitting the IR compiler or a
  control-plane binary out of `commerce-core`) is a workspace member
  addition, not a restructuring.
- The benchmark in `benches/catalog_bench.rs` is a harness smoke test at a
  moderate synthetic scale (5k products), not the Gate 3/7 scale-ladder
  benchmark; it exists to satisfy Gate 0's "benchmark harness exists"
  requirement and to give Gate 1 a non-trivial performance baseline for
  free.

## Alternatives considered

- **Single root crate, no workspace.** Rejected: Gate 0 names "Rust
  workspace" explicitly, and a workspace costs nothing extra now while
  avoiding a later migration.
- **Multi-crate split from day one** (`commerce-domain`, `commerce-ir`,
  `commerce-index`, `commerce-bench`). Rejected for now: Gate 1 only needs
  a domain model; splitting before the IR/index boundary is known would be
  speculative structure with no present benefit, and CLAUDE.md flags this
  as a premature abstraction risk. Revisit once Gate 2's compiler and Gate
  3's physical indexes have concrete shapes.
