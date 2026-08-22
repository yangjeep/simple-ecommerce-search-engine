# Phase 9 Decision (Issue #34) — P9-E00 through P9-E04, interim

**Decision: REVISE, with the root cause now precisely localized.** Not a
terminal Phase 9 decision — Issue #34's full scope (Sections A-H:
hit-rate frontier, canonicalization reuse, learned semantic implications,
final falsification) is much larger than the five experiments recorded
here. This document covers the user-approved increment (fix Phase 2's two
disclosed defects, re-run on WANDS: P9-E00-E02) plus a mandated follow-up
(separate and test the ranking-quality, semantic/lexicon-compilation, and
execution-advantage hypotheses behind that REVISE: P9-E03-E04). Both
disclosed defects are fixed and independently confirmed real. The re-run
they enabled found that Phase 2's STOP-leaning relevance finding
**replicates on WANDS**. The follow-up round then **falsified two of the
three candidate explanations** (ranking quality; an intrinsic native
execution-speed advantage) and **precisely localized the real cause** to
a specific, well-evidenced `compile_lexicon`/`compile()` resolution-
priority defect: a coincidental attribute-level match (e.g. a color word)
frequently wins over failing to find the real entity, producing a
confident but badly wrong hard structural constraint.

## What this covers

- **P9-E00**: fixed FastPath's missing ranking signal. CONFIRMED safe and
  real via 5 new unit tests; no regression across the workspace.
- **P9-E01**: fixed Hybrid's `TermSetQuery`-based delegate restriction
  with a bitmap-based mechanism. CONFIRMED, ~11.6-12.1x faster than the
  reference pattern across 6 runs, identical document set every time.
- **P9-E02**: wired both fixes into a real, integrated
  `commerce_core::plan::execute_planned` run against the real WANDS
  catalog, against a fresh, same-run Solr baseline. **REVISE** —
  structural-routed traffic (31.25%) trails Solr's relevance by a
  reproduced -20.7% to -20.8% relative NDCG@10 gap.
- **P9-E03**: isolates the semantic/lexicon-compilation hypothesis behind
  that gap via a pure measurement pass (no production code changed).
  **H2c (plural/singular) CONFIRMED material (25.8%); H2a (pipe-split)
  and H2b (category leaf segment) FALSIFIED** — including a specific
  prior speculation of mine in this same document (see the correction
  below).
- **P9-E04**: isolates ranking quality (H1) and execution-speed advantage
  under identical candidate-set control (H3), sharing one harness.
  **Both FALSIFIED.** A follow-on diagnostic within the same experiment
  then localizes the real cause: candidate-set recall of real
  judged-relevant documents is only 8.41% on average, and queries with a
  real entity constraint recall Exact-labeled ground truth 6.6x better
  (47.6% vs. 7.2%) than queries that resolve to only an attribute-level
  constraint — confirmed both statistically and via direct qualitative
  inspection of the actual resolved constraints.

See `docs/experiments/PHASE9_LOG.md` for each experiment's full
hypothesis, pre-registered decision criteria, implementation, and result.

## Correction to this document's own prior speculation, not erased

The first version of this document (P9-E02-only) offered two explanations
for the relevance gap that later experiments, run specifically to test
them, falsified:

1. **"`variant_scoped_structural`'s loss is a ranking-quality problem"** —
   based on qualitative inspection of 3 sample queries where native
   appeared to rank unjudged items above a true match. P9-E04's H1 tested
   this directly (same candidate set, both engines rank it) and found
   **native's ranking signal is NOT materially worse than Solr's** (-1.05%
   relative NDCG@10, noise-level). The 3 samples' apparent ranking failure
   was actually a *retrieval* failure — the candidate SETS differed
   between engines, not the ranking of a shared set. **FALSIFIED, corrected
   here rather than left standing.**
2. **"WANDS's compound/hierarchical `category_leaf` vocabulary
   (e.g. 'Furniture / Bedroom Furniture / Beds & Headboards / Beds / Twin
   Beds') is why richer structural entities didn't produce more
   pure-structural traffic"** — P9-E03's H2b tested this directly (does
   the category leaf's last path segment alone recover a real match for
   currently-zero-constraint queries?) and found it recovers only **1.0%**
   of them, far below the 10% materiality bar. **FALSIFIED.** The real,
   confirmed drivers are different: P9-E03's H2c (plural/singular
   mismatch, 25.8% of zero-constraint queries) for the *Punt-routing*
   question, and P9-E04's resolution-priority defect (below) for the
   *wrong-routing* question — a different, more specific, and more
   severe failure mode than "vocabulary is too compound."

Recorded here per this project's "record failed experiments, do not erase
evidence" discipline — both were reasonable hypotheses from the evidence
available at the time, both were then subjected to a real falsifiable
test, and both did not survive it.

## The central finding, stated precisely (P9-E02, still valid)

Traffic-weighted overall: native NDCG@10 = 0.4951, Solr = 0.4740 (+4.46%
relative — a KEEP-looking headline). **This headline is not a structural-
retrieval finding.** `execute_planned` routes 330/480 queries (68.75%) to
`Punt`, where the delegate runs unrestricted — "native" there is embedded
Tantivy's own plain-text relevance versus remote Solr's edismax relevance,
an engine-choice question unrelated to commerce-native structural
execution. On the 150/480 queries (31.25%) that actually reach
`FastPath`/`Hybrid`, native NDCG@10 (0.1192-0.1194, stable across 6 runs)
trails Solr's 0.1505 by a reproduced **-20.7% to -20.8% relative gap**.
This replicates Phase 2's own ESCI-catalog STOP-leaning relevance finding
(`structural_exact_entity` at -31.5% pre-fix) on a catalog with genuinely
richer structural entities, with both of Phase 2's own named prerequisite
defects fixed first. The gap narrowed (-31.5% → -20.7%) but did not close.

## The precise root cause (P9-E03 + P9-E04, new this pass)

**Ranking is not the problem (H1 FALSIFIED).** Given the identical
candidate set, native's ranking signal and Solr's BM25 perform
statistically the same (-1.05% relative NDCG@10). Whatever native's
candidate set contains, it orders about as well as Solr would.

**Native's apparent latency advantage is substantially a comparison-scope
artifact (H3 FALSIFIED).** Once both engines operate over the identical
candidate set, native's speed advantage evaporates (0.71x-1.14x across 6
warmed runs, well under the project's 2x bar) — P9-E02's end-to-end
2.25x-2.90x figure reflected Solr doing more expensive, broader,
unrestricted work, not a fundamental native-execution-model advantage.
(P9-E01's own bitmap-vs-TermSetQuery finding is unaffected by this — that
was already a fair, isolated comparison of two *restriction mechanisms*
on the same corpus; P9-E02's broader end-to-end number was answering a
different, less isolated question.)

**The real cause: native's structural candidate set has catastrophically
low recall of real relevant documents, and this concentrates specifically
in queries where no real entity constraint was found.** Prompted directly
by H1's falsification (if ranking a shared pool isn't the problem, what's
in the pool must be), a follow-on diagnostic found native's candidate set
contains, on average, only **8.41%** of a query's real judged-relevant
documents (0/136 queries reach 100% recall) — similarly low for Exact
(11.52%) and Partial (8.03%) grades, which itself rules out "WANDS's
graded relevance spans categories no hard constraint could capture" as
the primary explanation (if that were the whole story, Exact recall
should be much higher than Partial; it is not).

An aggregate test (not an anecdote) localizes this precisely: queries
whose compiled constraints include a real `ProductType`/`Category` entity
(n=11) average **47.6%** Exact recall; queries resolving to only an
attribute-level constraint with no entity at all (n=92, the large
majority) average just **7.2%** — a 6.6x difference. Direct inspection of
the actual resolved constraints confirms the mechanism, not just the
statistic: "smart coffee table" resolves to `Attribute(Enum{color=coffee})`
— "coffee" coincidentally matches a real catalog color value — instead of
the product-type phrase, because "Coffee & Cocktail Tables" (the real
`product_class`) never appears verbatim in the query. The same pattern
repeats: "acrylic clear chair" → `color=clear`; "chrome bathroom 4 light
vanity light" → `color=chrome`; "coffee table fire pit" → `color=coffee`
again. `compile()`'s longest-window-first phrase scan, on failing to find
the (rarely-literal) entity phrase, falls through to a shorter window
that happens to coincide with an unrelated attribute value — producing a
confident, hard, but badly wrong structural constraint. This is *worse*
for relevance than `Punt` would have been: a wrong hard filter excludes
nearly every genuinely relevant product, whereas `Punt` at minimum leaves
the full free-text query visible to a lexical delegate.

`commerce_core`'s own correctness contract is not violated — the
constraint IS satisfied by every returned hit, verified by every existing
test — the constraint itself is simply, frequently, the wrong one to have
resolved. This is a `compile_lexicon`/lexicon-*compilation* defect, fully
separated now from ranking (fine) and from any claimed intrinsic
execution-speed advantage (not fully real once isolated).

## Decision discipline applied

The traffic-weighted P9-E02 headline was not reported without the
per-class/per-routing breakdown that reveals the losing majority-relevant
slice. Both P9-E00/P9-E01 fixes were verified independently real *before*
being trusted as inputs to the P9-E02 re-run. P9-E04's own first,
unwarmed run produced a misleading H3 result (2.25x, appearing to clear
the bar) that a second run contradicted (0.98x) — caught before trusting
either number, by adding the same warmup discipline P9-E02 already
established, not by picking whichever number looked better. The H1
follow-on diagnostic (candidate-set recall) was not planned in advance —
it was added specifically *because* H1's falsification demanded an
explanation for where the end-to-end gap actually lives, and its own
Exact-vs-Partial split was added specifically to test (and rule out) an
alternative explanation before accepting the entity-constraint one.

## What this does NOT establish

- That commerce-native structural execution can never win on relevance —
  only that a specific, now-precisely-identified lexicon-compilation
  defect is currently responsible for most of the observed gap.
- That fixing the resolution-priority defect would close the gap
  entirely — untested; 72.0% of currently-zero-constraint queries (a
  different but related population) remain unrecoverable under every
  literal-matching relaxation P9-E03 tested, so a real ceiling likely
  remains even after a principled fix.
- That native's execution mechanism is never faster than Solr's — P9-E01
  already showed a real, isolated, 11.6-12.1x mechanism-level advantage
  for delegate restriction specifically; H3's falsification is about the
  *end-to-end* claim once relevance-affecting scope differences are
  removed, not about P9-E01's narrower claim.
- Anything about Punt-routed (68.75% of traffic) economics as a
  structural-retrieval claim — real and reproducible, but an
  embedded-engine-choice finding, not a commerce-native-thesis finding.

## What would be built next if continuing this thread

1. **Fix the resolution-priority defect** — the single highest-leverage,
   now precisely-scoped next step: when `compile()`'s phrase scan finds
   only an attribute-level match and no entity (`ProductType`/`Category`)
   constraint, prefer treating the query as more `Punt`-like (soft
   preference or residual text) rather than accepting the attribute match
   as an authoritative hard filter. Needs its own falsifiable design (what
   exactly should happen instead — demote to `Preference`? leave as
   residual? something else?) and before/after re-measurement against
   P9-E02's own numbers, not implemented speculatively in this pass.
2. **A scoped H2c (plural/singular) implementation** in
   `compile_lexicon`/`SemanticLexicon`, with its own falsifiable design
   (e.g. scoped to entity-family names only, to avoid new false-positive
   matches CLAUDE.md's "cross-variant false matches are bugs" rule would
   flag) and before/after re-measurement of both the Punt-routing share
   (P9-E03) and end-to-end relevance (P9-E02).
3. **A `min_enum_frequency` sweep on WANDS** (fixed at `1` throughout this
   pass), matching Phase 2's own {1,5,25,100} sweep discipline.
4. **Repeated-measurement latency rigor** (`bench-harness::measured_repeat`
   per query, bootstrap CIs) for any future economics claim relying on a
   specific latency number, given how sensitive the raw figures have
   already proven to warmup state across P9-E02 and P9-E04.
5. **The much larger remaining scope of Issue #34** (Sections B-H) — this
   document closes only the specific increment the user selected plus its
   mandated follow-up, not the epic.

## What should explicitly not be built yet

- A bespoke, hand-authored WANDS-specific lexicon compiler — the generic
  `commerce_core::cold_start::profile` infrastructure already works for
  WANDS as-is; the actual gap is a resolution-priority defect plus a
  literal-matching limitation, not a need for vertical-specific code —
  directly relevant evidence for Issue #35's own generalization question.
- Fabricated price data for WANDS to manufacture `range_plus_structural`
  traffic — WANDS genuinely has none.
- A full Issue #9-style canonicalization pass for WANDS's own vocabulary
  before the more basic, now precisely-diagnosed resolution-priority and
  pluralization gaps above are addressed.
- Any "attribute matches always lose to missing entity matches" hack
  patched directly into `compile()`'s resolution algorithm without a
  proper falsifiable design first — this is a real defect, but a rushed,
  undermotivated heuristic risks a new class of false negatives (queries
  where an attribute constraint genuinely IS the shopper's whole intent,
  with no entity phrase present at all) that this pass did not measure.
