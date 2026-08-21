# Phase 6C Experiment Log (Issue #21 Phase 6, extending 6A/6B: raw Lucene direct cross-engine baseline)

## Why this phase exists (a repaired evidence-chain gap, not a new ask)

Every cross-engine comparison in this project, from `SCALE_UP_DECISION.md`
through `PHASE8_DECISION.md`, has measured commerce-native against
**Solr, and only Solr**. Issue #21's own Phase 6 goal names Havenask as
a required second engine; `PHASE6A_DECISION.md` deferred it explicitly
("not required to block this issue"), and `PHASE6B_DECISION.md` found
it (and Retailrocket/H&M/Amazon Reviews 2023) genuinely blocked in this
environment (Docker daemon absent, `registry.cn-hangzhou.aliyuncs.com`
unreachable, no `bazel`) -- but its own "Unresolved risks" #4 said
explicitly: **"if network policy changes, they remain the preferred
real alternative to controlled-stress substitution and should be
revisited before Phase 7."** That revisit did not happen. Phase 7 and
Phase 8 both proceeded on Solr as the only lexical-backend evidence.
This phase is that overdue revisit, done now rather than left
un-repaired underneath five more phases of conclusions.

## Fresh live re-verification (this session, not a re-read of old claims)

Before building anything, every previously-named blocker was
re-tested live in this exact environment, not assumed unchanged from
Phase 6B's own investigation:

| Resource | Status (this session) | Method |
|---|---|---|
| Havenask Docker registry (`registry.cn-hangzhou.aliyuncs.com`) | **still blocked** (403) | direct HTTPS probe |
| Docker daemon | **still absent** (`/var/run/docker.sock` does not exist) | `docker info` |
| Havenask source (`github.com/alibaba/havenask`) | **reachable** | `git clone` (works; only the registry and daemon are blocked, unchanged from Phase 6B) |
| `bazel`/`bazelisk` | **not installed** | `which` |
| Elasticsearch official distribution (`artifacts.elastic.co`) | **blocked** (403), newly tested | direct HTTPS probe |
| OpenSearch official distribution (`artifacts.opensearch.org`) | **blocked** (403), newly tested | direct HTTPS probe |
| OpenSearch/Elasticsearch source (GitHub) | **reachable** via `git clone`, newly tested | `git ls-remote` |
| OpenSearch's own bundled-JDK provider (`api.adoptium.net`) | **blocked** (403), newly tested | direct HTTPS probe |
| OpenSearch's build-time Maven repo (`artifacts.opensearch.org`, referenced in its own `build.gradle`) | **blocked** (403) | grep + probe |
| Retailrocket / H&M (`kaggle.com`) | **still blocked** (403) | direct HTTPS probe |
| Amazon Reviews 2023 (`huggingface.co`) | **still blocked** (403) | direct HTTPS probe |
| `eCommerceSearchBench` (Issue #21's own named, previously "unresolved/not located" resource) | **corrected finding: it is `github.com/alibaba/eCommerceSearchBench`, reachable via `git clone`** | web search + `git ls-remote` |
| **Maven Central** (`repo1.maven.org`) | **fully reachable** (200 OK), newly tested | direct HTTPS probe |

**Conclusion**: Havenask, Elasticsearch, and OpenSearch as *installed,
running server instances* remain genuinely blocked in this
environment, on freshly re-verified evidence, not an assumption
carried forward from Phase 6B. Both Elasticsearch and OpenSearch would
additionally require a from-source build (no prebuilt distribution
reachable), and OpenSearch's own build hits a second, independent
blocker (its bundled-JDK provider is also unreachable) before even
reaching dependency resolution -- a materially larger, multi-blocker
undertaking than Phase 6B's own Havenask investigation found, not
attempted further given `CLAUDE.md`'s "avoid distributed systems work"
sequencing and the "materially larger infrastructure" stop condition.
`eCommerceSearchBench` is a synthetic Taobao-style data/workload
generator, not a search engine baseline -- a real, corrected finding
(it is reachable, contradicting its own prior "unresolved" status) but
not itself a cross-engine data point; noted as a genuine open item for
a future cross-workload (not cross-engine) experiment.

**But Maven Central is fully reachable** -- and Apache Lucene (the
actual retrieval engine every one of Solr/Elasticsearch/OpenSearch is
built on top of) is published there as a plain embeddable library, no
server, no Docker, no bundled JDK. This is the one genuinely new,
fully-feasible cross-engine data point this environment can produce
that no prior phase attempted.

## P6C-E00: raw Apache Lucene direct baseline (hypothesis stated before implementation)

**Falsifiable hypothesis**: every prior Phase 2-6B cross-engine number
compared commerce-native against *Solr*, a system that adds an HTTP
layer, schema/config machinery, and its own facet-wrapper API on top
of the underlying Lucene engine. It is untested whether any of the
measured native-vs-Solr gap is attributable to that wrapper rather than
to Lucene's own core retrieval/faceting cost. **H (P6C-E00)**: stripping
Solr's wrapper away and measuring bare Lucene directly, on the
identical real WANDS catalog and identical operation classes P6A-E00
already measured (category filter, product_class/color facet-scan,
sort by title/rating, deep pagination, numeric-range filter), will
show materially different -- specifically, faster -- numbers than
Solr's own wrapped API, revealing that part of the previously-reported
"native vs. generic engine" gap was actually a "native vs. Solr's own
HTTP/wrapper overhead" artifact. Falsifiable both ways: Lucene direct
could just as easily show the *same or worse* cost than Solr's own
mature, specialized implementation.

**Design**: a standalone Java/Maven module (`lucene-direct-bench/`,
not a Rust crate -- Lucene is an embedded library with no HTTP
interface and no mature Rust binding, so a minimal external-process
harness is the direct equivalent of how every other cross-engine
binary in this repo reaches Solr over HTTP). Indexes the real WANDS
catalog (`dataset_cache/wands/catalog.jsonl`, identical file
`scripts/datasets/solr_index_wands.py` and `crates/phase6a-eval` both
read) with the same dual indexed+docValues field pattern Solr's own
schema uses (`StringField` for `TermQuery` filtering, matching
`SortedDocValuesField`/`NumericDocValuesField` for facet-scan/sort,
`DoublePoint` for range queries) -- the same 7 real `category_depth_1`
checkpoints P6A-E00/P6B-E00 both used (Rugs, Storage & Organization,
Lighting, Outdoor, Décor & Pillows, Home Improvement, Furniture; 2,002
to 16,039 real products), the same `WARMUP=5`/`REPS=30`/`PAGE_SIZE=24`
timing convention as `p6a_e00_wands_vs_native_eval.rs`, and the same
`average_rating >= 4.0` numeric-range threshold P6B-E00 used.

**Self-caught build issue (tooling, not methodology)**: the first
build used `FSDirectory.open()` (Lucene's own recommended default,
which auto-selects `MMapDirectory` on capable platforms) and threw
`LinkageError: MemorySegmentIndexInputProvider is missing in Lucene
JAR file` at runtime -- the Maven Shade/Assembly-produced uber-jar
does not preserve Lucene's multi-release-JAR structure needed for its
Panama/`MemorySegment`-based mmap implementation. Fixed by switching to
`NIOFSDirectory` (a standard, fully-supported, non-mmap Lucene
backend) -- a real on-disk Lucene index either way, just without the
mmap fast path; disclosed as a real, if minor, methodology choice
rather than silently worked around.

**Correctness gate, checked before any timing claim was trusted**: every
`category_depth_1` filter count and the whole-catalog numeric-range
count were cross-checked live against the real, currently-running Solr
`wands_bench` core (the exact same catalog) via a direct HTTP request.
**All 8 counts (7 checkpoints + 1 range filter) matched exactly, in
all 3 repeated runs** -- 2,002/2,175/2,072/3,394/4,612/4,686/16,039 for
the category filters, 31,967 for the rating range filter. A facet-sum
sanity check (bucket-count sum must never exceed the filter's own
candidate count, since docs missing a value are legitimately excluded)
also passed for every checkpoint's product_class/color facet in every
run.

## P6C-E00 result: raw Lucene direct is often SLOWER than Solr's own wrapped facet API -- a real, counter-intuitive, mechanistically-plausible finding

Raw data: `docs/research/artifacts/p6c_e00_lucene_direct_run1/`
(3 full console logs, final CSV, and a same-session Phase 6A rerun log
used for the 3-way comparison below).

**Filter-only and numeric-range**: extremely fast and stable across all
3 runs (filter-only p50 0.012ms-0.17ms across the 7 checkpoints;
numeric-range p50 ~0.15-0.18ms for the whole 42,994-product catalog at
31,967 matches) -- consistent with a `TermQuery`/`DoublePoint` range
query being about as cheap an operation as Lucene's inverted index
supports, unsurprising and not the interesting part of this result.

**Color-facet-under-category, the one operation measured in BOTH this
new Lucene harness and a fresh, same-session rerun of P6A-E00's own
binary (avoiding the cross-session Solr-JVM confound `PHASE6B_DECISION.md`
already flagged)** -- a genuine, same-moment, three-way comparison:

| Checkpoint | Candidates | Native (ms) | Solr (ms) | Lucene direct (ms) | Lucene vs. Solr |
|---|---|---|---|---|---|
| Rugs | 2,002 | 1.4163 | 1.0950 | 1.4826 | 1.35x slower |
| Storage & Organization | 2,175 | 2.0260 | 1.1501 | 0.8928 | 0.78x (faster) |
| Lighting | 2,072 | 1.4004 | 1.2098 | 0.2283 | 0.19x (5.3x faster) |
| Outdoor | 3,394 | 2.6329 | 1.0735 | 1.7520 | 1.63x slower |
| Décor & Pillows | 4,612 | 3.9264 | 1.1969 | 3.9621 | 3.31x slower |
| Home Improvement | 4,686 | 3.4385 | 1.2659 | 1.5564 | 1.23x slower |
| Furniture | 16,039 | 10.6498 | 1.2055 | 4.8151 | 3.99x slower |

**Raw Lucene direct is SLOWER than Solr's own wrapped `facet.field` API
in 5 of 7 checkpoints, sometimes by a wide margin (3.3x-4.0x), and
faster in only 2 of 7** (one of those, Lighting, by a real 5.3x margin
-- not uniformly close to parity either way). This pattern reproduced
consistently across all 3 repeated runs (per-checkpoint values varied
by at most ~25% run to run, always in the same rank order -- Lighting
fastest, Décor & Pillows/Furniture slowest -- ruling out simple JIT-
warmup or measurement noise as the explanation).

**This directly falsifies the naive version of the hypothesis this
experiment set out to test.** Solr's HTTP/schema/wrapper layer is
*not* simply "overhead sitting on top of a faster underlying engine" --
for faceting specifically, Solr's own mature, specialized facet
implementation (per-segment ordinal maps / global ordinal remapping,
the product of two decades of tuning) frequently outperforms a
straightforward, correctly-implemented, DocValues-backed per-candidate
scan against the identical raw Lucene index. **This is mechanistically
consistent with, and materially strengthens, this project's own
repeated finding (Phase 5, 6A, 6B) that a naive per-candidate facet
scan** -- whether commerce-native's own `facet_counts_by_scan` or this
experiment's hand-rolled Lucene equivalent -- **loses to Solr's
faceting machinery past a real cardinality/complexity threshold, not
because of any HTTP/wrapper tax, but because Solr's specific algorithm
is doing genuinely better work.** The native facet crossover this
project has characterized four times now (Phase 5's ESCI ~9,000-12,000
candidates, Phase 6A's WANDS ~2,072-2,175, Phase 6B's scale-ladder
confirmation) is therefore evidence about facet ALGORITHMS, not about
Solr's serving-layer overhead being unfairly counted against
commerce-native -- a materially different, more precise causal
attribution than "Solr's overhead was masking the comparison" would
have been, and the opposite conclusion from what this experiment was
designed to be able to show.

**Named limitations**: only one operation (color facet-scan) has a
same-session, apples-to-apples three-way comparison; filter-only and
numeric-range were only measured Lucene-vs-Solr (both cross-checked for
correctness, but no fresh same-session native number exists at these
exact depth-1 checkpoints to complete a three-way table for those
operations). Only WANDS at its natural 1x scale was tested -- the
Phase 6B scale-ladder replication (2x-20x) was not repeated for Lucene
direct, a real, named next step. `NIOFSDirectory` (not `MMapDirectory`)
was used for a build-tooling reason, not a chosen representativeness
decision -- whether Lucene's own mmap-backed segments would show
materially different absolute numbers is untested. Sort-by-title/
-rating and deep-pagination were measured for Lucene direct and
correctness-gated only via the filter-count/range-count checks (no
Solr-side timing for these specific operations was re-collected this
session for a three-way table), though their absolute Lucene numbers
are recorded in the raw CSV for future reference.
