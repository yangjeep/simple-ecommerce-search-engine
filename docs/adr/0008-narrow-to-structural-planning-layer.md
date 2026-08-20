# ADR 0008: Narrow the product to a structural/semantic planning layer over a delegated lexical engine

## Status

Accepted (Issue #5, `ROUND1_DECISION_TREE.md`).

## Context

Issue #2/PR #4 (Phase 0) reached PROCEED on hand-authored fixtures,
explicitly not a validation of the full "commerce-native architecture
replaces generic document search" thesis — nine unresolved risks were
named, most centrally the absence of any external baseline and the small
size of every fixture. Issue #5 (Round 1, `docs/experiments/ROUND1_LOG.md`
R1-E01 through R1-E07) attacked those risks with a real 1,215,854-product
catalog, a real 22,458-query human-judged corpus (Amazon ESCI), a real
external baseline (Apache Solr/Lucene), and adversarial correctness,
physical-workload, and control-plane stress tests Phase 0 never ran.

The result is a materially different picture than Phase 0's fixtures
suggested, summarized in full in `ROUND1_DECISION_TREE.md`:

- Real cold-start lexicon construction (every distinct catalog field
  value trusted as a lexicon entry with no validation) produces a
  "Semantic FIB hit rate" that is substantially an artifact of noisy
  real-world data-entry values matching query words by coincidence, not
  genuine structural resolution (R1-E02); the resulting hard filters
  discard 95% of genuinely relevant products (R1-E02/E02b).
- The compiler actively misinterprets negation and disjunction, and has
  no scope-sufficiency check for context-poor queries (R1-E03).
- `commerce_core` has no ranking mechanism for free text at all; Solr's
  untuned default lexical ranking answers essentially every real query
  usefully while `commerce_core`'s structural path only ever activates
  on 55.4% of real queries (R1-E04).
- The physical advantage of the bitmap/range index is real but strictly
  conditional on a genuinely selective structural predicate; without one,
  performance collapses to worse than a linear scan (R1-E05) — fixable
  for the retrieval-speed half of the problem by using the already-built
  `lexical_postings` token index (R1-E07, ~6,660x speedup), but doing so
  exposes that raw retrieval was never the hard part — converting a wide
  candidate pool into a useful ranked top-K is, and nothing in this
  codebase does that (R1-E07).
- The control-plane promotion gate is mathematically incapable of
  rejecting a nonsensical mapping for any never-before-seen term,
  regardless of correctness (R1-E06) — a structural safety gap, not an
  incidental one.
- Solr's on-disk index is 7.3x larger than `commerce-core`'s approximate
  index, yet its live RSS grows by only 175MB vs. `commerce-core`'s
  3.76GB for the same real catalog (R1-E04) — a real, measured
  memory-architecture disadvantage.
- Scale itself is not the problem: 1.2M real products build in ~64s and
  fit in ~3.76GB RSS on a single node, with no OOM or latency blowup for
  the workload class the index targets (R1-E01).

## Decision

**Narrow the product.** Stop pursuing a full generic-document-search
replacement. Keep and continue investing in the parts of the
architecture that measured evidence supports:

- The typed, variant-safe structural/facet/range physical index (Gate 3)
  and the ambiguity-preserving Commerce IR compiler (Gate 2), for query
  classes with a genuinely selective, validated predicate — the one
  workload class this round found a real, substantial, uncontested
  physical advantage for.
- The control-plane propose/replay/promote mechanism (Gate 5), once
  given a precision-aware (not just coverage-aware) replay check.
- The cold-start profiling pipeline (Gate 6), once given a
  candidate-canonicalization/validation stage before raw catalog field
  values become trusted lexicon entries.

**Delegate, rather than rebuild, lexical retrieval and ranking.**
R1-E07's finding is decisive here: building a correct, fast retrieval
primitive (a token-postings inverted index) is cheap and this project
already has the pieces to do it; building a *good ranking function* is
not cheap, is not commerce-specific, and is exactly what a mature engine
has spent years tuning. Embed Tantivy (the Rust-native, in-process
equivalent of what Solr/Lucene demonstrated as a credible baseline in
R1-E04) as the lexical/ranking primitive, rather than continuing to grow
`commerce_core`'s own substring/token matching and building a ranking
function from scratch.

**Do not build**: distributed serving, sharding, replica management, HA,
multi-region serving, Kubernetes, an Elasticsearch-compatible query DSL,
or arbitrary generic document indexing. No evidence this round shows
single-node capacity near its limit (R1-E01), and none of the above is
implied by narrowing the product's scope.

## Consequences

- `commerce_core`'s existing `Constraint::Text`/`execute()` substring
  narrow-then-verify path is not deleted (it stays correct and
  regression-tested — R1-E07 added new methods alongside it rather than
  modifying it), but it is no longer the intended production path for
  free-text/lexical queries; Tantivy is.
- A new dependency (Tantivy) enters the workspace — the first
  non-`roaring`/`serde`-class dependency this project has taken on for
  production (not experiment-harness) code. This is a deliberate,
  evidence-driven exception to "prefer typed domain concepts over
  generic JSON/document abstractions" (CLAUDE.md): the evidence
  specifically shows the generic engine's ranking machinery is the
  differentiated, hard-to-replicate part, which is the opposite of the
  parts CLAUDE.md's architecture bias asks this project to build itself.
- The next concrete validation task (tracked in the Phase 2 follow-on
  issue) is testing this ADR's central bet directly: does an embedded
  Tantivy index, given the same real catalog and real ESCI judgments
  used throughout Round 1, recover (or improve on) Solr's measured
  relevance numbers in-process, without Solr's JVM/HTTP overhead. If it
  does not, this ADR's decision should be revisited before further
  integration work proceeds.
- Canonicalization and precision-aware promotion are new, required
  components, not optional polish — R1-E02/E02b and R1-E06 both found
  their absence to be a root cause of a measured failure, not a
  nice-to-have improvement.

## Alternatives considered

- **ENGINEIZE the original full-replacement vision.** Rejected: R1-E04's
  relevance gap, R1-E06's control-plane safety gap, and R1-E07's
  complexity-convergence finding are independent negative results across
  different subsystems, not a single fixable premise: proceeding would
  mean building index-bundle/mmap/ranking/lexical infrastructure this
  round's own evidence says is not differentiating relative to existing
  engines.
- **STOP entirely.** Rejected: every specific failure mode found this
  round traces to a concrete, already-identified root cause (missing
  canonicalization, missing ranking, missing precision check), not an
  unfixable premise. The structural/facet core itself accumulated zero
  negative correctness evidence across 12x Phase 0's largest scale
  tier (R1-E01/E05) — discarding that alongside the genuinely negative
  findings would be an overcorrection the evidence does not support.
- **REVISE THEN ENGINEIZE** (fix specific assumptions, keep the
  full-replacement scope). Rejected: the negative findings span
  extraction/validation, ranking, memory architecture, and control-plane
  safety — a breadth of independent findings, not an isolated wrong
  assumption — and Issue #5's own guidance treats exactly this pattern
  ("increasingly forces generic search-engine abstractions") as evidence
  against the specialization thesis at the original scope, not evidence
  for patching it in place.
