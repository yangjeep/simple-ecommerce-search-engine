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
