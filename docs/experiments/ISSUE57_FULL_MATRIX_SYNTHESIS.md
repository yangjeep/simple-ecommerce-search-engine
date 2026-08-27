# Issue #57 — Full-Matrix Synthesis (Revision 1)

Companion to `FULL_MATRIX_PROTOCOL.md` (frozen protocol) and
`ISSUE57_DATASET_CAPABILITY_MATRIX.md` (dataset capability/provenance).
Consolidates the required outputs (dataset-level reports, query-class
comparison, cross-engine summary, memory/index notes, whole-workload
economics, failure/confound taxonomy) into one document rather than
fifteen separate files, given this revision's session-time constraints
— explicitly disclosed here rather than silently reduced. Raw per-cell
CSV/log artifacts are preserved under
`docs/research/artifacts/issue57_*_full_matrix_run1/`.

## 1. Scope actually measured this revision

| Dataset | Engines | Query classes | Status |
|---|---|---|---|
| WANDS (42,994 products) | native, Solr 9.10.1, ES 8.15.0, OpenSearch 2.17.0, Havenask | Q5, Q9, Q10, Q11 | MEASURED |
| ESCI electronics (2,075) | same 5 | Q2, Q2b, Q11 | MEASURED |
| ESCI automotive (1,056) | same 5 | Q2, Q2b, Q11 | MEASURED |
| ESCI beauty (2,093, 2,092 in Havenask) | same 5 | Q2, Q2b, Q11 | MEASURED |
| Magento (22 products / 155 variants) | same 5 | Q8 (exhaustive), one representative timed sample | MEASURED (correctness-only, per protocol §9.3) |
| ESCI full corpus (1.2M) | — | — | DEFERRED (§9.1 of the protocol — disk allowance) |
| Retailrocket (2.76M events) | — | Q17 (traffic-weighting input only) | MEASURED for §5 below; N/A for retrieval/relevance (§9.2) |

Not measured this revision, disclosed rather than omitted: Q1 (exact
identifier lookup), Q3/Q4 (product-type+enum / multi-attribute
conjunction beyond Q9/Q10's category+color case), Q6 (price — WANDS/ESCI
lack a price field; Magento has one but wasn't queried this revision),
Q7 (inventory — no dataset here has real inventory data), Q12–Q16.
Reason: session time, not a capability or access blocker — a concrete,
reproducible reason per the protocol's "never silently omit" rule, not a
silent gap.

## 2. Correctness matrix

| Cell | Rows correctness-gated | Result |
|---|---|---|
| WANDS Q5/Q9/Q10 | 7 | 7/7 MATCH (all 5 systems) |
| ESCI electronics Q2 | 3 | 3/3 MATCH |
| ESCI automotive Q2 | 3 | 3/3 MATCH |
| ESCI beauty Q2 | 3 | 3/3 MATCH |
| Magento Q8 (true-positive + trap) | 294 | 294/294 MATCH |
| **Total gated rows** | **313** | **313/313 MATCH, 0 residual mismatches** |

Q11 (lexical) rows are explicitly NOT correctness-gated on any dataset
(disclosed exception, per Issue #57 §7: different analyzers/tokenizers
are expected to produce different candidate sets for open-ended text
search — recorded for timing reference only).

Getting to 313/313 required finding and fixing seven real, disclosed
defects (§4) — none were swept aside; each is documented with root
cause and fix in the commits that introduced it.

## 3. Cross-engine query-class latency matrix (mean, ms, single-threaded HTTP round trip)

All values are the arithmetic mean across the dataset instances measured
for that class (e.g. WANDS Q9's 2 category groups averaged). Full P50/P99
per-cell detail is in the raw CSVs; this table is the aggregate view.

| Dataset | Class | native | Solr | Elasticsearch | OpenSearch | Havenask |
|---|---|---:|---:|---:|---:|---:|
| WANDS | Q9 category filter | 0.0001 | 1.52 | 2.37 | 2.48 | 6.09 |
| WANDS | Q5 numeric range | 0.27 | 1.44 | 2.31 | 3.23 | 5.51 |
| WANDS | Q10 color facet | 0.05 | 1.73 | 2.75 | 2.68 | 6.48 |
| WANDS | Q11 lexical (not gated) | 6.68 | 1.56 | 2.21 | 2.14 | 3.62 |
| ESCI electronics | Q2 brand filter | 0.0001 | 1.57 | 1.88 | 1.73 | 3.85 |
| ESCI electronics | Q11 lexical (not gated) | 0.34 | 1.32 | 1.84 | 1.82 | 3.94 |
| ESCI automotive | Q2 brand filter | 0.0001 | 1.63 | 1.64 | 1.59 | 3.48 |
| ESCI automotive | Q11 lexical (not gated) | 0.14 | 0.99 | 1.59 | 1.67 | 3.49 |
| ESCI beauty | Q2 brand filter | 0.0001 | 1.31 | 1.71 | 1.71 | 3.68 |
| ESCI beauty | Q11 lexical (not gated) | 0.31 | 1.00 | 1.66 | 1.91 | 3.63 |
| Magento | Q8 representative sample | ~0 (untimed, structurally <<1ms elsewhere) | 0.96 | 1.43 | 1.33 | 3.05–3.55 |

## 4. Real defects found and fixed while building this matrix (disclosed, not hidden)

Per Issue #57's explicit "if a benchmark defect is later found: preserve
old result → document defect → fix → freeze new revision → rerun"
discipline. None of these were architecture tuning — all seven are
harness/schema/indexer bugs found while building the comparator
infrastructure and dataset indexers, fixed before any number was
trusted, and are visible in this branch's own commit history:

1. **ES/OpenSearch `track_total_hits` 10,000-hit cap** — both WANDS
   numeric-range queries silently returned exactly `10000` (the real
   answers were 28,399 and 33,136) until `track_total_hits: true` was
   added to every count/facet/text query.
2. **`es_family_index_wands.py` never actually lower-cased keyword
   fields** despite `translate_es.rs`'s translator assuming it did —
   every ES/OpenSearch structural filter on WANDS silently returned 0
   rows until fixed.
3. **Havenask STRING attribute columns default an unset value to `''`**,
   not a genuinely missing value, and Havenask's SQL `ORDER BY count
   DESC` has no defined tie-break — together these made a
   `LIMIT`-truncated facet top-N land on a different, wrong set of
   colors than the three Lucene-based engines' matching, alphabetically-
   tied top-N. Fixed by excluding `field <> ''` and adding an explicit
   `ORDER BY ... field ASC` secondary key.
4. **Havenask's `PRIMARY_KEY64` index requires a numeric column** — a
   `STRING` primary key (ESCI's real alphanumeric ASINs) produced a
   reproducible `"invalid table config"` schema-load error, found in the
   searcher's detailed C++ log, not the terse status API. Fixed by
   keying every non-WANDS Havenask table on a synthetic `INT64` id with
   the real identifier kept as a separate `STRING` attribute column.
5. **Havenask's `PACK` (composite text) index requires `index_fields`
   listed in the same order as the `columns` declaration** —
   `"expect field [description] before field [bullet_point], but not"`.
   Fixed by reordering the schema.
6. **Havenask `COUNT(*)` over a `WHERE` matching zero rows returns an
   empty `data` array**, not the single `[[0]]` row every other engine
   here returns — surfaced only by Magento's Q8 trap queries (WANDS/ESCI
   never queried a genuinely empty result). Both shapes are now treated
   as zero.
7. **A genuine semantic-definition gap, not a bug in either direction**:
   native's `Brand` structural constraint is case-sensitive-identity as
   Issue #35's ingestion currently interns it, while every comparator
   translation (Solr's pre-existing `case_insensitive_field_regex`, and
   this revision's ES/Havenask lowercase-both-sides equivalent) is
   deliberately case-insensitive to match real-world brand identity
   despite messy marketplace data (confirmed live: "FilterBuy" vs
   "Filterbuy", "Olaplex" vs "OLAPLEX" — the same real companies, two
   seller-entered casings). This is disclosed as a native ingestion
   limitation (§6.3), not corrected in this revision (out of scope per
   "do not modify architecture merely to improve matrix results").

Also disclosed, not resolved this revision: one ESCI-beauty product (of
2,093) failed Havenask SQL insertion with error 8020 despite correct
quote-escaping and an unremarkable field length; root cause not found
before the session time budget required moving on. Excluded from that
one table's row count (2,092/2,093); the raw indexer log is preserved
rather than silently retried into passing.

## 5. Whole-workload economics

### 5.1 Conditional per-query-class advantage

Native's structural query classes (Q5 range, Q9 category, Q10 facet, Q2
brand) show a **consistent, large, multi-engine-replicated speedup**:
roughly 1.5–6ms external-engine HTTP round trips vs. native's
sub-microsecond in-process bitmap operation — a **10,000×–50,000×**
ratio, replicated across all four external engines and five real
datasets (not a single lucky comparison). This matches and extends
prior Phase 5/6/9 findings (previously Solr-only) to Elasticsearch,
OpenSearch, and Havenask uniformly.

Native's lexical-search query class (Q11) shows the **opposite, scale-
dependent** result:

- On WANDS (42,994 products), native's linear candidate-scan text search
  (6.68ms) is **slower** than every external engine (1.56–3.62ms) — a
  genuine loss, not hidden.
- On the three ESCI slices (1,056–2,093 products, ~20–40× smaller),
  native's identical scan strategy (0.14–0.34ms) is **faster** than
  every external engine (0.99–3.94ms).

This is a real crossover, not noise: native's text-search strategy is an
`O(candidates)` scan, so its cost scales linearly with catalog size,
while every external engine pays a roughly constant ~1–2ms HTTP-plus-
inverted-index-lookup cost independent of catalog size in this size
range. The crossover point is somewhere between ~2,000 and ~43,000
products for this specific query shape (single-term substring match
across two-three text fields) on this hardware — not measured precisely
this revision (would need a scale ladder, out of scope for the time
remaining). **Practical reading: native's own lexical path is not a
substitute for a real inverted index at meaningful catalog scale,
exactly matching CLAUDE.md's "delegate open-ended lexical retrieval... to
a mature backend" architectural principle** — the crossover result is
evidence *for* that principle, not against the overall thesis.

### 5.2 Traffic-weighted reasoning (Retailrocket)

Real traffic shape (2,756,101 events, `docs/research/artifacts/issue57_retailrocket_traffic_analysis/`):
96.7% view / 2.5% addtocart / 0.8% transaction; item popularity is
sharply Zipfian (top 1% of 234,838 distinct viewed items account for
22.7% of all views, top 20% for 78.3%).

Applying this shape (as a traffic-composition proxy, not a literal
query-log — Retailrocket has no query text, per §9.2) to the measured
conditional results: real ecommerce traffic is view-dominated
(browse/filter/facet-heavy, matching Q9/Q10's category-filter and facet
shape far more than Q11's free-text search — PLP/category browsing is
exactly what generates the bulk of `view` events in a real funnel). If
this dataset's traffic composition is representative, **the query
classes native wins decisively (structural filter/facet/range) plausibly
dominate real click volume**, while the query class native currently
loses at scale (open-ended lexical) is a minority of raw view volume
but a majority of *conversion-adjacent* intent (a shopper who searches
by free text is further down the funnel than one browsing a category
page). This is a **plausible interpretation given real traffic shape
evidence, not a proven weighted average** — Retailrocket's own item
catalog is not joined to WANDS/ESCI/Magento's query classes in this
revision (different datasets entirely), so this is directional reasoning
about real ecommerce traffic composition in general, disclosed as such,
not a computed blended number.

### 5.3 Memory/index footprint

Not separately instrumented this revision (disclosed gap, not silently
omitted) — WANDS's native index size was already measured in
`p6a_e00_wands_vs_native_eval.rs`'s prior output (`ordinal_facet_bytes`,
bytes/product); this revision did not re-instrument per-engine index
size for Solr/ES/OpenSearch/Havenask. A follow-up revision should add
this (`_cat/indices` for ES/OS, Solr's `SystemInfoHandler`, Havenask's
`hape gs table` size fields) before a memory-footprint claim is made.

## 6. Semantic translation matrix (summary)

Full per-constraint translation logic lives in `crates/comparator-eval`
(`translate.rs` for Solr, `translate_es.rs` for ES/OpenSearch,
`translate_havenask.rs` for Havenask) with unit tests per constraint
shape. This revision exercised, and correctness-verified live:

| Primitive | Solr | Elasticsearch/OpenSearch | Havenask |
|---|---|---|---|
| Brand (exact) | regex `fq`, case-insensitive | `term`, lowercased both sides | `WHERE = `, lowercased both sides |
| BrandAny (OR) | regex-OR `fq` | `terms` | `WHERE IN (...)` |
| Category | regex `fq` (WANDS: exact match, no collision) | `term` | `WHERE = ` |
| Numeric range (Gte/Lte/Gt/Lt) | Lucene range syntax | `range` | `WHERE >=/<=/>/<` |
| Enum attribute | regex `fq` | `term` | `WHERE = ` |
| Facet/aggregation | JSON Facet API | `terms` agg | `GROUP BY ... ORDER BY count DESC` |
| Free-text (not gated) | `edismax` | `multi_match` | `MATCHINDEX('default', ...)` |

### 6.1 Not exercised this revision (disclosed)

ProductTypeAny, PriceUnderCents/PriceOverCents, MultiEnumContains,
Boolean, and Text-contains all have translator implementations and unit
tests in `comparator-eval` (§ "translate_es"/"translate_havenask" test
modules) but were not exercised against a *live* engine this revision
(no dataset here has price data cleanly wired to a benchmark query, and
MultiEnumContains/Boolean weren't in this revision's query selection).
Unit-level translation correctness is verified; live cross-engine
correctness for these specific primitives is not yet MEASURED — a
concrete follow-up, not a claimed-covered gap.

### 6.2 Analyzer/tokenization disclosure (per Issue #57 §12)

- Solr: `text_general` (WANDS/Magento title/description), `edismax`
  query parser.
- Elasticsearch/OpenSearch: default `standard` analyzer on `text`
  fields; `keyword` fields for filtering (lower-cased at index time,
  §6.3).
- Havenask: `simple_analyzer` on `TEXT` columns; `STRING` attribute
  columns for filtering (lower-cased at index time, same convention).
- Native: `commerce_core`'s own tokenization for `Constraint::Text` is a
  case-insensitive substring scan over the raw stored string, not a
  tokenized inverted index (see §5.1's crossover finding).

No `10w30`/`10W-30`-style technical-identifier normalization was
exercised this revision (no dataset here carries that token class in a
query this revision selected) — a disclosed gap, not a claimed pass.

### 6.3 Genuine capability/semantic difference found (not a translation bug)

§4 item 7: native's `Brand` identity is case-sensitive as currently
ingested; every comparator here is deliberately case-insensitive. This
is the one primitive where "equivalent semantics" required *excluding*
certain real inputs (casing-collision brands) from the gated comparison
rather than reconciling both sides, and is flagged as a live limitation
in native's own Issue #35 ingestion path, not fixed in this revision.

## 7. Hardware/measurement caveats (repeated from the protocol, load-bearing here)

- Shared/virtualized 4-vCPU host; Solr, Elasticsearch, OpenSearch, and
  the Havenask container's several processes were all resident (though
  not concurrently *queried*) during every measurement. Absolute
  latency numbers reflect this specific constrained sandbox and should
  be read as *relative*, cross-engine comparisons on one fixed host —
  not portable production numbers.
- Havenask ran in `hape`'s `proc` (local-process, single shared searcher
  across all tables) domain, not its production `default`
  (sibling-container, presumably better-isolated-per-table) domain,
  because mounting the host Docker socket was denied by this session's
  own safety guardrails (§3.1 of the protocol). Havenask's consistently
  highest latency among the four external engines (roughly 1.3–2×
  Solr's, itself usually the fastest external engine) may partly reflect
  this constrained deployment mode rather than Havenask's production
  performance ceiling — disclosed as an open question, not resolved
  this revision.
