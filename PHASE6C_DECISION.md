# Phase 6C Decision (Issue #21 Phase 6, extending 6A/6B — a repaired evidence-chain gap)

**Decision: PROCEED**, with a genuinely new, previously-missing
cross-engine data point added to the evidence chain, and one prior
hypothesis about *why* the native facet crossover exists corrected
rather than merely narrowed.

This document exists because a fresh, whole-campaign re-audit (prompted
by an explicit user request to re-verify cross-engine validation rather
than assume prior "blocked" verdicts still hold) found that
`PHASE6B_DECISION.md`'s own "Unresolved risks" #4 — *"if network policy
changes, [Havenask/Retailrocket/H&M/Amazon Reviews 2023] remain the
preferred real alternative... and should be revisited before Phase
7"* — was never actually revisited. Phase 7 and Phase 8 both proceeded
with Solr as the only lexical-backend evidence. This document is that
overdue revisit, executed now rather than left as a silent gap
underneath five more phases of conclusions, per the explicit instruction
to repair the evidence chain before building further on top of it.

## Recap: what this phase was asked to answer

Two questions, both stated by the user's own audit request rather than
by a GitHub issue: (1) has cross-engine validation actually been
completed, given Solr has been the only baseline engine through Phase
8, and (2) what does live re-verification of every previously-named
"blocked" resource (Havenask, Elasticsearch, OpenSearch, Retailrocket,
H&M, Amazon Reviews 2023, eCommerceSearchBench) show in *this*
environment, today, rather than trusting historical claims.

## Live re-verification (this session, not a re-read of old claims)

Full table in `docs/experiments/PHASE6C_LOG.md`. Summary:

- **Havenask**: still blocked on the same two independent grounds Phase
  6B found (Docker daemon absent; `registry.cn-hangzhou.aliyuncs.com`
  unreachable), re-tested live, unchanged. Source is reachable via
  `git clone` (also unchanged) but a from-source build needs `bazel`
  (not installed) plus a large, distributed-system-scale dependency
  graph — not attempted, consistent with `CLAUDE.md`'s distributed-
  systems sequencing rule.
- **Elasticsearch** and **OpenSearch**: newly tested this phase, never
  previously attempted. Both engines' official prebuilt distributions
  are blocked (`artifacts.elastic.co`, `artifacts.opensearch.org`, both
  403). Both are open-source and buildable from GitHub source (also
  newly confirmed reachable via `git clone`), but OpenSearch's own
  build hits a **second, independent** blocker before dependency
  resolution even completes: its bundled-JDK provider
  (`api.adoptium.net`) is also unreachable (403). This is a materially
  larger, multi-blocker undertaking than Havenask's own — not attempted
  further.
- **Retailrocket, H&M, Amazon Reviews 2023**: re-tested live, all still
  blocked (`kaggle.com`, `huggingface.co`, both 403), unchanged from
  Phase 6B.
- **`eCommerceSearchBench`**: Phase 6B recorded this as "no accessible,
  confidently-identified source located." A fresh search found it —
  `github.com/alibaba/eCommerceSearchBench`, reachable via `git clone`
  — a real correction to a prior "unresolved" status. It is a synthetic
  Taobao-style data/workload generator, not a search-engine baseline,
  so it does not itself supply a cross-engine data point; it is a
  genuine open item for a future cross-*workload* experiment, not
  pursued further here.
- **Maven Central** (`repo1.maven.org`): fully reachable, newly tested
  — the one genuinely new, fully-feasible opportunity this audit found.
  Apache Lucene, the retrieval engine underlying Solr, Elasticsearch,
  and OpenSearch alike, is published there as a plain embeddable Java
  library — no server, no Docker, no bundled JDK, no distribution
  blocker of any kind.

## Architecture tested

A new, standalone Java/Maven module (`lucene-direct-bench/`) — the
first non-Rust, non-Solr engine implementation this project has ever
built for a benchmark. Indexes the real WANDS catalog with the same
dual indexed+docValues field pattern Solr's own schema uses, and
measures the same operation classes (`category_depth_1` filter,
product_class/color facet-scan, sort by title/rating, deep pagination,
numeric-range filter on `average_rating`) at the same 7 real
`category_depth_1` checkpoints P6A-E00/P6B-E00 both used, with the
identical `WARMUP=5`/`REPS=30`/`PAGE_SIZE=24` timing convention.

## Measured results

**Correctness gate, checked before any timing claim**: every filter
count and the numeric-range count were cross-checked live against the
real, currently-running Solr `wands_bench` core — **8/8 exact matches,
reproduced in all 3 repeated runs** (2,002/2,175/2,072/3,394/4,612/
4,686/16,039 category counts; 31,967 rating-range count).

**The central, counter-intuitive finding**: for the one operation
measured as a genuine same-session three-way comparison (color
facet-scan under category filter, native vs. Solr vs. raw Lucene —
full table in `PHASE6C_LOG.md`), **raw Lucene direct is SLOWER than
Solr's own wrapped `facet.field` API in 5 of 7 checkpoints**, by as
much as 3.3x-4.0x, and faster in only 2 of 7 (one of those by a real
5.3x margin) — a pattern that reproduced consistently across all 3
runs, not run-to-run noise.

**This falsifies the naive version of this experiment's own
hypothesis.** The experiment was designed to test whether Solr's HTTP/
schema/wrapper layer was masking part of the native-vs-generic-engine
gap this project has repeatedly measured. It was not: Solr's own
faceting implementation frequently *outperforms* a straightforward,
correctly-implemented, DocValues-backed scan against the identical raw
Lucene index. This is mechanistically consistent with — and sharpens
the causal attribution of — this project's own repeated finding
(Phase 5, 6A, 6B) that a naive per-candidate facet scan loses to Solr
past a cardinality/complexity threshold: the crossover is evidence
about facet *algorithms* (a naive scan vs. Solr's mature, specialized
implementation), not evidence that Solr's serving-layer overhead was
ever unfairly inflating the comparison against commerce-native.

Full tables, raw CSVs, console logs: `docs/experiments/PHASE6C_LOG.md`,
`docs/research/artifacts/p6c_e00_lucene_direct_run1/`.

## Failed / fixed experiments (preserved, not erased)

A real build-tooling issue, not a methodology error: the first build
used `FSDirectory.open()` (Lucene's recommended default, auto-selecting
`MMapDirectory`), which threw `LinkageError:
MemorySegmentIndexInputProvider is missing in Lucene JAR file` at
runtime — the Maven Assembly-produced uber-jar does not preserve
Lucene's multi-release-JAR structure its Panama/mmap implementation
needs. Fixed by switching to `NIOFSDirectory`, a standard, fully-
supported, non-mmap Lucene backend — disclosed as a real, if minor,
methodology choice (see "Unresolved risks" below), not silently worked
around.

## Unresolved risks

1. **Only one operation (color facet-scan) has a genuine same-session
   three-way (native/Solr/Lucene) comparison.** Filter-only and
   numeric-range were correctness-gated against Solr but have no fresh
   same-session native number at these exact checkpoints; sort/deep-
   pagination have Lucene-only timing, correctness-gated only via the
   count checks.
2. **`NIOFSDirectory`, not `MMapDirectory`, was used**, for a build-
   tooling reason (see above), not a deliberate representativeness
   choice. Solr itself uses `MMapDirectory` in this same environment.
   Whether Lucene's own mmap path would show materially different
   absolute numbers is untested — a real, disclosed limitation on the
   absolute (not directional) comparison.
3. **Only WANDS at its natural 1x scale was tested.** The Phase 6B
   scale-ladder (2x-20x controlled-stress replication) was not repeated
   for Lucene direct — whether the native-loss facet crossover's
   candidate-count location shifts, holds, or the Lucene-vs-Solr
   pattern found here changes at larger controlled-stress scale is
   untested.
4. **The mechanism behind Solr's faceting advantage is inferred, not
   profiled.** "Per-segment ordinal maps / global ordinal remapping" is
   the standard, documented explanation for why Solr's `facet.field`
   beats a naive per-candidate DocValues scan, but no profiling
   (JFR/async-profiler) was run to confirm this specific explanation for
   this specific result.
5. **Havenask, Elasticsearch, and OpenSearch remain genuinely blocked**
   as installed, running engines in this environment — re-verified
   live, not assumed. If network policy changes, they remain the
   preferred real alternative and should be revisited again before any
   further phase treats this gap as permanently closed — the same
   instruction Phase 6B gave and this phase is itself the overdue
   answer to.
6. **`eCommerceSearchBench` is now known reachable but unexplored** — a
   real, corrected finding, not yet turned into an experiment.

## What would be built next if scaling up

1. **Extend Lucene direct to Phase 6B's own scale ladder** (2x-20x
   controlled-stress replication) — the natural, most direct follow-up,
   testing whether the facet-algorithm finding above holds at larger
   candidate-set sizes the same way Phase 6B confirmed for native.
2. **Re-run with `MMapDirectory`** (fixing the assembly-plugin's
   multi-release-JAR handling, or using a plain classpath instead of an
   uber-jar) to remove the one disclosed representativeness gap.
3. **A genuine same-session native comparison for filter-only/sort/
   deep-pagination**, completing the three-way table this phase only
   partially built.
4. **Explore `eCommerceSearchBench`** as a cross-workload (not
   cross-engine) resource, now that it is known reachable.
5. **Re-check Elasticsearch/OpenSearch/Havenask reachability again**
   before any future phase that would otherwise treat single-engine
   (Solr) validation as sufficient — this is now the second time this
   project has had to fulfill a "should be revisited" instruction from
   its own prior decision document; a lighter-weight, periodic
   re-check (not a full phase) may be worth adding to this project's own
   process discipline.

## What should explicitly not be built yet

- **A from-source OpenSearch/Elasticsearch build**, working around the
  bundled-JDK blocker by substituting the system JDK or vendoring a
  JDK from elsewhere — a real, nontrivial build-system engineering
  effort disproportionate to the incremental evidence value over what
  raw Lucene direct already supplies, since all three engines
  (Solr/ES/OpenSearch) share the same Lucene core this phase already
  tested directly.
- **A Havenask source build** — CLAUDE.md's own distributed-systems
  sequencing rule plus the `bazel`/dependency-graph blocker both argue
  against it; unchanged from Phase 6B's own conclusion.
- **Any planner/architecture change based on this phase's facet
  finding** — it sharpens *why* the existing facet crossover exists, it
  does not change the crossover's own measured location or the
  planner-implication guidance Issue #21 already states (native
  execution promoted only inside its measured advantage region).

## What this decision does and does not claim

**Does claim**: Havenask, Elasticsearch, and OpenSearch remain
genuinely blocked as installed, running engines in this environment as
of this session's live re-verification (not an assumption carried
forward). Apache Lucene, the shared core underlying all of
Solr/Elasticsearch/OpenSearch, is directly benchmarkable via Maven
Central, and doing so for the first time in this campaign shows that
Solr's own facet implementation frequently *outperforms* a
straightforward, correctly-implemented, DocValues-backed Lucene scan
(slower in 5 of 7 real checkpoints, by up to 3.3x-4.0x) — falsifying
the hypothesis that Solr's wrapper overhead was masking part of the
native-vs-generic-engine gap, and instead sharpening this project's own
repeated facet-crossover finding into a claim about facet *algorithms*
specifically, not serving-layer overhead.

**Does not claim**: that this result generalizes beyond faceting to
filter/sort/pagination (only facet had a genuine same-session
three-way comparison); that `MMapDirectory` would show the same
absolute numbers (untested, a disclosed limitation); that the
scale-ladder pattern holds beyond WANDS' natural 1x scale (untested);
that Havenask/Elasticsearch/OpenSearch are permanently unreachable
(only that they are blocked *today*, in *this* environment, per this
session's own live re-check — the same disclosure Phase 6B made and
this phase is itself proof should be periodically repeated); or that
`eCommerceSearchBench`'s newly-confirmed reachability has been turned
into any real workload evidence yet (it has not).

**Decision: PROCEED.** This phase repairs a real, previously-silent gap
in the evidence chain (cross-engine validation was never more than
one engine deep) with a genuinely new, correctness-gated, reproduced
data point, and corrects rather than merely narrows this project's own
understanding of *why* the facet crossover exists. Phase 7's and Phase
8's own conclusions are not overturned by this — none of their claims
depended on Solr being the only *possible* engine, only on Solr being a
fair, mature baseline, which this phase's own Lucene comparison
reinforces rather than undermines (Solr's facet implementation is
shown to be *better*, not artificially advantaged by wrapper overhead).
