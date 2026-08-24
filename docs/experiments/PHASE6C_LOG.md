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

## P6C-E01: adversarial check -- was P6C-E00's "raw Lucene loses to Solr" finding about Lucene itself, or about one naive implementation? (stated before implementation)

**Self-directed adversarial review, before letting a surprising result
stand.** P6C-E00's own hand-rolled facet-scan (iterate every matching
doc, look up its `SortedDocValues` ordinal, tally in a `HashMap`) is
*a* way to compute facet counts from raw Lucene, but it is not
Lucene's OWN best-available mechanism for this exact task. Lucene ships
a dedicated `lucene-facet` module with
`SortedSetDocValuesFacetCounts` -- a specialized, purpose-built
facet-counting implementation, analogous in spirit to Solr's own
`facet.field` machinery, that P6C-E00 did not use. Concluding "Solr
beats raw Lucene" from a comparison against a hand-rolled scan risks
conflating "Solr beats a naive per-candidate scan" (unsurprising,
consistent with this project's own native `facet_counts_by_scan`
finding) with "Solr beats the best Lucene can do" (a much stronger,
and, if true from a naive baseline alone, unsupported claim).

**H (P6C-E01)**: Lucene's own `SortedSetDocValuesFacetCounts` module
will show materially different -- specifically, faster and more
competitive with Solr -- results than P6C-E00's hand-rolled scan, at
the identical checkpoints and identical index. Falsifiable both ways:
the specialized module could just as easily perform similarly to the
naive scan, which would strengthen (not weaken) P6C-E00's original
conclusion.

**Design**: added `SortedSetDocValuesFacetField` entries for
`product_class`/`color` at index time (requiring `FacetsConfig.build()`
to rewrite them into indexable form), a single shared
`DefaultSortedSetDocValuesReaderState` (both dimensions share Lucene's
own default `"$facets"` indexed field), and
`SortedSetDocValuesFacetCounts.getTopChildren()` for counting --
Lucene's own documented, standard usage pattern for exactly this
use case, not a custom implementation. Same correctness discipline as
P6C-E00: the facet-sum-never-exceeds-candidates sanity check was
re-applied to the module-based counts and passed in every run.

**A note on reproducibility of this write-up itself**: an earlier
attempt at this exact experiment, in a prior session, was lost before
being committed (a container-lifecycle issue, not a data issue -- see
the git history around this commit for the full account). This section
reflects a genuine, fresh re-run of the experiment from scratch in a
new environment instance, not a recycled write-up. The qualitative
conclusion below matches what the lost attempt had also found, but the
exact numbers here are freshly measured and are the only numbers this
project actually has committed evidence for.

**Correctness gate**: the same facet-sum-never-exceeds-candidates
sanity check applied to the scan-based facets was re-applied to the
module-based counts and passed for every checkpoint in every run (no
`IllegalStateException` thrown); all 8/8 filter/range counts against
the live Solr core matched exactly in all 3 fresh runs (24/24 total).

## P6C-E01 result: CONFIRMED with real nuance -- the naive scan, not Lucene itself, is the main reason Solr wins, but the module is not uniformly faster either

Raw data: `docs/research/artifacts/p6c_e01_lucene_facet_module_run1/`
(3 full console logs and CSVs for the Lucene facet-module run, plus 3
fresh same-session reruns of `p6a_e00_wands_vs_native_eval` for the
native/Solr side of the three-way table -- all 6 runs executed back to
back in this session against the same live Solr `wands_bench` core).

**Lucene's own specialized facet module is faster than this
experiment's own hand-rolled scan at 6 of 7 checkpoints, reproduced
across all 3 runs** -- e.g. Furniture (16,039 candidates): scan median
5.52ms vs. module median 2.05ms (2.7x faster); Décor & Pillows: scan
4.25ms vs. module 1.65ms (2.6x faster); Outdoor: scan 1.91ms vs. module
0.95ms (2.0x faster). **The one exception is the smallest, simplest
checkpoint (Lighting, 2,072 candidates, the smallest real color-value
cardinality of the 7): the module is 1.9x SLOWER than the scan there**
(0.44ms vs. 0.23ms) -- consistent with the module's `FacetsCollector`
setup carrying fixed overhead that a trivially small per-candidate scan
can undercut, a genuine, disclosed exception rather than a uniform win
papered over.

**Updated three-way comparison (native / Solr / Lucene's own facet
module), medians across 3 runs, color facet-scan under category
filter**:

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
FASTER than Solr in 5 of 7 real checkpoints** (up to 3.0x, at Lighting),
**and slower in only 2 of 7 -- Décor & Pillows (1.11x) and Furniture
(1.30x) -- both a much smaller margin than the naive scan's worst cases
(P6C-E00's own 3.31x-3.99x at these same two checkpoints).**

**This substantially revises P6C-E00's own headline conclusion.**
Solr's facet implementation does not categorically outperform "raw
Lucene" -- it outperforms a *naive per-candidate scan*, which both
this experiment's own hand-rolled attempt and (by the same
architectural pattern) commerce-native's own `facet_counts_by_scan`
represent. When Lucene's own specialized, ordinal-based counting
mechanism is used instead, the picture mostly reverses to favor Lucene,
with Solr's own advantage persisting -- narrowed, not eliminated -- at
the two largest, highest-color-cardinality checkpoints. That residual
pattern (Solr still ahead specifically where candidate count and
distinct-color-value count are both largest) is consistent with *some*
real scale-dependent cost the module does not fully close, not with
"Solr's advantage was entirely a naive-scan artifact."

**This reframes this project's four-times-repeated facet-crossover
finding (Phase 5, 6A, 6B, and P6C-E00 itself) with a materially more
specific, more actionable mechanistic explanation than "Solr's
algorithm beats any Lucene-based approach": the crossover is
substantially -- though evidently not entirely -- a property of naive
per-candidate facet-scanning specifically, not of generic-engine
faceting versus commerce-native faceting in general.** A specialized,
ordinal-based facet-counting approach -- the same class of technique
both Solr's `facet.field` and Lucene's own
`SortedSetDocValuesFacetCounts` module use -- is a genuine, concrete,
previously-untested candidate fix for commerce-native's own facet
crossover, not merely a hypothetical "Solr does something clever we
can't access." This is the single highest-value newly-enabled question
this experiment surfaces.

**Named limitations**: only the `SortedSetDocValuesFacetCounts` variant
was tested, not Lucene's alternative taxonomy-based faceting (which
supports hierarchical facets and might perform differently); no
profiling confirms *why* the module beats the scan at 6 of 7
checkpoints or loses at the 7th (the working hypothesis -- per-segment
ordinal counting avoiding a full per-document `HashMap` merge, traded
against `FacetsCollector`'s own fixed setup cost -- is standard,
documented Lucene facet-module behavior, not independently profiled
here); the two checkpoints where the module still trails Solr (Décor &
Pillows, Furniture) were not further investigated to determine whether
Solr's own advantage there is itself closeable with additional tuning;
whether commerce-native's own architecture could adopt an equivalent
ordinal-based approach, and by how much it would close the native
crossover, is untested -- a concrete implementation question, not yet
an experiment.
