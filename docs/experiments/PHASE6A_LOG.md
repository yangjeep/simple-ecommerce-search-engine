# Phase 6A Experiment Log — Issue #23: Cross-Dataset PLP Validation on WANDS

## Governing context

Issue #23 (Epic #21, Phase 6A) asks whether Phase 5's browse/PLP structural
advantages and cardinality/selectivity breakpoints are real properties of
commerce-native structural execution, or artifacts of ESCI and its
Brand/Color proxy workload. The named primary dataset is Amazon Reviews
2023 metadata, chosen for its real category hierarchy, price, and richer
product metadata. The governing rule for this pass is "characterize before
optimizing": reuse Phase 5's benchmark methodology, correctness gates, and
already-fixed native implementations as closely as the new schema permits;
do not introduce new engine optimizations, planner heuristics, or
benchmark-specific shortcuts.

## Dataset blocker and substitution (posted before any implementation)

Amazon Reviews 2023's only real distribution channels are unreachable from
this environment:

- `huggingface.co` and every CDN subdomain (`cdn-lfs.huggingface.co`,
  `cdn-lfs-us-1.huggingface.co`, `datasets-server.huggingface.co`) —
  confirmed organization-policy 403 (the proxy's own diagnostic explicitly
  distinguishes this from a transient failure: "Do not retry or route
  around it — report the blocked host").
- `mcauleylab.ucsd.edu` (the authors' own site) — same 403.
- `amazon-reviews-2023.github.io` (the project's docs site, itself a
  GitHub Pages domain, not `raw.githubusercontent.com`) — same 403.
- The authors' companion code repository (`hyp1231/AmazonReviews2023`,
  reachable via `raw.githubusercontent.com`) contains only preprocessing
  *scripts* that themselves fetch from Hugging Face — no raw metadata
  lives there.

No GitHub-LFS or GCS mirror of the raw metadata exists. Issue #21's other
named Phase 6 datasets were checked as fallbacks: Retailrocket is
Kaggle-only (`kaggle.com` also 403, no GitHub mirror found).
**WANDS (Wayfair ANnotation Dataset, `github.com/wayfair/WANDS`) is fully
reachable** via `raw.githubusercontent.com`, including the actual data
files, and was substituted with explicit user sign-off after presenting
the trade-offs directly:

- **Real, gains**: WANDS has a genuine, multi-level nested category
  hierarchy (unlike ESCI, which has none) and a real, independent
  `product_class` taxonomy (unlike ESCI's `ProductTypeId(0)` sentinel).
- **Real, disclosed losses**: no price field at all (confirmed absent by
  direct grep against the raw file, not just the parsed feature-key
  counter); no parent-ASIN/variant-grouping equivalent; a much lower
  total-corpus ceiling (42,994 products vs ESCI's 1,215,854).

This substitution changes what Phase 6A can test relative to Issue #23's
literal ask (no price-range/price-sort workload is possible at all), but
preserves the core research question (does the structural filter/facet/
sort/pagination advantage and its cardinality-dependent breakpoints
reproduce on an independent, genuinely hierarchical dataset).

## Dataset acquisition and profiling (before any mapping code was written)

Pinned to commit `3b74dcf4ba29ab8ff3e6a50b5b09fc627cb882b5` (not `main`) for
reproducibility; checksums archived in
`scripts/datasets/wands_checksums.sha256`. Full provenance:
`docs/research/artifacts/p6a_dataset_acquisition/manifest.json`.

`scripts/datasets/profile_wands.py`'s findings (raw output:
`docs/research/artifacts/p6a_dataset_acquisition/wands_profiling_output.log`)
governed every mapping decision below:

- **42,994 products**, 860 distinct `product_class` values (2,852 null),
  1,556 products with no category hierarchy at all.
- **Category hierarchy depth 0–6**, distinct real nodes per depth: 55 (d1),
  138 (d2), 458 (d3), 806 (d4), 492 (d5), 71 (d6). `product_class` is a
  genuinely separate taxonomy from the hierarchy breadcrumb — only 28.3%
  of products have their hierarchy's leaf segment exactly equal to
  `product_class` (11.5% match the second-to-last segment) — so it is
  mapped independently, not derived.
- **`product_features` key-frequency analysis**: `color` (61.2%
  coverage among products with any color, 2,825 distinct values — high
  cardinality), `style` (65 distinct — low cardinality), `material`
  (162), `primarymaterial` (244), `shape` (94) are the only keys with
  broad, real coverage suitable as facet dimensions.
- **Confirmed absent, not fabricated**: no price/cost/msrp-like key
  anywhere in the raw file (direct grep, zero matches); no
  brand/manufacturer/store key with meaningful coverage (the closest
  matches — `brand`, `fashionbrand`, `manufacturerpreferredname` — each
  appear on 1–2 products out of 42,994); no parent-ASIN/variant-group
  column of any kind; no availability/inventory field.
- **Zero casing collisions** across every field used in the benchmark
  (color, product_class, category strings, style, material,
  primarymaterial, shape) — WANDS' fields come from one internal Wayfair
  taxonomy, not free-text sellers, so (unlike Phase 5's brand field) no
  case-insensitive-regex fairness fix is needed for query construction.

## Commerce mapping (crates/phase6a-eval)

- `product_id` → `ProductId`, one `Product` with exactly one `Variant`
  each (no parent-ASIN grouping exists to support anything richer — the
  same degenerate mapping `round1_eval::catalog` already uses for ESCI).
- The deepest available hierarchy segment (as a full prefix path) →
  `Product::category` (`CategoryId`) — the first real, non-sentinel
  exercise of `CatalogIndex`'s dedicated `category_bitmaps` index in this
  project's history.
- `product_class` → `Product::product_type` (`ProductTypeId`) — likewise
  the first real exercise of `product_type_bitmaps`.
- `category_depth_1..6` → typed `Enum` attributes, valued by the *full
  prefix path* at that depth (not the bare segment name), so two
  different subtrees sharing a segment name at the same depth never
  collide. Subtree-browse-at-any-ancestor-depth is therefore a plain
  `Constraint::Enum` filter, reusing exactly the generic enum-attribute
  bitmap machinery Phase 5 used for `color` — no new query semantics.
- `color`/`style`/`primarymaterial`/`material`/`shape` → variant `Enum`
  attributes (present only when the source record has a value).
- `rating_count`/`average_rating`/`review_count` → variant `Numeric`
  attributes — explicitly **not** a price substitute (recorded as
  confirmed absent above); carried through only as a disclosed
  business-order-sort analog (`average_rating desc`), never presented as
  price.

### Base indexing infrastructure added (not an optimization)

`commerce_core::index::CatalogIndex` already had dedicated
`category_bitmaps`/`product_type_bitmaps` (parallel to `brand_bitmaps`)
since Gate 3, but only `brand_facet_counts`/`brand_facet_counts_by_scan`
existed as dedicated-bitmap facet methods (added in Phase 5, since ESCI
never had real category/product_type values to facet on). Added
`category_facet_counts`/`_by_scan` and
`product_type_facet_counts`/`_by_scan`, mirroring `brand_facet_counts`
exactly — completing existing, already-designed capability for two
fields that already had their own bitmap index, not a new mechanism. 4
new tests in `physical_index.rs` verify correctness against
`cold_start_catalog` and cross-check scan-parity.

## P6A-E00: real PLP filter/facet/sort/pagination benchmark

`crates/phase6a-eval/src/bin/p6a_e00_wands_vs_native_eval.rs`. Reuses
Phase 5's REPS=30/WARMUP=5 methodology and already-fixed native
implementations (`select_nth_unstable`-based top-K sort,
`facet_counts_by_scan`) from the start, rather than re-discovering either
bug. Real category-leaf/depth-3 groups selected across real size buckets
(seeded, ChaCha8Rng seed=7), mirroring Phase 5's `pick_one`/`bucket_for`
pattern exactly.

### Two real bugs found and fixed before trusting any result

1. **Casing-representation mismatch** (the same pattern Phase 5 found for
   `brand`): `build_catalog` normalizes (`trim`+`lowercase`)
   `product_class` before interning it as a `ProductTypeId`, but Solr's
   raw `product_class` field holds the original casing. Comparing
   native's normalized facet key against Solr's raw-cased key produced
   spurious mismatches (`"desks": 2` vs `"Desks": 2` — same count,
   different string). Fixed with a `product_type_raw_by_id` map
   (first-seen raw string per `ProductTypeId`), mirroring Phase 5's own
   `brand_raw_by_id` fix exactly.

2. **Facet-bucket-sum used as candidate count** — a self-repeat, within
   this same work session, of the exact bug already found and corrected
   in `PHASE5_DECISION.md` during Issue #21's repo-normalization pass.
   Every facet-type row initially used
   `native_facets.values().sum()`/`solr_facets.values().sum()` (both
   top-50-truncated) as the row's `native_count`/`solr_count`. This was
   caught immediately by a real, surprising discrepancy: a "Rugs"
   `category_depth_1` group reported 554 candidates in the printed table,
   while a direct, independently-verified check (raw `catalog.jsonl`
   grep, a properly-`--data-urlencode`d curl against Solr, and the
   benchmark's own row-1 `category_render` count for the same filter)
   all agreed on 2,002 — the true count. Fixed by reusing each block's
   already-computed true filter bitmap's own `.len()` (or, where not
   already available, one untimed `solr_num_found` call) for every facet
   row's count columns; facet-*map* correctness checking (the
   `mismatches` list) was already, and remains, entirely separate from
   this count.

### Results (`docs/research/artifacts/p6a_e00_run1/`)

Final correctness: **41/41 rows match on true candidate/filter count**.
The only two remaining `FACET MISMATCH` entries are the exact same
explained, non-bug missing-value-sentinel pattern Phase 5 already
documented for `BrandId(0)` — native includes an empty-string `""`
bucket for the `ProductTypeId(0)` sentinel (products with no
`product_class`); Solr's terms facet excludes missing-field documents
entirely. Reproducing here for a *different* field on a *different*
dataset is itself a small, reassuring confirmation that the mechanism is
general, not an ESCI/brand-specific quirk.

| request class | tiny (2) | small (13) | medium (121/132) | large (1,063–1,103) |
|---|---|---|---|---|
| category_render_filter_first_page | 8,020x | 19,768x | 26,769x | 18,865x |
| subtree_browse_depth3_filter_first_page | 10,280x | 21,699x | 17,442x | 9,584x |
| deep_pagination_under_category_filter | — | — | 26,330x | 19,203x |
| color_facet_under_category_filter (high card., 2,825) | 682x | 144x | 13.3x | 1.39x |
| mixed_color_facet_under_subtree_depth3 | 243x | 69x | 14.2x | 1.63x |
| product_class_facet_under_category_filter (860) | 5,520x | 2,072x | 232x | 26.2x |
| style_facet_under_category_filter (low card., 65) | 606x | 102x | 9.3x | 1.74x |
| sort_title_under_category_filter | 5,670x | 2,583x | 223x | 16.6x |
| sort_rating_desc_under_category_filter (disclosed substitute) | 4,231x | 1,992x | 99.4x | 8.95x |

### Crossover-characterization sweep (real depth-1 subtrees, targeted checkpoints)

Leaf categories top out at only 1,103 real products in this taxonomy (no
leaf reaches even the low end of Phase 5's own "huge" bucket) — a real,
disclosed scale-ceiling difference from ESCI's color groups (up to
11,264), not a fabricated distribution. Real depth-1 subtrees reach much
larger real sizes (up to 16,039 for "Furniture"), letting the
native-loss crossover be characterized precisely:

| depth-1 subtree | candidates | native/Solr ratio |
|---|---|---|
| Rugs | 2,002 | 1.00x (parity) |
| Lighting | 2,072 | 1.01x (parity) |
| Storage & Organization | 2,175 | 0.51x (loss) |
| Outdoor | 3,394 | 0.48x |
| Décor & Pillows | 4,612 | 0.27x |
| Home Improvement | 4,686 | 0.23x |
| Furniture | 16,039 | 0.07x |

The crossover to a native loss occurs between **2,072 and 2,175
candidates** — a real, sharply *lower* threshold than Phase 5's
~9,000–12,000-candidate ESCI crossover. The transition itself is steeper
(near-parity at 2,072 to a 2x loss at 2,175, a ~5% candidate-count
change) than Phase 5's own gradual decay, and Solr's own cost stays
nearly flat (1.5–1.7ms) across this entire range while native's grows
roughly linearly with candidate count — consistent with the `_by_scan`
method's `O(|candidates|)` cost model.

**Plausible physical explanation** (not independently isolated by a
controlled ablation, so stated as plausible rather than proven): WANDS'
per-product attribute map carries more keys (up to 6 category-depth
attributes plus color/style/material/primarymaterial/shape/3 numeric
rating fields — roughly a dozen) than ESCI's (brand/color/description/
bullets — around 4), and `effective_attributes` clones and merges this
map on every scanned candidate. A larger per-candidate attribute map
would make each `_by_scan` iteration more expensive, shifting the
crossover to a lower absolute candidate count without changing its
fundamental `O(|candidates|)` shape. Note also that facet *cardinality*
itself does not appear to drive the threshold: `style` (65 distinct
values, low cardinality) crosses below the 80x floor at almost exactly
the same candidate count as `color` (2,825 distinct, high cardinality) —
606x→102x→9.3x→1.74x vs 682x→144x→13.3x→1.39x — consistent with a
per-candidate fixed cost (attribute lookup/clone) dominating over the
number of distinct output buckets.

## P6A-E01: concurrency sweep

`crates/phase6a-eval/src/bin/p6a_e01_concurrency_sweep.rs`. Mirrors
Phase 5's `p5e03_concurrency_sweep` exactly, including
`std::hint::black_box` applied from the start (Phase 5 found and fixed
this as an afterthought; here it was built in from the first run,
avoiding a repeat of that lesson). Real category_leaf/category_depth_1
queries (25 each), 4 concurrency levels (1/2/4/8; this container has 4
real CPUs).

Result (`docs/research/artifacts/p6a_e01_concurrency_run1/`): native's
single-thread throughput (3.02M req/s) exceeds Solr's best 8-worker
throughput (4,219 req/s) by **~717x**, growing to **~2,578x** at native's
own 8-worker level. Directionally and in order of magnitude, this matches
Phase 5's ESCI finding (~460x–1,780x) closely — **ROBUST** across both
datasets.

## Scope decisions (stated explicitly, not silently dropped)

- **Price-range and price-sort workloads**: not attempted. WANDS has no
  price field at all (confirmed absent). NOT COMPARABLE, not fabricated.
- **True variant-scoped constraints**: not attempted. WANDS has no
  parent-ASIN/variant-grouping equivalent. NOT COMPARABLE.
- **Availability gating**: not attempted. WANDS has no availability/
  inventory field. NOT COMPARABLE.
- **Multiple simultaneously-active filters** (e.g. category AND color as
  two live constraints in one request, as opposed to one filter plus one
  facet): not separately measured in this pass. The `mixed_color_facet_
  under_subtree_depth3` rows combine a subtree filter with a facet, which
  is the closest tested analog, but a genuine 2-active-filter selectivity
  sweep (correlating breakpoint movement with "number of active filters"
  as its own physical variable, as Issue #23 asks) is deferred, not
  silently dropped — noted as a concrete Phase 6B/6C candidate.
- **A catalog-scale ladder beyond WANDS' own real ceiling**: WANDS' real
  corpus is fixed at 42,994 products; no synthetic upsampling was applied
  to reach Issue #23's 100k+ tiers, since that would require inventing
  data the source doesn't have. The scale/cardinality axis actually
  varied here is real group/subtree size (2 to 16,039 real products),
  matching the axis Phase 5 itself varied against ESCI's own fixed
  1,215,854-product catalog.
