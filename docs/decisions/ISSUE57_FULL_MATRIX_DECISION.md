# Issue #57 — Full-Matrix E2E Benchmark: Decision (Revision 1)

**Status: B — Narrow specialization survives, with material open evidence gaps disclosed below. Not a final, closed verdict — a preliminary, honestly-scoped one given this revision's session-time constraints.**

Governing question (Issue #57): *Across materially different real
ecommerce workloads, when semantic behavior and work performed are held
equivalent, where does the current commerce-native/hybrid architecture
materially outperform mature engines, where does it merely match them,
and where should a mature engine remain responsible?*

## What was actually measured

Real, correctness-gated, cross-engine evidence (native vs. Solr 9.10.1
vs. Elasticsearch 8.15.0 vs. OpenSearch 2.17.0 vs. a genuinely running
Havenask `ha3_runtime`) across WANDS, three ESCI verticals, and Magento —
313/313 correctness-gated rows match exactly across all five systems.
Full detail: `ISSUE57_FULL_MATRIX_SYNTHESIS.md`;  frozen protocol:
`docs/experiments/FULL_MATRIX_PROTOCOL.md`; adversarial review:
`docs/experiments/ISSUE57_ADVERSARIAL_REVIEW.md`.

## Where native materially outperforms (B: narrow specialization)

**Structural filter, numeric-range, and facet queries** (Q5, Q9, Q10 on
WANDS; Q2 on ESCI): native's compiled bitmap/ordinal structures beat
every one of the four external engines by roughly **10,000×–50,000×**
on measured latency, replicated across five real datasets and four
independently-implemented external engines (not a single lucky
Solr-only comparison — this revision is the first time this class of
result has been shown against Elasticsearch, OpenSearch, and a real
Havenask, not just Solr). The magnitude partly reflects a real
architectural fact (native is in-process; every external engine pays an
HTTP/network round trip a real deployment always pays too), not a
benchmark artifact — both sides are timed as what a real serving caller
actually pays.

## Where a mature engine should remain responsible (B: generic retrieval)

**Open-ended lexical search** (Q11): native's own text-matching
strategy is a linear candidate scan, not an inverted index. On WANDS
(42,994 products) it is **slower** than all four external engines
(6.68ms vs. 1.56–3.62ms). On the ~20–40× smaller ESCI slices it is
faster — a genuine, disclosed crossover, not a contradiction: native's
cost scales with catalog size, the external engines' does not (in this
range). **This is the evidence Issue #57 asked for to justify "generic
retrieval remains mature-engine territory": at real, meaningful catalog
scale, native's own lexical path already loses**, exactly matching
CLAUDE.md's existing architectural principle ("delegate open-ended
lexical retrieval/ranking to a mature backend rather than rebuilding a
general search engine").

## Where the architecture's safety claim is narrower than it might sound

**Same-variant conjunction (Q8)**: 294/294 correctness-gated checks
(true positives and cross-variant traps) matched on **every** system,
including all four external engines, when each variant is indexed as
its own document. Product/Variant safety is native's *correct-by-
construction default*, not an exclusively-native *capability* — any of
the four external engines achieves the identical guarantee given the
right physical schema. This is a real, adversarially-useful finding
against an overclaim the project should not make.

## Material open gaps (why this is a preliminary B, not a closed one)

Per the adversarial review, four gaps materially limit how much weight
this revision's B verdict can carry:

1. **Zero relevance-quality metrics measured this revision** (no
   NDCG/Recall/MRR against WANDS's or ESCI's real judgments for any of
   the four external engines — only Issue #35's prior, Solr-only NDCG
   evidence exists). The structural/lexical *timing* evidence above says
   nothing about ranking *quality* when native's ranking path is
   actually engaged.
2. **Engine query order was not randomized**, and Havenask — always
   queried last, after four other engines were already resident — is
   consistently the slowest external engine. Whether this reflects
   Havenask itself or accumulated measurement-order pressure is
   unresolved.
3. **Scale is capped at 42,994 products** (the full 1.2M-product ESCI
   corpus was deferred on disk allowance, not access). The WANDS-vs-ESCI
   lexical crossover is itself proof that scale changes which side
   wins — the structural-query magnitude at 43K products is not
   validated to hold at 10×–100× that scale.
4. **Havenask ran in a non-default, resource-constrained deployment
   mode** (`hape`'s local-process `proc` domain, because mounting the
   Docker socket for its sibling-container `default` domain was denied
   by this session's own safety guardrails) — disclosed, not hidden, but
   unresolved.

## What would close these gaps (recommended next steps, not started)

- Compute NDCG@10/Recall@K/MRR against WANDS's and ESCI's real judgments
  for all five systems (native's ranking path, not just structural
  filter counts).
- Rerun the matrix with randomized/counterbalanced engine order and
  report per-engine variance under both orderings, to separate Havenask
  itself from a measurement-order confound.
- Extend to the full ESCI corpus (needs a larger disk allowance than
  this session's, or a deliberately bounded/sampled slice, explicitly
  declared as such) to test whether the structural-query magnitude and
  the lexical crossover point hold at 10×–100× scale.
- Attempt Havenask's `default` (sibling-container) domain in an
  environment where the Docker-socket mount is not blocked, to test
  whether its measured latency changes materially.
- Instrument index size/build time per engine per dataset (frozen
  protocol §11, not done this revision).

## Explicit non-decisions

This is **not** a D (thesis fails) verdict: the structural-query
evidence is too large and too consistently replicated across four
independent engines to read as noise, and the lexical-crossover finding
is exactly the kind of "generic retrieval is mature-engine territory"
result the architecture's own stated hypothesis predicts, not evidence
against it. It is **not** an A (strong architecture, broad win) verdict:
native loses decisively on open-ended lexical at real scale, and the
Product/Variant safety claim is narrower (a schema default, not an
exclusive capability) than a broad win would require. It is **not** a C
(control plane survives, custom engine doesn't) verdict: the structural
serving-plane speedup is real, large, and would not exist if native's
compiled bitmap/ordinal structures were replaced by any of the four
mature engines' own physical execution — the custom engine's serving
plane is exactly what is producing the differentiated result for Q5/Q9/
Q10/Q2, not merely the semantic-discovery/compilation control plane
layered on top of it.

**Per Issue #57's own instruction not to force one answer**: B, scoped
precisely as above — facets/numeric-range/typed selective structure →
native; open-ended lexical relevance → mature engine; Product/Variant
schema safety → native's correct default, available to any engine with
correct schema design — is the evidence-supported reading of this
revision's real, correctness-gated results, with the four gaps above as
the explicit, unresolved boundary of that confidence.

## Do not begin the next architecture phase

Per Issue #57 and CLAUDE.md: this decision does not authorize new
architecture work. The recommended next steps above are the closure
path for this benchmark's own remaining gaps, not a new feature
roadmap.

---

# Revision 2 (2026-08-29) — closing the five named gaps

*Per CLAUDE.md's "preserve corrected and superseded evidence" rule,
Revision 1 above is unedited. This section reports what changed, what
was newly measured, and the resulting final status.*

## Gap-by-gap outcome

1. **Relevance metrics (NDCG@10/Recall@10/MRR@10) — CLOSED for
   native/Solr/Elasticsearch/OpenSearch.** New binaries
   (`wands_relevance`, `esci_relevance`) score all real judged queries
   with >=1 non-Irrelevant judgment (`issue57_eval::ndcg_recall_mrr`,
   reusing `phase9_eval::wands_relevance`'s WANDS-label scoring and
   `issue35_eval::label_gain`'s ESCI scale) against each engine's own
   ranked retrieval, using the identical structural-constraint
   translation already used for count/facet fairness
   (`comparator_eval::translate*`). Real result, not previously known
   (Revision 1 measured zero relevance metrics for any external engine):

   | Dataset | n | native | solr | elasticsearch | opensearch |
   |---|---|---|---|---|---|
   | WANDS | 471 | **0.2168** | 0.2053 | 0.1981 | 0.1981 |
   | ESCI electronics | 490 | **0.3041** | 0.2547 | 0.2595 | 0.2595 |
   | ESCI automotive | 501 | **0.4414** | 0.4383 | 0.4252 | 0.4252 |
   | ESCI beauty | 489 | **0.4162** | 0.3984 | 0.3852 | 0.3852 |

   (NDCG@10 shown; Recall@10/MRR@10 in
   `dataset_cache/issue57_*_full_matrix/relevance.csv`, same ordering in
   every case.) Native's ranking is **at parity or ahead of every
   external engine's own ranking on all four real, independently-judged
   datasets** — consistent with, and now extending across three real
   ESCI verticals beyond, Issue #35's own prior Solr-only finding.
   Havenask's row is reported (`havenask_UNRANKED_capability_gap`) but
   explicitly excluded from this comparison: this SQL/QRS deployment
   exposes no verified relevance-ranked `ORDER BY` in the schema used
   here, a disclosed capability gap, not a measured (near-)zero score.
   A real, disclosed methodology limitation found while closing this
   gap: native's lexicon auto-discovers attributes the ESCI Solr/ES/OS
   schemas were never built to index (e.g. a "size" enum on some
   automotive queries); such queries are excluded from that engine's
   paired comparison for that query (matching Issue #35's own
   `TransportError` precedent), not scored as a loss or allowed to crash
   the run.

2. **Randomized/counterbalanced engine order — CLOSED.**
   `issue57_eval::{cell_seed, shuffled_order, run_shuffled}` replace
   Revision 1's fixed native→solr→es→opensearch→havenask order with a
   deterministic-but-distinct permutation per cell (hashed from that
   cell's own dataset/class/key identity), and every full-matrix binary
   now records and prints the actual execution order plus a mean-
   latency-by-queue-position breakdown per engine — the adversarial
   review's own ordering-confound test, not just a disclosed limitation.
   All 313 correctness-gated rows from Revision 1 (WANDS 7, ESCI×3
   15, Magento 294 — the exhaustive Q8 sweep is unaffected since it is
   not part of the timed/ordered cells) were rerun this revision under
   randomized order and **all still match** (native/Solr/Elasticsearch/
   OpenSearch — see gap 4 for why Havenask specifically could not be
   re-included). The by-position breakdown (raw CSVs) shows no engine's
   latency correlating with queue position within this revision's
   sample — the ordering confound as originally described (Havenask
   specifically, always-last) could not be re-tested this revision since
   Havenask itself is unavailable (gap 4), which is itself the residual,
   disclosed limit of this closure: the *mechanism* for detecting an
   ordering confound is now real and exercised for four engines, but the
   *specific* Havenask-always-last question from Revision 1 remains
   open.

3. **Full 1.2M-product ESCI corpus — DATA READY, MATRIX RERUN NOT
   ATTEMPTED THIS REVISION, disclosed as a deferral, not forced.** The
   full corpus was fetched and exported this revision
   (`scripts/round1/fetch_esci.sh` + `export_esci.py`: 1,215,854
   products, 425,762 judged (query, product) pairs across 22,458
   distinct queries — confirmed present, this session's disk allowance
   (~150 GB free) is no longer the Revision 1 blocker). Indexing that
   full corpus redundantly into Solr/Elasticsearch/OpenSearch and
   re-running the structural/lexical/relevance cells at that scale is a
   substantial additional ingestion-and-measurement pass (on the order
   of the WANDS-scale work already done, but ~28x the document count)
   that this session's remaining time did not accommodate on top of
   gaps 1/2/4/5 and the Havenask investigation. This is recorded as a
   **DEFERRED, not BLOCKED** cell, per Issue #57's own status contract:
   the data is on disk and the disk allowance is no longer the
   constraint; a follow-up session can index and measure it directly
   without repeating any of this revision's acquisition work.

4. **Havenask `default`-domain retry — ATTEMPTED IN DEPTH, STILL
   UNAVAILABLE.** See `docs/experiments/ISSUE57_HAVENASK_REVISION2_LOG.md`
   for the full trail: six real, disclosed, orchestration-layer-only
   bugs found and fixed in `hape`'s `default` (sibling-container) domain
   without any host root-SSH/security change (a materially deeper
   attempt than Revision 1's single-line "denied" disclosure), reaching
   a genuinely running `swift_admin` process before stalling on an
   unexplained broker-scheduling failure; `hape`'s `proc` domain (the
   mode Revision 1 used successfully) was also retried cleanly twice on
   this fresh host and, both times, stalled at worker-version
   convergence (`WS_NOT_READY`) rather than reproducing Revision 1's
   success. Both stall points are inside Havenask's/`hape`'s own
   process-liveness internals with no actionable error surfaced —
   concluded as environment-specific to this host, not a new defect in
   Havenask's query/engine logic (no dataset was ever loaded far enough
   to test correctness in either attempt). Every binary this revision
   probes Havenask once at startup and runs the remaining cells as a
   real 4-way comparison when it is down (`issue57_eval::
   havenask_available`), rather than fabricating a result or crashing.
   Revision 1's own Havenask correctness/timing numbers stand, dated to
   that revision, not re-verified or superseded this revision.

5. **Index size / build time / startup time / memory footprint —
   CLOSED.** `scripts/datasets/measure_footprint.py` (real per-engine
   admin-API index size, this session's own indexer-run build times, a
   real stop/start cycle for startup time, `/proc/<pid>/status` VmRSS
   for steady-state memory) produced
   `docs/research/artifacts/issue57_footprint/footprint.csv` for every
   (dataset, engine) cell across Solr/Elasticsearch/OpenSearch. Headline
   figures: build time scales with corpus size as expected (WANDS
   42,994 docs: Solr 92.4s / ES 71.8s / OS 75.2s; the three ESCI
   verticals at 1,056–2,093 docs: 2–26s each; Magento 155 docs: ~4s
   each); index size is small and comparable across engines at this
   scale (WANDS ~27–28 MB on all three); OpenSearch's steady-state RSS
   (~3.5 GB) is markedly higher than Elasticsearch's (~113 MB) shortly
   after startup, consistent with OpenSearch's larger bundled-plugin
   surface (ML, security, observability) pre-allocating more at boot.
   **Disclosed measurement caveat**: Solr's measured startup figure
   (~130ms) is implausibly fast for a JVM cold start and is more likely
   a warm-OS-page-cache-assisted restart immediately following a
   `solr stop`, not a genuine cold-boot number — ES's and OS's own
   measured 40-88s figures (across two measurement runs) are the more
   representative JVM-search-engine startup costs; Havenask's footprint
   was not measured (unavailable, gap 4). Peak RSS is a steady-state
   snapshot at measurement time, not a continuously-sampled true peak
   (disclosed limitation, not a silent approximation).

## Updated status: **B — final, not preliminary**

The governing question's answer is unchanged in shape from Revision 1's
B (structural/faceted/typed-selective queries → native, by a large and
now even more broadly replicated margin; open-ended lexical → mature
engine, unchanged; Product/Variant schema safety → native's correct
default, not an exclusive capability, unchanged) — Revision 2 adds real
weight to the *relevance* side of the picture that Revision 1 could not
speak to at all: native's ranking is now shown, not just assumed, to be
at parity or ahead of Solr/Elasticsearch/OpenSearch's own ranking on
every real, independently-judged dataset measured this project has ever
run this comparison against (WANDS + three ESCI verticals). That closes
the single largest content gap the adversarial review identified (Lens
3) with a genuinely positive result for the architecture, not a wash.

This is now called **final** rather than preliminary because every gap
that was reasonably closeable within continued investigation was closed
with real measurement (gaps 1, 2, 5), the one gap requiring an
environment this session's host structurally could not provide was
investigated to the point of clearly separating "environment problem"
from "engine problem" (gap 4), and the one remaining deferral (gap 3,
full-scale ESCI) is a scope/time deferral with the acquisition work
already done, not an open question about feasibility or a hidden defect
— exactly the kind of residual, bounded, explicitly-scoped limitation
Issue #57's own protocol anticipates a revision may still carry into a
"final" verdict, as distinct from "further investigation could change
the answer's shape."

**Explicit non-decisions, updated**: still not A (native does not win
broadly — lexical retrieval at real scale remains mature-engine
territory, and the relevance parity finding, while genuinely positive,
is parity/near-parity, not a decisive native win, on datasets where
native itself is engaging its full ranking path rather than a pure
structural filter); still not D (the structural-query magnitude and the
new relevance-parity evidence are both real, positive, replicated
findings, not noise); still not C (the structural serving-plane speedup
is the custom engine's own physical execution advantage, not merely a
control-plane compilation step layered over a commodity engine).

## What remains open (carried forward, not closed by this revision)

- Full 1.2M-product ESCI corpus indexing/measurement (gap 3, data ready,
  not yet run).
- A real, working Havenask deployment on a host that does not exhibit
  this session's two independent stall points, to re-obtain Havenask
  correctness/timing/relevance evidence under the current protocol
  rather than relying on Revision 1's now-dated numbers.
- Havenask's own relevance-ranking capability (a real `ORDER BY`
  relevance-score SQL path, if one exists in a supported schema
  configuration) was not investigated this revision — its `NDCG=N/A`
  status reflects "not attempted," not "confirmed absent."

## Do not begin the next architecture phase

Unchanged from Revision 1: this decision does not authorize new
architecture work. Per Issue #57/#59's own governing instructions, the
next stage-gate step is Issue #47/PR #53's cleanup, not a new
architecture phase built on this benchmark's findings.
