# Phase 6E Decision (Issue #21 Phase 6, extending 6A/6B/6C/6D — a repaired evidence-chain gap)

**Decision: PROCEED**, with a genuinely new cross-engine data point —
the first real Elasticsearch measurement in this research campaign's
history — and a correction to Phase 6C's own "genuinely blocked"
verdict for Elasticsearch specifically.

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

## Correction to `PHASE6C_DECISION.md`

Phase 6C's "Live re-verification" section and its "Does claim" /
headline verdict state Elasticsearch and OpenSearch "remain genuinely
blocked." That statement is now **incomplete, not false** — it was true
for the two routes Phase 6C actually tested (prebuilt distribution,
from-source build), but a third route (the Maven-library route this
phase used) was never tried and turns out to work, for Elasticsearch at
least. Per this project's own "do not erase evidence because an
approach was abandoned" / "do not rewrite history" discipline,
`PHASE6C_DECISION.md`'s own text is left as the historical record of
what that phase actually tested; this document is the correction and
extension, cross-referenced from here rather than edited into Phase
6C's own text after the fact.

## Failed/fixed experiments (disclosed, not erased)

Four real, concrete blockers were hit and fixed in sequence, each
documented above and in `docs/experiments/PHASE6E_LOG.md` with the exact
exception, cause, and fix: a Maven dependency jar-hell conflict, JDK
21's SecurityManager opt-in requirement, and two file-permission denials
from ES's own test-framework security policy. None of the four was a
genuine environment/network blocker — all four are now permanently
fixed in this module's own `pom.xml`/code, and a future run needs no
rediscovery. The raw console logs from the two earlier failing attempts
are archived at
`docs/research/artifacts/p6e_e00_es_direct_run1/blockers_found_and_fixed/`
alongside the three clean final runs, matching this project's "record
failed experiments" rule.

## Unresolved risks

1. **OpenSearch's library artifacts were confirmed resolvable from
   Maven Central (the same `mvn dependency:get` proof as Elasticsearch)
   but a full embedded-node bootstrap was not attempted** — OpenSearch
   forked from Elasticsearch 7.10, and its own test-framework bootstrap
   code differs enough (different package names, possibly different
   security-policy specifics) that the four fixes found here are not
   guaranteed to transfer unmodified.
2. **Only `color_facet_under_category` has a clean, same-session Solr
   timing comparison at these exact checkpoints.** `filter_only`'s ES
   numbers are correctness-verified against Solr's live count but have
   no same-session Solr *timing* counterpart at these checkpoints in
   this pass (no such operation exists in this session's archived Solr
   runs at exactly these checkpoints) — a real, disclosed gap.
   `product_class_facet_under_category`, `sort_title_asc`,
   `sort_rating_desc`, and `deep_pagination` are ES-only measurements in
   this pass, with no same-session Solr/native/Lucene-direct timing
   counterpart.
3. **No genuine same-session, same-binary four-way
   native/Solr/Lucene-direct/Elasticsearch timing comparison exists** —
   each engine's numbers come from its own process/session, correctness
   cross-checked against the same live Solr core as the common
   reference point, but not measured side-by-side in one harness.
4. **The mechanism for embedded ES's aggregation being slower than
   Solr's own facet component is inferred, not profiled** — no
   JFR/perf/valgrind run confirms whether this is transport/action-layer
   overhead, aggregation-framework generality, or something else
   specific to this embedded configuration.
5. **Only a single-node, single-shard ES topology was tested** —
   consistent with `CLAUDE.md`'s "avoid distributed-systems work" rule,
   but real production ES deployments are typically multi-node/
   multi-shard, a materially different performance profile this phase
   does not speak to.
6. **This is Elasticsearch 8.15.0 specifically, not a version sweep** —
   whether the same four fixes and the same qualitative "ES aggregation
   slower than Solr facet.field" finding hold on other ES versions
   (older or newer major versions) is untested.

## What would be built next if scaling up

1. **Attempt the same embedded-node approach for OpenSearch**, applying
   the same four-fix playbook and documenting where it diverges.
2. **Add a same-session, same-checkpoint Solr `filter_only` timing
   measurement** to close the gap named in risk #2, for a fully
   apples-to-apples ES-vs-Solr-vs-native-vs-Lucene-direct comparison at
   every operation class, not just color-facet.
3. **Profile embedded ES's aggregation path** (JFR/async-profiler) to
   confirm or refute the transport/action-layer-overhead hypothesis
   named above.
4. **A genuine one-binary four-way comparison harness** (native, Solr,
   Lucene-direct, embedded ES) at the same checkpoints in the same
   process/session, removing the last "different session" caveat this
   phase's own comparison still carries.

## What should explicitly not be built yet

- **A multi-node or multi-shard Elasticsearch topology** — this phase
  deliberately stayed single-node/single-shard, matching `CLAUDE.md`'s
  distributed-systems sequencing rule; the single-node thesis is not
  yet exhausted enough to justify this.
- **Wiring Elasticsearch as a production dependency anywhere in this
  codebase** — this is a benchmark-only comparison artifact
  (`es-direct-bench/`), not a step toward adopting ES as a real backend;
  no such proposal is implied by this phase's results.
- **A from-source OpenSearch/Elasticsearch build** — CLAUDE.md's own
  "avoid production polish" and effort-proportionality principles apply
  equally here: the Maven-library route already answers the load-bearing
  question (can a real ES instance be measured in this environment?)
  more cheaply than a from-source build would, without the two
  independent from-source blockers Phase 6C already found.

## Does / does not claim

**Does claim**: Elasticsearch is not "genuinely blocked" in this
environment in the unqualified sense Phase 6C's own headline stated —
a real, correctness-verified, running embedded instance now exists,
with a real (if narrow) timing comparison against Solr for one operation
class; that this is a genuinely new fourth cross-engine data point
supporting (not contradicting) commerce-native's own Phase 6D
color-facet finding; that all four blockers hit along the way were
concrete, understood, and fixed, not silently worked around.

**Does not claim**: that OpenSearch is unblocked by the same route
(confirmed only at the dependency-resolution level, not the
running-node level); that this phase's single narrow timing comparison
(color facet under 7 depth-1 categories) generalizes to every operation
class this project measures; that the "ES aggregation slower than Solr
facet.field" finding is mechanistically understood rather than
observed; that a multi-node ES topology would show the same profile;
that Havenask's from-source-build blockers (Phase 6B/6C, unchanged)
have been revisited in this phase (they have not — this phase's scope
was specifically the Maven-library route Phase 6C never tried for
Elasticsearch/OpenSearch, not a fresh Havenask attempt).

**Decision: PROCEED.** This phase directly answers the user's own
standing instruction to revisit Elasticsearch/OpenSearch rather than
accept an unqualified "blocked" verdict, finds a real and previously
untried route that works for Elasticsearch, and produces this research
campaign's first genuine Elasticsearch data point — one that
strengthens rather than complicates the existing evidence chain.
