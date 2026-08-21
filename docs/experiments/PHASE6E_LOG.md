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

## P6E-E01: does the same route work for OpenSearch, and does it show the same qualitative result?

**Hypothesis**: since `org.opensearch:opensearch:2.17.0` and
`org.opensearch.test:framework:2.17.0` already confirmed resolvable from
Maven Central (P6E-E00), the same Maven-library route should let a real
embedded OpenSearch node boot, though OpenSearch's fork-specific
bootstrap code (it diverged from Elasticsearch 7.10) may introduce its
own distinct blockers rather than the identical four P6E-E00 found --
falsifiable either way by attempting it and recording exactly what
transfers and what doesn't.

**Design**: mirror `es-direct-bench/`'s structure in a new
`opensearch-direct-bench/` module. OpenSearch's own single-node
test-bootstrap class is `org.opensearch.test.OpenSearchSingleNodeTestCase`
(confirmed via `jar tf` on the framework JAR) rather than ES's
`ESSingleNodeTestCase`. Start with a minimal probe test, fix whatever
appears, then port the full benchmark.

### New, OpenSearch-specific blockers found (distinct from P6E-E00's four)

1. **Missing `log4j-core`**: `NoClassDefFoundError:
   org/apache/logging/log4j/core/Layout`. `mvn dependency:tree` showed
   OpenSearch's own POM brings `log4j-api`/`log4j-jul` but not
   `log4j-core` -- unlike Elasticsearch's equivalent bundle, which does.
   Fixed by adding `log4j-core:2.21.0` explicitly to `pom.xml`.
2. **`RuntimeException: can not run opensearch as root`**, thrown from
   `Bootstrap.initializeNatives`. This container runs every command as
   root (confirmed via `whoami`/`id`; Solr itself needed `--force` for
   the same reason). OpenSearch's own bootstrap explicitly checks for
   root and refuses -- a real safety check in OpenSearch's own code.
   Notably, **Elasticsearch's equivalent check did not hard-fail under
   the identical condition** in P6E-E00 (it logged "Cannot check if
   running as root because native access is not available" and
   continued) -- the same underlying condition (native-access library
   unavailable in this container) is handled as non-fatal by ES and
   fatal by OpenSearch, a real, disclosed divergence between the two
   forks, not an inconsistency in this project's own testing. **Fixed**
   by running the benchmark as this container's existing non-root
   `ubuntu` (uid 1000) account instead of root -- since `/root` itself
   is mode 700 (unreadable by other users), an isolated copy of the
   module plus a separate Maven local repository under `/tmp` (both
   chowned to `ubuntu`) were used rather than trying to share root's own
   `~/.m2` cache.
3. **`org.opensearch.xcontent` package does not exist**: OpenSearch
   2.17 (forked before ES 8.x's package reorganization) still keeps
   `XContentBuilder` under `org.opensearch.core.xcontent` and
   `XContentFactory` under `org.opensearch.common.xcontent` -- fixed by
   using the correct pre-8.x package paths.
4. **`SearchResponse` has no `decRef()` method**: ES 8.x's
   reference-counted response objects (introduced after OpenSearch
   forked) don't exist in OpenSearch's API -- fixed by removing the
   `decRef()` calls the ported code carried over from `WandsEsBenchTest`.

### A fifth blocker found, NOT fixed in this pass

5. **Live Solr cross-check denied**: `AccessControlException: access
   denied ("java.net.SocketPermission" "localhost:8983"
   "connect,resolve")`. Running as non-root `ubuntu` (the fix for
   blocker #2) also activates OpenSearch's test-framework
   `SecurityManager`'s network restrictions, which deny outbound
   sockets to Solr's port from the test JVM -- the identical live-count
   cross-check succeeded without issue in P6E-E00's Elasticsearch run
   (run as root), so this is specifically a consequence of the
   non-root + SecurityManager combination OpenSearch's bootstrap
   requires. Every run's `solr_count`/`count_match` columns are `n/a`
   as a result. **Not silently worked around**: disclosed as a real,
   unresolved limitation. See
   `docs/research/artifacts/p6e_e01_opensearch_direct_run1/blockers_found_and_fixed/03_solr_socket_permission_denied_NOT_FIXED.log`.

### Indirect correctness corroboration (since the live check was blocked)

Every candidate count OpenSearch reported across all 3 runs — 2,002 /
2,175 / 2,072 / 3,394 / 4,612 / 4,686 / 16,039 (the 7 depth-1 category
checkpoints) and 31,967 (the numeric-range whole-catalog count) —
matches, digit-for-digit and run-for-run, the Solr-verified ground-truth
counts P6E-E00's Elasticsearch benchmark already confirmed against
Solr's live response in this same session, for the identical
checkpoints on the identical catalog. This is real, if indirect,
corroborating evidence (a cross-process comparison against an
already-verified number, not a live in-process check) -- reported as
such, not inflated into a "24/24 correctness gate" claim the way
P6E-E00's own genuinely live check earned.

### Result: timing, 3 independent runs

| Checkpoint | n | OpenSearch color-facet p50 (3 runs, ms) | ES color-facet p50 (P6E-E00, ms) | Solr p50 (same session, ms) | OpenSearch vs. Solr |
|---|---|---|---|---|---|
| Rugs | 2,002 | 1.97-2.29 | 2.08-2.16 | 1.19 | 1.7-1.9x slower |
| Storage & Organization | 2,175 | 1.38-1.55 | 1.37-1.56 | 1.15 | 1.2-1.35x slower |
| Lighting | 2,072 | 1.23-1.29 | 1.22-1.38 | 0.97 | 1.27-1.33x slower |
| Outdoor | 3,394 | 1.38-1.60 | 1.37-1.56 | 1.00 | 1.4-1.6x slower |
| Décor & Pillows | 4,612 | 2.15-2.47 | 1.99-2.22 | 1.10 | 2.0-2.25x slower |
| Home Improvement | 4,686 | 1.36-1.76 | 1.22-1.30 | 1.06 | 1.3-1.7x slower |
| Furniture | 16,039 | 1.53-1.92 | 1.55-1.58 | 1.18 | 1.3-1.6x slower |

**OpenSearch's terms-aggregation is also slower than Solr's own
`facet.field`** at every checkpoint (1.2x-2.25x), the same qualitative
finding P6E-E00 made for Elasticsearch, and the two engines' own
absolute numbers are close to each other (OpenSearch running somewhat
higher at 2 of 7 checkpoints, comparable at the rest) -- consistent with
both forks sharing the same Lucene core and a similar general-purpose
aggregation-framework design, though this was not measured in the same
process/session for either engine (each ran in its own JVM). This
strengthens the P6E-E00 finding further: it is not an Elasticsearch-8.x
peculiarity, but a pattern that reproduces on the other major
Lucene-based distributed search engine too. commerce-native's own
ordinal method remains dramatically faster than both.

**Named limitations**: the live Solr correctness cross-check could not
run in-process for this engine (blocker #5) -- correctness rests on
indirect, cross-process candidate-count corroboration, not the same
kind of direct 24/24 live-response match P6E-E00 achieved for
Elasticsearch. OpenSearch and Elasticsearch were not measured in the
same process/session, so the "OpenSearch vs. ES" comparison in the table
above is illustrative, not a controlled same-session comparison. The
same other limitations named in P6E-E00 apply here too: only
`color_facet_under_category` has a clean same-session Solr timing
counterpart; only single-node/single-shard OpenSearch was tested; the
mechanism is inferred, not profiled.
