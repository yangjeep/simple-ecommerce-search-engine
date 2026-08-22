# Issue #38 Decision — E1-E3, interim

**Decision: GO on E1/E2/E3's specific questions, scope boundary explicitly
held at E3.** Issue #38 opens a much larger epic (E1 through E5: the
architectural falsification gate, unseen-catalog feature discovery,
mixed-category merchants, learned relevance priors, bitmap reuse for
faceting). This document originally covered only the four
prerequisite-then-E1 steps; it now also covers E2 and E3, run in a
follow-on pass per an explicit governing instruction that **pivoted their
methodology** away from this document's original plan (see below). E4/E5
remain **not** started -- named explicitly as the next increment, not
silently deferred.

**Methodology pivot, E2/E3 (disclosed explicitly)**: this document
originally scoped E2 as "LLM-assisted feature discovery on a previously
unseen catalog" and E3 as "combining WANDS' furniture/home-goods data
with a materially different vertical." The instruction governing the
actual E2/E3 pass asked instead for **deterministic synthetic datasets**
(generator, fixed seeds, schema, ground truth, provenance, validation all
committed to the repo), run through the full real production pipeline,
with external-dataset search only as a non-blocking parallel check. This
is a deliberate, user-directed change of plan, not scope creep or a
missed requirement -- LLM-assisted feature discovery specifically remains
untested and is still open (see "What this does NOT establish").

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
    bitmap): B's p50 overhead vs A is **+63.03% to +64.50%** across 5
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
    is **-4.96% to -6.37%** across 5 independent runs -- **PASSES**
    comfortably, with per-query allocations (2/query) identical to A.
  - **C** (a deliberate runtime-generic strawman, no precomputed index at
    all) is ~7,452x-7,595x slower than A; **D** (Solr, cross-process
    context) is ~7,133x-7,851x slower than A.
  - **Two** real measurement-methodology bugs were caught and fixed
    mid-experiment, not one: (1) sub-microsecond individual-call timing
    producing a sign flip on B's overhead between two runs, fixed via
    call-batching; (2) after batching, an adversarial check found the
    batch loops were missing `std::hint::black_box` around each
    iteration's result, leaving room for the optimizer (this workspace
    builds release with `lto=true, codegen-units=1`) to treat 200
    identical repeated calls as partly redundant -- adding `black_box`
    changed B's overhead from a ~75% band to the ~63-65% band reported
    above, and B2's from ~-10-13% to ~-5-6% (same qualitative verdicts,
    corrected magnitudes). Both are disclosed in full in
    `ISSUE38_LOG.md`, including the pre-correction numbers, rather than
    silently replaced.
- **I38-E2 (unseen vertical, generalization)**: full detail in
  `docs/experiments/ISSUE38_LOG.md`. A deterministic synthetic
  automotive-parts catalog (3,000 products, materially different
  attribute schema from WANDS, plus a genuinely new many-to-many
  structural relationship, `compatible_fitment`, via the pre-existing
  `MultiEnum`/`MultiEnumContains` mechanism) run through the real,
  unmodified production pipeline. **Fitment NDCG@10 mean 0.9472** (n=8,
  min 0.7211) -- the new relationship generalizes cleanly, no production
  code changes. A real methodology bug (a pipe-joined fitment key that
  could never match `compile()`'s space-joined phrase lookup) was caught
  by direct code read *before* the experiment ever ran and fixed, with a
  dedicated test proving the fix against the real `compile()`. A
  **second** methodology bug -- the fitment query set itself iterating a
  fixed candidate list with no guarantee any combination was ever
  generated -- was caught by an adversarial review *after* the first E2
  run (original figure: mean 0.9913, min 0.9306; both support the same
  generalization finding, see `docs/experiments/ISSUE38_LOG.md`'s
  correction note). One disclosed, distinct finding: `exact_lookup`
  (part-number search) scores near-zero NDCG because the reused
  `BitmapTantivyDelegate` only indexes product-level `Text` attributes,
  not the variant-level field `part_number` actually lives on -- a
  lexical-delegate scope gap, not a generalization failure, and not
  patched (shared Phase 9 infra, out of scope).
- **I38-E3 (mixed-category merchant, schema management)**: full detail
  in `docs/experiments/ISSUE38_LOG.md`. Furniture + apparel + automotive
  (1,000 products each) ingested as one undifferentiated 3,000-product
  catalog, with shared ambiguous field names, a deliberate `size`
  Enum-vs-Numeric schema conflict, sparse attributes, and noisy titles.
  **Confirmed by direct measurement (not just source reading)**:
  ingestion's schema decisions (`CatalogIndex::build`,
  `CatalogProfile`/`compile_lexicon`) are catalog-agnostic and
  type-tag-driven, with zero per-family conditional logic anywhere.
  **Two disclosed, safe recall gaps found**, both filed as scoped design
  questions rather than patched: (1) the `size` schema conflict --
  `compile()`'s hard-coded `"size N"` keyword branch never consults the
  lexicon, so a `size 34` query recovers 0/64 (0%) of apparel's matching
  products while recovering 10/10 (100%) of automotive's, though
  `Constraint::matches`'s catch-all guarantees this never produces a
  wrong hard filter; (2) a residual-lexical-veto behavior in
  `plan::execute_planned`'s `Hybrid`/`Punt` outcomes, found while
  building this experiment's own control-query template -- an
  unmatchable residual free-text term can zero out an entire
  structurally-well-formed query even when the structural constraint
  alone identifies hundreds of correct candidates already in the index.

## The E2/E3 verdicts, stated precisely

**E2**: the architecture generalizes to this unseen vertical, including
its genuinely new many-to-many structural relationship, with no
production code changes -- a positive result. The `exact_lookup` finding
is real and worth acting on eventually, but is a distinct
lexical-delegate-scope question, not evidence against generalization.

**E3**: ingestion's schema-management behavior is confirmed
catalog-agnostic by direct measurement -- the positive result this
experiment was asked to establish. Both named recall gaps are real,
safe, and disclosed, not correctness violations; both are scoped as
design questions for a future dedicated cycle (matching how P9-E05
treated a structurally similar resolution-priority gap), not patched in
this pass.

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

- That LLM-assisted feature discovery produces useful, validated
  classifications at all -- **still untested**. E1 used a hand-known
  field (`product_type`) discovered via existing deterministic profiling
  (`CatalogProfile`); E2/E3 (this pass) used the same deterministic
  profiling mechanism against synthetic catalogs, not an actual
  model-assisted discovery step against a schema with no prior mapping.
  This remains open for a future E4-adjacent experiment.
- That E2/E3's positive generalization results transfer to *real* (not
  synthetic) unseen-vertical/mixed-category data -- the external-validity
  check was attempted and explicitly did not succeed (see
  `docs/experiments/ISSUE38_LOG.md`'s E2/E3 section): this sandbox could
  not reach any dataset with both real content and a confirmed
  license. E2/E3's evidence is real, ground-truth-by-construction
  synthetic evidence, not real-world validation, and should not be
  represented as the latter.
- Whether `compile()`'s numeric keyword branches (`"size N"`, and by the
  same reasoning `"under $N"`/`"over $N"`) should consult the lexicon and
  participate in the entity-corroboration demotion rule before becoming
  hard filters, and whether `plan::execute_planned`'s `Hybrid`/`Punt`
  outcomes should fall back to the structural candidate set when the
  lexical delegate returns nothing -- both are real, measured E3 findings,
  filed as scoped design questions, deliberately **not** decided or
  patched in this pass.
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
- Anything about E4/E5's own subjects (LLM-derived relevance priors,
  bitmap reuse for faceting) -- not started in this pass.

## What would be built next if continuing this thread

1. **Two scoped design cycles from E3's own findings**, each on the
   P9-E05 model (RED test first, root-cause fix, re-verify): (a) should
   `compile()`'s numeric keyword branches consult the lexicon and
   participate in entity-corroboration demotion; (b) should
   `plan::execute_planned` fall back to the structural candidate set,
   ranked by a default signal, when the lexical delegate returns zero
   hits for a non-empty residual term. Both are safe-but-incomplete gaps
   today, not correctness bugs, so neither blocks E4/E5 -- but both
   should be resolved (or deliberately declined with reasoning recorded)
   before treating recall numbers from either path as final.
2. **LLM-assisted feature discovery**, still entirely untested: run
   against a genuinely unseen schema with no prior mapping, validated
   against deterministic statistics (cardinality/density/selectivity/
   memory/workload) before any classification is trusted -- per
   CLAUDE.md, using the existing `control_plane::ModelProvider`/
   `FixtureModelProvider` abstraction (already deterministic-fixture-based,
   no real API key required) rather than a new mechanism.
3. **A genuine external-validity check**, from an environment that can
   reach a dataset with both real content and a confirmed permissive
   license (Open Food Facts' ODbL/CC-BY-SA export, or a confirmed-license
   check on the Zenodo-origin E-Commerce-Text-Classification data) --
   this pass's own attempt was blocked by sandbox network restrictions,
   not by dataset unavailability in general.
4. **E4**: a compiled deterministic reranker fed by an LLM-proposed
   (then statistically validated) feature-importance prior, directly
   informed by P9-E06's own new finding that native's current
   `execute_ranked` cost scales with candidate-set size -- any reranking
   design must budget for that, not assume the ranking pass is free.
5. **E5**: bitmap reuse for faceting (`popcount(Q AND V)`), building on
   the ordinal/dictionary machinery `CompiledOrdinalIndex` (E1's own B2)
   and `CatalogIndex`'s existing Phase 6D facet structures already
   establish.
6. **Profile/localize P9-E06's ranking-cost-vs-candidate-set-size scaling**
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
