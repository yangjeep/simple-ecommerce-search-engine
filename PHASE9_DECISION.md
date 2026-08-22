# Phase 9 Decision (Issue #34) — P9-E00 through P9-E02, interim

**Decision: REVISE.** Not a terminal Phase 9 decision — Issue #34's full
scope (Sections A-H: hit-rate frontier, canonicalization reuse, learned
semantic implications, final falsification) is much larger than the three
experiments recorded here. This document covers the first concrete
increment the user approved: fix Phase 2's two disclosed-but-unfixed
defects, then re-run the physical-advantage-by-query-class measurement on
a real catalog with genuine multi-entity structural data (WANDS, not
ESCI). That increment is now complete, with a real, reproduced, mixed
result: both defects are fixed and independently confirmed real (P9-E00,
P9-E01), but the re-run they were prerequisites for (P9-E02) found that
Phase 2's STOP-leaning relevance finding **replicates on WANDS**, not
reverses — commerce-native structural execution (`FastPath`/`Hybrid`)
still trails a fresh Solr baseline on relevance by ~-20.7% relative NDCG@10,
even with both fixes in place. It clears the latency bar decisively
(2.25x-2.90x), not the relevance bar.

## What this covers

- **P9-E00**: fixed FastPath's missing ranking signal (`execute_ranked`
  returned `0.0` for every hit whenever no curated `Preference` existed —
  true of every real query to date). CONFIRMED safe and real via 5 new
  unit tests; no regression across the workspace.
- **P9-E01**: fixed Hybrid's `TermSetQuery`-based delegate restriction
  (undermined its own narrow-first-is-cheap cost advantage per P2-E17)
  with a bitmap-based mechanism. CONFIRMED, ~11.6-12.1x faster than the
  reference pattern across 6 runs, identical document set every time.
- **P9-E02**: wired both fixes into a real, integrated
  `commerce_core::plan::execute_planned` (FastPath/Hybrid/Punt) run
  against the real WANDS catalog (42,994 products, 1,623 categories, 860
  product types, 480 real judged queries), against a fresh, same-run Solr
  baseline. REVISE.

See `docs/experiments/PHASE9_LOG.md` for each experiment's full
hypothesis, pre-registered decision criteria, implementation, and result.

## The central finding, stated precisely

Traffic-weighted overall: native NDCG@10 = 0.4951, Solr = 0.4740 (+4.46%
relative — a KEEP-looking headline). **This headline is not a structural-
retrieval finding and must not be read as one.** `execute_planned` routes
330/480 queries (68.75%) to `Punt` (no structural constraint recognized at
all), where the delegate runs unrestricted — "native" there is embedded
Tantivy's own plain-text relevance versus remote Solr's edismax relevance
on the same text, an engine-choice question with nothing to do with
commerce-native structural execution. Splitting the traffic-weighted
number by routing outcome, the way this project's "traffic-weighted
economics never hides a losing class" discipline requires, shows the
+4.46% headline is produced entirely by that 68.75% Punt-routed majority
(native 0.666 vs Solr 0.621 there). On the 150/480 queries (31.25%) that
actually reach `FastPath`/`Hybrid` — the only traffic Issue #34's
structural-execution question is actually about — native NDCG@10 (0.1192-
0.1194, stable across 6 runs) trails Solr's 0.1505 by a reproduced
**-20.7% to -20.8% relative gap**.

This is exactly the failure mode Phase 2's own P1-D/P1-E found on ESCI
(`structural_exact_entity` at -31.5% NDCG@10 pre-fix), now measured again
on a catalog with genuinely richer structural entities (`Category` +
`ProductType`, both real and populated, versus ESCI's Brand-only), with
both of Phase 2's own named prerequisite defects fixed first. The gap
narrowed (from -31.5% pre-fix on ESCI to -20.7% post-fix on WANDS) but did
not close, and did not reverse.

## Why the fixes didn't close the gap — evidence, not speculation

1. **`variant_scoped_structural` (n=10) carries the worst loss** (native
   0.0396 vs Solr 0.2995). Direct inspection of 3 sample queries found a
   real ranking-quality problem, not a retrieval-coverage one: for
   "peacock," native's structural candidate set does contain the real
   Exact match, but ranks two unjudged items above it; for "industrial,"
   native's entire top-3 is unjudged while Solr's is all Exact. P9-E00's
   intrinsic text-overlap default signal is a real, working ranking
   signal (proven by its own unit tests and by `structural_plus_lexical_residual`
   landing within noise of Solr), but it is evidently not as good at
   surfacing the single best match among a same-scoring candidate set as
   Solr's BM25 is — especially when the query token itself is a
   descriptive/color/material word ("peacock," "industrial") that may not
   even appear in the winning product's own title/description text, only
   in an attribute value the text-overlap signal does not weight.
2. **`structural_plus_lexical_residual` (n=103, the class P9-E01's fix
   most directly targets) is within noise of Solr** (native 0.1585-0.1591
   vs Solr's 0.1619) — evidence the bitmap-delegate fix is not itself the
   remaining bottleneck; the gap concentrates in classes with little or
   no delegate involvement.
3. **WANDS's richer schema did not translate into more pure-structural
   traffic.** Zero queries classified as `structural_exact_entity`,
   `selective_multi_attribute_structural`, or `range_plus_structural`.
   `compile_lexicon`'s exact-lowercased-phrase keying (inherited unchanged
   from ESCI's single-token brand-name design) rarely matches a real
   shopper query verbatim against WANDS's compound/hierarchical
   `category_leaf`/`product_class` vocabulary ("Massage Chairs";
   "Furniture / Bedroom Furniture / Beds & Headboards / Beds / Twin
   Beds") — shoppers type "chair," not the taxonomy's own compound
   strings. This is a real limitation of the lexicon-*compilation* step
   applied to a richer schema, separate from whether the schema itself is
   rich (it is) or whether execution-once-routed is correct (it is,
   per every existing correctness test).

## Decision discipline applied

Per this project's evidence discipline, the traffic-weighted headline was
not reported without the per-class and per-routing breakdown that reveals
the losing majority-relevant slice — the exact failure mode the
"traffic-weighted economics never hides a losing class" rule exists to
prevent, and the exact failure mode this pass's own first draft would
have committed had the routing-split diagnostic not been added before
trusting the result. Both new defect fixes (P9-E00, P9-E01) were verified
independently real (unit tests, a dedicated microbenchmark with its own
pre-registered bar) *before* being trusted as inputs to this re-run —
neither fix's own correctness is in question; what's in question, and
answered negatively, is whether fixing both was *sufficient* to reverse
Phase 2's relevance verdict.

## What this does NOT establish

- That commerce-native structural execution can never win on relevance —
  only that, on this catalog, with this lexicon-compilation approach and
  this ranking signal, it does not yet.
- That the lexicon-compilation gap (compound-vocabulary mismatch) is
  unfixable — it was diagnosed, not attempted to fix, in this pass.
- That a richer ranking signal (e.g. a real Preference-emitting lexicon,
  or a smarter text-relevance weighting than P9-E00's flat title/attribute
  weights) would not close more of the gap — untested this pass.
- Anything about Punt-routed (68.75% of traffic) economics as a
  structural-retrieval claim — that comparison is real and reproducible,
  but is an embedded-engine-choice finding, not a commerce-native-thesis
  finding, and is reported separately for exactly that reason.

## What would be built next if continuing this thread

1. **A lexicon-compilation fix for compound/hierarchical vocabulary** —
   the single highest-leverage next step per the diagnostic above: make
   `compile_lexicon` (or a variant of it) resolve a query token/phrase
   against *substrings* or *leaf segments* of a compound category path,
   not only an exact full-phrase match, so WANDS's genuinely richer
   schema can actually populate `structural_exact_entity`/
   `selective_multi_attribute_structural` traffic instead of falling
   through to `Punt`/`lexical_first`.
2. **A real ranking-signal improvement for `variant_scoped_structural`**
   specifically — its loss is concentrated and its cause (ranking, not
   retrieval) is now diagnosed; a numeric-attribute-aware or catalog-
   popularity-aware ranking signal (this project has generic infra for
   this — `rating_count`/`average_rating`/`review_count` are already
   ingested as `Numeric` attributes) is a concrete, scoped next
   experiment.
3. **A `min_enum_frequency` sweep on WANDS** (this pass fixed it at `1`),
   matching Phase 2's own {1,5,25,100} sweep discipline, to check whether
   raw-value noise filtering changes the picture materially.
4. **Repeated-measurement latency rigor** (`bench-harness::measured_repeat`
   per query, bootstrap CIs) — this pass's latency numbers are real and
   reproduced across 6 runs, but single-shot per query, not to this
   project's usual statistical-rigor bar for a number that would be relied
   on for a final economics claim.
5. **The much larger remaining scope of Issue #34** (Sections B-H: hit-
   rate frontier, canonicalization reuse from #9, learned semantic
   implications, final falsification across the full required system
   matrix) — this document closes only the specific increment the user
   selected, not the epic.

## What should explicitly not be built yet

- A bespoke, hand-authored WANDS-specific lexicon compiler — the generic
  `commerce_core::cold_start::profile` infrastructure already works for
  WANDS as-is (confirmed, not assumed, before this pass); the actual gap
  is in phrase-matching granularity, not in needing vertical-specific code
  — directly relevant evidence for Issue #35's own generalization question.
- Fabricated price data for WANDS to manufacture `range_plus_structural`
  traffic — WANDS genuinely has none; this project does not manufacture
  workload shape to flatter a hypothesis.
- A full Issue #9-style canonicalization pass for WANDS's own vocabulary
  before the more basic phrase-matching-granularity gap above is
  addressed — the diagnosed root cause is structural (compound-phrase
  matching), not vocabulary-normalization, so canonicalization would not
  be the highest-value next step.
