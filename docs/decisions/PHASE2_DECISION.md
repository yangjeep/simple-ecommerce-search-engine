# Phase 2 Decision

**Decision: STOP** the whole-engine 5-10x QPS/$ thesis, on the real
catalog/query evidence gathered this phase. Do not scale up the
commerce-native structural/hybrid architecture as a general-purpose
Solr/Lucene replacement. A narrower, differently-scoped thesis (below)
remains open, and two of the phase's five sub-questions (P1-B, P1-C)
independently reached their own REVISE/NARROW verdicts that are not
overturned by this STOP.

This document is Phase 2's terminal decision artifact, in the same spirit
`SCALE_UP_DECISION.md` closed Phase 0 and `ROUND1_DECISION_TREE.md` closed
Round 1 — not overwriting either (both remain historically accurate for
their own scope and evidence base), but synthesizing what this phase
(Issue #6, `docs/experiments/PHASE2_LOG.md` P2-E01 through P2-E17) found,
now that its final and highest-information sub-question (P1-D/P1-E,
physical advantage by query class and traffic-weighted economics) has
reached a stable, adversarially-reviewed conclusion.

## Recap: what Phase 2 was asked to answer

Round 1 (`ROUND1_DECISION_TREE.md`) chose NARROW THE PRODUCT: keep the
typed structural/facet retrieval core, delegate lexical retrieval/ranking
to an embedded Tantivy index, add a cold-start canonicalization stage and
a precision-aware control-plane gate. Issue #6 reframed the resulting
architecture's *purpose* as a single falsifiable question: can a
commerce-vertical hybrid search engine achieve roughly **5-10x QPS/$ (or
an equivalently compelling latency/cost advantage) versus mature
Solr/Lucene, on representative real ecommerce workloads, without
materially degrading relevance/correctness**? Issue #6 decomposed this
into six research priorities (P1-A through P1-F), all of which are now
answered:

- **P1-A** (semantic coverage: what fraction of real queries obtain
  useful, correct commerce structure) — answered as a byproduct of
  P1-D's 9-class taxonomy breakdown on the real 22,458-query corpus:
  0.68%+0.13%+0.01%=0.82% resolve to a pure structural `FastPath`;
  14.48%+0.25%=14.73% resolve to a real structural-plus-residual
  `Hybrid`; 36.84%+22.29%+25.31%=84.44% never obtain useful structure at
  all (`lexical_first`/`ambiguous_punt`/`long_tail_noisy`, all routed to
  `Punt`). Useful commerce structure is real but a small minority of real
  traffic on this catalog.
- **P1-B** (enforcement policy / softer hybrid semantics) — **REVISE**
  (P2-E11): confidence-tiered enforcement (alias-normalized hard match,
  fuzzy soft `Preference`) is a sound, bug-free mechanism, but alias/
  spelling variance explains only ~1-5% of the real brand-filter recall
  gap Issue #9 identified — a minor, not dominant, contributor.
- **P1-C** (predictive semantic prefill) — **NARROW** (P2-E12): a
  catalog-derived title-phrase-to-brand predictor genuinely moves 0.56%
  of real traffic from `Punt` to `Hybrid`/`FastPath` and improves
  structural filter recall +0.5-0.6pp, a real but modest, narrow effect;
  integrated relevance impact is not distinguishable from measurement
  noise in a single run.
- **P1-D** (physical advantage by query class) — **NEGATIVE RESULT**
  (P2-E13-E17, this document's central evidence): see below.
- **P1-E** (weighted whole-workload economics) — **NEGATIVE RESULT**,
  folded into P1-D's own final entry (P2-E17): traffic-weighted,
  commerce-native is ~2.3-3.0x *slower* than Solr on this real corpus,
  not 5-10x faster.
- **P1-F** (mature-system archaeology) — substantively already answered
  pre-dating this phase's north-star reorientation, via Issue #7's own
  research cycle (`docs/experiments/ISSUE7_LOG.md`): Havenask/Algolia/
  Solr-Lucene/commerce-search-API archaeology, cross-referenced against
  this project's own findings, feeding directly into Round 1's NARROW
  decision and this phase's P1-B/P1-C experiment design (P1-C's own
  motivation — franchise/media-property brand mismatches — traces
  directly to P2-E11's diagnostic, which in turn was informed by this
  archaeology's residual-opportunity framing).

## Architecture tested

Everything in `SCALE_UP_DECISION.md` plus Round 1's additions, plus this
phase's own layer (`crates/commerce-core/src/plan/`): a `LexicalDelegate`
trait (no lexical-engine dependency in `commerce_core` itself, mirroring
`control_plane::provider::ModelProvider`), three execution outcomes
(`FastPath`/`Hybrid`/`Punt`) chosen by `plan()` on `residual_lexical`
emptiness and structural-candidate selectivity, `execute_planned`
re-verifying every hard constraint against every returned hit regardless
of outcome (a delegate is trusted for ranking/recall, never for
correctness). This phase's real implementation of `LexicalDelegate` is an
embedded Tantivy index (`phase2-eval`'s `TantivyDelegate`), reusing P2-E01's
already-validated-equivalent-relevance configuration.

Also tested and fixed this phase, each with a RED-first regression test
(`docs/experiments/PHASE2_LOG.md` P2-E13-E16): an unconditional
per-candidate attribute merge in `execute_ranked` costing ~1078ms per
non-selective `FastPath` query for no benefit; a compiler defect ANDing
two independently-resolved, mutually-exclusive `Brand`/`Constraint::Enum`
constraints together (structural entities and attribute-level enums
alike); and a `commerce_core::plan` inefficiency (`delegate_oversample`
applied even when no constraint could ever reject a delegate hit, wasting
work on the single largest real-traffic class).

## Datasets / workloads

The full real evidence base Round 1 established, reused throughout this
phase: the real 1,215,854-product Amazon ESCI catalog and the real
22,458-query, human-judged corpus (`dataset_cache/export/{catalog,queries}.jsonl`,
gitignored — too large to version, but fingerprinted by `(size, mtime)`
in every `bench_harness::RunManifest`), plus a fresh, same-environment,
locally-running Apache Solr 9.10.1 instance re-indexed with the identical
catalog (`dataset_cache/solr/solr-9.10.1`). This phase adds a stable
9-class query-structural-shape taxonomy (`round1_eval::query_taxonomy::QueryClass9`)
distinct from Round 1's resolution-state taxonomy, and a statistical-rigor
crate (`crates/bench-harness`: repeated measurement, warmup separation,
round-robin method interleaving, bootstrap confidence intervals,
manifest/CSV artifact capture) built specifically to meet this phase's own
paper-grade evidence bar.

**A named, real dataset limitation, not an engineering gap**: the Amazon
ESCI export carries no structured `product_type`/`category`/price data at
all (`round1_eval::catalog`'s documented `UNKNOWN_PRODUCT_TYPE`/
`UNKNOWN_CATEGORY` sentinel, `UNKNOWN CATEGORY` carried since R1-E01). The
only real, per-product-diverse structural entity in this real catalog is
brand. This is why `selective_multi_attribute_structural` (Issue #6's own
named query class, requiring 2+ structural entity constraints) is empty
on real data once P2-E14's compiler fix stopped it from being populated by
a bug (two independently-matched *brands* ANDed together) rather than
genuine multi-entity structure. A materially different real catalog with
genuine structured multi-entity data remains untested and could change
this specific finding — though not the broader Hybrid/Punt result below,
which does not depend on `product_type`/`category` at all.

## Measured results

Full class-by-class table, the adversarial review's seven-question
checklist, and raw artifacts: `docs/experiments/PHASE2_LOG.md` P2-E17,
`docs/research/artifacts/p1d_run5/`.

| Question (Issue #6 priority) | Evidence | Result |
|---|---|---|
| Semantic coverage (P1-A) | P1-D's 9-class breakdown | 0.82% pure structural, 14.73% structural+residual, 84.44% no useful structure, on real traffic |
| Enforcement policy (P1-B) | P2-E11 | REVISE — mechanism sound, alias/spelling variance is a minor (~1-5%) contributor to the real recall gap |
| Predictive prefill (P1-C) | P2-E12 | NARROW — real but modest (0.56% of traffic route-changed), integrated relevance not distinguishable from noise |
| Physical advantage by class (P1-D) | P2-E17 | `structural_exact_entity`/`variant_scoped_structural`: 87x/105x faster (0.81% of traffic, real relevance cost); six classes totaling 99.19% of traffic: 2.7x-3.8x *slower* |
| Weighted whole-workload economics (P1-E) | P2-E17 | commerce-native ~2.3-3.0x SLOWER than Solr (median-/mean-weighted), not 5-10x faster |
| Mature-system archaeology (P1-F) | Issue #7 (pre-phase) | No generic-IR or consumer-search gap identified that this phase's own findings don't already explain; informed P1-B/P1-C's design |

**The physical-advantage wins are not clean, even where they exist.** A
dedicated relevance-guardrail audit (part of the P2-E17 adversarial
review) traced the mechanism to source: `execute_ranked` only computes a
nonzero score when `query.preferences` is non-empty; the shipping baseline
lexicon (`compile_lexicon`) never populates it for any real query in this
benchmark. `FastPath`'s "top 10" is therefore an arbitrary, ingestion-
order-`ProductId`-sorted subset, not a relevance-ranked one — costing
`structural_exact_entity` -31.5% NDCG@10 and -58% MRR relative to Solr.
This is a real, fixable engineering gap (a default ranking signal could be
added without touching routing), not evidence that structural exact-match
retrieval can never be both fast and relevant — but as measured, it means
even the narrow win is a tradeoff, not a pure one.

## Failed experiments (preserved, not erased)

Five real bugs were found and fixed this phase, each investigated to root
cause with real data before being treated as architecture-level evidence,
per this project's own "if a benchmark methodology problem is discovered,
fix the methodology first" rule — including once, in P2-E16, when the
flawed measurement happened to make the *baseline* (Solr) look
artificially fast, not commerce-native. All five are recorded in full in
`docs/experiments/PHASE2_LOG.md` P2-E13-E16, not smoothed over:

1. `execute_ranked`'s unconditional full-catalog attribute merge (~1078ms/query wasted).
2. Solr's structural filtering against a completely unpopulated `brand_lower` schema field.
3. The compiler ANDing two independently-resolved `Brand` constraints together.
4. The same defect generalized to attribute-level `Constraint::Enum`.
5. The P1-D harness's own *latency* measurement of Solr silently hitting an
   unpopulated `_text_` default field (a guaranteed-zero-hit lookup)
   instead of the real query the correctness loop already used — found by
   a 4-agent adversarial-review workflow, not by inline debugging, and
   live-verified against the running Solr core before being trusted.

**The negative result survived, and narrowed, after fixing the one bug
that favored the baseline** (~4.05x -> ~3.01x mean-weighted slower once
Solr's latency measurement was corrected to do real work) — the opposite
of what would happen if the finding were an artifact of an unfair
benchmark. This is the strongest evidence that P1-D's STOP-leaning verdict
is not itself a benchmark artifact.

Two `NARROW`/`REVISE` (not `STOP`) verdicts from earlier in this phase are
independent of P1-D's result and are not re-litigated or overturned here:
P1-B's enforcement-mechanism work and P1-C's predictive-prefill work both
concern *semantic interpretation quality* on the `Hybrid`/`Punt` path,
which P1-D independently found to be the traffic-dominant (99%+),
economically-decisive path regardless of semantic-layer quality —
improving classification further cannot, by itself, close a gap currently
dominated by execution-path overhead, not by classification accuracy.

## Unresolved risks

1. **Latency samples are 20 unique queries per class, repeated 30 times
   each — not 30 independent draws from the class's real population** (up
   to 8,274 real queries in `lexical_first`), and the correctness/latency
   query samples are a deterministic "smallest-N-by-native-ESCI-query-id"
   selection, not random or stratified (found by the P2-E17 adversarial
   review). This covers as little as ~2.4-6% of the four classes that
   carry >99% of real traffic. The bootstrap CIs are tighter-looking than
   the true query-to-query variance across each class's full population
   would support; the *direction* of every finding is corroborated by
   Solr's own separately-measured, larger (though still non-random)
   200-query correctness-loop `avg_server_qtime`, but the *exact*
   traffic-weighted multiplier (~2.3-3.0x) should be read as a defensible
   estimate, not a tight confidence interval, absent a random/stratified,
   larger-N re-run.
2. **`plan::plan`'s `FastPath` route has no selectivity safeguard**,
   unlike `Hybrid`/`Punt` (which explicitly gate on
   `selectivity <= policy.selectivity_threshold`). `range_plus_structural`
   (n=2, 0.01% of traffic) resolved to a large, non-selective candidate
   set and showed commerce-native's single worst P1-D latency (mean
   ~30ms, slower than both baselines) — a real, source-verified signal
   that the two winning classes' dramatic advantage (87-105x) may be a
   property of this corpus's favorable samples for those two classes
   specifically, not a general architectural guarantee that every
   `FastPath` query is cheap.
3. **The reference `TantivyDelegate` used throughout this phase turns
   `Hybrid`'s `restrict_to` into a per-query Lucene `TermSetQuery`** over
   the full narrowed candidate set (up to ~60K terms for
   `structural_plus_lexical_residual`), which the P2-E17 review found
   undermines much of `Hybrid`'s own "narrow first is cheap" cost
   advantage. This is a limitation of this phase's reference delegate
   implementation, not of `commerce_core::plan` itself (the trait boundary
   is respected) — a production-grade, bitmap/doc-id-set-based delegate
   restriction remains untested and could narrow the gap for the 14.7% of
   traffic in `structural_plus_lexical_residual`/`structural_plus_semantic_residual`.
4. **No real catalog with genuine structured multi-entity data has been
   tested.** This real catalog's only structural entity is brand; a
   different real dataset with real `product_type`/`category`/price
   fields could populate `selective_multi_attribute_structural` and
   `range_plus_structural` meaningfully and shift the traffic-class
   distribution toward `FastPath`-eligible shapes. Untested, not assumed
   either way.
5. **`FastPath` has zero ranking signal for any real query in this
   benchmark** (above) — a real, disqualifying-for-a-clean-win
   relevance cost on the two classes that do show a physical advantage,
   not yet fixed.
6. Every unresolved risk `SCALE_UP_DECISION.md` and `ROUND1_DECISION_TREE.md`
   already named and Round 1 did not close remains open where this phase
   didn't touch it (no incremental index update path; no concurrent-load
   benchmarking; single shared/virtualized 4-vCPU environment, no
   cross-hardware validation).

## What would be built next if scaling up (conditional -- see decision)

This phase's own evidence does **not** currently justify building any of
these; they are named as the concrete, falsifiable path that *would*
justify revisiting the STOP verdict, per item 7 (below) and the P2-E17
adversarial-review checklist's own "what would falsify this" answer:

1. **A bitmap/doc-id-set-based `Hybrid` delegate-restriction mechanism**,
   replacing the reference `TantivyDelegate`'s `TermSetQuery` approach, to
   test whether `Hybrid`'s intended narrowing benefit is realizable at all
   for the 14.7% of real traffic currently paying its cost without its
   benefit.
2. **A default ranking signal for `FastPath`** (even a simple catalog-
   derived proxy — popularity, recency, or a real `Preference`-emitting
   lexicon), so the two classes with a genuine physical advantage stop
   trading it for a measurable relevance loss.
3. **A `FastPath` selectivity safeguard**, mirroring `Hybrid`/`Punt`'s
   existing gate, so a non-selective structural-only query degrades
   gracefully instead of fully materializing and sorting a large candidate
   set (`range_plus_structural`'s 18x-slower result).
4. **A random/stratified, larger-N re-run of the latency sub-experiment**
   per class, to tighten the traffic-weighted multiplier's confidence
   interval beyond "defensible estimate."
5. **A real catalog with genuine structured multi-entity data** (real
   `product_type`/`category`/price, unlike this Amazon ESCI export), to
   test whether `selective_multi_attribute_structural`/`range_plus_structural`
   materially change the weighted-economics picture on a dataset where
   they are not empty/negligible.

## What should explicitly not be built yet

- **Any further semantic-interpretation-layer work** (a fourth
  canonicalization strategy, more prefill confidence tiers, additional
  enforcement policies) as a way to chase the 5-10x thesis. P1-D found the
  traffic-dominant path's cost is currently dominated by execution-path
  overhead, not classification accuracy — improving P1-B/P1-C further
  cannot by itself close this gap.
- **Distributed/sharded serving, cluster coordination, multi-tenancy,
  HA**, exactly as `SCALE_UP_DECISION.md` already concluded and Round 1
  did not revisit — nothing in this phase changes that.
- **A production LLM-backed `ModelProvider`** or any model call in the
  default query hot path, per CLAUDE.md's hard rule, unaffected by this
  phase's results either way.
- **Scaling the current architecture to more traffic/infrastructure**
  before any of the four falsifying conditions above are tested. Building
  more of an architecture whose own traffic-weighted economics measure
  worse than the mature baseline it was meant to beat would be exactly
  the premature scale-up CLAUDE.md's engineering discipline warns against.

## What this decision does and does not claim

**Claims**: on the real 1,215,854-product Amazon ESCI catalog and real
22,458-query human-judged corpus, measured against a fresh, same-
environment, real Apache Solr 9.10.1 instance and a validated-equivalent-
relevance embedded Tantivy baseline, with five real bugs found and fixed
(including one that would have made the baseline look artificially fast,
not commerce-native) and a 4-agent adversarial review answering every one
of Issue #6's required verification questions: commerce-native's
structural/hybrid architecture shows a real, large (87-105x), but
relevance-costly physical advantage on under 1% of real traffic, and a
real, consistent (2.7-3.8x) *disadvantage* on the 99%+ of real traffic
that reaches its `Hybrid`/`Punt` execution path. The traffic-weighted
whole-workload result is ~2.3-3.0x slower than the mature baseline, not
5-10x faster. This negative result survived and narrowed, rather than
disappeared, after correcting the one severe measurement bug found —
evidence it is not itself a benchmark artifact.

**Does not claim**: that structural exact-match retrieval can never be
both fast and relevant (a fixable engineering gap, not tested to
resolution); that a different real catalog with genuine structured
multi-entity data would show the same result (untested); that a
production-grade `Hybrid` delegate-restriction mechanism couldn't close
some of the measured gap (untested); that P1-B's/P1-C's own REVISE/NARROW
verdicts are overturned (they are independent, semantic-layer findings);
or that no commerce-vertical specialization is ever worthwhile — only
that *this* architecture, measured *this* way, on *this* real evidence,
does not support the specific 5-10x whole-engine QPS/$ thesis Issue #6 set
out to test.
