# Round 1 Decision Tree

**Decision: NARROW THE PRODUCT.**

This document is required by Issue #5 and evaluates the architectural
thesis using the real, independent evidence accumulated in
`docs/experiments/ROUND1_LOG.md` (R1-E01 through R1-E07), on top of
Phase 0's fixture-only evidence (`SCALE_UP_DECISION.md`). Writing this
document is not the end of the work: the selected branch is executed
starting immediately below "Execution," per Issue #5's explicit
"writing the decision tree is not completion" instruction.

## Recap: what changed between Phase 0 and Round 1

Phase 0 (`SCALE_UP_DECISION.md`) reached PROCEED on hand-authored
fixtures (tens of products, a 20-query hand-built set) with nine
explicitly named unresolved risks — most centrally, "no external
baseline anywhere" and "all fixtures are small and hand-authored."
Round 1 closed every one of those specific risks with real, independent
evidence: a real 1,215,854-product catalog and 22,458-query, human-judged
corpus (Amazon ESCI), a real external baseline (Apache Solr 9.10.1 /
Lucene 9.12.3), adversarial correctness and physical-workload stress
tests, and a control-plane adversarial-provider test Phase 0 never ran.
The result is not a confirmation of Phase 0's numbers at larger scale —
it is a materially different picture, summarized dimension-by-dimension
below.

## Dimension-by-dimension evidence

### 1. Semantic usefulness

**Weak, and the headline number is misleading.** R1-E02 measured a 55.4%
Semantic FIB hit rate on the real query corpus — comparable to Phase 0's
55-60% fixture number — but the diagnostic trace showed this is
substantially an artifact: naive cold-start lexicon construction turns
essentially every distinct real brand/color string (206,227 + 169,085
values) into a trusted lexicon entry with zero validation, so queries
like `"#2 pencils"` compile to `structural_only` because "#2" and
"Pencils" both happen to appear as `color` field values somewhere in a
1.2M-product catalog of real sellers' data-entry noise — not because the
compiler understood shopper intent. Ambiguity rate is 38.4% (vs. Phase
0's single deliberately-planted collision). When the compiler does claim
resolution, filter recall against real Exact-labeled relevant products is
**5.0%** (R1-E02) — a hard structural filter built this way discards 95%
of what a human judge called an exact match. R1-E02b tested and
**rejected** the obvious fix (OR-within-attribute aggregation, 5.0% ->
6.0%, nowhere near sufficient) and traced the dominant cause to
extraction quality, not aggregation logic: cold start has no
canonicalization/validation stage between "profile the catalog" and
"trust every distinct value as a lexicon entry." R1-E03 additionally
found the compiler **actively misinterprets** 3 of 10 brief-specified
adversarial queries (disjunction silently dropped, negation silently
inverted, no scope-sufficiency check for context-poor queries like
"black size 9") — not missing coverage, but confidently wrong output.

**Does not support**: "deterministic structural/hybrid coverage is
materially useful on realistic queries" as currently built. **Does
support**: the underlying idea (typed, ambiguity-preserving structural
constraints) is sound *when fed validated vocabulary* — Gate 1's
variant-safety guarantee held throughout Round 1 with zero cross-variant
false matches, and R1-E03's disjunction/negation bugs are compiler-syntax
gaps, not evidence the typed-constraint model itself is wrong.

### 2. Physical advantage

**Real but strictly conditional, and only within a narrow workload
class.** A genuinely selective structural filter (`brand=Nike` alone,
0.5% of the real 1.2M-product catalog) executes in **12.4us** p50 —
consistent with Phase 0's scale-growing advantage claim, now confirmed at
12x Phase 0's largest tier. But R1-E05 found this advantage is workload-
class-conditional, not general: a `Text` constraint with no selective
structural predicate to narrow first costs **961ms** p50 (~36,700x the
selective baseline) because `indexed_candidates` falls back to the full
catalog and narrow-then-verify degenerates to exactly the linear scan
Gate 3 exists to avoid. R1-E07 confirmed this gap is *fixable* — routing
lexical retrieval through the already-built, previously-unread
`lexical_postings` token index instead of substring scanning is
**~6,660x faster** (0.14ms p50) — but also found that fast retrieval
alone produces either too-strict candidate sets (AND-mode: 15.1%
zero-result rate) or hollow ones (OR-mode: 91.6% recall achieved by
returning a **median 7.5% and p95 77.5% of the entire catalog** as
"candidates," which is not retrieval a shopper would recognize as useful
without a ranking step to cut it down). Separately, R1-E04 found Solr's
on-disk index is 7.3x larger than `commerce-core`'s approximate index,
yet Solr's live RSS grows by only 175MB indexing the full catalog,
compared to `commerce-core`'s 3.76GB RSS for a *smaller* logical index —
a real, measured memory-architecture disadvantage (fully-resident heap
storage vs. Lucene's mmap'd segments), not a hypothetical one.

**Does not support**: a general physical-advantage claim across
realistic query traffic. **Does support**: the structural/facet/range
bitmap index itself (Gate 3's core contribution) is fast, correct, and
worth keeping for the query classes it targets — the disadvantage is
concentrated in memory representation (fixable by delegating storage to
a mmap-based engine) and in the absence of anything to do with a wide
lexical candidate set once retrieved (a ranking problem, dimension 3).

### 3. Relevance

**Loses decisively, and the reason is now precisely localized.** R1-E04
found Solr's default, untuned lexical ranking answers 99.8% of real
queries with a plausible result (0.2% zero-result rate) and NDCG@10 of
0.305 — a normal, credible figure with no commerce-specific tuning at
all. `commerce_core` has **no ranking mechanism for free text** —
`execute()` returns an unordered set from hard filters;
`execute_ranked` only reorders by a small hand-derived `Preference` list,
not a text-similarity score — so NDCG/Recall@10/MRR are architecturally
absent for the query classes structural retrieval doesn't fully resolve,
not merely lower. R1-E07 sharpened this: the underlying real text *does*
carry retrievability signal (OR-mode token lookup's unranked recall
ceiling is 91.6%, higher than Solr's own *ranked* Recall@10 of 0.1811),
so this is not a "the data lacks signal" problem. It is specifically
that nothing in this codebase converts a wide, mostly-irrelevant
candidate pool into a small, useful, ranked top-K — precisely the
scoring/ranking machinery (term weighting, field boosts, a BM25-family
function at minimum) that a mature engine has spent two decades tuning.

**Does not support**: "specialization preserves or improves retrieval
quality" — it currently loses badly for the majority of real query
traffic (R1-E02: only 55.4% of queries even reach a structural
resolution attempt, and R1-E05/E07 show the remainder's execution path
is either catastrophically slow or rankingless).

### 4. Complexity boundary

**Converging toward Lucene, the explicit negative signal CLAUDE.md and
Issue #5 both name.** Fixing R1-E05's physical gap required building a
whole-word inverted index (R1-E07) — standard lexical-engine
infrastructure. Fixing R1-E04's relevance gap requires a ranking
function — standard lexical-engine infrastructure. Fixing R1-E04's
memory gap plausibly requires mmap'd, segment-based storage — standard
lexical-engine infrastructure. None of this is commerce-specific work;
all of it is generic document-search machinery this project would be
reinventing, each piece a well-trodden, mature problem elsewhere.
Issue #5's own instruction applies directly: "if accommodating realistic
ecommerce queries increasingly forces generic search-engine
abstractions, explicitly treat that as evidence against the
specialization thesis."

### 5. Scale behavior

**Genuinely boring at 1.2M products on a single node — the one
dimension with no negative finding.** R1-E01: 1.2M real products build
in ~64s and fit in ~3.76GB RSS peak, well within this environment's
15GiB. No OOM, no latency blowup for the workload class the index
targets (structural filters stay sub-30-microsecond at full scale).
This dimension does not by itself justify ENGINEIZE (it says nothing
about semantic usefulness or relevance), but it does mean nothing found
this round motivates distributed serving, sharding, or multi-node
infrastructure — single-node capacity is nowhere near exhausted.

### 6. Learning/control-plane value

**Fails its safety bar.** R1-E06's central finding: the promotion
gate's regression check (`fully_resolved` before/after,
zero-per-query-regression) is **mathematically incapable of rejecting a
nonsensical mapping for any previously-never-seen term**, because such a
term could never have contributed to any prior query's successful
resolution — any query containing it was already `residual`. A control
experiment proving this — mapping the single most frequent real residual
term to an unrelated `waterproof=true` constraint — was **accepted**,
"resolving" 42 queries to a semantically meaningless constraint while
reporting zero regressions, indistinguishable from the entry's own
genuine fix (the stopword correction, +77 queries, zero risk) using the
gate's own reported signals. This is exactly the failure mode Issue #5
names explicitly: "if punt rate falls only because the model guesses
aggressively, reject the approach." Separately, R1-E06's evidence-backed
provider (propose Brand only for residual terms that are real catalog
brand names) found **zero** qualifying candidates among 9,081 real
residual terms — real unresolved vocabulary is overwhelmingly
product-type/category words this dataset has no ground truth for at
all, not low-hanging brand-vocabulary fruit the control plane could
safely harvest.

### 7. Freshness feasibility

**Under-tested but not load-bearing for this decision.** R1-E01's
build-time measurement (~64s full rebuild at 1.2M real products) is the
only evidence collected this round; it's compatible with periodic
full-rebuild for slow-changing metadata but says nothing about
real-time price/inventory freshness, which this dataset has no real
ground truth for (R1-E01: no price/inventory fields exist in the source
data at all). Not pursued further this round because dimensions 1-4 and
6 already determine the outcome independent of this one; a dedicated
overlay/mutation experiment is deferred to the narrowed system's own
roadmap rather than gating this decision.

## Branch selection

**NARROW THE PRODUCT**, over the other three:

**Against ENGINEIZE**: dimensions 1 (semantic usefulness), 3
(relevance), 4 (complexity boundary), and 6 (control-plane safety) all
show clear, measured negative evidence against a full generic-search
replacement. Proceeding to Phase 2 engineering of the original
full-replacement vision would mean building index-bundle/mmap/ranking/
lexical infrastructure this round's own evidence says is not
differentiating relative to existing engines (R1-E07's central finding).

**Against STOP**: every specific failure mode found this round has a
concrete, targeted fix already implied by the evidence itself, not a
speculative rescue: R1-E02/E02b's recall catastrophe traces to a missing
canonicalization/validation stage in cold start, not a flaw in
structural retrieval's premise (Gate 1's variant-safety guarantee held
throughout, at 12x Phase 0's largest scale). R1-E04/E07's relevance and
memory gaps trace to the *absence* of a ranking function and mmap
storage, both of which a mature embeddable engine (Tantivy) already
provides — not evidence that commerce semantics add no value, only that
this project should not build a ranking/storage engine from scratch.
R1-E06's control-plane gap traces to a missing precision-aware
replay check, a scoped, well-understood addition (this round's real ESCI
judgments are exactly the held-out data such a check would replay
against). STOP is warranted when "further work would merely route
around the evidence" (Issue #5's own STOP criterion) — every fix listed
here targets a specific root cause the evidence identified, the opposite
of routing around it. Dimension 5 (scale) also found zero negative
evidence, meaning the single-node structural/facet core this round
validated is not itself in question.

**Against REVISE THEN ENGINEIZE**: REVISE's own examples (Issue #5) are
about correcting one or a few specific assumptions while preserving the
original full-replacement scope. This round's evidence is broader than
that: independent negative findings span extraction/validation
(dimension 1), ranking (dimension 3), memory architecture (dimension 2),
and control-plane safety (dimension 6) — a pattern, not an isolated wrong
premise. Revising each in place while still aiming at a full
generic-search replacement would, per dimension 4's own finding, just
walk the project toward rebuilding Lucene component-by-component. NARROW
better fits the evidence: keep the parts that measurably work (typed
structural/facet retrieval, ambiguity-preserving compilation *once fed
validated vocabulary*, the control-plane mechanism *once given a
precision check*) and stop trying to also be the lexical/ranking engine.

## The narrowed product

**A Commerce Structural & Semantic Planning Layer, delegating lexical
retrieval and ranking to an embedded Tantivy index**, not a full
generic-search-engine replacement:

1. **Owns**: typed, variant-safe structural/facet/range retrieval
   (brand, product type, category, price, typed enum/boolean/numeric
   attributes) for query classes where a genuinely selective, validated
   predicate exists — the one workload class this round found a real,
   substantial, uncontested physical advantage for (R1-E05's baseline:
   12.4us at 1.2M-product scale). Preserves Gate 1's variant-safety
   guarantee and Gate 2's ambiguity-preservation design.
2. **Requires** (new, directly fixing R1-E02/E02b's root cause): a
   candidate-canonicalization/validation stage between cold-start
   profiling and lexicon construction — a raw catalog field value only
   becomes a trusted hard-filter lexicon entry after passing a validity
   check, not by default.
3. **Requires** (new, directly fixing R1-E06's root cause): a
   precision-aware promotion gate — replay a candidate semantic route
   against held-out relevance judgments (this round's real ESCI split
   supplies exactly this kind of data) and reject any candidate that
   doesn't hold or improve precision, not just coverage.
4. **Delegates, does not rebuild**: lexical retrieval and relevance
   ranking to Tantivy (the Rust-native, embeddable equivalent of what
   Solr/Lucene demonstrated as a credible, competent baseline in
   R1-E04) — directly matching Issue #5's own instruction ("robust
   lexical path, using Tantivy/Lucene as a primitive if evidence says
   rebuilding lexical retrieval is not differentiating") and R1-E07's
   finding that retrieval-primitive-building is cheap but
   ranking-quality is the actual differentiator a mature engine
   provides.
5. **Still does not build**: distributed serving, sharding, HA,
   Elasticsearch-API compatibility, or a generic document DSL —
   dimension 5 found zero evidence single-node capacity is a limit, and
   nothing above requires abandoning that boundary.

## Execution

Per Issue #5, this decision is not complete until the branch is
executed through its next genuine architecture boundary. `docs/adr/0008-narrow-to-structural-planning-layer.md`
records this decision as an ADR; a new GitHub issue tracks the Phase 2
epic (delegated-lexical validation, canonicalization stage, precision-
aware promotion gate, and eventual integration) with priorities ordered
by the evidence above. The first concrete task — validating the central
bet of this narrowed architecture (does an embedded Tantivy index
actually recover the relevance Solr demonstrated, in-process, without
Solr's JVM/HTTP overhead, on the exact same real catalog and judgments
used throughout Round 1) — begins immediately following this document,
before any further planning work, since every other narrowed-system task
depends on confirming that bet first.

---

## Post-decision evidence update (Issues #7, #8, #9)

Per this document's own append-only discipline: this decision
(NARROW THE PRODUCT) stands unchanged. Nothing below reopens it. Three
follow-on epics, each executing/stress-testing part of "the narrowed
product" defined above, produced real evidence that refines several
dimensions — recorded here rather than silently left implicit in three
separate experiment logs (`docs/experiments/ISSUE7_LOG.md`,
`REALTIME_LOG.md`, and `PHASE2_LOG.md` P2-E07-E10).

### Dimension 2 (Physical advantage) — the ~36,700x Punt-path risk is now closed, and a second real fix confirmed

R1-E05's ~36,700x non-selective-predicate degradation, named above as
the physical-advantage disadvantage's sharpest edge, was **not** an open
risk by the time Issue #7 measured it (`ISSUE7_LOG.md` I7-E01): the
planner+Tantivy-delegate composition this document's own "narrowed
product" item 4 already committed to (delegate lexical retrieval to
Tantivy) already routes exactly this query shape to `Punt`, and
reproducing R1-E05's named adversarial case through that already-built
path lands at 1.17ms p50 — 820x faster, not a new mechanism, a missing
regression check. Record this so it is never rediscovered as a surprise.
A second, real, additive fix was confirmed for compound structural+
numeric-range queries specifically (I7-E03): pre-bucketed
`RoaringBitmap` composition for price-range constraints is 12.2-12.5x
faster than the current binary-search-and-collect mechanism, independently
reproduced twice.

R1-E04's memory disadvantage (3.76GB `commerce-core` RSS vs. Solr's
175MB growth) is now more precisely attributed, not resolved: I7-E02
found a genuinely columnar attribute layout only recovers ~2.08x RSS
reduction for this real catalog, not the originally-hoped >=5x, because
the dominant share of `Catalog`'s footprint is real, uncompressed
title/description/bullets text (1,372.75MB of the 2,926.16MB measured),
not per-item object overhead — a hypothetical zero-overhead columnar
design could not exceed ~2.13x for this dataset without adding actual
byte-level text compression, a materially larger, separate, unattempted
undertaking. This does not change dimension 2's verdict (the structural
bitmap/range index itself remains fast and worth keeping) but does mean
"columnar layout" alone is not the fix for the RSS gap this document
flagged — compression, or continuing to lean on delegation (Tantivy's own
mmap'd storage, confirmed by I7-E05 to reopen 5,890x-14,868x faster than
`commerce-core`'s only option, a full rebuild) are the real paths forward.

### Dimension 7 (Freshness feasibility) — no longer "under-tested" for the availability/inventory-status subset

This document deferred dimension 7 as under-tested, with only R1-E01's
~64s full-rebuild number as evidence and no real price/inventory ground
truth in the dataset. Issue #8 (`REALTIME_LOG.md` R-E01) closes the
*state-mutation-cost* half of that gap for the class of field CLAUDE.md
and Issue #8 both name (fast-changing operational state — variant
availability): a variant-scoped commerce-state overlay
(`commerce_core::state`, in-place `RoaringBitmap` mutation, the same
algorithmic pattern independently validated by Havenask/IndexLib
archaeology) applies a state change in 342ns p50 against a real
1.2M-product catalog, vs. the ~62-70s full rebuild this document's own
"minimal overlays/sidecars" language anticipated — roughly 10^8x, correctness-
verified at real scale (1% of real products marked OOS, exact bitmap
match against the independently-computed complement). Two real gaps
remain, named rather than smoothed over: the overlay has no
durability/replay (state is lost on restart, confirmed directly), and a
naive `RwLock` loses ~83% read throughput and ~40x writer throughput
under concurrent read+write load. Price freshness remains genuinely
untested — the real ESCI dataset has no price field at all, unchanged
since R1-E01.

### Item 2 of "the narrowed product" (canonicalization stage) — real, refined, more subtle than a single threshold

This document's item 2 named a "candidate-canonicalization/validation
stage" as the fix for R1-E02/E02b's recall catastrophe, without
specifying the mechanism beyond "a validity check." Issue #9
(`PHASE2_LOG.md` P2-E07-E10, now complete) tested all three named
candidate mechanisms — a raw frequency threshold, a deterministic
heuristic, and a model-assisted arm — against real reconciled ground
truth (three independent human-equivalent adjudication passes, 209 real
candidates) and full real end-to-end replay (all 22,458 real judged
queries, plus a real, fairness-controlled 500-brand sample for the
model-assisted arm's own end-to-end test, since classifying the full
~206K-brand vocabulary with an agent is not tractable). **Final decision:
CANONICALIZATION FRONTIER IS FUNDAMENTAL** (one of Issue #9's four named
options, GitHub comment on issue #9 records the full reasoning). All
three mechanisms — regardless of how confidently or accurately each
classifies individual brand strings in isolation — show the *same*
directional tradeoff once wired into the real query path: trusting more
strings as hard filters increases FIB coverage but *decreases* real
recall against Exact/Substitute-judged relevant products (P2-E08:
heuristic's real recall caps near 14-16% regardless of threshold vs.
frequency-only's 43% at higher thresholds, despite roughly doubling FIB
coverage; P2-E10: a real, isolated 500-brand model-assisted sample
reproduces the same direction at proportionally smaller magnitude). The
mechanism, now well-supported by three independent, structurally
different canonicalizers converging on the same result: the tension is
inherent to enforcing trust as a *hard* structural AND filter on an
exact-matched string — vulnerable to real spelling/aliasing/formatting
variation between a compiled constraint and a product's actual field
value — not to which values get trusted or how accurately they're
classified. This means item 2's "a validity check" was correct in spirit
but the wrong lever: refining *which* strings pass validation cannot
fix a problem inherent to *how* trust is enforced. The concrete next
experiment this points to is a softer enforcement mechanism (alias-
normalized/fuzzy matching, or a scored `Preference` instead of a hard
`Constraint` when confidence isn't maximal) — recorded as a follow-up,
not attempted here.

### What does not change

Dimensions 1 (semantic usefulness), 3 (relevance), 4 (complexity
boundary), 5 (scale), and 6 (control-plane safety) are unaffected by
this evidence — none of Issues #7/#8/#9 tested those questions. The
branch selection (NARROW THE PRODUCT, over ENGINEIZE/STOP/REVISE THEN
ENGINEIZE) and "the narrowed product" definition both stand. This
update refines *how* two of the five items in that definition (physical
advantage's memory/latency edges, and the canonicalization stage) should
be built next, and closes one previously-open risk (dimension 2's
non-selective-predicate degradation) outright.
