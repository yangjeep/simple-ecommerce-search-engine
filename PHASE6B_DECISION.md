# Phase 6B Decision (Issue #21 Phase 6, continuing from Issue #23 / Phase 6A)

**Decision: PROCEED**, with two explicit narrowings and one withdrawn
claim from this document's own first draft.

Phase 6A found WANDS' facet-scan crossover at ~2,072–2,175 real
candidates and attributed the shift from Phase 5's ESCI finding
(~9,000–12,000) to WANDS' richer per-candidate attribute map. Phase 6B
built a controlled-stress scale ladder (the real WANDS catalog
replicated 2x/5x/10x/20x, holding facet cardinality and per-candidate
attribute complexity fixed while scaling only candidate-set size) to
test whether that explanation is complete. **After adversarial review
found the first-draft analysis overstated a "genuine super-linear
scaling" finding using a single, non-representative checkpoint, a
follow-up measurement (5 independent process relaunches plus a
reversed-execution-order check) was run to resolve it properly.** The
corrected result: Phase 6A's "attribute complexity, not raw scale"
explanation **substantially holds** in aggregate, with one real,
noise-robust, narrower exception at the ~4,000–11,000-candidate range
for the smallest checkpoints, whose cause is not yet identified. A
genuinely new operator (numeric-range filter on `average_rating`,
untestable in Phase 6A for lack of a price field) has its own real,
distinct, materially higher crossover.

This document is Phase 6B's terminal decision artifact, governed by
Epic #21. It does not overwrite `PHASE6A_DECISION.md` or any earlier
phase decision — all remain historically accurate for their own scope.

## Recap: what Phase 6B was asked to answer

Issue #21's Phase 6 goal is to determine whether Phase 5's structural
wins and breakpoints are real commerce properties or dataset artifacts,
and to characterize the 80x reference boundary, lower-advantage region,
1x crossover, and native-loss region for every operator. Phase 6A
answered this for one dataset pair (ESCI vs. WANDS) at each dataset's
natural size. Phase 6B asks the natural follow-on: **is the facet-scan
crossover shift governed by per-candidate attribute complexity alone (as
Phase 6A concluded), by candidate-set size, or both — and does the
native-loss region continue to widen past WANDS' real ~43K-product
ceiling, or plateau?**

## Blocked-dataset and blocked-engine survey

Before building a synthetic ladder, every other named Phase 6 resource
was re-checked from this environment (also posted to Issue #21):

- **Retailrocket** (Issue #23's own named "Phase 6B candidate") —
  Kaggle-only; `kaggle.com` returns `CONNECT tunnel failed, response
  403` (organization-policy block). No GitHub/GCS mirror found.
  **Blocked.**
- **H&M Personalized Fashion Recommendations** (Issue #23's other named
  optional follow-up) — also Kaggle-only, same 403. **Blocked.**
- **Amazon Reviews 2023** — re-confirmed still blocked (Hugging Face and
  all CDN subdomains, mcauleylab.ucsd.edu, and the GitHub Pages docs
  site all still return organization-policy 403; the authors' companion
  GitHub repo still contains only fetch scripts, no data).
- **Havenask** (Issue #21's required second engine) — the source repo
  (`github.com/alibaba/havenask`) is reachable, but its prebuilt Docker
  images live on `registry.cn-hangzhou.aliyuncs.com` (not on this
  environment's network allowlist, confirmed unreachable), and a source
  build needs a Bazel dependency graph resolving against further
  non-allowlisted hosts. Independently, this container has **no Docker
  daemon at all** (`docker info` fails with the same
  daemon-unreachable error Round 1 already recorded for this
  environment). **Blocked on two independent grounds.** Havenask is
  also, by nature, oriented toward distributed serving; CLAUDE.md's
  "avoid distributed systems work until the single-node thesis has been
  measured" is a secondary reason to defer it rather than force a
  fragile single-node workaround.
- **eCommerceSearchBench** — no accessible, confidently-identified
  source located. Recorded as unresolved.

Issue #23 explicitly anticipated exactly this situation in its own
"Scale handling" section: *"Do not duplicate/rescale WANDS merely to
manufacture 100k/1M/10M product tiers. Large-scale characterization
belongs to subsequent Phase 6 datasets such as Retailrocket, H&M where
practical, Amazon Reviews 2023 when unblocked, **or clearly labeled
controlled stress datasets**."* With every named real alternative
confirmed blocked, Phase 6B takes that explicitly-sanctioned path. This
is a different thing from Issue #23's own non-goal ("no fake scaling of
WANDS to claim large-catalog evidence") — that non-goal is about not
*presenting* a replicated catalog as organic large-catalog evidence, not
a prohibition on controlled stress testing as a distinct, honestly
labeled methodology. Every result below is labeled accordingly.

## Architecture tested

Unchanged from Phase 6A: `commerce_core::index::CatalogIndex`'s
`facet_counts_by_scan` (an O(candidates) scan-based facet method) and
generic `numeric_index` (a sorted-array structural index, previously
built but never exercised against real WANDS data since WANDS has no
price field) versus Apache Solr 9.10.1. No engine optimization, planner
heuristic, or benchmark-specific shortcut was introduced — the same hard
rule Phase 6A operated under.

## Datasets / workloads

Real WANDS catalog (42,994 products, pinned commit
`3b74dcf4ba29ab8ff3e6a50b5b09fc627cb882b5`, re-verified against its
checksums this session) plus four controlled-stress replicated tiers
(2x/5x/10x/20x = 85,988/214,970/429,940/859,880 products), generated by
`scripts/datasets/replicate_wands_scale.py`. Every record beyond the
real 1x tier is a byte-identical copy of a real record except for a
suffixed `id` — facet cardinality and per-candidate attribute complexity
are fixed by construction, only candidate-set size scales. This is
explicitly **not** a model of organic catalog growth (see
`docs/experiments/PHASE6B_LOG.md`'s "Methodology" section for the full
disclosure).

Workload: the identical 7 real depth-1 category checkpoints Phase 6A
used for its own crossover sweep (Rugs, Storage & Organization, Lighting,
Outdoor, Décor & Pillows, Home Improvement, Furniture), rerun at every
tier, plus two new workload classes never attempted in Phase 6A: a
numeric-range filter on WANDS' real `average_rating` field, and a mixed
category+rating request.

## Measured results

**Correctness gate**: 50/50 rows across all 5 tiers had exactly matching
native/Solr counts — zero mismatches, verified before any timing claim
was trusted.

**Facet-scan crossover — qualitative reproduction, not exact cross-session
reproduction.** The real 1x tier's crossover sits in the same
~2,000–2,200-candidate neighborhood Phase 6A found. The exact speedup
values differ from Phase 6A's archived numbers (e.g. Rugs: Phase 6A
1.00x, Phase 6B 1x-tier 2.40x) because this session's Solr JVM has all 5
scale-ladder cores loaded simultaneously sharing one 4GB heap, unlike
Phase 6A's single-core session — a real, identified cross-session
confound, not evidence either system changed. Every checkpoint is
already in native-loss territory by the 2x tier and continues degrading
through 20x (by Furniture/20x, 320,780 candidates, native is ~50x
slower than Solr for this operator).

**Per-candidate cost — Phase 6A's explanation substantially holds, with
one narrower exception.** This document's own first draft claimed a
"genuine super-linear scaling" finding using only the Rugs checkpoint
(0.81 -> 2.05 ms/1,000-candidates, 1x->20x). Adversarial review found
this did not survive checking all 7 checkpoints: the sweep-wide
aggregate is **flat within ~13% across the whole ladder** (1.71 -> 1.86
-> 1.63 -> 1.76 -> 1.75 ms/1,000-candidates, 1x->20x), with the three
largest checkpoints flat-to-declining. A follow-up measurement (5
independent process relaunches plus a reversed-execution-order check,
detailed in `PHASE6B_LOG.md`) found a real, order-independent, ~80-100%
jump in per-candidate cost specifically between the 2x and 5x tiers for
the three smallest checkpoints (n≈4,000-4,400 -> n≈10,000-10,900) — a
genuine phenomenon, not noise, but narrower than originally claimed and
of unconfirmed cause (hypothesized: a CPU cache-locality threshold).

**Numeric-range filter — a new operator, its own distinct crossover.**
`average_rating` range filtering (real WANDS data, never testable in
Phase 6A for lack of a price field) shows native winning clearly at
1x-2x (2.17x-5.62x), roughly parity at 5x (0.73x-1.14x), and losing at
10x-20x (0.21x-0.51x) — a crossover at ~57,000-160,000 candidates,
materially higher than the color-facet operator's ~2,100, consistent
with `numeric_index` being a sorted-array binary-search structure rather
than a per-candidate scan.

**Build time / RSS / index size** scale close to linearly with product
count (24x build time, 18x RSS for a 20x product-count increase), with a
mild, consistent super-linear component in per-product build cost
(19.78 -> 23.82 microseconds/product, 1x->20x) — independently endorsed
by adversarial review as the most credible standalone scaling-cost
signal in this experiment. `index_bytes_per_product` decreases slightly
with scale (188.6 -> 169.4 bytes) — flagged as a replication-design
artifact (fixed facet cardinality amortizing over a larger population),
not evidence of real memory efficiency gains at organic scale.

Full tables, raw CSVs, and per-tier logs: `docs/experiments/PHASE6B_LOG.md`,
`docs/research/artifacts/p6b_e00_scale_ladder_run1/`.

## Cross-phase comparison: Phase 6A (WANDS natural size) vs Phase 6B (WANDS scale ladder)

| Finding | Phase 6A | Phase 6B | Classification |
|---|---|---|---|
| Facet-scan crossover location (candidate count) | ~2,072-2,175 | Same neighborhood at 1x; degrades further from 2x onward | **ROBUST** (qualitative location); absolute cross-session speedup values **NOT COMPARABLE** (JVM confound) |
| "Attribute complexity explains the ESCI-vs-WANDS shift" | Proposed explanation | Substantially confirmed in aggregate (flat within ~13% across 1x-20x) | **ROBUST**, narrowed with one exception below |
| A pure candidate-size effect exists independent of attribute complexity | Not tested | A real, narrow, order-independent ~80-100% jump at ~4,000-11,000 candidates for the smallest checkpoints only | **SHIFTED** — narrower and more specific than this document's own first-draft claim, cause unconfirmed |
| Native-loss region exists and widens with scale | Observed up to 16,039 real candidates (0.11x at Furniture) | Confirmed continuing to widen through 320,780 candidates (still ~0.02x-0.11x, no plateau observed) | **ROBUST** |
| Numeric-range filter crossover | Not testable (no price field) | New: ~57,000-160,000 candidates | **NEW FINDING**, not previously comparable |

Nothing here is classified **FALSIFIED** against Phase 6A. The
first-draft "genuine super-linear scaling" claim in this document's own
`PHASE6B_LOG.md` was self-caught and corrected before promotion — it is
recorded, not erased, in the log per this project's archive discipline.

## Self-caught bug (tooling, not commerce_core)

`scripts/datasets/solr_index_wands.py`'s schema setup was not idempotent
for `add-copy-field` (unlike `add-field`, which Solr correctly rejects
as "already exists"): re-indexing an already-initialized core silently
duplicated the `title -> title_sort` copy rule, breaking every
subsequent update with `400: Multiple values encountered for non
multiValued copy field`. This was latent since Phase 6A (which never
needed to re-run indexing against a live core) and only surfaced because
Phase 6B is the first time this repository re-runs the WANDS indexing
script against an already-initialized core. Fixed by checking
`GET /schema/copyfields` before posting.

## Unresolved risks

1. **Candidate-set size (n) and total catalog size (N) are perfectly
   collinear in this ladder** — every tier scales both by the same
   factor K, so this design cannot separate "cost is a function of
   candidates touched" from "cost is a function of total resident
   index/heap size." A genuine decoupling (fixed-N/varying-n, or vice
   versa) is a named follow-up, not attempted here.
2. **The ~4,000-11,000-candidate jump's cause is unconfirmed.** A CPU
   cache-locality threshold is the most plausible unproven hypothesis;
   no profiling evidence (perf, cachegrind) was collected in this pass.
3. **Cross-session absolute-latency comparison is unreliable** in this
   environment — this session's shared-JVM-across-5-cores Solr setup
   produced systematically different absolute numbers than Phase 6A's
   single-core session for the identical real query. Future phases
   should either isolate one Solr core per measurement session or
   explicitly caveat any cross-session absolute-number comparison.
4. Havenask, Retailrocket, H&M, Amazon Reviews 2023, and
   eCommerceSearchBench remain blocked from this environment; if network
   policy changes, they remain the preferred real alternative to
   controlled-stress substitution and should be revisited before Phase 7.

   **Addendum (post-Phase-8 audit, see `PHASE6C_DECISION.md`)**: this
   revisit did not happen before Phase 7 as instructed above — Phase 7
   and Phase 8 both proceeded with Solr as the only lexical-backend
   evidence. A full audit after Phase 8 found this gap and closed it
   late rather than leaving it silent: Havenask, Retailrocket, H&M, and
   Amazon Reviews 2023 are all still blocked, re-verified live in that
   session (unchanged). Elasticsearch and OpenSearch were tested for
   the first time and are also blocked (official distributions
   unreachable; OpenSearch's own source build hits a second,
   independent blocker). `eCommerceSearchBench` was actually located
   this time (`github.com/alibaba/eCommerceSearchBench`, reachable) —
   a correction to "no accessible source located" above, though it is a
   workload/data generator, not an engine. Apache Lucene itself,
   however, is directly reachable via Maven Central and was benchmarked
   as a new, real, correctness-gated cross-engine data point — see
   `PHASE6C_DECISION.md` for the full result.

## What would be built next if scaling up

A properly decoupled n-vs-N scale experiment (fixed total catalog size,
varying only the queried candidate-set size via different filter
selectivities) to isolate the ~4,000-11,000-candidate jump from general
index-size effects; a targeted profiling pass (perf/cachegrind) on
`facet_counts_by_scan` at that specific candidate range if the jump
reproduces; single-core-isolated Solr sessions for any future
cross-session absolute-latency claim.

## What should explicitly not be built yet

No planner/admission changes based on the unconfirmed candidate-range
jump — it is too narrowly characterized (3 checkpoints, one candidate
range, unconfirmed mechanism) to promote into a planner input yet. No
further controlled-stress replication beyond 20x without first
attempting the n/N decoupling above, since more replication tiers would
only add more points confounded the same way, not new information.

## What this decision does and does not claim

**Does claim**: Phase 6A's core attribution (per-candidate attribute
complexity explains most of the ESCI-vs-WANDS crossover shift) survives
a controlled, complexity-held-fixed replication test; a real but
narrower, noise-robust, order-independent exception exists at a specific
candidate range; a new operator (numeric-range filter) has its own real,
distinct crossover; the native-loss region is real, substantial, and
widens with candidate-set size well past WANDS' natural ceiling.

**Does not claim**: that native `facet_counts_by_scan` has a confirmed
general super-linear complexity class (the original first-draft claim,
withdrawn); that Phase 6B's absolute latency numbers are directly
comparable to Phase 6A's archived numbers; that the candidate-range
jump's cause is understood; that this controlled-stress catalog
represents what a genuinely organic 860K-product WANDS-like catalog
would look like.

**Decision: PROCEED** to the next independent Phase 6 resource when one
becomes accessible (dataset or Havenask), without changing the
underlying commerce-native mechanism. The planner implication Issue #21
already states continues to hold: native execution should be promoted
only inside a measured physical-advantage region, which this phase has
further narrowed (operator-specific, and now known to include at least
one candidate-range-specific irregularity) rather than widened or
falsified.
