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

**Correction (P2-E12)**: the above is true for *integer-count* metrics
(structural filter recall/precision, route-distribution counts,
`measure_precision`'s output) but not quite true for `f64`-*averaged*
metrics (NDCG@10/Recall@10/MRR). Two independent runs of the identical
baseline configuration produced NDCG@10=0.2278 vs. 0.2279 — every eval
binary in this campaign iterates `judged_by_query.values()` (a `HashMap`,
whose iteration order is randomized per-process by Rust's default hasher),
and floating-point addition is not associative, so summing one score per
query in a different order can shift the result by roughly 1e-4. Any
NDCG/Recall/MRR delta at or below that level between two runs cannot be
attributed to a real effect without either a fixed iteration order (a
`BTreeMap` or sorted `Vec`, not yet applied to any eval binary) or repeated
measurement with a bootstrap CI (the machinery exists in `bench-harness`
but has so far only been used for timing, not relevance). Recorded as a
real, previously-unstated threat to validity — full detail in
`docs/experiments/PHASE2_LOG.md` P2-E12.

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

### 8.1 P1-B: confidence-tiered brand enforcement — **REVISE** (full cycle complete)

**Hypothesis**: alias-normalized hard matching (tier 1) and/or a fuzzy soft
`Preference` (tier 2) preserve more real recall than Issue #9's exact-BrandId
hard match, without collapsing route coverage to `Punt`-only.

**Method**: `crates/phase2-eval/src/bin/alias_enforcement_eval.rs`, reusing
P2-E05's exact planner-integration harness and P2-E07–E10's own
`measure_precision` structural-recall function. Three modes (`baseline`,
`alias_only`, `alias_fuzzy`) × two trust thresholds (`min_enum_frequency`
∈ {25, 100}), full 22,458-query real replay per cell. Full writeup:
`docs/experiments/PHASE2_LOG.md` P2-E11.

**RED → GREEN, a real bug, not a modeling artifact**: the first run
measured `alias_fuzzy` *regressing* relevance (NDCG@10 0.2278→0.2095,
Recall@10 0.1354→0.1248) and running 2.3× slower. Root cause:
`ir::query::apply_candidates` removed a phrase from `residual_lexical` when
it resolved to *only* a soft `Preference`, hiding it from the lexical
delegate entirely for a ranking-only boost in return — a real,
production-relevant defect the fuzzy tier was the first real caller to
expose (I7-E04 had already found no candidate pool ever produced a real
`Preference` before this phase). Fixed (`apply_candidates` now keeps the
phrase in `residual_lexical` too); a length-difference edit-distance
prefilter also closed the 2.3× slowdown. Four existing test files needed
cascading, explained fixture updates.

**Final real-data result, both thresholds, post-fix**:

| min_enum_frequency | mode | NDCG@10 | Recall@10 | zero-result | wall time |
|---|---|---|---|---|---|
| 25 | baseline | 0.2278 | 0.1354 | 9.55% | 136.0s |
| 25 | alias_only | 0.2278 | 0.1354 | 9.55% | 133.5s |
| 25 | alias_fuzzy | 0.2279 | 0.1355 | 9.42% | 135.8s |
| 100 | baseline / alias_only / alias_fuzzy | 0.2704 (all three) | 0.1611 (all three) | 2.93% (all three) | ~107s each |

Tier 1 is **byte-for-byte identical** to baseline at both thresholds — a
reproduced, robust null result. Tier 2's gain over baseline is noise-level
(+0.0001 NDCG@10 at best). Neither tier meaningfully moves relevance, route
coverage, or candidate-set reduction.

**Root-cause follow-up** (`crates/phase2-eval/src/bin/brand_recall_gap_diagnostic.rs`):
classified every judged-Exact product that fails a compiled brand filter.
Alias/spelling variance (tier 1+2's entire territory) explains only ~1.1%
of failing rows / ~4.5% of distinct affected queries. The dominant ~95%
splits into at least four distinct causes, manually inspected on a
query-deduplicated real sample: (1) generic English words mis-recognized as
brands ("case," "zoom," "head," "king," "duck" — a canonicalization
false-*positive* problem, the mirror of Issue #9's false-negative framing);
(2) sub-brand/product-line naming ("Dove" vs. "Dove Men + Care" — same
parent brand, needs containment matching, not edit distance); (3)
franchise/media-property vs. licensed-manufacturer mismatch ("Pokemon" vs.
"Ultra Pro" — no string-similarity mechanism can bridge this, it needs real
entity-relationship knowledge); (4) missing brand field entirely. A fifth
pattern (genuinely different/compatible-aftermarket brand) is arguably
*correct* strict-filter behavior, not a defect.

**Decision: REVISE.** The mechanism is sound and bug-free, but P2-E10's own
"spelling/aliasing/formatting variation" framing was directionally correct
and quantitatively minor on this real catalog. This sharpens rather than
reverses P2-E10: the next lever is not a better string-matching enforcement
mechanism, it is (a) a false-positive-aware trust gate for generic
brand-shaped strings, and (b) genuine latent-structure inference for
franchise/missing-brand cases — concretely, Issue #6's P1-C.

### 8.2 P1-C: predictive semantic prefill — **NARROW** (full cycle complete)

**Hypothesis**: catalog-derived title-phrase-to-brand co-occurrence can
predict a brand for a query phrase the existing lexicon cannot resolve at
all, moving some real Punt-shaped traffic to `Hybrid`/`FastPath` and/or
improving structural recall, without materially degrading integrated
relevance. Motivated directly by P2-E11's franchise/manufacturer-mismatch
and missing-brand-data failure cases. Full writeup: `docs/experiments/PHASE2_LOG.md`
P2-E12.

**Method**: `cold_start::prefill` (mechanism: `TitlePhraseIndex` trait,
`predict_brand_from_phrase`, `apply_predictive_prefill` — confidence-tiered
per Issue #6's explicit instruction not to assume predictions must be hard
constraints). Real implementation: `phase2-eval/src/bin/prefill_eval.rs`,
a Tantivy title-field phrase index, reusing P1-B's exact harness.

**Real-data result, `min_enum_frequency=25`, full 22,458-query corpus**:
1,133 queries (5.0%) gained a new hard `Brand` constraint they had none of
before; of those, 125 (0.56% of the full corpus) had their execution route
genuinely change (80 to `Hybrid`, 45 to `FastPath`) — a real, if modest,
positive answer to "does inferred structure move Punt→Hybrid." Structural
filter recall rose +0.5pp (Exact+Substitute) / +0.6pp (Exact only) — both
exact integer-count metrics, not noise. Zero-result rate rose a small
+0.09pp (occasionally-wrong predictions). Integrated NDCG@10/Recall@10/MRR
moved by -0.0003/-0.0001/-0.0004 respectively — all within the ~1e-4
floating-point noise floor §4.2's correction identifies, so indistinguishable
from zero in this single run.

**Decision: NARROW.** The mechanism is real, bug-free, and moved real
traffic for exactly the failure class it was built to address — but the
effect is small in absolute terms (0.56% of traffic), and whether
integrated relevance genuinely improves, stays flat, or slightly regresses
cannot be determined from a single run given the newly-identified noise
floor. Real, positive, reproducible-in-mechanism evidence for a narrow
slice of real traffic (franchise/product-family-shaped queries), not
evidence for a broad win on its own.

### 8.3 P1-D/P1-E: physical advantage by class, weighted economics — **NEGATIVE RESULT** (full cycle complete)

**Setup**: `crates/phase2-eval/src/bin/p1d_physical_advantage_eval.rs`
measures commerce-native (`plan::execute_planned`) against a live, fresh,
same-environment Apache Solr 9.10.1 (re-indexed with the real catalog)
and an embedded Tantivy-standalone baseline (P2-E01's validated-
equivalent-relevance engine), across all 9 `QueryClass9` classes, on the
full real 22,458-query judged corpus. Per class: single-pass correctness
(NDCG@10/Recall@10/MRR/zero-result-rate, up to 200 real queries,
`BTreeMap`-ordered to avoid the §4.2 HashMap-noise floor) and a separate
repeated-measurement latency phase (20 queries × 30 reps/method,
interleaved via `bench_harness::round_robin_schedule`, bootstrap CIs for
the commerce-native-vs-baseline diff).

**Five real bugs found and fixed across the experiment loop** (each
root-caused, fixed with the smallest correct change, RED-first regression
test, full quality gate) before any number could be trusted:
`docs/experiments/PHASE2_LOG.md` P2-E13 (an unconditional full-catalog
attribute merge costing ~1078ms per FastPath query; Solr filtering
against a completely unpopulated `brand_lower` schema field), P2-E14 (the
compiler ANDing two independently-resolved, mutually-exclusive `Brand`
constraints together — e.g. "harry potter lego" compiling to
`Brand(Harry Potter) AND Brand(Lego)`, impossible for any real product),
P2-E15 (the same defect generalized to attribute-level `Constraint::Enum`
— "skeleton toy" → `color=Skeleton AND color=Toy`), and P2-E16 (an
adversarial-review workflow finding the harness's own *latency*
measurement of Solr was silently hitting Solr's unpopulated `_text_`
default field — a guaranteed-zero-hit lookup — instead of the real
edismax/`all_text` query the correctness loop already used; plus a
correctness-neutral fix removing a 20x-oversampled delegate call for
`lexical_first`, the single largest real-traffic class, where an empty
constraint list can never reject a delegate hit).

**Final, adversarially-reviewed result** (P2-E17), traffic-weighted by
each class's real query-count share of the full 22,458-query corpus:

| class | traffic share | CN vs. Solr mean latency |
|---|---|---|
| structural_exact_entity | 0.68% | **87x faster** |
| variant_scoped_structural | 0.13% | **105x faster** |
| range_plus_structural | 0.01% | 18x *slower* (n=2, no traffic weight) |
| structural_plus_lexical_residual | 14.48% | 2.9x slower |
| structural_plus_semantic_residual | 0.25% | 3.8x slower |
| lexical_first | 36.84% | 3.0x slower |
| ambiguous_punt | 22.29% | 3.4x slower |
| long_tail_noisy | 25.31% | 2.7x slower |

**Weighted whole-workload economics: commerce-native ~2.3-3.0x SLOWER
than Solr** (median-/mean-weighted respectively) — against Issue #6's
5-10x-*faster* north star. Every one of the six classes representing more
than 0.1% of real traffic (99.19% of the corpus) individually shows
commerce-native slower than Solr; its only wins are confined to two
classes totaling 0.81% of traffic, and — per a dedicated relevance-
guardrail audit tracing the mechanism to source (`execute_ranked` never
computes a nonzero score because the shipping baseline lexicon never
populates `query.preferences`) — those wins carry a real, uncorrected
relevance cost (NDCG@10 -31.5%, MRR -58% relative to Solr on
`structural_exact_entity`), not a clean win on both axes.

Full class-by-class table, the adversarial review's seven-question
checklist, and threats to validity: `docs/experiments/PHASE2_LOG.md`
P2-E17. Raw artifacts: `docs/research/artifacts/p1d_run5/`.

## 9. Ablations

**P1-B**: `alias_only` vs. `alias_fuzzy` vs. `baseline` (§8.1) is itself a
tier-by-tier ablation (tier 1 alone, tier 1+2, neither) — result: neither
tier's removal/addition changes the outcome measurably, which is itself the
finding (§11). The root-cause diagnostic (`brand_recall_gap_diagnostic.rs`)
goes further than a component ablation: rather than just measuring that the
mechanism doesn't help, it attributes *why* by classifying real failing
cases into causal buckets — a form of error analysis this campaign will
reuse for future negative/marginal results rather than stopping at "no
effect."

Remaining ablations planned once a promising configuration exists
elsewhere: remove/replace one component at a time (predictive prefill,
structural narrowing, confidence-aware routing) to attribute any observed
gain to its actual cause rather than the configuration as a whole.

**P1-D**: the adversarial review's `hybrid_overhead_rootcause` audit is
itself an ablation-by-source-reading rather than a re-run: it isolated
which piece of `Hybrid`/`Punt`'s cost is (a) a confirmed harness
measurement bug (P2-E16's broken Solr latency query — fixed and
re-measured, narrowing the ratio from ~4.05x to ~3.01x mean-weighted),
(b) a genuinely fixable core inefficiency (`delegate_oversample` applied
even when no constraint could ever reject a hit — fixed, same-run
re-measurement not separately isolated from (a) since both fixes shipped
together), and (c) a delegate-implementation limitation not yet fixed or
ablated (`TantivyDelegate`'s `TermSetQuery`-based `restrict_to`,
undermining `Hybrid`'s narrowing benefit for `structural_plus_lexical_residual`/
`structural_plus_semantic_residual`, 14.7% of traffic combined) — flagged
as an open risk, not measured in isolation this cycle. A true ablation
disentangling (a)+(b)'s combined effect, or measuring (c)'s standalone
cost via a bitmap-based delegate restriction, remains future work if this
architecture is revisited.

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
  work — fixed as a side effect of the P2-E11 `apply_candidates` fix, not
  independently verified**: `plan::plan` treats an entirely empty compiled
  query (`residual_lexical.is_empty()` with zero constraints) as `FastPath`,
  which then ranks the *entire* catalog. A query that resolves to *only* a
  soft `Preference` used to hit exactly this shape; since a
  preference-resolved phrase now always stays in `residual_lexical`
  (P2-E11), that specific trigger no longer occurs, but the underlying
  `plan::plan` behavior (empty query -> `FastPath` -> rank everything) is
  itself unchanged and could still be reached by some other path not yet
  identified. No dedicated regression test was added for `plan::plan`
  itself.
- **This session's benchmark machine is shared/virtualized** (4 vCPU Xeon,
  cloud sandbox) — absolute QPS/latency numbers are environment-specific;
  the round-robin scheduling (§4.2) mitigates but does not eliminate
  shared-tenant noise, and no isolated bare-metal run has been performed.
- **HashMap-iteration-order floating-point noise, found and documented
  (§4.2 correction, P2-E12) but not yet fixed**: every eval binary in this
  campaign sums per-query NDCG/Recall/MRR scores by iterating a `HashMap`,
  producing ~1e-4-level run-to-run noise on those specific metrics. Every
  NDCG/Recall/MRR delta reported anywhere in §8 that is at or below that
  level should be read as "not distinguishable from noise in a single
  run," not as a real effect — this applies retroactively to P2-E11's own
  table as well, not only P2-E12's. (P1-D's own binary, `p1d_physical_advantage_eval.rs`,
  fixed this specific noise source by using `BTreeMap`-ordered iteration
  throughout — a real methodological correction carried forward from
  P2-E12's own finding, not repeated in P1-D's numbers.)
- **P1-D's latency samples are 20 unique queries per class, repeated 30
  times each — not 30 independent draws from the class's real
  population**, and the correctness/latency samples are a deterministic
  "smallest-N-by-native-ESCI-query-id" selection, not random or
  stratified (found by the P2-E17 adversarial review's `fairness_audit`
  agent). This covers as little as ~2.4-6% of the four classes that carry
  >99% of real traffic. The bootstrap CIs are therefore tighter-looking
  than the true query-to-query variance across each class's full
  population would support; the *direction* of every finding is
  corroborated by Solr's own correctness-loop `avg_server_qtime` (computed
  over a separate, larger, though still non-random, 200-query sample),
  but the *exact* traffic-weighted multiplier (~2.3-3.0x slower) should be
  read as a defensible estimate, not a tight confidence interval, until a
  random/stratified, larger-N re-run is performed.
- **`plan::plan`'s `FastPath` route has no selectivity safeguard**, unlike
  `Hybrid`/`Punt` (which explicitly gate on `selectivity <=
  policy.selectivity_threshold`): a real query in `range_plus_structural`
  (n=2, 0.01% of traffic) resolved to a large, non-selective candidate
  set and showed commerce-native's single worst P1-D latency (mean
  ~30ms), slower than both baselines — a genuine, source-verified signal
  that `structural_exact_entity`/`variant_scoped_structural`'s dramatic
  wins (87-105x) may be a property of this corpus's favorable samples for
  those two classes specifically, not a general architectural guarantee.
  Currently negligible traffic weight; not fixed this cycle (P2-E17).
- **The reference `TantivyDelegate` used throughout P1-D turns `Hybrid`'s
  `restrict_to` into a per-query Lucene `TermSetQuery`** over the full
  narrowed candidate set (up to ~60K terms for
  `structural_plus_lexical_residual`), which the P2-E17 adversarial review
  found undermines much of the "narrow first is cheap" cost advantage
  `Hybrid` is meant to realize. This is a limitation of this benchmark's
  reference delegate implementation, not of `commerce_core::plan` itself
  (the `LexicalDelegate` trait boundary is respected), but it means the
  measured `Hybrid`-path cost may not reflect what a production-grade,
  bitmap/doc-id-set-based delegate restriction could achieve.

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
- §8.1/P2-E11 (this phase, final): alias-normalized hard matching (tier 1)
  produced **zero measurable effect**, reproduced identically at both
  `min_enum_frequency` thresholds tested — not a preliminary result, a
  robust null finding. Fuzzy soft matching (tier 2) is bug-fixed and
  correct but its real-data gain is noise-level. Root-caused: alias/
  spelling variance explains only ~1-5% of real brand-filter recall
  misses; P2-E10's own framing of the dominant cause was directionally
  right but quantitatively minor.
- §8.1/P2-E11: a real production defect existed, silently unreachable,
  in code that predates this phase — `ir::query::apply_candidates`
  removing a `Preference`-only phrase from `residual_lexical`. Found only
  because tier 2 was the first real caller to ever produce a non-trivial
  `Preference` (I7-E04 had already established `compile_lexicon` never
  did). A concrete instance of "real end-to-end measurement finds bugs
  classifier-quality-only evaluation cannot."
- §4.2/P2-E12: this campaign's own "relevance numbers are exactly
  reproducible bit-for-bit" methodological claim was itself falsified by
  running the identical baseline twice — a real instance of the campaign's
  own rigor protocol catching a gap in itself, not just in the system
  under test.
- §8.3/P2-E17 (P1-D/P1-E, final): **the whole-engine 5-10x QPS/$ thesis is
  a negative result on this real catalog/query corpus.** Commerce-native's
  structural/hybrid architecture is dramatically faster (87-105x) on two
  query classes totaling under 1% of real traffic, and consistently
  slower (2.7-3.8x) on the six classes totaling 99.19% of real traffic
  that reach `Hybrid`/`Punt`. Traffic-weighted, commerce-native is
  ~2.3-3.0x *slower* than mature Solr, not 5-10x faster. This is not a
  benchmark artifact masking a real win: the review found and fixed a
  real, severe measurement bug that was making Solr's baseline look
  artificially fast (P2-E16), and the negative result *survived and
  narrowed* after that correction rather than disappearing — a stronger
  form of evidence than a single favorable-looking number would have
  been. The two classes that do win physically also carry a real,
  uncorrected relevance cost (§8.3), so even the narrow win is not a clean
  one as currently implemented.
- §8.3/P2-E17: this campaign's own real-world dataset (Amazon ESCI) has
  no structured `product_type`/`category`/price data at all (P2-E14),
  which is itself why `selective_multi_attribute_structural` — one of
  Issue #6's original 9 named query classes — is empty on real data: not
  because commerce-native fails at it, but because no real query in this
  corpus can even be classified into it once the compiler correctly
  refuses to fabricate a second structural entity constraint from noise.
  A materially different real catalog with genuine multi-entity
  structured data remains untested and could change this specific
  finding, though not the broader Hybrid/Punt result (which does not
  depend on `product_type`/`category` at all).

## 12. Conclusion on the 5–10× Thesis

**NEGATIVE RESULT**, on this real catalog (1,215,854-product Amazon ESCI)
and real query corpus (22,458 human-judged queries), reached via the
campaign's full required discipline: fair, same-environment baselines
(Solr and Tantivy); repeated measurement with bootstrap CIs; a stable
9-class query taxonomy reported per-class and traffic-weighted; five real
bugs found, root-caused, and fixed with RED-first regression tests before
trusting any number (P2-E13–P2-E16); and a 4-agent adversarial review
that independently reproduced the headline weighted-economics number,
audited baseline fairness, audited the relevance guardrail, and
root-caused the dominant-traffic-class slowdown — which is precisely how
it caught the one bug (a broken Solr latency measurement) that would have
made the negative result look more severe than reality, and *narrowed*
the finding rather than reversed it.

Three P1 semantic-layer/physical-execution results are now complete:
**P1-B** (REVISE — confidence-tiered enforcement is a sound mechanism, but
alias/spelling variance is a minor, not dominant, contributor to the real
recall gap), **P1-C** (NARROW — predictive prefill genuinely moves a
small, real slice of traffic and improves structural recall, modest
effect, integrated-relevance impact not distinguishable from measurement
noise), and **P1-D/P1-E** (NEGATIVE RESULT — see above). None of the
three individually reverses another: P1-B/P1-C concern semantic
interpretation quality on the Punt/Hybrid path, which P1-D independently
found to be the traffic-dominant, economically-decisive path and to be
*slower* than the mature baseline regardless of semantic-layer quality —
improving semantic interpretation further cannot, by itself, close a gap
that is currently dominated by execution-path overhead (embedded delegate
call cost, `TermSetQuery`-based restriction, HTTP-boundary-controlled-for
but still real per-query cost), not by classification accuracy.

The campaign's decision discipline (KEEP/REVISE/DELEGATE/P2/STOP) applied
to the whole-engine thesis: closest to **STOP** — evidence, produced
under this campaign's own rigor bar and adversarially checked, makes a
5-10x commerce-vertical advantage implausible on real ecommerce workloads
resembling this one, absent an architecturally different execution path
for the traffic-dominant Hybrid/Punt classes (a cheaper delegate-
restriction mechanism, and/or a materially different real catalog with
richer structured data than this one has). This is reported as the
evidence supports it, per Issue #6's explicit instruction that any of
SUPPORT/NARROW/NEGATIVE RESULT is a valid, defensible outcome.
