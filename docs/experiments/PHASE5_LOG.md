# Phase 5 Experiment Log — Issue #17: Browse/PLP as a Commerce-Native Fast-Path Workload

## Governing context

Issue #17 asks whether structurally-defined browse/category/collection/PLP
retrieval (category pages, filters, faceting, sorting, pagination) executes
materially cheaper on a commerce-native physical representation than on a
properly-tuned Solr baseline, and whether that advantage is large enough to
move whole-workload economics when combined with Issue #14's safe free-text
offload. Its own text is explicit that the baseline must be allowed to win:
"Solr/Elasticsearch have substantial optimizations for filters, caches,
docValues, faceting, and repeated queries. The experiment must be designed
so that the baseline is allowed to win." A follow-up review comment adds a
mandatory Stage A (strongest-realistic-Solr-baseline-first) / Stage B
(saturation/breakpoint campaign) execution order.

## Real-data scoping finding (posted to Issue #17 before any implementation)

Before writing any benchmark code, a multi-angle investigation (raw
`products.parquet` schema via `pyarrow`, full `catalog.jsonl` field scan,
`round1_eval::catalog` ingestion re-read, Solr `managed-schema.xml`
inspection, and a git-history check for any alternative dataset ever
fetched) confirmed, triangulated across three independent layers:

1. **The raw source has no category/product-type/browse-node/price/
   inventory field at all** — not a mapping gap. `dataset_cache/products.parquet`'s
   own on-disk schema (read directly, not inferred) has exactly
   `product_id/title/description/bullet_point/brand/color(/locale)`.
2. **The ingested domain model hardcodes every real product to the
   identical sentinel** `ProductTypeId(0)`/`CategoryId(0)`/`Price::usd(0)`/
   `Inventory::in_stock(0)` (`round1_eval::catalog::build_catalog`) —
   constants, not derived values. `commerce_core::domain::Category` is
   flat (`{id, name}`), no parent/hierarchy field; no `Collection`/
   `BrowseNode` concept exists anywhere in `crates/`.
3. **The Solr schema side has the identical absence** — `managed-schema.xml`
   defines no category/collection/price/inventory field, not even an
   unpopulated placeholder (unlike `brand_lower`, which exists but was
   never populated, a separate already-documented bug, P2-E13).

**No alternative real dataset was ever fetched, partially wired in, or
evaluated-and-rejected** in this repo's history (confirmed via
`scripts/`/`crates/`/`docs/` search and git log). `eCommerceSearchBench`
(named in `docs/research/HOW_DRIVEN_THESIS.md`) is real, Apache-2.0, and
models Taobao's Ha3/Havenask stack, but is a multi-service build-and-run
system (Gradle/Maven/JDK8/Docker/Kubernetes), not a downloadable corpus,
with its own goods schema unconfirmed (category/price fields visible only
in an unopened image asset) — adopting it now would mean standing up a
distributed reference architecture, directly conflicting with CLAUDE.md's
"avoid distributed systems work"/"avoid production polish" rules for this
epic. Flagged as a named future candidate, not adopted.

**What real structured data does exist, and its real shape**:

- **Brand**: 206,227 distinct values, 94.05% catalog coverage, but capped
  at ~6,165 products for the single largest brand (nike) — a
  tiny→medium size distribution only, no "very large" tier the way a real
  category ("Electronics") would have. Many singleton "brands" are junk
  (slogan/title fragments in the brand field).
- **Color**: 175,292 distinct raw values, 66.60% coverage, genuinely spans
  tiny→huge (11 values over 10,000 occurrences; "Black" alone at 125,446,
  ~10% of the catalog) — the right *size shape* for category-like testing.
  But its *content* is independently documented (R1-E02/R1-E02b,
  `docs/experiments/ROUND1_LOG.md`) as frequently non-color noise
  (marketing fragments like "10 Gallon"/"With Heater", casing-split
  duplicates, single-valued-attribute AND-contradiction effects from
  multi-value extraction), with only 5.0-6.0% real-query filter recall
  when trusted as-is.

**Scope decision, stated explicitly rather than silently narrowed**:
category, collection, hierarchy, price-range, and inventory-gating testing
are **dropped from the real-data track** — any such test would necessarily
run against constant fabricated values or an invented taxonomy, exactly
what Issue #17's own text and CLAUDE.md prohibit ("do not fabricate").
Phase 5 instead benchmarks a **Brand-primary, Color-secondary (noise
disclosed, never smoothed over)** filter/facet/sort/pagination workload —
still real, non-fabricated catalog structure, and (a genuine silver
lining) the one comparison where neither system has an asymmetric
handicap, since Solr's own schema is missing the identical dimensions.

## Falsifiable hypothesis (narrowed scope, stated before implementation)

Among real Brand/Color-based filter, facet, sort, and pagination requests
constructed from this catalog's own real value distributions (not
fabricated), commerce-native execution via `CatalogIndex`'s existing
bitmap-based structural filtering/faceting (`indexed_candidates`,
`facet_counts`, `execute_ranked` — all already built, Gate 3/Phase 2) is
materially cheaper than a *strongest-realistic* Solr baseline (non-scoring
`fq`, filter/DocSet cache reuse, correct facet method, warm-state
measurement) for at least some request classes, and that advantage either
does or does not survive under Stage B's saturation/breakpoint campaign
(catalog scale, selectivity, facet cardinality, sort diversity,
concurrency, cache temperature, mutation/churn).

**Falsification conditions, stated up front**: if the strongest-tuned Solr
baseline erases the expected advantage for a request class (Issue #17's
own explicit "NEGATIVE" outcome is valid and must be preserved, not
smoothed over), or if a large native win cannot survive the required
adversarial checks (cache misses, wrong facet method, unequal result
counts), that is the recorded result for that class.

## Measurement plan (defined before implementation)

Following this project's established discipline: a required baseline
audit artifact (per-class schema fields, DocValues config, facet method,
cache mode, sort/index-sort relationship, result-count validation, warm/
cold regime) before any promoted measurement; Stage A (strongest-
realistic Solr baseline, iterated until no further tuning is found, before
/after evidence preserved) strictly before Stage B (one-dimension-at-a-time
saturation sweep, then combined realistic stress); >=30 reps for
paper-grade timing; bootstrap CIs; adversarial review of any large win;
raw artifacts under `docs/research/artifacts/p5e{NN}_run1/`.

## P5-E00 — real Brand/Color benchmark, Solr Stage A fixes, two adversarial follow-ups

**Setup**: `crates/phase5-eval/src/bin/p5e00_solr_vs_native_eval.rs` against
the real 1,215,854-product catalog, seeded (`SEED=7`) representative
brand/color groups across every non-empty real size bucket
(tiny/small/medium/large[/huge for color]), four request classes per group
(filter-only first page, facet, sort-by-title, deep pagination where the
group is large enough), 5 warmup + 30 measured reps each. Every count
checked against Solr's `numFound`; every facet map checked against Solr's
JSON Facet API buckets. Raw artifacts: `docs/research/artifacts/p5e00_run1/`
(`full_run_output.log`, `results.csv`).

**Stage A Solr baseline fixes (required before any promoted measurement,
per Issue #17's own mandate)**: live Schema API confirmed `brand`/`color`
had `docValues=false` (a real unfair-baseline gap) — fixed via
`replace-field` + full reindex (1,215,854 docs, ~290s). Solr cannot sort on
a tokenized `text_general` title field, so added a dedicated `title_sort`
(string, `docValues=true`, `copyField<-title`) and reindexed again. Full
audit table is emitted by every run (see `print_baseline_audit_table` in
the binary, or the tail of `full_run_output.log`).

**Correctness**: 37/41 measured rows had exact native/Solr count agreement.
The 4 that didn't reduce to exactly 2 independent, fully-explained
artifacts (each counted twice because the vocabulary-scan and
candidate-scan brand-facet methods are mathematically identical over the
same input, confirmed below) — not real bugs:
1. **Top-50 rank-boundary tie-breaking**: native's tie-break (alphabetical)
   and Solr's own internal tie-break for equally-tied counts disagree at
   the truncation boundary of a size-capped facet request, so a handful of
   count-5/6 brands appear in one side's top-50 and not the other's.
2. **Brand-casing consolidation**: native facets by `BrandId`, which
   `round1_eval::catalog` interns case-insensitively, so `"STAR WARS"` and
   `"Star Wars"` collapse into one native bucket (`"STAR WARS": 9`); Solr
   facets on the raw string field and keeps them separate (`"Star Wars":
   6"`, no `"STAR WARS"` bucket). Forcing an exact match would require
   abandoning either normalization consistency or reverse-engineering
   Solr's undocumented tie-break rule — not worth it for a footnote.

**Finding 1 (real, now investigated to a resolution): native faceting's
O(global-vocabulary) cost was an avoidable representation choice, not
fundamental — but the fix has its own real limit.** The existing
`facet_counts`/`brand_facet_counts` scan the *entire* field vocabulary
(206K distinct brands / 175K distinct colors) regardless of candidate-set
size, measured at 133-425ms against Solr's 1.2-3.0ms. Two new
`O(|candidates|)` sibling methods (`facet_counts_by_scan`,
`brand_facet_counts_by_scan`, `crates/commerce-core/src/index/mod.rs`)
were added, proven byte-for-byte identical to the existing methods via a
dedicated parity test
(`facet_counts_by_scan_matches_facet_counts_exactly`,
`crates/commerce-core/tests/physical_index.rs`) before trusting any timing
comparison. Real measured results (`color_facet_under_brand_filter_scan` /
`brand_facet_under_color_filter_scan` rows):

| candidates (facet under filter) | vocab-scan (old) | candidate-scan (new) | Solr | scan vs Solr |
|---|---|---|---|---|
| 2 (S2 Black) | 42.7ms | 0.0002ms | 1.47ms | 7792x faster |
| 28 (Subalpine) | 80.1ms | 0.0012ms | 1.47ms | 1226x faster |
| 106 (Grey/White) | 156.4ms | 0.0102ms | 1.90ms | 186x faster |
| 1,249 (simple joys, color facet under brand filter) | 198.9ms | 0.70ms | 2.99ms | 4.3x faster |
| 2,112 (Multicolored) | 232.5ms | 0.55ms | 2.39ms | 4.3x faster |
| **11,264 (Clear)** | 424.9ms | **4.05ms** | **3.00ms** | **0.74x — Solr wins** |

The candidate-scan method resolves the slowness for every realistic
filtered-PLP-facet candidate-set size in this real catalog's own
distribution (tiny through large), turning a 20-80x native *loss* into a
4-7800x native *win*. But it does not uniformly win: at the huge end
(11,264 candidates) it crosses over to being slower than Solr's
docValues/ordinal-backed facet, because its cost is linear in
`|candidates|` while Solr's is effectively flat regardless of candidate-set
size. This crossover is a real, disclosed boundary, not smoothed over —
exactly the kind of breakpoint Stage B (P5-E03) is supposed to characterize
precisely (at what candidate-set size does the crossover occur, and does a
hybrid dispatch — scan below a threshold, vocabulary-scan or a future
columnar structure above it — recover the win at every scale).

**Finding 2 (real, partially resolved): naive full-sort inefficiency in
sort-by-title.** `native_title_sorted` (the benchmark's own helper, not
`commerce-core`) did a full `Vec::sort()` of the entire candidate set even
though only the first `limit` (24, a PLP page) results are ever consumed.
Replaced with `top_k_sorted`, using `select_nth_unstable` to partition in
O(n) average and sort only the surviving `limit` elements (O(n + k log k)
instead of O(n log n)) — proven equivalent to the naive
sort-then-truncate baseline across 200 randomized trials plus edge cases
(`limit=0`, `limit` larger than input) in
`top_k_sorted_tests::matches_naive_full_sort_then_truncate_across_random_inputs`
(this test caught a real bug on first write: the initial guard
`items.len() > limit && limit > 0` silently returned *everything* instead
of empty when `limit == 0`, fixed by handling `limit == 0` as its own
early-return case). Real measured improvement:

| candidates | before (full sort) | after (top-k) | speedup vs Solr, before -> after |
|---|---|---|---|
| 2,112 (Multicolored) | 0.94ms | 0.63ms | 5.64x -> 8.62x |
| 11,264 (Clear) | 4.16ms | 2.98ms | 1.24x -> 1.67x |

This is a genuine, honest partial fix, not a full resolution: the
remaining cost for large candidate sets is dominated by the O(n)
per-candidate `lookup_variant` + `title.clone()` needed to even know each
candidate's sort key, which no partial-selection algorithm can avoid
without a precomputed columnar/sorted title structure -- exactly the
`sort/index-sort` gap the Stage A audit table already flags as a
disclosed, unexploited Solr-side optimization too (no explicit Lucene
index sort configured). Both sides have room to improve further here;
flagged for Stage B rather than claimed as closed.

**Interpretation**: this is real evidence that native structural
filter/facet/sort/pagination execution can materially beat a
strongest-realistic, fairly-tuned Solr baseline on every request class and
every real group-size bucket this catalog produces except one (the facet
crossover at very large candidate-set sizes), *given* the two real
implementation bugs above are fixed rather than left as unexamined
"native is just slower" folklore. Both fixes were adversarial
self-corrections, not required by the falsification conditions being
missed -- exactly the "deepen every result" discipline this campaign
operates under.

## Experiment index

- **P5-E00** — real Brand/Color workload generation from actual catalog
  distributions (not fabricated); Solr Stage A baseline audit + fixes
  (docValues, `title_sort`); two adversarial follow-ups (O(|candidates|)
  facet-scan methods, top-k sort) with real measured before/after and one
  disclosed remaining crossover. Done.
- **P5-E01** — formal Stage A closeout: fold this entry's Solr-side fixes
  into a standalone audit doc; confirm no further realistic Solr tuning is
  available before Stage B. (next)
