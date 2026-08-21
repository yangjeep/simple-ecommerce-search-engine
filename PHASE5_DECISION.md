# Phase 5 Decision (Issue #17, governed by Issue #18's Workstream B)

**Decision: REVISE — Issue #18's own framework: NARROW BUT PUBLISHABLE.**
Commerce-native structural execution shows a real, large, and
correctness-verified physical advantage over a fairly-tuned Solr baseline
for browse/PLP-style filter, facet, sort, and pagination requests on this
catalog's real Brand/Color structure — but that advantage does **not**
uniformly clear Issue #18's own 80x physical-multiplier floor. Filter,
pagination, and concurrent-basic-filter throughput clear the floor at
every real size tested (by a wide margin: 2,799x-18,354x). Facet and sort
do not: both remain genuine wins through medium-sized real groups, but
collapse below 80x — and, for facet, fully reverse to a native *loss* — at
the large/huge real group sizes this catalog actually produces. This is a
precise, measured breakpoint, not a uniform result, and is reported as
such rather than smoothed toward a single headline multiplier.

This document is Phase 5's terminal decision artifact for Issue #17,
governed by Issue #18's Workstream B (Category/Collection/PLP toward
P95) and its cross-cutting Physical Multiplier Floor requirement. It does
not overwrite `PHASE2_DECISION.md` (a different epic, Issue #6) or the
Phase 3/4 decisions living on their own branches (Issues #14/#16) — all
remain historically accurate for their own scope.

## Recap: what Phase 5 was asked to answer

Issue #17 asks whether structurally-defined browse/category/collection/PLP
retrieval executes materially cheaper on commerce-native physical
representation than on a properly-tuned Solr baseline, with an explicit
mandate that "the baseline must be allowed to win." Issue #18's Workstream
B reframes this as: does browse/PLP native coverage approach 95%, and do
promoted mechanisms preserve the program's 80x physical-multiplier floor,
characterized via saturation/breakpoint curves rather than one favorable
idle-latency number.

## Architecture tested

`commerce_core::index::CatalogIndex`'s existing bitmap-based structural
retrieval (`indexed_candidates`, Gate 3/Phase 2 infrastructure, unchanged
this phase) plus two new methods added and adversarially verified this
phase: `facet_counts_by_scan`/`brand_facet_counts_by_scan`
(`O(|candidates|)` alternatives to the pre-existing `O(global vocabulary)`
`facet_counts`/`brand_facet_counts`, proven byte-identical via a dedicated
parity test in `crates/commerce-core/tests/physical_index.rs` before any
timing claim). Compared against a Solr 9.10.1 baseline with two real
fair-baseline gaps found and fixed this phase (missing `docValues` on
`brand`/`color`, no sortable `title_sort` field) via the live Schema API
plus full reindex — confirmed missing, not assumed. `CatalogIndex` has no
interior mutability (every field is a plain `HashMap`/`Vec`/
`RoaringBitmap`), making it naturally `Sync` for the concurrency
sub-experiment with zero synchronization overhead beyond `Arc`'s refcount.

## Datasets / workloads

The real 1,215,854-product Amazon ESCI catalog (identical to every prior
phase) and a live, locally-running Solr 9.10.1 instance re-indexed with
the identical catalog, schema-fixed as above.

**A named, real dataset limitation, established before any implementation
this phase and posted to Issue #17 directly**: this catalog has no
category/collection/hierarchy/price/inventory data anywhere — not a
mapping gap, but an absence confirmed by triangulating the raw
`products.parquet` schema, the full `catalog.jsonl` field scan, the
ingestion code's own hardcoded sentinels, and the Solr schema itself. Real
category/collection PAGE rendering (Issue #18's actual browse/PLP
scenario) is therefore **architecturally untestable on this dataset** —
not attempted, not approximated with fabricated data. Phase 5 instead
benchmarked the one real, non-fabricated structural slice this catalog
supports: Brand (206,227 distinct values, 94.05% coverage, tiny-to-medium
group sizes) and Color (175,292 distinct values, 66.60% coverage,
genuinely tiny-to-huge group sizes, independently documented as
frequently non-color noise). **Issue #18's "native coverage toward 95%"
metric cannot be evaluated for genuine category/collection traffic on
this dataset at all** — this is a real scope boundary on this decision's
applicability, not a result.

Within the Brand/Color slice that *is* real and testable, native coverage
is structurally 100%: every real product's brand/color attributes are
unconditionally bitmap-indexable, with no ambiguity-gating or admission
mechanism needed (unlike the free-text/search side of this program) —
this is the one clean, unqualified positive result of this phase.

## Measured results

Full data: `docs/experiments/PHASE5_LOG.md` (P5-E00 through P5-E03), raw
artifacts under `docs/research/artifacts/p5e00_run1/`, `p5e03_run1/`,
`p5e03_concurrency_run1/`.

**Correctness (checked before any speed claim, per this project's own
discipline)**: 37/41 measured P5-E00 rows had exact native/Solr count
agreement; all 4 mismatches were traced to root cause via direct Solr
queries and confirmed as two already-understood, non-bug artifacts (top-50
rank-boundary tie-breaking; brand-casing consolidation — native interns
brand case-insensitively, Solr facets the raw string). A follow-up P5-E03
sweep hit an 11/12 mismatch rate at larger candidate-set sizes; every
discrepancy checked (at both a 497-candidate and a 22,782-candidate real
group) reconciled exactly under three verified mechanisms: the
`BrandId(0)` "no brand field" sentinel (native buckets it as `""`, Solr's
terms facet excludes missing-field documents entirely — confirmed via
`missing:true`), n-way casing consolidation (verified to reconcile
exactly via direct Solr queries, e.g. `"STAR WARS":3` + `"Star Wars":20` =
native's merged `23`), and a cascading top-50 boundary effect caused by
that consolidation (verified via a higher-limit Solr query). None of this
affects any timing claim below.

**The 80x physical-multiplier floor (Issue #18's hard red line), checked
directly against the persisted `speedup_mean` data, using native's best
implementation at each request class**:

| request class | clears 80x floor at every real size tested? | where it fails |
|---|---|---|
| filter-only (brand or color) | **yes** — 2,799x-18,354x, tiny through huge | never |
| deep pagination | **yes** — 5,331x-12,715x, every size tested | never |
| concurrency (basic filter, 1-8 workers) | **yes** — native's single thread alone beats Solr's best 8-worker throughput by ~460-1,780x | never |
| facet (color-under-brand-filter) | **no** | already below floor at "medium" (86 candidates, 67.0x); further below at "large" (437, 4.26x) |
| facet (brand-under-color-filter) | **no** | holds through "medium" (104, 186.0x); fails at "large" (841, 4.32x); **reverses to a loss** at "huge" (1,844, 0.74x) |
| sort-by-title (brand-scoped) | **no** | holds through "medium" (86-product group, 193.7x); fails at "large" (1,249-product group, 16.97x) |
| sort-by-title (color-scoped) | **no** | holds through "medium" (106, 240.1x); fails at "large" (2,112, 8.62x) and "huge" (11,264, 1.67x) |

**Facet-cardinality crossover, characterized precisely (P5-E03)**: the
facet-scan method's `O(|candidates|)` cost crosses Solr's near-flat
`O(1)`-ish docValues-backed cost at a **~9,000-12,000-candidate transition
band**, not a sharp point — two independent runs of the identical binary
against the identical catalog/Solr instance placed the empirical crossover
one sampled data point apart (8,910/11,112 vs. 11,112/11,612 candidates).
Reported as a band, not a single number, to avoid overclaiming precision
the measurement doesn't have.

**Concurrency (P5-E03)**: native's single-thread throughput for the basic
filter operation (2.96-3.91 million requests/sec, `std::hint::black_box`-
verified against dead-code elimination given this workspace's
`lto=true, codegen-units=1` release profile) beats Solr's own best
8-concurrent-worker throughput (6,215-6,416 req/s) by **~460x-1,780x**
across both runs. Native scales sub-linearly with this container's 4 real
CPUs (2.7x-3.6x throughput from 1->4 workers, plausibly limited in part by
a shared `AtomicU64` counter in the *benchmark harness itself*, not
`CatalogIndex` — not further isolated), then behaves inconsistently under
oversubscription (regressed in one run, held flat in the other — an
honest disclosure of measurement noise at that specific regime, not a
clean story either way). Solr scales consistently upward at every level
in both runs, because its ~0.5-3.5ms per-request cost is dominated by
real network + JVM time that overlaps concurrent I/O wait — a genuinely
different scaling regime from native's CPU-bound, sub-microsecond,
no-I/O-to-overlap operation.

## Failed / fixed experiments (preserved, not erased)

Two real implementation bugs were found and fixed this phase, root-caused
before being trusted as architecture-level evidence:

1. **Native faceting's `O(global vocabulary)` cost** (`facet_counts`/
   `brand_facet_counts` scan all 206K/175K distinct values regardless of
   candidate-set size) — measured at 133-425ms vs. Solr's 1.2-3.0ms,
   independent of the real 80x-floor question above. Fixed with
   `O(|candidates|)` sibling methods, proven byte-identical via a parity
   test before any timing claim. Resolves the slowness for small/medium
   real groups (4x-7,800x faster); does not resolve it at large/huge real
   sizes (the crossover above) — a genuine, disclosed architectural limit
   of the scan approach itself, not a bug still to be fixed.
2. **A naive full-sort in the benchmark's own `native_title_sorted`
   helper** (sorted the *entire* candidate set for a 24-row page).
   Replaced with a `select_nth_unstable`-based top-k partial sort, proven
   equivalent to the naive baseline across 200 randomized trials — this
   test caught a real `limit=0` edge-case bug on first write (the initial
   guard silently returned *everything* instead of empty). Real, partial
   improvement (1.24x -> 1.67x speedup at 11,264 candidates); the
   remaining cost is an unavoidable `O(n)` per-candidate title lookup that
   no partial-selection algorithm can eliminate without a precomputed
   columnar/sorted structure — the same gap the Stage A audit already
   flags on Solr's own side (no explicit Lucene index sort configured).

Both fixes were self-directed adversarial corrections, not required by a
falsification condition being missed — consistent with this campaign's
"deepen every result" discipline. Both also **did not fully close the
gap** they targeted: the fixed facet method still loses to Solr past
~9,000-12,000 candidates; the fixed sort still degrades (though never
reverses to a loss) at large/huge sizes. Recording the partial nature of
both fixes, rather than declaring them "resolved," is itself part of this
decision's evidence base.

## Scope decisions (stated explicitly, not silently dropped)

- **Mutation/churn**: out of scope. `CatalogIndex::build` constructs an
  immutable snapshot with **no incremental-update API at all** — no
  `insert`/`update`/`remove` method exists anywhere in `commerce-core`.
  Benchmarking mutation cost would require building a new product
  capability first, which CLAUDE.md's "avoid production polish during
  this epic" rule prohibits. This is a real, disclosed architectural gap:
  any future scale-up must treat live catalog mutation support as
  currently **unimplemented**, not assumed away.
- **Sort diversity beyond title**: this catalog has no real
  price/rating/popularity field to test a second sort dimension against
  (the same real-data gap already established) — deferred as low marginal
  value given this dataset's actual constraints, not fabricated.
- **Cache temperature under a materially wider query mix**: confirmed
  (P5-E01) that Solr's default 512-entry caches are not a binding
  constraint for this phase's own narrow (5+5, then 25+25) query sets. A
  test that genuinely stresses cache capacity needs hundreds of distinct
  real queries with realistic reuse skew — a real, tractable follow-up,
  larger in scope than any single sub-experiment run this phase. Deferred.
- **Catalog-scale sensitivity on the Solr side**: would require Solr fully
  reindexed at each additional catalog size tested (a real ~5-minute
  operation per size, per this phase's own reindex timings) — materially
  larger infrastructure/time investment than this phase's other
  sub-experiments needed. The clearest instance this phase hit of
  CLAUDE.md's own stop condition ("the next meaningful experiment
  requires materially larger infrastructure... scope"); flagged as the
  first thing to build if this program scales up, not attempted here.

## Unresolved risks

1. **The concurrency advantage's fairness question is open, not
   resolved.** Native's ~460-1,780x throughput advantage compares an
   in-process Rust bitmap operation against an HTTP+JVM round trip on the
   same machine. No realistic Solr configuration changes the fact that it
   is a networked service — but this means part of the measured gap may
   reflect that structural difference rather than a purely
   representation-level architectural advantage. Not adjudicated here;
   flagged for whoever next decides how much weight this number should
   carry in a scale-up case.
2. **The facet/sort breakpoints were characterized at only the real
   group sizes this catalog happens to produce** (up to ~22,782 for the
   concurrency workload's color sample; up to 11,264 for the timed P5-E00
   facet/sort sweep). A catalog with proportionally larger real facet
   groups (more products per distinct brand/color, or genuine
   high-cardinality categories once real data existed) could shift these
   breakpoints — untested, not assumed either way.
3. **True category/collection browse-page coverage remains completely
   untested** on any real dataset available to this project (per the
   scope boundary above) — Issue #18's own "≥95% of category/collection/
   filter/facet traffic" target can only be partially evaluated here (the
   filter/facet/sort/pagination slice, at 100% structural coverage but a
   real facet/sort ceiling above); the category/collection portion of that
   target has no real evidence either way.
4. **The atomic-counter contention hypothesis for native's sub-linear
   core scaling was raised but not isolated** — a dedicated
   micro-experiment (e.g. per-thread local counters merged only at the
   end) would be needed to separate genuine `CatalogIndex` read
   contention from benchmark-harness bookkeeping overhead. Not attempted
   this phase.
5. Every unresolved risk `PHASE2_DECISION.md` already named and this
   phase didn't touch remains open (no cross-hardware validation; single
   shared/virtualized 4-vCPU environment; no production traffic
   validation).

## What would be built next if scaling up (conditional — see decision)

This phase's own evidence does not currently justify building any of
these; they are the concrete, falsifiable path that would extend or
re-test this decision:

1. **A hybrid facet-computation dispatch** — scan-based below the
   ~9,000-12,000-candidate transition band, a further-optimized (e.g.
   columnar/pre-sorted) approach above it — to test whether the facet
   floor violation at large/huge real sizes is fixable rather than
   fundamental, the same question this phase already answered for the
   *vocabulary-scan* approach (fixable) but has not yet answered for the
   *candidate-scan* approach's own large-size ceiling.
2. **A precomputed columnar/sorted title (or general sort-key) structure**,
   to test whether the sort floor violation is similarly fixable — the
   per-candidate `O(n)` title-lookup cost this phase identified as the
   remaining bottleneck is exactly what such a structure would eliminate.
3. **A multi-size Solr reindex sweep** (the deferred catalog-scale
   dimension), to test whether the measured breakpoints are a property of
   this specific 1.2M-product catalog or would shift materially at other
   real scales.
4. **A genuinely category/collection-structured real dataset**, to test
   Issue #18's actual browse/PLP coverage target rather than the
   Brand/Color proxy this phase was constrained to.
5. **A native incremental-update capability**, before any mutation/churn
   claim can be made at all — currently unimplemented, not merely
   unbenchmarked.

## What should explicitly not be built yet

- **Any facet/sort "fix" that trades correctness for a better number.**
  Both fixes this phase made were verified via dedicated parity/property
  tests before any timing claim; any future attempt at closing the
  large-size facet/sort gap must clear the same bar.
- **Distributed/sharded serving, cluster coordination, multi-tenancy,
  HA** — unaffected by this phase's results, per CLAUDE.md's standing
  rule for this epic.
- **A production LLM-backed reasoning path in the hot path** — unaffected
  by this phase's results, per CLAUDE.md's hard rule.
- **A claim that this phase's Brand/Color results generalize to true
  category/collection browse traffic** — the real-data scope boundary
  above makes this an open question, not a small extrapolation.

## What this decision does and does not claim

**Claims**: on the real 1,215,854-product Amazon ESCI catalog and a
fresh, same-environment, schema-fixed Solr 9.10.1 baseline, with
correctness verified before every timing claim (37/41 exact matches, 4/4
remaining mismatches traced to root cause and confirmed non-bugs) and two
real implementation bugs found, fixed, and disclosed as only partially
resolved: commerce-native structural filter and pagination operations,
and basic-filter throughput under concurrency, robustly clear Issue #18's
80x physical-multiplier floor across every real Brand/Color group size
this catalog produces (by 2,799x-18,354x, and by ~460x-1,780x for
concurrency). Facet and sort operations are genuine, real native wins
through medium-sized real groups but do not clear the 80x floor at
large/huge real sizes, with facet fully reversing to a native loss at the
largest size sampled. This is a precise, measured breakpoint, reported as
found rather than smoothed into a single headline multiplier.

**Does not claim**: that this result generalizes to genuine category/
collection browse-page traffic (architecturally untestable on this
dataset, a real scope boundary, not a finding either way); that the
facet/sort floor violations at scale are fundamental rather than fixable
(both remaining gaps have a named, plausible next fix, untested); that
the concurrency advantage is entirely architectural rather than partly an
artifact of comparing an in-process computation to a networked service
(an open, unadjudicated question); that native supports live catalog
mutation at any level (it does not, currently); or that Issue #18's
STRONG verdict criteria are met (they are not — the 80x floor is violated
for two of four measured operation classes at realistic real-data scale).
