# Paper Notes: A Commerce-Vertical Hybrid Search Engine vs. Mature Lexical Baselines

**Status: living document, updated continuously as evidence accumulates (per Issue #6's
research-campaign instruction). This is not a finished paper — sections marked
`[IN PROGRESS]` reflect the current state of an ongoing multi-round experimental
campaign, not a final conclusion.**

**Repository**: `yangjeep/simple-ecommerce-search-engine`, branch `claude/github-issue-2-gates-puv0wb`.
**Governing documents**: `CLAUDE.md` (autonomy contract, hard rules, quality gate),
Issue #2 (Phase 0 feasibility), Issue #5 (Round 1 reality validation), Issue #6
(Phase 2 — this campaign's active epic), Issue #7 (mature-system archaeology),
Issue #8 (P2, realtime state — parked), Issue #9 (canonicalization frontier).

---

## 1. Motivation / Problem

Commerce search is usually built one of two ways: (a) a generic lexical/vector
search engine (Elasticsearch/Solr/Lucene-family, or a hosted equivalent) with
commerce semantics bolted on as query-time filters, or (b) a fully custom
engine that reinvents lexical retrieval and ranking from scratch. Neither
directly asks the question this project exists to answer:

> Can a **commerce-vertical hybrid engine** — one that treats product/variant/
> brand/category/price as first-class typed structure rather than generic
> fields, and *delegates* mature lexical/ranking work rather than rebuilding
> it — achieve a **materially compelling economic advantage** (target: roughly
> 5–10× QPS/$ or an equivalent latency/cost improvement) over a competently
> configured mature baseline (Solr/Lucene/Tantivy-class), **without**
> materially degrading relevance or correctness?

This is a falsifiable, evidence-gated research question, not a product pitch.
CLAUDE.md's own framing: "The strongest outcome is a defensible
[decision] backed by reproducible experiments, even if the decision is REVISE
or STOP." This document exists to make that defensibility real: every claim
below is traceable to a real-data experiment, a commit, and a raw-sample
artifact, not a summary asserted without evidence.

## 2. Research Questions

Restated from Issue #6, the questions this campaign is structured around:

- **P1-A (semantic coverage)**: what fraction of real ecommerce queries obtain
  useful, *correct* commerce structure (fully structural / structural+lexical
  residual / structural+semantic residual / lexical-first / ambiguous-punt)?
- **P1-B (enforcement policy)**: given a recognized commerce entity (e.g. a
  brand), what *enforcement* semantics (exact hard `Constraint`,
  alias-normalized hard `Constraint`, fuzzy/soft `Preference`, residual-only)
  best trade off candidate-set reduction against real relevance/recall?
- **P1-C (predictive semantic prefill)**: can catalog/control-plane knowledge
  infer *latent* commerce structure not literally present in the query text,
  and does that inference move real traffic from `Punt`→`Hybrid` or
  `Hybrid`→cheaper execution while preserving relevance?
- **P1-D (physical advantage by query class)**: for which query classes, if
  any, does the best commerce-aware plan beat a mature lexical baseline by
  roughly ≥5×, on p50/p95/p99, QPS/core, and a defensible QPS/$ proxy?
- **P1-E (weighted workload economics)**: does the *traffic-weighted* mix of
  query classes plausibly approach the 5–10× target, or does the advantage
  live only in a query-class slice too small to matter?
- **P1-F (mature-system archaeology)**: what has Havenask/Algolia/Solr/
  Lucene-class systems and prior ecommerce search-API research already
  solved, so this project does not re-litigate solved problems?

## 3. Related-System Observations

Summarized from prior archaeology; full detail in the cited documents.

- **Tantivy/Lucene-class lexical retrieval and BM25 ranking are mature,
  competent primitives, not a differentiation target.** P2-E01
  (`docs/experiments/PHASE2_LOG.md`) showed an embedded Tantivy index
  recovers Solr's real relevance numbers on the real 1.2M-product ESCI
  catalog almost exactly (NDCG@10 0.3033 vs. 0.3052, Recall@10 0.1801 vs.
  0.1811). Rebuilding this is not where a vertical engine can win.
- **Havenask/IndexLib already has fast, generic in-place update machinery**
  (bitmap mutation, PK/hash lookups, realtime segments) but **no
  commerce-specific state overlay** and no credible published update-latency
  number (`docs/research/havenask-realtime-update-archaeology.md`,
  Classification B of the three-tier taxonomy Issue #8 defined). This
  informed Issue #8's P2 park, not the north-star thesis directly.
- **No prior archaeology in this repo covers query understanding / entity
  linking / predictive semantic expansion** in any external system — this is
  a genuine, currently-unfilled research gap this project's own P1-C work is
  the first attempt to address, not a rediscovery of known prior art.
- Issue #7's full cross-reference matrix (`docs/experiments/ISSUE7_LOG.md`)
  and the "narrowed product" decision (`docs/adr/0008-narrow-to-structural-planning-layer.md`,
  `docs/adr/0009-structural-lexical-execution-contract.md`) remain the
  architectural starting point for this campaign: delegate lexical
  retrieval/ranking, keep the typed structural/facet core, add a
  cold-start canonicalization stage and a precision-aware control-plane gate.

## 4. Methodology

### 4.1 The experiment loop

Every experiment in this campaign follows: **observe → diagnose → formulate
alternative → add permutation → baseline → repeated measurement →
distribution/relevance analysis → ablation → KEEP/REVISE/DELEGATE/P2/STOP →
next highest-information experiment.** Multiple complete cycles are run, not
one; a single apparent 5× win is actively stress-tested (query classes,
seeds, candidate-set sizes, catalog slices, planner parameters) before being
trusted, and a single negative mechanism does not end the campaign while
materially different hybrid strategies remain plausible.

### 4.2 Statistical rigor protocol

Infrastructure: `crates/bench-harness` (new, this phase). Applies to every
runtime-sensitive performance experiment from this point forward:

- Warm up separately from the measured section (`bench_harness::warmup_then`).
- ≥10 measured repetitions during exploration, ≥30 for any number used in an
  architecture decision or a paper table (`bench_harness::measured_repeat`).
- Method execution order rotated/randomized where practical, via a
  seed-deterministic round-robin schedule (`bench_harness::round_robin_schedule`)
  — prevents shared-machine drift (thermal, cache pressure) from silently
  favoring whichever method happens to run first.
- Raw per-rep samples preserved, not just summaries (`bench_harness::append_raw_samples`).
- Every result reports the full distribution: p10/p25/p50/p75/p90/p95/p99,
  mean, standard deviation, min, max (`bench_harness::Distribution`).
- Headline relative-improvement claims carry a percentile bootstrap
  confidence interval (`bench_harness::bootstrap_ci_diff_of_means`),
  seed-deterministic, ≥2000 resamples for decision-grade numbers.
- Every run's manifest records commit SHA, git-dirty flag, hostname, CPU
  count, OS, dataset path+fingerprint, query-set path+fingerprint, config,
  and seed (`bench_harness::RunManifest`).

**A deliberate, documented scope boundary**: `commerce_core`'s compile/plan/
execute path is fully deterministic — no model call, no randomness anywhere
in the hot path (CLAUDE.md's own hard rule). This means relevance/
correctness/route-distribution/candidate-set-size numbers computed by a
real-data replay are exactly reproducible bit-for-bit given the same
catalog/query/config inputs; repeating that computation adds no statistical
information. Repetition and distribution-reporting are therefore applied
specifically to **wall-clock timing** measurements, which do have genuine
run-to-run variance from OS scheduling, cache state, and other processes on
a shared machine. This is recorded here as a considered methodological
decision, not an evasion of the rigor requirement.

### 4.3 Query taxonomy

Two taxonomies are maintained, for different questions:

- `round1_eval::classify::QueryClass` (7 classes, R1-E02): *did semantic
  interpretation succeed*, and how (exact-id / ambiguous / structural-only /
  structural+lexical / semantic-occasion / lexical-dominant / unresolved-punt).
- `round1_eval::query_taxonomy::QueryClass9` (9 classes, this phase): *what
  does the compiled plan look like*, matching Issue #6's execution-mode
  dimensions directly — structural-exact-entity, selective-multi-attribute-
  structural, variant-scoped-structural, range+structural,
  structural+lexical-residual, structural+semantic-residual, lexical-first,
  ambiguous/punt, long-tail/noisy. See `crates/round1-eval/src/query_taxonomy.rs`
  for the exact, tested precedence rules.

Every permutation-matrix result reports per-class numbers using `QueryClass9`,
plus a traffic-weighted aggregate (each class's real share of the 22,458-query
corpus × its per-class metric).

### 4.4 Method/permutation matrix

Four dimensions, per Issue #6. **Staged elimination**, not brute-force
Cartesian coverage: broad cheap sweep → eliminate dominated methods →
identify the Pareto frontier → deepen replication on survivors → adversarial
testing → ablation → architecture decision.

| Dimension | Options |
|---|---|
| Semantic interpretation/prefill | none; deterministic/catalog-derived; model-assisted predictive prefill |
| Enforcement | exact hard `Constraint`; alias-normalized hard `Constraint`; fuzzy/entity-family; scored `Preference`; residual-only |
| Execution | structural-only; structural narrow→lexical rank; structural narrow→vector rank; structural+lexical+vector fusion; full Tantivy/Lucene `Punt` baseline |
| Planner | fixed threshold; selectivity-aware; confidence-aware; confidence+selectivity |

`[IN PROGRESS]` — status of each cell tracked in §8/§9 as evidence lands;
the matrix is not brute-forced (no vector leg exists yet, so
execution×vector cells are marked NOT YET TESTABLE rather than skipped
silently — see §10).

### 4.5 Fair baseline discipline

- The Tantivy delegate uses the *same* real catalog/query corpus, real
  restrict_to-pushed-into-the-query filtering (not a naive global-then-
  truncate approach — an earlier real-data-caught bug, `planner_integration_eval.rs`'s
  own doc comment, is the concrete precedent for taking this seriously).
- `commerce_core` re-verifies every delegate hit against its own hard
  constraints unconditionally (ADR 0009) — a delegate is trusted for
  ranking/recall, never for correctness, so no comparison can be won by
  silently skipping equivalent semantic work.
- R1-E04 already established the external baseline used where a second,
  fully independent engine is warranted: Apache Solr/Lucene (Docker/ES/
  OpenSearch were unreachable in this environment — recorded as a real
  external blocker, not assumed away).

## 5. Datasets / Workloads

- **Catalog**: Amazon ESCI export, 1,215,854 real products
  (`dataset_cache/export/catalog.jsonl`, gitignored, ~1.5GB). Per-product
  fields actually populated: `title`, `description`, `bullets`, `brand`,
  `color`. `product_type`/`category`/`price` are **sentinel values** for
  every real product (`round1_eval::catalog::build_catalog`) — there is no
  real per-product signal for those fields in this dataset, a real
  limitation carried through every experiment that touches them (§10).
- **Query set**: 22,458 real, human-judged queries (`dataset_cache/export/queries.jsonl`),
  Amazon's own ESCI relevance judgments (Exact/Substitute/Complement/Irrelevant).
- **Brand vocabulary**: 206,227 distinct real raw brand strings, 49.4%
  occurring on exactly one product — the concrete motivating case for both
  Issue #9's canonicalization work and this phase's alias-enforcement work.

## 6. Baselines

- **Apache Solr/Lucene** (R1-E04): real external baseline, 1,000-query
  sample, NDCG@10=0.3052, Recall@10=0.1811, MRR=0.4910, p50=1486µs.
- **Standalone Tantivy** (P2-E01): full 22,458-query set, NDCG@10=0.3033,
  Recall@10=0.1801, MRR=0.4838, p50=1.09ms, zero-result=0.6%.
- **`compile_lexicon`'s exact-BrandId hard match** (Issue #9/P2-E07–E10):
  the enforcement baseline this phase's P1-B work is tested against.

## 7. Architecture / Methods

See `ROUND1_DECISION_TREE.md` and ADRs 0008/0009 for the full structural/
lexical execution-contract design (`FastPath`/`Hybrid`/`Punt`,
`plan::PlannerPolicy`, `plan::LexicalDelegate`). This phase's additions:

- `StructuralConstraint::BrandAny(Vec<BrandId>)` — alias-normalized hard
  matching (§8.1).
- `Preference::StructuralBoost(StructuralConstraint, f64)` — soft,
  ranking-only structural signal (§8.1).
- `cold_start::alias::{alias_key, edit_distance}` — deterministic
  corporate-suffix/punctuation normalization + bounded fuzzy matching.
- `crates/bench-harness` — statistical rigor infrastructure (§4.2).
- `round1_eval::query_taxonomy` — the 9-class structural-shape taxonomy (§4.3).

## 8. Experimental Results

### 8.1 P1-B: confidence-tiered brand enforcement `[IN PROGRESS]`

**Hypothesis**: alias-normalized hard matching (tier 1) and/or a fuzzy soft
`Preference` (tier 2) preserve more real recall than Issue #9's exact-BrandId
hard match, without collapsing route coverage to `Punt`-only.

**Method**: `crates/phase2-eval/src/bin/alias_enforcement_eval.rs`, reusing
P2-E05's exact planner-integration harness and P2-E07–E10's own
`measure_precision` structural-recall function. Three modes (`baseline`,
`alias_only`, `alias_fuzzy`) × two trust thresholds (`min_enum_frequency`
∈ {25, 100}), full 22,458-query real replay per cell.

**Preliminary real-data result** (min_enum_frequency=25, single run, not yet
repeated under the §4.2 protocol — see caveat below):

| mode | outcome dist. | filter recall (Exact+Sub) | filter recall (Exact) | NDCG@10 | Recall@10 | p50 |
|---|---|---|---|---|---|---|
| baseline | Hybrid 5589 / FastPath 328 / Punt 16541 | 31.7% | 35.6% | 0.2278 | 0.1354 | 2.37ms |
| alias_only | *identical to baseline* | *identical* | *identical* | *identical* | *identical* | 2.38ms |
| alias_fuzzy | running — see limitation below | — | — | — | — | — |

**Real, unplanned finding**: at `min_enum_frequency=25`, tier 1
(deterministic alias-normalization) produced **byte-for-byte identical**
routing and relevance numbers to the exact-BrandId baseline. This means, at
this threshold, no two *already-trusted* real brand strings in this catalog
actually share a deterministic alias key (no `Brand`/`Brand Inc`/`Brand, Inc.`
pair both independently clearing the frequency-25 trust gate) — a real
negative result for tier 1 alone at this threshold, not a bug (`StructuralConstraint::BrandAny`
is unit-tested and behaves identically to `Brand` for singleton groups by
construction). Whether this holds at `min_enum_frequency=100` or is an
artifact of this specific threshold is part of what the in-progress run is
checking.

**Known limitation, live**: the `alias_fuzzy` cell is taking materially
longer than `baseline`/`alias_only` (minutes, not the ~126s/cell those two
took) on this run — under investigation (§10) rather than reported as a
final number. This preliminary table is explicitly **not** decision-grade:
it is a single run, not the ≥30-repetition, bootstrap-CI-backed measurement
§4.2 requires for an architecture decision. It is recorded here as an
honest snapshot of an in-progress cycle, per this document's own "no
marketing conclusions" rule.

### 8.2 P1-C: predictive semantic prefill `[NOT YET STARTED]`

Scoping research complete (this phase): no existing mechanism in
`cold_start` associates a query phrase with catalog structure it does not
literally contain; `compile()`'s lexicon resolution is exact-substring-only.
Building a real n-gram↔Brand/Color co-occurrence table (zero model calls,
matching `CatalogProfile::build`'s existing convention) plus a new
query-time injection point is additive, not a rewrite. Implementation not
yet started as of this document's current revision.

### 8.3 P1-D/P1-E: physical advantage by class, weighted economics `[NOT YET STARTED]`

Depends on §8.1/§8.2 landing on a defensible enforcement/prefill
configuration first, per the staged-elimination discipline (§4.4) — no
QueryClass9-segmented physical-advantage sweep has been run yet.

## 9. Ablations `[NOT YET STARTED]`

Planned once a promising configuration exists: remove/replace one component
at a time (predictive prefill, structural narrowing, alias normalization,
soft `Preference`, confidence-aware routing) to attribute any observed gain
to its actual cause rather than the configuration as a whole.

## 10. Limitations / Threats to Validity

- **`product_type`/`category` are sentinel values for every real product** in
  the ESCI ingestion path — `SelectiveMultiAttribute` and any
  multi-entity-structural query class is necessarily thin-to-empty on this
  real dataset; this is a dataset property, not an engine limitation, but it
  means this campaign's real-data evidence cannot speak to multi-entity
  structural performance the way it can to brand/color.
- **No real price field exists in the ESCI dataset** — `RangeStructural`
  evidence is similarly unavailable from real data alone (carried forward
  from R1-E01).
- **Dataset/query-set fingerprints in `RunManifest` are `(size, mtime)`, not
  a content hash** — a deliberate speed/rigor tradeoff for a ~1.5GB file,
  documented in `bench-harness::manifest`'s own doc comment.
- **No vector/semantic backend exists yet** — every "structural+semantic
  residual" or execution-dimension "vector rank"/"fusion" cell is currently
  untestable, not tested-and-negative; §4.4's matrix marks these explicitly
  rather than omitting them silently.
- **A latent planner edge case, found while building the P1-B enforcement
  work, not yet fixed**: `plan::plan` treats an entirely empty compiled
  query (`residual_lexical.is_empty()` with zero constraints) as `FastPath`,
  which then ranks the *entire* catalog. A query that resolves to *only* a
  soft `Preference` (P1-B tier 2's fuzzy match, when the matched token is
  the query's only token) hits exactly this shape. Recorded here rather
  than silently patched into an in-flight experiment; real-data impact not
  yet measured.
- **This session's benchmark machine is shared/virtualized** (4 vCPU Xeon,
  cloud sandbox) — absolute QPS/latency numbers are environment-specific;
  the round-robin scheduling (§4.2) mitigates but does not eliminate
  shared-tenant noise, and no isolated bare-metal run has been performed.

## 11. Negative Findings (preserved, not erased)

- Issue #9/P2-E07–E10: **better brand-string recognition does not imply
  better hard-filter recall** — three independent canonicalization
  mechanisms (frequency threshold, heuristic, model-assisted) all show the
  same directional tradeoff. The concrete motivation for this phase's
  enforcement-layer (not classifier) work.
- I7-E04 (`docs/experiments/ISSUE7_LOG.md`): tiered ranking was falsified,
  and in the course of falsifying it, found `compile_lexicon` never emitted
  a real `Preference` candidate at any threshold — ranking had zero real
  signal prior to this phase's `Preference::StructuralBoost` work.
- §8.1 (this phase, preliminary): alias-normalization tier 1 alone produced
  **zero measurable effect** at `min_enum_frequency=25` on this real
  catalog — recorded as a real negative result for that specific cell, not
  smoothed over.

## 12. Conclusion on the 5–10× Thesis

**`[NOT YET DETERMINED]`** — insufficient evidence has been gathered under
the rigor protocol (§4.2) to render SUPPORT / NARROW / NEGATIVE RESULT.
This section will be updated as each experimental cycle completes; per
Issue #6, any of the three outcomes is a valid, defensible result, and this
document commits to reporting whichever the evidence actually supports.
