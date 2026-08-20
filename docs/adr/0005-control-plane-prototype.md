# ADR 0005: Offline control-plane prototype (observe/propose/replay/promote)

## Status

Accepted (Gate 5, Issue #2).

## Context

Gate 5 asks for an offline flow that observes unresolved/ambiguous spans,
proposes candidate semantic mappings, replays them against historical/
synthetic queries, and promotes/rejects candidates into a new context
version — with model-provider access behind an interface and deterministic
fixtures/mocks so no test needs a real model API key (CLAUDE.md hard
rules: "No LLM/model call in the default query hot path," "No test may
require a real model API key. Model-provider code must have deterministic
fixtures/mocks.").

## Decision

- **A new top-level `control_plane` module, not a submodule of `ir`.**
  The project's mission statement names "semantic forwarding plane" and
  "learned control plane" as the two architectural halves; giving the
  control plane its own top-level module keeps that boundary visible in
  the crate layout, not just in prose. `ir`/`index` (the hot path) have no
  dependency on `control_plane` and never will: `control_plane` depends on
  `ir`, not the reverse.
- **`ModelProvider` is a plain trait** (`fn propose(&self, observation:
  &Observation) -> Option<Proposal>`) with exactly one shipped
  implementation, `FixtureModelProvider` — a fixed, in-memory term-to-
  candidate table. This satisfies the "behind an interface" requirement
  without speculatively building a real model-backed implementation this
  gate doesn't need; standing one up later is a second `impl
  ModelProvider`, not a redesign. The hot-path enforcement is structural,
  not a runtime check: `ir::compile` and `CatalogIndex::execute` simply
  never reference `ModelProvider` or anything in `control_plane`, so no
  code path from a live query can reach a model call regardless of which
  provider is wired in elsewhere.
- **Scope cut: only `residual_lexical` terms are observed/proposed for,
  not `ambiguous` spans.** A residual term has no lexicon entry, so a
  proposal is a straightforward insert. An ambiguous span already has
  multiple candidates; "fixing" it means narrowing or reweighting an
  existing entry, which is a materially different operation (edit, not
  insert) with its own regression risks (narrowing wrong loses a
  legitimate reading). Building both in one gate would double the surface
  to get right; ambiguity resolution is deferred to a future iteration
  once the insert-only path has evidence behind it.
- **Promotion requires per-query, not just aggregate, replay evidence.**
  `ReplayResult::passes_promotion_gate` is `regressions.is_empty() &&
  candidate.fully_resolved > baseline.fully_resolved` — both conditions,
  not a net-coverage threshold. `replay` computes `regressions` by
  comparing each query's fully-resolved status individually before/after,
  so a candidate lexicon that improves the aggregate number while breaking
  even one previously-resolved query is rejected. This is CLAUDE.md's "no
  model-generated semantic route promoted without deterministic
  replay/evaluation evidence" applied literally: the evidence is
  query-level, and one counterexample vetoes the whole candidate batch
  (there is no per-proposal accept/reject within one promotion attempt —
  see Alternatives).
- **`try_promote` returns `Result<SemanticContext, ReplayResult>`**, not a
  bool or an `Option`: a rejected promotion still returns the full replay
  evidence (what regressed, what the coverage numbers were) so a caller —
  human or, later, an automated loop — can see *why* without re-running
  anything.

## Consequences

- Promoting a batch of proposals is all-or-nothing: if any single
  proposal in a batch causes a regression, the entire batch is rejected,
  including proposals that were individually fine. This is a deliberately
  conservative default (favors correctness over throughput of accepted
  proposals) that a future iteration could refine into
  per-proposal-isolated replay if batch rejection turns out to discard
  too much good signal in practice.
- `SemanticContext.source` (free text) is the only promotion provenance
  recorded today; there is no persisted history of rejected attempts. That
  is acceptable for a prototype exercised in tests, not yet for a real
  promotion workflow with an audit trail — flagged as future work, not
  built speculatively now.
- Because `FixtureModelProvider` is the only implementation, this gate
  proves the *mechanism* (interface, replay, gate) works correctly, not
  that any particular model or heuristic proposes good mappings in
  practice — see `docs/experiments/LOG.md` E005 for what is and is not
  demonstrated.

## Alternatives considered

- **Evaluate and promote each proposal independently** (per-term replay
  and gate, rather than batching all accepted proposals into one candidate
  lexicon). More precise — a good proposal wouldn't be held hostage by a
  bad one in the same batch — but `N` times the replay cost for `N`
  proposals, and CLAUDE.md's stop conditions favor evidence over
  throughput at this stage. Revisit if/when the observed proposal volume
  makes all-or-nothing batching a real bottleneck rather than a
  theoretical one.
- **A live/pluggable LLM-backed `ModelProvider` as part of this gate.**
  Explicitly out of scope: Gate 5 only asks for the interface plus
  deterministic fixtures, and CLAUDE.md forbids any test requiring a real
  model API key. Adding one now would also risk the exact anti-pattern the
  hard rules warn about — one model call per unresolved term, un-gated by
  replay evidence.
