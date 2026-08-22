# Issue #38 Decision — E1, interim

**Decision: GO on E1's specific question, scope boundary explicitly held
at E1.** Issue #38 opens a much larger epic (E1 through E5: the
architectural falsification gate, unseen-catalog feature discovery,
mixed-category merchants, learned relevance priors, bitmap reuse for
faceting). This document covers exactly the four steps Issue #38's own
"Execution order" names as prerequisite-then-first: (1) repair the
Phase 9 `compile_lexicon`/`compile()` resolution-priority defect, RED
tests first; (2) re-run the affected Phase 9 H1/H3 measurements from the
corrected baseline; (3) freeze that baseline; (4) run E1. All four are
done. E2-E5 are **not** started in this pass -- named explicitly as the
next increment, not silently deferred.

## What this covers

- **Steps 1-3 (the corrected Phase 9 baseline)**: done as P9-E05/P9-E06,
  documented fully in `docs/experiments/PHASE9_LOG.md` and
  `PHASE9_DECISION.md`. Summary: the resolution-priority defect (a
  coincidental attribute match, e.g. `color=coffee` in "smart coffee
  table," winning over a missing entity constraint) is fixed at its root
  cause via a general corroboration rule, not a special case for
  disclosed examples. H1 (ranking quality) and H3 (execution-speed
  advantage) both remain FALSIFIED after the fix, with one materially new
  fact the fix itself exposed: native's ranking-pass cost scales with
  candidate-set size, and the earlier near-parity H3 reading was itself
  partly an artifact of the defect producing degenerate, unrealistically
  tiny candidate sets. This is directly relevant to E1's own physical-
  execution-advantage measurement below (see "What this does NOT
  establish").
- **Step 4 (I38-E1)**: full detail in `docs/experiments/ISSUE38_LOG.md`.
  Four (then five) paths over the identical real WANDS catalog and a
  40-query, selectivity-stratified `product_type` workload:
  - **A** (hard-coded `CatalogIndex::product_type_bitmaps`) vs
    **B** (naive compiled schema, `(field,value)`-tuple-keyed generic
    bitmap): B's p50 overhead vs A is **+74.94% to +77.13%** across 5
    independent runs -- **DOES NOT PASS** the `<=5%` initial target.
  - Per-query allocation counts (a real, deterministic metric) precisely
    localize the cause: A allocates 2/query, B allocates 4/query --
    exactly the two owned `String`s B's tuple-key `HashMap::get` must
    clone on every lookup.
  - Per Issue #38's own instruction ("determine whether it is fundamental
    or implementation-specific... redesign and re-test only if there is
    a falsifiable reason to believe it can be eliminated"), and using
    this codebase's own Issue #21 Phase 6D dictionary/ordinal precedent
    as that falsifiable reason, a redesigned compiled-schema path
    (**B2**, one dedicated `HashMap<String, RoaringBitmap>` per compiled
    field, no tuple key) was built and re-tested: B2's p50 overhead vs A
    is **-10.16% to -13.31%** across 5 independent runs -- **PASSES**
    comfortably, with per-query allocations (2/query) identical to A.
  - **C** (a deliberate runtime-generic strawman, no precomputed index at
    all) is ~8,145x-8,232x slower than A; **D** (Solr, cross-process
    context) is ~9,353x-9,477x slower than A.
  - A real measurement-methodology bug (sub-microsecond individual-call
    timing producing a sign flip on B's overhead between two runs) was
    caught and fixed mid-experiment via call-batching, disclosed in
    `ISSUE38_LOG.md` rather than silently corrected.

## The E1 verdict, stated precisely

**PASSES**, via B2. The naive B design's failure was real and
reproducible, not measurement noise -- but it was **implementation-
specific** (an avoidable per-query allocation in a naive lookup-key
design), not a **fundamental** cost of "compile a dynamically-discovered
feature schema into the same physical operators a hard-coded executor
uses." B2 demonstrates the architecture's central claim holds for this
one concrete case: `product_type`, discovered the same catalog-agnostic
way `compile_lexicon` already discovers real attribute vocabulary and
compiled into the same `RoaringBitmap`-based physical primitive
`CatalogIndex` already uses generically, serves at a cost
indistinguishable from (not measurably worse than) the hard-coded,
Rust-struct-field-backed equivalent.

## What this does NOT establish

- That an arbitrary, more complex merchant feature (a numeric range, a
  relationship/fitment constraint, a hierarchy) compiles this cheaply --
  E1 tested exactly one field kind (a single-valued enum/entity lookup),
  the simplest case. E2 (unseen-vertical feature discovery) and E3
  (mixed-category merchants) are needed before generalizing.
- That LLM-assisted feature discovery (E2's own subject) produces useful,
  validated classifications at all -- E1 used a hand-known field
  (`product_type`) discovered via existing deterministic profiling
  (`CatalogProfile`), not a genuinely unseen schema with no prior
  mapping.
- That the redesign (B2) generalizes to every compiled field without
  re-checking: B2's per-field dedicated `HashMap<String, RoaringBitmap>`
  is a reasonable default, but a real physical compiler must still choose
  representations per Issue #38's own criteria (cardinality, density,
  selectivity, workload frequency) -- E1 did not exercise that choice
  space, only one already-obvious case.
- That P9-E06's newly-found native ranking-cost-vs-candidate-set-size
  scaling is resolved -- it remains open (named in `PHASE9_DECISION.md`)
  and is a real confound E1's own timing measurements did not need to
  address (E1 measures raw bitmap-lookup cost, not `execute_ranked`'s
  full ranking pass), but any later experiment that DOES measure
  end-to-end ranked retrieval must account for it.
- Cycles/instructions/branch-misses/cache-misses -- not measurable in
  this environment (no `perf`, `perf_event_paranoid` unreadable),
  disclosed rather than fabricated. Allocation counts and RSS are the
  real, measured substitutes used instead.
- Anything about E2-E5's own subjects (unseen catalogs, mixed-category
  merchants, LLM-derived relevance priors, bitmap reuse for faceting) --
  not started in this pass.

## What would be built next if continuing this thread

1. **E2**: LLM-assisted feature discovery on a previously unseen catalog
   from a materially different commerce vertical, validated against
   deterministic statistics (cardinality/density/selectivity/memory/
   workload) before any classification is trusted -- per CLAUDE.md, using
   the existing `control_plane::ModelProvider`/`FixtureModelProvider`
   abstraction (already deterministic-fixture-based, no real API key
   required) rather than a new mechanism.
2. **E3**: a genuinely mixed-category catalog (e.g. combining WANDS'
   furniture/home-goods data with a materially different vertical), one
   ingestion/compiler pipeline, no vertical-specific serving code,
   verifying Product/Variant scope and relationship correctness survive
   mixing.
3. **E4**: a compiled deterministic reranker fed by an LLM-proposed
   (then statistically validated) feature-importance prior, directly
   informed by P9-E06's own new finding that native's current
   `execute_ranked` cost scales with candidate-set size -- any reranking
   design must budget for that, not assume the ranking pass is free.
4. **E5**: bitmap reuse for faceting (`popcount(Q AND V)`), building on
   the ordinal/dictionary machinery `CompiledOrdinalIndex` (E1's own B2)
   and `CatalogIndex`'s existing Phase 6D facet structures already
   establish.
5. **Profile/localize P9-E06's ranking-cost-vs-candidate-set-size scaling**
   directly (is `execute_ranked` a full sort where a partial top-K
   selection would do?) before E4 treats it as a settled cost.

## What should explicitly not be built yet

- A full LLM-driven feature-discovery pipeline before E2's own
  deterministic-validation gate is designed -- LLM output is a
  hypothesis, never production truth (CLAUDE.md), and E1 did not exercise
  this at all.
- A universal per-field-type compiled representation chosen once and
  reused for every future field without re-checking cardinality/density/
  selectivity per Issue #38's own criteria -- B2's design answers E1's
  specific question, not "the one true compiled representation."
- Any generic query DSL or document-schema abstraction to make E2/E3
  easier -- CLAUDE.md's explicit warning against "solving schema
  flexibility by recreating Solr/Elasticsearch abstractions in Rust"
  applies with full force starting now, not just retrospectively to E1's
  own deliberate strawman (C).
- Expansion of Issue #35 / Workstream A beyond its already-frozen state --
  unrelated to this thread, not touched in this pass.
