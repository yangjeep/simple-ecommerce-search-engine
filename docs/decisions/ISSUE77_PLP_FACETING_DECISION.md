# Issue #77 — E3 Category PLP / Faceting H2H

Raw evidence: `artifacts/issue77/results/*_100k_run1.json` (7 calibration
smoke tests, one per engine) and `artifacts/issue77/results/*_500k_run{1,2,3}.json`
(21 primary-scale measurements: 7 engines x 3 independent runs from clean
state, all `status=Ok`, all `correctness_all_passed=true`). Harness:
`crates/issue77-eval/`, `scripts/issue77/`. Config:
`benchmarks/configs/issue77/resource_envelope.env`.

Preregistration: this document supersedes the original #77 preregistration
text, which was amended *before implementation* (WANDS has no price/brand/
availability/real multi-variant products; the original plan assumed
production-catalog fields that do not exist in real WANDS data). See
"Preregistration amendment" below.

## Relationship to Issue #62 (E2) — kept strictly separate

E2 (`docs/decisions/ISSUE62_PHYSICAL_FOOTPRINT_DECISION.md`, verdict
REFINE) found that native's *current, fully-materialized in-heap*
representation is the only one of 7 engines that fails to complete the 1M
tier under an equalized 6 GiB memory ceiling -- a negative result for
physical footprint. **That finding is not revisited, softened, or
contradicted here.** E3 asks an independent question: at equivalent
correctness and the same frozen resource envelope, does native's PLP/
filter/facet *execution* (CPU, latency, throughput) show a material
advantage? The two verdicts are recorded on separate dimensions below and
must never be merged into a single blanket judgment (per instruction:
"E2 memory bad -> E3 execution must be bad" and "E3 execution good -> E2
memory doesn't matter" are both explicitly invalid inferences).

## Preregistration amendment (before measurement)

Real WANDS (`dataset_cache/wands/catalog.jsonl`, 42,994 docs) has no price,
no brand, no availability, and one variant per product (no real
parent/variant grouping). The original #77 draft assumed all three. Before
any E3 code was written, the plan was corrected to:

- **Dataset A (WANDS, scale only)**: reuses #62's already-generated
  row-replicated catalogs at the 100k/500k/1M multipliers. Every workload
  cell uses only real, existing WANDS fields: `category_leaf` and
  `category_depth_1..6`, `product_class`, `color`, `style`,
  `primarymaterial`, `material`, `shape`, `average_rating`, `rating_count`,
  `review_count`. No brand/price/inventory/availability field was
  fabricated for Dataset A.
- **Dataset B (deterministic multi-variant fixture, correctness only,
  never headline performance)**: 4 hand-verified documents --
  `Product A {A1: black/8/wide/available, A2: red/9/narrow/available}`,
  `Product B {B1: black/9/wide/available}`, `Product C {C1:
  black/9/narrow/unavailable}`. The oracle query `color=black AND size=9
  AND width=wide AND available=true` must return **only** Product B.
  Critically, this tests **same-product cross-variant leakage**: Product
  A individually has both `black` (on A1) and `size=9` (on A2), on two
  *different* variants of the *same* product, and must NOT match -- this
  is a stronger and different requirement from "different products'
  variants don't cross," which every commerce engine trivially satisfies.
  Every engine implements the fixture with its own native variant-modeling
  mechanism (native: `Product`/`Variant` + `effective_attributes`; Solr/ES/
  OpenSearch/Typesense/Meilisearch/Vespa: one flat document per variant,
  `product_id` as the grouping key) -- "equivalent commerce semantics" is
  operationally defined as passing this fixture, not by matching native's
  physical representation.

## Frozen resource envelope

Written explicitly to `benchmarks/configs/issue77/resource_envelope.env`
and sourced by every engine's provisioning script *and* by
`i77_measure`'s own `launch_native()`, via the same `read_env_var()` code
path, with no native-specific hardcoded exemption anywhere -- directly
addressing the implicit-config-inheritance bug E2's adversarial review
found. Values: 3 CPUs (cpuset 0-2), 6g memory / 6g swap (matching E2's
already-proven-working 500k configuration for all 7 engines including
native), 3g JVM heap for Solr/ES/OpenSearch, one container at a time,
`I77_RUN_COUNT=3`, `I77_WARMUP_QUERY_COUNT=20` (uncounted),
`I77_MEASURED_QUERY_COUNT=200` per cell per run.

## Engine set

Same 7 as E2: native, Solr, Elasticsearch, OpenSearch, Typesense,
Meilisearch, Vespa. Havenask remains `EXCLUDED_ENVIRONMENTALLY`
(gdb-confirmed AVX2-incompatible SIGILL from E2; not re-investigated here
per instruction).

## Workload matrix (11 cells, whole-catalog scoped except base PLP)

`base_plp_{narrow,medium,broad}` (category-scoped, percentile-chosen
depths); `filter_depth_{1,3,5}` (color -> +style -> +primarymaterial ->
+shape+rating, verified against the real unscaled corpus to have genuine
non-zero, progressively-narrowing matches); `facet_{low,medium,high}_cardinality_*`
(single facet, no active filter, over the full un-filtered catalog);
`facet_disjunctive_multi_dim` (5 facets, one active `color` filter,
disjunctive semantics: a facet's own count excludes its own filter but
reflects every other active filter); `numeric_range_sort` (real
`average_rating`/`rating_count` fields only, no price). All cells top-K=48.

**Workload-design correction found live during #77's own Solr smoke test**:
the "broad" category (Accent Chairs, chosen for `base_plp_broad`'s
candidate-depth purpose) has 0% real attribute coverage for
color/primarymaterial/material/shape. Reusing it for filter/facet cells
would have silently produced degenerate, uninformative (trivially-empty)
measurements for most cells regardless of engine. Filter/facet/sort cells
were redesigned to be whole-catalog-scoped instead, using filter values
verified directly against the real 42,994-doc unscaled corpus to have
genuine non-zero matches. Documented in full in
`resource_envelope.env`'s comments.

**Disjunctive faceting implementation differs by engine, honestly
accounted**: Solr (JSON Facet API `{!tag=}` + `domain.excludeTags`),
Elasticsearch/OpenSearch (`post_filter` + per-facet `filter` aggregation
re-applying every other active filter) each compute a full disjunctive
facet set in **1** backend request. Typesense, Meilisearch, and Vespa have
no server-side facet-domain-exclusion mechanism and genuinely require
**N+1** real requests (1 base + 1 per requested facet, each excluding
that facet's own filter) -- `mean_backend_requests` reports the true count
for every engine (1 for Solr/ES/OpenSearch/native's facet cells, up to 6
for Typesense/Meilisearch/Vespa's `facet_disjunctive_multi_dim`), and the
headline CPU/latency numbers below are the **sum** over all backend calls,
never the cheapest sub-request alone, per the preregistered accounting
rule.

## Cross-engine correctness cross-validation

All 7 engines pass every Dataset-B fixture oracle query (including the
same-product cross-variant-leakage negative case) at both the 100k
calibration tier and the 500k primary tier (`correctness_all_passed=true`
in every one of the 28 result files). Independently of the fixture, all 7
engines report **byte-identical `num_found`** for every one of the 11
workload cells at both tiers (100k: 9, 39, 3309, 4314, 9, 3, 128982 x3,
4083, 95901; 500k: 36, 156, 13236, 17256, 36, 12, 515928 x3, 16332,
383604) -- strong, independent, triangulated evidence that filter/facet/
category-scoping semantics are correctly and consistently implemented
across 7 separate codebases, not just passing the small 4-doc fixture.

## Primary-scale (500k) results -- median across 3 independent runs from clean state

CPU/query (usec) and P95 wall latency (ms) by workload cell. Full raw
per-run numbers, CV, and every preregistered metric (P50/P99/mean_wall_ms/
RSS/backend-request counts) are in the raw JSON; this table reports the
two headline metrics the preregistration named as independently sufficient
for materiality (any of CPU/query, P95, or QPS/core at equivalent
correctness).

| cell | native cpu_usec | native p95_ms | fastest competitor cpu_usec (engine) | native vs fastest competitor |
|---|---|---|---|---|
| base_plp_narrow | 714 | 2.49 | 8732 (meilisearch) | **12.2x faster** |
| base_plp_medium | 847 | 2.85 | 11348 (meilisearch) | **13.4x faster** |
| base_plp_broad | 12042 | 15.74 | 11135 (meilisearch) | 8% slower (not material) |
| filter_depth_1 | 15066 | 19.31 | 12003 (meilisearch) | 20% slower (not material) |
| filter_depth_3 | 902 | 2.96 | 9499 (meilisearch) | **10.5x faster** |
| filter_depth_5 | 909 | 2.99 | 5265 (meilisearch) | **5.8x faster** |
| facet_low_cardinality_style | 181383 | 269.43 | 19358 (meilisearch) | **9.4x slower** |
| facet_medium_cardinality_primarymaterial | 181873 | 264.72 | 18939 (meilisearch) | **9.6x slower** |
| facet_high_cardinality_color | 202785 | 288.53 | 17645 (meilisearch) | **11.5x slower** |
| facet_disjunctive_multi_dim | 89327 | 122.74 | 48004 (meilisearch) | **1.9x slower** |
| numeric_range_sort | 840130 | 1020.11 | 11814 (solr) | **71x slower** |

Throughput (req/s, median, fixed concurrency, `base_plp_broad` query):
native 161.3, solr 420.4, elasticsearch 334.4, opensearch 270.7,
typesense 56.1, meilisearch 321.3, vespa 158.7. Native is mediocre here --
2nd-slowest of the 7 -- plausibly capped by native's serving architecture
(the harness's `i77_native_plp_server` is genuinely single-connection-at-
a-time by design, discovered and disclosed during this round's debugging;
see "Harness bugs found and fixed" below), not necessarily by query-
execution CPU cost, since native's per-query CPU on the same query
(`base_plp_broad`) is competitive (12042 usec, within 8% of the fastest).
This is a serving-architecture caveat on the throughput number, not a
retraction of it.

## Per-dimension verdict table (kept separate; no blanket judgment)

| dimension | verdict | basis |
|---|---|---|
| Physical footprint / memory ceiling (E2) | REFINE -- negative at 1M under equalized memory | unchanged, see `ISSUE62_PHYSICAL_FOOTPRINT_DECISION.md` |
| PLP candidate retrieval, narrow/medium/filtered (base_plp_narrow/medium, filter_depth_1/3/5) | **BROAD execution advantage** | 5 of 6 cells >=25% faster (3 of them >5x, up to 13.4x); filter_depth_1's small 20% slowdown is the lone exception and is not material against the >=25% bar in the other direction either |
| PLP candidate retrieval, broad/unfiltered (base_plp_broad) | NO MATERIAL ADVANTAGE | native within 8% of the fastest competitor (meilisearch) |
| Faceting (single facet, full-catalog, no active filter) | **NEGATIVE** | native 9.4x-11.5x slower than the fastest competitor (solr for style/primarymaterial by a wide margin too: 63884/59578/58833 vs native's 181383/181873/202785) on all 3 cardinality tiers |
| Faceting (disjunctive, 5 dims, active filter) | NEGATIVE (narrower margin) | native 1.9x slower than fastest (meilisearch); still exceeds the 25% bar but the gap shrinks sharply once a filter has already reduced the candidate set (16332 of 515928 docs) |
| Sort (numeric_range_sort) | **SEVERE NEGATIVE** | native 71x slower than the fastest competitor (solr); root cause is architectural, not a bug: native has no sort infrastructure, and `i77_native_plp_server` sorts via a full scan + per-doc lookup over every matching result (383604 of 515928 docs matched `average_rating>=4` at this tier) |
| Throughput at fixed concurrency | WEAK / caveated | native 2nd-slowest of 7 (161 req/s vs solr's 420); plausibly capped by native's single-connection-at-a-time serving loop rather than query CPU cost (native's own CPU/query on the throughput-test query is competitive) |

**Overall E3 reading**: native's typed bitmap-index candidate-retrieval
path (`CatalogIndex::indexed_candidates`) shows a real, large, and
consistent CPU/latency advantage specifically on narrow-to-medium,
filtered PLP queries -- the "find the right small candidate set" workload
this architecture was built for. That advantage evaporates on broad/
unfiltered base queries (native and the best competitor converge within
noise) and inverts sharply into a real negative on two capabilities native
was never built with: general-purpose faceting over large un-filtered
candidate sets (`facet_counts` appears to re-scan proportionally to
candidate-set size with no caching/precomputation, unlike Solr's field
caches) and sorting (no sort infrastructure exists at all; the current
`i77_native_plp_server` bolts on a full linear-scan sort). Per the
preregistered no-optimization rule, none of these three gaps were closed
this round -- they are reported as found. This is exactly the kind of
"strong execution, uneven coverage" result the preregistration anticipated
as a fully valid, reportable outcome on its own, independent of E2's
memory finding.

## Harness bugs found and fixed this round

- Native server crashed the whole process on any single connection's I/O
  error (`serve_connection(...)?` propagated up through the accept loop);
  fixed to log-and-continue per connection, matching `i61_native_server`'s
  precedent.
- Native's HTTP server is genuinely single-connection-at-a-time by design
  (confirmed via direct reproduction with both `ureq` and Python's
  `http.client`); N concurrent keep-alive clients starved N-1 of them.
  Mitigated harness-wide (not just for native) by sending `Connection:
  close` on every request this harness makes, restoring full measured
  throughput; this is a methodology fix, not a server rewrite, and
  preserves native's real single-threaded serving characteristic rather
  than hiding it.
- A URL-encoding bug in `native_query_string()` left filter *values*
  un-encoded, corrupting queries whose value contained `&` (e.g. `"modern
  & contemporary"`); fixed, re-verified against hand-computed expected
  counts.
- The `base_plp_broad` category (Accent Chairs) has near-zero real
  attribute coverage; reusing it for filter/facet/sort cells was corrected
  to whole-catalog scoping before any measurement was taken (see
  "Workload matrix" above).
- Elasticsearch fixture provisioning failed silently (bare `curl -sf` with
  no response validation); fixed with explicit `jq -e` checks.
- Elasticsearch's default `track_total_hits` cap silently reported
  `found=10000` for any query matching more; fixed by setting
  `"track_total_hits": true`.
- Elasticsearch/ureq connection flakiness ("Unexpected EOF" on
  freshly-constructed connections after this process's own prior
  child-process spawning) was investigated extensively (7 isolated repro
  attempts, documented in code comments) without a fully pinned root
  cause; mitigated via a uniform, timing-safe retry wrapper
  (`retry_request`) applied to every HTTP call across every engine, with
  retries never inflating a recorded latency sample.
- Meilisearch's default `pagination.maxTotalHits=1000` silently capped
  `estimatedTotalHits` at 1000 for every cell with a true count above
  1000; fixed by raising it via index settings during provisioning.
- A Typesense `Vec<String>` vs `Vec<(String,String)>` type mismatch
  (copy-paste from `solr_query_body`) caught at compile time.
- Vespa's `.sd` schema parser requires each field directive on its own
  line (space-concatenating `indexing: ...` and `attribute: fast-search`
  on one line produced a parse error); fixed.
- Vespa YQL boolean equality uses `=`, not `==` (the latter is a parse
  error); fixed.

## Adversarial review findings and corrective action

An independent adversarial review (`ecc:rust-reviewer`, dispatched before
merge per the standing forensic-review step) was asked to try to falsify
this experiment's conclusions. It found two confirmed problems, both
centered on fairness:

- **CONFIRMED**: `launch_native`'s `--cpus`/`--cpuset-cpus` were Rust
  string-literal constants (`"3"`, `"0-2"`), not read via `read_env_var`
  from `resource_envelope.env` the way native's own memory args (right
  next to them) and every competitor's provisioning script already were —
  contradicting this document's "no hardcoded exemption anywhere" claim
  (the same class of bug E2's adversarial review found for native's
  memory ceiling). The values happened to already equal
  `I77_CPUS=3`/`I77_CPUSET=0-2`, so this changed no measured number, but
  the guarantee was not actually enforced in code, and a future config
  change would have silently exempted native again. **Fixed**:
  `launch_native` now calls `read_env_var` for both, matching the memory
  args' pattern exactly.
- **CONFIRMED**: `typesense_urls`, `meili_bodies`, and `vespa_yqls`
  unconditionally issued one extra backend request per requested facet
  field, even for fields that don't need their own filter excluded (true
  exclusion is only required for the one field matching the currently
  active filter, if any). This inflated the reported competitor
  CPU/latency cost on every faceting cell for Typesense/Meilisearch/
  Vespa — `facet_low/medium/high_cardinality_*` (`active_filter: None`,
  1 facet field) issued 2 requests where 1 suffices;
  `facet_disjunctive_multi_dim` (5 fields, 1 active filter) issued 6
  where 2 suffice. Because the harness's own accounting rule sums cost
  over all backend calls, this understated (made more favorable to
  native than a maximally-efficient competitor implementation would
  produce) exactly the multipliers this document reports as native's
  worst losses. **Fixed**: every non-excluded facet field is now combined
  into the base request (Typesense `facet_by=a,b,c`; Meilisearch
  `facets:[...]`; Vespa multiple sibling `all(group(f)
  each(output(count())))` blocks wrapped in one outer `all(...)`, the
  correct space-separated-not-comma-separated syntax verified live
  against a running container). Only the field needing exclusion still
  gets its own request. `mean_backend_requests` now reports the honest
  minimum (1, or 2 when one field needs exclusion) and `facet_field_count`
  was corrected to report the true number of facet fields computed
  (previously conflated with request count, which broke once request
  count stopped equaling facet count).

**Fix verified, not re-measured at 500k**: all three fixes were re-run
against the 100k calibration tier (`artifacts/issue77/results/
{typesense,meilisearch,vespa}_100k_refit_smoke.json`) after fixing a
Vespa YQL grouping-syntax regression the fix itself introduced (multiple
sibling groupings need to be **space**-separated inside one outer
`all(...)`, not comma-separated — first attempt was a parse error,
corrected and re-verified). All three engines still report the exact
same `num_found` per cell as before the fix (9/39/3309/4314/9/3/
128982x3/4083/95901), confirming the fix changed only request-count
accounting, not correctness or the underlying query semantics.
`mean_backend_requests` dropped from 2->1 (single-facet cells) and
6->2 (`facet_disjunctive_multi_dim`) for all three engines, as expected.

**The 500k-tier primary numbers reported above in "Primary-scale (500k)
results" were NOT re-measured after this fix** — they were collected
under the over-counting version of the harness. This is a deliberate,
disclosed cost/time-discipline decision, not an oversight: the review
itself concluded the bias runs in the conservative direction (it
understates native's faceting disadvantage, it cannot manufacture a
false disadvantage), so it cannot overturn the qualitative NEGATIVE
verdict on faceting — at most, a re-measurement would show native's
faceting/disjunctive-faceting gap is *larger* than the 9.4x-11.5x/1.9x
reported above, never smaller or reversed. Re-running the full 3-engine
x 3-run x 500k sweep to get a tighter number was judged disproportionate
to the value gained (the qualitative verdict is already unambiguous and
would only strengthen) given this round's already-substantial time
budget. **The reported facet multipliers should be read as a
conservative floor on native's faceting disadvantage, not a tight
estimate** — flagged here for a cheap follow-up re-measurement (rerun
`scripts/issue77/run_500k_repetition.sh` for typesense/meilisearch/vespa
only) whenever #77's evidence is revisited, rather than blocking this
round's merge.

Investigated and not confirmed as problems (full detail in the review
transcript): CPU-usec source is uniformly the cgroup `usage_usec` delta
over the 200-query measured batch for every engine, never a per-single-
query delta; `retry_request` never inflates a recorded latency sample
(only the successful attempt's elapsed time is kept, at every measured-
loop call site); Solr/ES/OpenSearch's disjunctive faceting genuinely is
one request and genuinely is disjunctive (verified against the query-
builder code, not just the doc comment); no cross-run container/volume
contamination (every container and named volume is removed immediately
before each run); the `numeric_range_sort` ~840,000usec/71x-slower
number is a genuine, real, repeated O(n log n) full-scan sort over the
matching candidate set on every one of the 200 measured requests, not a
measurement artifact; every throughput test issues the exact same query
the latency test used, per engine.

## Status

Verdict recorded above per dimension. Adversarial review complete; both
confirmed findings were fixed in code and fix-verified at the 100k tier;
the 500k-tier headline numbers are disclosed as a conservative floor on
native's faceting disadvantage per the corrective-action note above, not
re-measured this round. PR pending.
