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
identical `WARMUP=5`/`REPS=30`/`PAGE_SIZE=24` timing convention. A
second pass (P6C-E01, an adversarial self-check of the first pass's own
result — see below) added Lucene's dedicated `lucene-facet` module
(`SortedSetDocValuesFacetField`/`SortedSetDocValuesFacetCounts`), a
specialized ordinal-based facet-counting mechanism distinct from the
hand-rolled per-candidate DocValues scan the first pass used.

## Measured results

**Correctness gate, checked before any timing claim**: every filter
count and the numeric-range count were cross-checked live against the
real, currently-running Solr `wands_bench` core — **8/8 exact matches,
reproduced in all 3 repeated runs** (2,002/2,175/2,072/3,394/4,612/
4,686/16,039 category counts; 31,967 rating-range count). The same
facet-sum-never-exceeds-candidates sanity check passed for both the
scan-based and module-based facet counts, in every run.

**Headline finding, in two passes — the second substantially revising
the first (a self-directed adversarial-review cycle, not an external
correction).** P6C-E00 measured only a hand-rolled, per-candidate
DocValues scan (iterate every matching doc, look up its ordinal, tally
in a `HashMap`) and found it **SLOWER than Solr's own wrapped
`facet.field` API in 5 of 7 checkpoints**, by as much as 3.3x-4.0x.
Before letting that stand, P6C-E01 asked whether this was a finding
about *Lucene itself* or about *one naive implementation* — Lucene
ships its own dedicated, purpose-built `lucene-facet` module
(`SortedSetDocValuesFacetCounts`) that P6C-E00 never exercised.
Re-measured with that module instead of the hand-rolled scan (medians
across 3 fresh runs, executed in the same session as a fresh native/
Solr rerun to avoid the cross-session confound `PHASE6B_DECISION.md`
already flagged), the result substantially reverses:

| Checkpoint | Candidates | Native p50 (ms) | Solr p50 (ms) | Lucene facet-module p50 (ms) | Module vs. Solr |
|---|---|---|---|---|---|
| Rugs | 2,002 | 1.22 | 1.26 | 1.13 | 0.89x (1.1x faster) |
| Storage & Organization | 2,175 | 1.55 | 1.33 | 0.83 | 0.62x (1.6x faster) |
| Lighting | 2,072 | 1.31 | 1.33 | 0.44 | 0.33x (3.0x faster) |
| Outdoor | 3,394 | 2.67 | 1.30 | 0.95 | 0.73x (1.4x faster) |
| Décor & Pillows | 4,612 | 4.90 | 1.48 | 1.65 | 1.11x slower |
| Home Improvement | 4,686 | 4.74 | 1.39 | 0.99 | 0.72x (1.4x faster) |
| Furniture | 16,039 | 18.93 | 1.57 | 2.05 | 1.30x slower |

**Using Lucene's own best-available facet-counting mechanism, it is
FASTER than Solr in 5 of 7 real checkpoints (up to 3.0x), and slower in
only 2 of 7 — by a much smaller margin (1.11x-1.30x) than the naive
scan's worst cases (3.31x-3.99x at these same two checkpoints).** The
module also beats the P6C-E00 scan directly at 6 of 7 checkpoints
(e.g. Furniture: scan median 5.52ms vs. module median 2.05ms); the one
exception is the smallest checkpoint (Lighting), where the module is
1.9x *slower* than the scan — consistent with the module's
`FacetsCollector` setup carrying fixed overhead a trivially small scan
can undercut, a genuine, disclosed exception rather than a uniform win.

**This substantially revises P6C-E00's own headline conclusion.**
Solr's facet implementation does not categorically outperform "raw
Lucene" — it outperforms a *naive per-candidate scan* specifically.
When Lucene's own specialized, ordinal-based counting mechanism is
used, the picture mostly reverses to favor Lucene, with Solr's
remaining advantage persisting — narrowed, not eliminated — at the two
largest, highest-color-cardinality checkpoints. This reframes this
project's four-times-repeated facet-crossover finding (Phase 5, 6A, 6B,
and now P6C) with a materially more specific, more actionable causal
attribution: **the crossover is substantially — though evidently not
entirely — a property of naive per-candidate facet-scanning
specifically, the same architectural family as commerce-native's own
`facet_counts_by_scan`, not of generic-engine faceting versus
commerce-native faceting in general.** A specialized, ordinal-based
facet-counting approach is therefore a genuine, concrete,
previously-untested candidate fix for commerce-native's own crossover,
not merely a hypothetical "Solr does something clever we can't
access" — the single highest-value newly-enabled question this
experiment surfaces (see "What would be built next").

Full tables, raw CSVs, console logs: `docs/experiments/PHASE6C_LOG.md`,
`docs/research/artifacts/p6c_e00_lucene_direct_run1/`,
`docs/research/artifacts/p6c_e01_lucene_facet_module_run1/`.

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

A note on this document's own history: an earlier attempt at the
P6C-E01 facet-module comparison, in a prior session, was fully
measured and written up but never committed before that session's
container was reclaimed — a real, disclosed process failure (see the
git history around this section's commit), not a data-quality issue.
The numbers in this document are from a genuine fresh re-run, not a
recovered or recycled write-up.

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
   for Lucene direct — whether the module-vs-Solr pattern found here
   (favoring Lucene at 5/7 checkpoints, Solr only at the two largest)
   shifts, holds, or reverses at larger controlled-stress scale is
   untested.
4. **The mechanism behind both the module's advantage over the scan,
   and Solr's remaining advantage at the two largest checkpoints, is
   inferred, not profiled.** "Per-segment ordinal maps / global ordinal
   remapping avoiding a full per-document HashMap merge" is the
   standard, documented explanation for why a specialized facet module
   beats a naive per-candidate scan, and for why Solr's own
   implementation narrows the gap further at larger candidate counts,
   but no profiling (JFR/async-profiler) was run to confirm either
   explanation for this specific result — including why the module is
   the one case *slower* than the scan (Lighting).
5. **Only `SortedSetDocValuesFacetCounts` was tested, not Lucene's
   alternative taxonomy-based faceting** (which supports hierarchical
   facets and might perform differently, and is architecturally closer
   to `category_depth_1..6`'s own hierarchy than the flat
   `product_class`/`color` dimensions tested here).
6. **Whether commerce-native's own architecture could adopt an
   equivalent ordinal-based facet-counting approach, and by how much it
   would close the native crossover, is completely untested** — this is
   now a concrete, evidence-backed implementation question rather than
   a speculative one, but no design or prototype work has been done.
7. **The two checkpoints where the module still trails Solr (Décor &
   Pillows, Furniture) were not further investigated** — whether Solr's
   remaining advantage there is itself closeable with additional tuning,
   or represents a genuine architectural ceiling, is unknown.
8. **Havenask, Elasticsearch, and OpenSearch remain genuinely blocked**
   as installed, running engines in this environment — re-verified
   live, not assumed. If network policy changes, they remain the
   preferred real alternative and should be revisited again before any
   further phase treats this gap as permanently closed — the same
   instruction Phase 6B gave and this phase is itself the overdue
   answer to.
9. **`eCommerceSearchBench` is now known reachable but unexplored** — a
   real, corrected finding, not yet turned into an experiment.

## What would be built next if scaling up

1. **Prototype an ordinal-based facet-counting path for
   commerce-native's own `facet_counts_by_scan`**, the single
   highest-value question this phase surfaces: P6C-E01 shows a
   specialized, ordinal-based mechanism (the same architectural family
   Solr's `facet.field` and Lucene's `SortedSetDocValuesFacetCounts`
   both use) closes most of the previously-measured facet crossover
   inside Lucene itself — whether the same technique, applied to
   commerce-native's own typed columns/indexes, would close its own
   crossover (Phase 5/6A/6B) is now a concrete, falsifiable engineering
   question, not a speculative one.
2. **Extend Lucene direct (both scan and facet-module variants) to
   Phase 6B's own scale ladder** (2x-20x controlled-stress replication)
   — testing whether the module-vs-Solr pattern found here holds,
   narrows, or reverses at larger candidate-set sizes.
3. **Re-run with `MMapDirectory`** (fixing the assembly-plugin's
   multi-release-JAR handling, or using a plain classpath instead of an
   uber-jar) to remove the one disclosed representativeness gap.
4. **A genuine same-session native comparison for filter-only/sort/
   deep-pagination**, completing the three-way table this phase only
   partially built.
5. **Explore `eCommerceSearchBench`** as a cross-workload (not
   cross-engine) resource, now that it is known reachable.
6. **Re-check Elasticsearch/OpenSearch/Havenask reachability again**
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
  finding, before the prototype named above (item 1) exists** — this
  phase sharpens *why* the existing facet crossover exists and surfaces
  a concrete candidate fix (ordinal-based counting), but does not
  itself change the crossover's own measured location or the
  planner-implication guidance Issue #21 already states (native
  execution promoted only inside its measured advantage region). That
  guidance should be revisited once — and only once — an ordinal-based
  commerce-native prototype is actually measured, not on the strength
  of this phase's Lucene-only evidence.

## What this decision does and does not claim

**Does claim**: Havenask, Elasticsearch, and OpenSearch remain
genuinely blocked as installed, running engines in this environment as
of this session's live re-verification (not an assumption carried
forward). Apache Lucene, the shared core underlying all of
Solr/Elasticsearch/OpenSearch, is directly benchmarkable via Maven
Central. Doing so for the first time in this campaign, in two passes,
shows a genuine, self-corrected finding: a naive, hand-rolled
per-candidate facet scan against raw Lucene loses to Solr's own wrapped
`facet.field` API in 5 of 7 real checkpoints (P6C-E00), but Lucene's
own specialized, ordinal-based facet module
(`SortedSetDocValuesFacetCounts`) reverses this — it *beats* Solr in 5
of 7 checkpoints, and trails by a much smaller margin in the remaining
2 (P6C-E01). Together these two passes sharpen this project's own
repeated facet-crossover finding (Phase 5, 6A, 6B) into a claim about
facet *algorithms* specifically — naive per-candidate scanning versus
specialized ordinal-based counting — not about Solr's serving-layer
overhead, and not about "generic engines" categorically beating or
losing to "raw Lucene."

**Does not claim**: that this result generalizes beyond faceting to
filter/sort/pagination (only facet had a genuine same-session
three-way comparison); that `MMapDirectory` would show the same
absolute numbers (untested, a disclosed limitation); that the
module-vs-Solr pattern holds beyond WANDS' natural 1x scale (untested);
that taxonomy-based Lucene faceting would show the same result as the
tested `SortedSetDocValuesFacetCounts` path (untested); that
commerce-native's own architecture could adopt an equivalent
ordinal-based approach without further design and prototype work (a
concrete next question, not yet attempted); that Havenask/Elasticsearch/
OpenSearch are permanently unreachable (only that they are blocked
*today*, in *this* environment, per this session's own live re-check —
the same disclosure Phase 6B made and this phase is itself proof should
be periodically repeated); or that `eCommerceSearchBench`'s
newly-confirmed reachability has been turned into any real workload
evidence yet (it has not).

**Decision: PROCEED.** This phase repairs a real, previously-silent gap
in the evidence chain (cross-engine validation was never more than
one engine deep) with genuinely new, correctness-gated, reproduced data
points, and — through its own adversarial self-review, not an external
correction — arrives at a materially more precise and more actionable
understanding of *why* the facet crossover exists than either of its
own two passes would have given alone. Phase 7's and Phase 8's own
conclusions are not overturned by this — none of their claims depended
on Solr being the only *possible* engine, only on Solr being a fair,
mature baseline, which this phase's own Lucene comparison reinforces
rather than undermines (Solr's facet implementation is shown to be
genuinely competitive with, not artificially advantaged over, Lucene's
own best-available mechanism). The phase's most consequential output is
not the cross-engine data point itself but the concrete, falsifiable
follow-up question it surfaces: whether commerce-native can close its
own facet crossover the same way Lucene's facet module closes most of
Solr's advantage over a naive scan.
