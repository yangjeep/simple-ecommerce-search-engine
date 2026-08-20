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

## Experiment index

- **P5-E00** — real Brand/Color workload generation from actual catalog
  distributions (not fabricated); Solr baseline audit artifact. (next)
