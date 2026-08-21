# Phase 6E Decision (Issue #21 Phase 6, extending 6A/6B/6C/6D — a repaired evidence-chain gap)

**Decision: PROCEED**, with two genuinely new cross-engine data points —
the first real Elasticsearch and OpenSearch measurements in this
research campaign's history — and a correction to Phase 6C's own
"genuinely blocked" verdict for both engines.

This document exists because the user's own standing research-loop
instruction was explicit: cross-engine validation against Solr alone is
"not enough," and Elasticsearch/OpenSearch/Havenask should be revisited
with a genuine attempt at what is feasible in this environment before
building further conclusions on top of an unqualified "blocked" verdict.
Phase 6C's own cross-engine audit (`PHASE6C_DECISION.md`) tested two
routes for Elasticsearch — the official prebuilt distribution and a
from-source build — and found both blocked. It never tried the route
that made P6C-E00's own raw-Lucene benchmark possible: fetching the
engine's published library JARs directly from Maven Central. This phase
is that missing attempt.

## What this phase found

**Elasticsearch's server library and test-framework JARs
(`org.elasticsearch:elasticsearch:8.15.0`,
`org.elasticsearch.test:framework:8.15.0`) resolve cleanly from Maven
Central in this environment** — a fact Phase 6C never tested, since it
only checked the prebuilt distribution and from-source build routes.
Confirmed via `mvn dependency:get` and a full `mvn dependency:tree`
(`BUILD SUCCESS`, 83 resolved artifacts, zero errors). The same is true
for OpenSearch's equivalent artifacts.

**A real, embedded, single-node Elasticsearch 8.15.0 cluster
successfully bootstraps in-process** using the test framework's own
`ESSingleNodeTestCase` machinery, after fixing two concrete, disclosed,
one-time blockers — neither a network/environment restriction:

1. A jar-hell conflict between this module's own `junit` dependency's
   transitive `hamcrest-core:1.3` and the framework's own
   `hamcrest:2.1` (fixed with a Maven dependency exclusion).
2. JDK 17+'s refusal to install a legacy `SecurityManager` without
   `-Djava.security.manager=allow` (JEP 411; fixed via a Surefire
   `argLine` flag).

Two further, similarly one-time fixable issues appeared when the
benchmark itself tried to read the real catalog and write results: ES's
own test-framework `SecurityManager` policy only grants file access
under `java.io.tmpdir` and a few ES-internal paths, not arbitrary
repo-relative paths outside its expected Gradle project layout — fixed
by relocating both the catalog input and the CSV output through
`/tmp`.

**A real benchmark against the real WANDS catalog then ran cleanly, 3
independent times, with zero correctness mismatches**: all 7 depth-1
category filter counts and the whole-catalog numeric-range count
matched Solr's own live response exactly, every run. See
`docs/experiments/PHASE6E_LOG.md#P6E-E00` for the full hypothesis,
design, and blocker-by-blocker account.

## Measured results

**Correctness gate (the precondition for trusting any timing below)**:
8 distinct count checks (7 category filters + 1 numeric range), 3
independent runs each = 24 checks, **24/24 exact matches against
Solr's live `numFound`**.

**Timing — the one operation with a clean same-session Solr
comparison** (color facet count under each of the 7 real depth-1
category checkpoints; Solr numbers from the same session's fresh
`p6a_e00_wands_vs_native_eval` run used for P6D-E03):

| Checkpoint | n | ES p50 (ms) | Solr p50 (ms) | ES vs. Solr | commerce-native ordinal (ms) |
|---|---|---|---|---|---|
| Rugs | 2,002 | 2.08-2.16 | 1.19 | 1.7-1.8x slower | 0.031 |
| Storage & Organization | 2,175 | 1.37-1.56 | 1.15 | 1.2-1.4x slower | 0.029 |
| Lighting | 2,072 | 1.22-1.38 | 0.97 | 1.3-1.4x slower | 0.023 |
| Outdoor | 3,394 | 1.37-1.56 | 1.00 | 1.4-1.6x slower | 0.073 |
| Décor & Pillows | 4,612 | 1.99-2.22 | 1.10 | 1.8-2.0x slower | 0.199 |
| Home Improvement | 4,686 | 1.22-1.30 | 1.06 | 1.2-1.3x slower | 0.089 |
| Furniture | 16,039 | 1.55-1.58 | 1.18 | 1.3-1.3x slower | 0.246 |

Embedded Elasticsearch's terms-aggregation is **slower than Solr's own
`facet.field`** at every checkpoint tested (1.2x-2.0x) — a genuinely new
and somewhat counter-intuitive finding, since both engines sit on the
same Lucene core. commerce-native's own ordinal method remains
dramatically faster than both (by extension, faster than ES as well as
Solr), which reinforces rather than complicates this project's existing
Phase 6D color-facet finding with a fourth engine's data point, not a
contradiction of it.

## P6E-E01: the same route, attempted for OpenSearch

The same Maven-library route was then attempted for OpenSearch, since
its library and test-framework JARs (`org.opensearch:opensearch:2.17.0`,
`org.opensearch.test:framework:2.17.0`) had already been confirmed
resolvable from Maven Central alongside Elasticsearch's own. **A real,
embedded, single-node OpenSearch 2.17.0 cluster also boots and serves a
real benchmark against the real WANDS catalog** — but only after fixing
four more concrete, disclosed blockers distinct from P6E-E00's four
(missing `log4j-core`, wrong pre-8.x `xcontent` package paths, no
`decRef()` method on `SearchResponse`, and — the most consequential —
**OpenSearch's own bootstrap hard-refuses to run as root**, unlike
Elasticsearch's equivalent check, which only warned under the identical
condition in this container. Running as this container's existing
non-root `ubuntu` account fixed the root-refusal, but that in turn
activated OpenSearch's `SecurityManager` network restrictions, denying
the benchmark's live Solr HTTP cross-check — **a fifth blocker this pass
did NOT fix**, disclosed rather than silently worked around. See
`docs/experiments/PHASE6E_LOG.md#P6E-E01` for the full account.

Correctness for this engine's numbers rests on indirect corroboration:
every candidate count OpenSearch reported (2,002 / 2,175 / 2,072 / 3,394
/ 4,612 / 4,686 / 16,039 / 31,967, across all 3 runs) matches
digit-for-digit the Solr-verified ground truth P6E-E00's own live check
already established for the identical checkpoints — not the same
strength of evidence as a live in-process check, and reported as such.

| Checkpoint | n | OpenSearch color-facet p50 (3 runs, ms) | ES color-facet p50 (P6E-E00, ms) | Solr p50 (ms) | OpenSearch vs. Solr |
|---|---|---|---|---|---|
| Rugs | 2,002 | 1.97-2.29 | 2.08-2.16 | 1.19 | 1.7-1.9x slower |
| Storage & Organization | 2,175 | 1.38-1.55 | 1.37-1.56 | 1.15 | 1.2-1.35x slower |
| Lighting | 2,072 | 1.23-1.29 | 1.22-1.38 | 0.97 | 1.27-1.33x slower |
| Outdoor | 3,394 | 1.38-1.60 | 1.37-1.56 | 1.00 | 1.4-1.6x slower |
| Décor & Pillows | 4,612 | 2.15-2.47 | 1.99-2.22 | 1.10 | 2.0-2.25x slower |
| Home Improvement | 4,686 | 1.36-1.76 | 1.22-1.30 | 1.06 | 1.3-1.7x slower |
| Furniture | 16,039 | 1.53-1.92 | 1.55-1.58 | 1.18 | 1.3-1.6x slower |

**OpenSearch's terms-aggregation is also slower than Solr's own
`facet.field`** at every checkpoint (1.2x-2.25x) — the same qualitative
finding as Elasticsearch's own, with broadly comparable absolute
numbers between the two engines (not measured in the same
process/session for either). This is not an Elasticsearch-8.x
peculiarity: the pattern reproduces on the other major Lucene-based
distributed search engine too, strengthening rather than narrowing the
P6E-E00 finding. commerce-native's own ordinal method remains
dramatically faster than both.

## Correction to `PHASE6C_DECISION.md`

Phase 6C's "Live re-verification" section and its "Does claim" /
headline verdict state Elasticsearch and OpenSearch "remain genuinely
blocked." That statement is now **incomplete, not false** — it was true
for the two routes Phase 6C actually tested (prebuilt distribution,
from-source build), but a third route (the Maven-library route this
phase used) was never tried and turns out to work, for **both**
Elasticsearch and OpenSearch. Per this project's own "do not erase
evidence because an approach was abandoned" / "do not rewrite history"
discipline, `PHASE6C_DECISION.md`'s own text is left as the historical
record of what that phase actually tested; this document is the
correction and extension, cross-referenced from here rather than edited
into Phase 6C's own text after the fact.

## Failed/fixed experiments (disclosed, not erased)

**Elasticsearch (P6E-E00)**: four real, concrete blockers were hit and
fixed in sequence: a Maven dependency jar-hell conflict, JDK 21's
SecurityManager opt-in requirement, and two file-permission denials from
ES's own test-framework security policy. All four are now permanently
fixed in `es-direct-bench/`'s own `pom.xml`/code.

**OpenSearch (P6E-E01)**: four more, distinct blockers: a missing
`log4j-core` dependency, OpenSearch's own bootstrap hard-refusing to run
as root (fixed by running as this container's non-root `ubuntu`
account, unlike ES's equivalent check, which only warned under the
identical condition), wrong pre-8.x `xcontent` package paths, and a
missing `decRef()` method (an ES-8.x-only API the ported code initially
carried over). **A fifth blocker was found but NOT fixed**: running as
non-root activates OpenSearch's `SecurityManager` network restrictions,
which deny the benchmark's live Solr HTTP cross-check — disclosed as an
open limitation, not silently worked around; correctness for this
engine's numbers rests on cross-process candidate-count corroboration
against P6E-E00's own already-verified counts instead.

None of the nine blockers across both engines was a genuine
network/environment restriction unrelated to this specific
Maven-library-embedding approach. The raw console logs from every
failing attempt are archived at
`docs/research/artifacts/p6e_e00_es_direct_run1/blockers_found_and_fixed/`
and
`docs/research/artifacts/p6e_e01_opensearch_direct_run1/blockers_found_and_fixed/`
alongside each engine's three clean final runs, matching this project's
"record failed experiments" rule.

## Unresolved risks

1. **Resolved by P6E-E01, with a real qualifier**: OpenSearch's
   embedded-node bootstrap was attempted and succeeds, using a
   modified four-fix playbook (three of the four fixes were genuinely
   new, not transfers of P6E-E00's own four) — but its own live Solr
   correctness cross-check could not be made to work in this pass (the
   new blocker #5 above), so OpenSearch's numbers carry weaker
   correctness evidence (indirect, cross-process) than Elasticsearch's
   own (direct, 24/24 live matches).
2. **Only `color_facet_under_category` has a clean, same-session Solr
   timing comparison at these exact checkpoints, for either engine.**
   `filter_only`'s numbers are correctness-verified against Solr's live
   count (Elasticsearch) or indirectly corroborated (OpenSearch) but
   have no same-session Solr *timing* counterpart at these checkpoints
   — a real, disclosed gap. `product_class_facet_under_category`,
   `sort_title_asc`, `sort_rating_desc`, and `deep_pagination` are
   engine-only measurements in this pass, with no same-session
   Solr/native/Lucene-direct timing counterpart.
3. **No genuine same-session, same-binary comparison harness exists**
   across native/Solr/Lucene-direct/Elasticsearch/OpenSearch — each
   engine's numbers come from its own process/session; the
   Elasticsearch-vs-OpenSearch comparison in this document is
   illustrative, not controlled.
4. **The mechanism for both engines' aggregation being slower than
   Solr's own facet component is inferred, not profiled** — no
   JFR/perf/valgrind run confirms whether this is transport/action-layer
   overhead, aggregation-framework generality, or something else
   specific to these embedded configurations.
5. **Only single-node, single-shard topologies were tested for both
   engines** — consistent with `CLAUDE.md`'s "avoid distributed-systems
   work" rule, but real production deployments are typically
   multi-node/multi-shard, a materially different performance profile
   neither engine's measurement here speaks to.
6. **This is Elasticsearch 8.15.0 and OpenSearch 2.17.0 specifically,
   not a version sweep** — whether the same fixes and the same
   qualitative "aggregation slower than Solr facet.field" finding hold
   on other versions of either engine is untested.
7. **The OpenSearch network-permission blocker (unresolved risk #1) is
   itself unresolved** — a real fix (an additive security policy
   grant, or a different non-root sandboxing approach) was not found in
   this pass; only the workaround of accepting indirect correctness
   evidence instead.

## What would be built next if scaling up

1. **Fix the OpenSearch network-permission blocker** so its own live
   Solr cross-check can run in-process, bringing its correctness
   evidence up to the same standard as Elasticsearch's own 24/24 live
   match.
2. **Add a same-session, same-checkpoint Solr `filter_only` timing
   measurement** to close the gap named in risk #2, for a fully
   apples-to-apples comparison at every operation class, not just
   color-facet.
3. **Profile both engines' aggregation paths** (JFR/async-profiler) to
   confirm or refute the transport/action-layer-overhead hypothesis
   named above, and to determine whether the same mechanism explains
   both engines' shared slowdown or two different ones happen to look
   similar.
4. **A genuine one-binary comparison harness** (native, Solr,
   Lucene-direct, embedded ES, embedded OpenSearch) at the same
   checkpoints in the same process/session, removing the "different
   session" caveat every comparison in this document still carries.

## What should explicitly not be built yet

- **A multi-node or multi-shard Elasticsearch/OpenSearch topology** —
  this phase deliberately stayed single-node/single-shard for both
  engines, matching `CLAUDE.md`'s distributed-systems sequencing rule;
  the single-node thesis is not yet exhausted enough to justify this.
- **Wiring Elasticsearch or OpenSearch as a production dependency
  anywhere in this codebase** — these are benchmark-only comparison
  artifacts (`es-direct-bench/`, `opensearch-direct-bench/`), not a step
  toward adopting either as a real backend; no such proposal is implied
  by this phase's results.
- **A from-source OpenSearch/Elasticsearch build, or a Havenask
  from-source build** — CLAUDE.md's own "avoid production polish" and
  effort-proportionality principles apply equally here: the
  Maven-library route already answers the load-bearing question (can a
  real instance of each engine be measured in this environment?) more
  cheaply than a from-source build would, without the independent
  from-source blockers Phase 6B/6C already found for all three engines.

## Does / does not claim

**Does claim**: neither Elasticsearch nor OpenSearch is "genuinely
blocked" in this environment in the unqualified sense Phase 6C's own
headline stated — real, running embedded instances of both now exist,
with real (if narrow) timing comparisons against Solr for one operation
class each; that this is a genuinely new pair of cross-engine data
points supporting (not contradicting) commerce-native's own Phase 6D
color-facet finding; that all nine blockers hit across both engines
were concrete, understood, and (all but one) fixed, not silently worked
around.

**Does not claim**: that OpenSearch's numbers carry the same strength of
correctness evidence as Elasticsearch's own (indirect cross-process
corroboration vs. direct 24/24 live matches — a real, disclosed
difference, not an oversight); that either phase's single narrow timing
comparison (color facet under 7 depth-1 categories) generalizes to every
operation class this project measures; that the "aggregation slower
than Solr facet.field" finding is mechanistically understood rather than
observed, or that the same mechanism explains both engines' shared
result; that a multi-node topology would show the same profile for
either engine; that Havenask's from-source-build blockers (Phase 6B/6C,
unchanged) have been revisited in this phase (they have not — this
phase's scope was specifically the Maven-library route Phase 6C never
tried for Elasticsearch/OpenSearch, not a fresh Havenask attempt).

**Decision: PROCEED.** This phase directly answers the user's own
standing instruction to revisit Elasticsearch/OpenSearch rather than
accept an unqualified "blocked" verdict, finds a real and previously
untried route that works for **both** engines, and produces this
research campaign's first genuine Elasticsearch and OpenSearch data
points — both of which strengthen rather than complicate the existing
evidence chain.
