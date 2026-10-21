# Independent acceptance tests

`acceptance.rs` is a separately authored test oracle. It does not call `commerce-core`
filtering, tokenization, ranking, or facet helpers to compute its expected results.
The engine implementation and this oracle are intentionally separate.

## Coverage

- **288-case exhaustive fixture matrix:** products/variants × inherited or explicit
  product/variant facet counts × color/size filters × stock policy × variant hide.
  It verifies complete eligible IDs, selected-variant eligibility, per-product
  matching variant counts, exact totals, and self-excluding facet buckets.
- **Typed filter and facet cases:** enum, multienum with duplicate input values,
  boolean, numeric, text, brand/category/product type, strict price bounds, and
  variant overrides of shared product attributes.
- **Ranking:** all candidates are ranked before truncation or product grouping;
  rank-100 promotion, boosted eligible siblings, relevance bucket priority,
  ordered merchant axes, whole-scope rule-set override, and lexical-score
  coexistence with bounded personal factors.
- **Pins:** hard eligibility, hide veto, deterministic conflicts, absolute
  position before pagination, exact variant display only for a winning explicit
  variant pin, organic fallback after a lost slot, and explicit rejection of an
  eligible out-of-range position. An ineligible or absent target is ignored.
- **Validation/determinism:** finite bounded factors, final rather than stepwise
  clamping, factor order, typed nonfinite values, supplied-hit identity/score
  validation, duplicate catalog/rule IDs, strict request fields, full-f64 JSON parse/replay precision, catalog-order
  independence, empty input, zero page size, and huge offset.
- **Seven optional BM25 cases:** variant text overrides, full-width u64 IDs, strict
  parse errors, explicit missing-index error, self-excluding facets over the full
  lexical universe, rank-100 promotion, exact lexical-trace replay, and a huge query-boost score-domain guard.

- **Eight literal-BM25 cases (v1.1 plus v1.1.1 provenance revision):** legacy
  naive-TSV robustness strings AND separately verified official decoded query
  values, literal operators/field-like syntax, apostrophes/Unicode, repeated-token
  score deduplication, blank/nonblank empty-token behavior, 39/40-byte analyzer
  boundaries, and cross-field matching without sibling leakage. An additional
  schema test verifies that explicit profile selection preserves supplied text.
  Existing strict parser tests remain intact. The historical
  `fixtures/wands_raw_query_regressions.json` is unchanged; despite its old name,
  it contains serialized-field artifacts from a naive TSV reader, not official
  decoded queries. `fixtures/wands_official_decoded_query_regressions.json` and
  the exact official TSV record excerpt provide the corrected independent source
  attribution. The `expected_product_id` values 1/4/6 refer only to the small
  synthetic regression catalog, not original WANDS product IDs or judgments.
  `scripts/athos80_verify_query_provenance.py` proves CSV-aware
  decoding and preserves the actual inch-quote characters. Neither fixture is
  silently substituted for the other. Both profiles still run full-candidate
  rank-100 and self-excluding-facet tests. Arbitrary token-order score-bit
  invariance is not claimed.

The naive scan oracle's rule handling is intentionally limited to the global,
unconditional hide rules used by its exhaustive matrix. Dedicated tests check
scope/condition/precedence behavior with independent explicit expected results,
including inactive query hides and active conditional category hides during facet
self-exclusion.
It is not presented as a second implementation of all rule semantics.

## Run

```sh
cargo test -p athos80-eval --test acceptance
cargo test -p athos80-eval --features bm25 --test acceptance
```

The feature-enabled suite has 48 tests (33 core + 15 BM25), with the exhaustive
matrix inside one test. Counts are not an Athos feature-coverage percentage,
and correctness test success does not prove a cost target or text-relevance parity.
