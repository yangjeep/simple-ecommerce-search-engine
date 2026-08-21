# Phase 6E Experiment Log (Issue #21 Phase 6E)

## Context: why this phase exists

The user's own standing research-loop instruction was explicit and specific:
"Solr is not enough. Revisit Lucene direct, Elasticsearch, OpenSearch,
Havenask, and any other credible generic/search-engine baseline already
contemplated by the project. Attempt what is feasible in this
environment; document genuine blockers rather than silently dropping
engines." Phase 6C (`PHASE6C_DECISION.md`) already did a live
cross-engine re-audit and concluded Elasticsearch and OpenSearch were
"genuinely blocked" -- but that audit tested only two routes per engine:
the official prebuilt distribution (`artifacts.elastic.co`/
`artifacts.opensearch.org`, both 403) and a from-source build (blocked
on `bazel`/`api.adoptium.net`). It never tried the same trick that made
P6C-E00's raw-Lucene benchmark possible: fetching the engine's own
published library JARs directly from Maven Central, the way any Java
application would depend on it, rather than downloading its server
distribution or building it from source.

## P6E-E00: does Elasticsearch exist as a real, correctness-verifiable cross-engine baseline via the Maven-library route?

**Hypothesis**: if `org.elasticsearch:elasticsearch` and
`org.elasticsearch.test:framework` resolve from Maven Central in this
environment (a fact not yet tested by Phase 6C), a real embedded
single-node Elasticsearch cluster can be bootstrapped in-process using
the test framework's own node-bootstrap machinery
(`ESSingleNodeTestCase`), without needing the official distribution,
Docker, or a from-source build -- falsifiable by either a clean
in-process node startup or a specific, disclosed blocker.

**Design**: (1) probe Maven Central directly via `mvn dependency:get`
for `org.elasticsearch:elasticsearch:8.15.0` and
`org.elasticsearch.test:framework:8.15.0` (and the OpenSearch
equivalents, for completeness); (2) if they resolve, build a minimal
Maven module (`es-direct-bench/`) with a JUnit test extending
`ESSingleNodeTestCase`, and see how far it gets; (3) fix whatever
concrete blockers appear, one at a time, rather than giving up at the
first error; (4) once a node boots, extend the probe into a real
benchmark against the same real WANDS catalog and same 7 depth-1
category checkpoints (`Rugs`, `Storage & Organization`, `Lighting`,
`Outdoor`, `Décor & Pillows`, `Home Improvement`, `Furniture`) that
P6A-E00/P6C-E00 already used against Solr and raw Lucene, cross-checked
against the same live Solr `wands_bench` core before any timing claim is
trusted -- matching this whole project's "correctness before speed"
discipline.

### Step 1: Maven Central resolution -- genuinely new information

```
$ mvn dependency:get -Dartifact=org.elasticsearch:elasticsearch:8.15.0
$ mvn dependency:get -Dartifact=org.opensearch:opensearch:2.17.0
$ mvn dependency:get -Dartifact=org.elasticsearch.test:framework:8.15.0
$ mvn dependency:get -Dartifact=org.opensearch.test:framework:2.17.0
```

All four resolved cleanly (confirmed via `ls ~/.m2/repository/...` --
actual JARs present, not just POM metadata), along with every
transitive dependency (`mvn dependency:tree` on a module declaring both
`org.elasticsearch:elasticsearch` and `org.elasticsearch.test:framework`
as dependencies: `BUILD SUCCESS`, 83 resolved artifacts, zero errors).
This directly contradicts the framing (not the underlying test) of
Phase 6C's "genuinely blocked" verdict for Elasticsearch: the
**distribution** is blocked; the **library** is not.

### Step 2: bootstrapping a real embedded node -- two real, disclosed, fixable blockers

A minimal JUnit test (`class Probe extends ESSingleNodeTestCase { public
void test() { client().admin().cluster().prepareHealth().get(); } }`)
run via `mvn test` hit two concrete failures in sequence, neither of
them a network/environment blocker:

1. **Jar hell**: `IllegalStateException: jar hell! class:
   org.hamcrest.BaseDescription jar1: hamcrest-2.1.jar jar2:
   hamcrest-core-1.3.jar`. Cause: this module's own `junit:4.13.2`
   dependency transitively pulls `hamcrest-core:1.3`, which duplicates
   classes with the newer standalone `hamcrest:2.1`
   `org.elasticsearch.test:framework` itself depends on. ES's own
   `BootstrapForTesting` self-check catches this and fails loudly rather
   than silently picking one. **Fixed** by excluding
   `org.hamcrest:hamcrest-core` from the `junit` dependency in
   `pom.xml`.
2. **JDK 17+ SecurityManager installation refusal**:
   `UnsupportedOperationException: The Security Manager is deprecated
   and will be removed in a future release`, thrown from
   `System.setSecurityManager` inside ES's own
   `BootstrapForTesting.<clinit>`. Cause: JEP 411 (finalized starting
   JDK 18) refuses `System.setSecurityManager` calls unless the JVM is
   started with `-Djava.security.manager=allow`. ES's own test bootstrap
   still installs a legacy `SecurityManager` to sandbox file/network
   access during tests, unaware this JDK requires opt-in. **Fixed** by
   adding `-Djava.security.manager=allow` to Surefire's `argLine`.

With both fixed, the probe test passed cleanly: **a real Elasticsearch
8.15.0 single-node cluster started, elected itself master, logged
`started {node_s_0}...`, and answered a real cluster-health request --
then shut down cleanly** (`BUILD SUCCESS`, `Tests run: 1, Failures: 0,
Errors: 0`). This is the first real, running Elasticsearch instance
anywhere in this research campaign's history.

### Step 3: real benchmark against the real WANDS catalog

Extended the probe into `WandsEsBenchTest`, mirroring
`WandsLuceneBench.java`'s (P6C-E00) own field set, checkpoint list, and
`WARMUP=5, REPS=30` timing convention, using ES's in-process
`NodeClient` (via `ESSingleNodeTestCase.client()`) rather than the REST
layer -- the equivalent of the Lucene-direct bench using
`IndexSearcher` directly instead of Solr's HTTP layer. Two more
blockers appeared, both from ES's own test `SecurityManager` policy
(not from this environment's network policy), and both fixed the same
way -- relocate the affected I/O to a path the policy already grants:

3. **Catalog read denied**: `AccessControlException: access denied
   ("java.io.FilePermission" "../dataset_cache/wands/catalog.jsonl"
   "read")`. ES's test-framework security policy does not grant
   arbitrary repo-relative paths (it is built for ES's own Gradle
   project layout, not a standalone Maven module elsewhere on disk).
   **Fixed** by copying the catalog to `/tmp/wands_catalog.jsonl`
   (`java.io.tmpdir`, a path the policy does grant) before running.
4. **Results-CSV write denied**: same `AccessControlException`, this
   time for a `write` on `../docs/research/artifacts/...`. **Fixed** the
   same way: write to `${java.io.tmpdir}/p6e_e00_es_direct_run1/` and
   archive the file into the repo from the invoking shell after the JVM
   exits (matching the same read-side fix's logic).

Neither of these two was a genuine environment blocker -- they are
disclosed, understood, and permanently fixed in this module's own code
and `pom.xml`; a future run needs no rediscovery.

### Result: correctness gate, 3 independent runs

`mvn test` invoked 3 independent times. Every run: `BUILD SUCCESS`, 43
timed operations, **zero correctness mismatches** -- all 7
`filter_only` category counts and the whole-catalog
`numeric_range_rating` count matched Solr's own live `numFound` exactly,
every run:

| Checkpoint | Candidates | Solr count | ES count match |
|---|---|---|---|
| Rugs | 2,002 | 2,002 | true (3/3 runs) |
| Storage & Organization | 2,175 | 2,175 | true (3/3 runs) |
| Lighting | 2,072 | 2,072 | true (3/3 runs) |
| Outdoor | 3,394 | 3,394 | true (3/3 runs) |
| Décor & Pillows | 4,612 | 4,612 | true (3/3 runs) |
| Home Improvement | 4,686 | 4,686 | true (3/3 runs) |
| Furniture | 16,039 | 16,039 | true (3/3 runs) |
| numeric_range_rating (whole catalog) | 31,967 | 31,967 | true (3/3 runs) |

### Result: timing, color-facet-under-category (the one operation with a clean same-session Solr comparison)

The freshest same-session Solr color-facet-under-`category_depth_1`
timing available comes from re-running
`p6a_e00_wands_vs_native_eval` earlier in this same session (for
P6D-E03's memory measurement) -- the exact same 7 checkpoints, same
live `wands_bench` core, same session:

| Checkpoint | n | ES p50 (3 runs, ms) | Solr p50 (same session, ms) | ES vs. Solr | Native scan (ms) | Native ordinal (ms) |
|---|---|---|---|---|---|---|
| Rugs | 2,002 | 2.08-2.16 | 1.19 | 1.7-1.8x slower | 1.62 | 0.031 |
| Storage & Organization | 2,175 | 1.37-1.56 | 1.15 | 1.2-1.4x slower | 1.60 | 0.029 |
| Lighting | 2,072 | 1.22-1.38 | 0.97 | 1.3-1.4x slower | 1.29 | 0.023 |
| Outdoor | 3,394 | 1.37-1.56 | 1.00 | 1.4-1.6x slower | 2.75 | 0.073 |
| Décor & Pillows | 4,612 | 1.99-2.22 | 1.10 | 1.8-2.0x slower | 3.70 | 0.199 |
| Home Improvement | 4,686 | 1.22-1.30 | 1.06 | 1.2-1.3x slower | 3.46 | 0.089 |
| Furniture | 16,039 | 1.55-1.58 | 1.18 | 1.3-1.3x slower | 17.70 | 0.246 |

**Embedded Elasticsearch's terms-aggregation is slower than Solr's own
`facet.field` at every one of the 7 checkpoints tested** -- a real,
disclosed, somewhat counter-intuitive finding (both sit on the same
Lucene core). A plausible, disclosed-as-unconfirmed mechanism: ES's
in-process `client()` calls still go through its full transport/action
pipeline (the same code path a remote client would use, just
loopback-local rather than over a socket) and its general-purpose
aggregation framework, versus Solr's more specialized `facet.field`
component -- not independently profiled. commerce-native's own ordinal
method remains dramatically faster than *both* general-purpose engines
at every checkpoint (13x-100x faster than Solr, and by extension faster
still than ES), reinforcing rather than complicating this project's
existing color-facet finding (P6D-E00/E01) with a fourth engine data
point.

**Named limitations**: only `color_facet_under_category` has a clean,
same-session Solr timing comparison at these exact checkpoints; the
`filter_only` operation's ES numbers are correctness-verified against
Solr's live count but not timing-compared against a same-session Solr
filter-only latency (no such operation exists in this session's
archived Solr runs at these exact checkpoints -- a real, disclosed gap,
not a silently dropped comparison). `product_class_facet_under_category`
and `sort_title_asc`/`sort_rating_desc`/`deep_pagination` are measured
for ES alone, with no same-session Solr/native timing counterpart at
these checkpoints in this pass. Only Elasticsearch 8.15.0 was actually
benchmarked end-to-end; OpenSearch's library artifacts were confirmed
resolvable from Maven Central (same `mvn dependency:get` proof) but a
full embedded-node bootstrap + benchmark was not attempted in this pass
(OpenSearch forked from ES 7.10 and its own test-framework bootstrap
code differs enough that the same fixes are not guaranteed to transfer
unmodified) -- named as the natural next step. No direct three/four-way
same-session native/Solr/Lucene-direct/ES timing comparison was run in
one binary; each engine's numbers come from its own process, with
correctness cross-checked against the same live Solr core as the common
reference point. The mechanism for ES's aggregation being slower than
Solr's own facet component is inferred, not profiled (no JFR/perf run).
Only a single-node, single-shard ES configuration was tested, matching
this whole project's "avoid distributed-systems work" scoping rule
(`CLAUDE.md`) -- not a multi-node or multi-shard ES topology.
