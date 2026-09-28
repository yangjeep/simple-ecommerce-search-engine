# ADR 0014: Bounded numeric sort structures and deterministic facet/sort path selection

## Status

Accepted with caveats (Issue #79 / Infra E3b, `docs/decisions/ISSUE79_FACET_SORT_RECOVERY_DECISION.md`). Like ADR 0011, these are `commerce-core` primitives plus an experiment-crate executor (`issue79-eval::plp`). No product serving path wires them in by default yet.

## Context

E3 (#77) measured native at 71x slower than the fastest competitor on a filter + numeric-sort PLP cell, and 9.4–11.5x slower on full-catalog facets. Inspection of `9482f5d` (#79 §1) found three separate causes:

1. **Sort had no physical structure.** Every candidate's full attribute map was cloned (`effective_attributes`) to read one value, then the whole candidate set was stably sorted to keep 48.
2. **Result assembly was O(|candidates|) even without a sort.** The E3 server hash-looked-up every candidate to return 48 IDs in ordinal order.
3. **The E3 server faceted with the legacy vocabulary-driven `facet_counts`**, which materializes an intermediate bitmap and allocates a key per value. It did not use ADR 0011's already-accepted `facet_counts_ordinal`, and it recomputed the candidate set for every facet.

## Decision

**Sort** (`commerce_core::index::sort`), for one numeric field only. It deliberately is not an expression sort engine.

- **One defined order** (`SortKey`): present values before missing ones in both directions, then value asc/desc, then variant ordinal ascending. NaN counts as missing, and `-0.0 == 0.0`.
- **Strategy A, `top_k_scan`:** an optional dense `NumericSortColumn` (`Vec<f64>`, NaN sentinel, 8 B/variant/field) plus a bounded heap of `offset + k`. It never sorts more than `offset + k` keys.
- **Strategy B, `top_k_presorted`:** walks the index's **pre-existing** value-sorted `numeric_index` (range-filter structure, no new copy) in direction, by runs of numerically-equal values merged by ordinal, membership-testing the candidate bitmap and stopping after `offset + k` hits. The missing-value tail comes from an optional `PresenceBitmap`.
- **`choose_sort_path`:** presorted iff `|C|^2 >= rho * (offset + k) * N`, else strategy A.

**Facets:**

- `CatalogIndex::facet_counts_bitmap`: per-dictionary-value `intersection_len`, one reused key buffer, same semantics as `facet_counts` (including `MultiEnum`).
- `choose_facet_path`: bitmap iff `|C_f| >= tau * V_f`, else ADR 0011's ordinal scan. It never routes an attribute with any `MultiEnum` value (`attribute_is_single_valued_enum`) to the ordinal column.
- Disjunctive faceting recomputes a facet's candidate set only when that facet has its own active filter.

**Result assembly:** unsorted requests materialize only the first `offset + k` candidate ordinals, which is the same order #77 returned.

**Constants** were fitted only on calibration cells and frozen before the held-out headline cells were measured: `tau = 919.5` and `rho = 0.0834`, on WANDS 500k (#79 §8, commit `4d78435`). They are experiment constants, not universal ones, and the sort boundary in particular is loosely constrained (feasible ρ ∈ (0.001, 7)).

**Not built by `CatalogIndex::build`:** `NumericSortColumn` and `PresenceBitmap` are opt-in, so their memory is paid, and accounted, only when a caller builds them. Everything else reuses existing structures (`enum_columns`, `enum_bitmaps`, `numeric_index`).

## Consequences

Same host, 500k WANDS, FINAL vs the fastest competitor (Meilisearch), CPU/query:

- **Sort:** 72.7x slower → **0.855x**, i.e. parity. The sort phase is 0.15 ms (48 IDs inspected). The rest is range-filter candidate construction, which is unchanged.
- **Facets:**
  - style 0.858x and primarymaterial 1.20x (parity);
  - **color 2.86x** and the **disjunctive** cell **1.28x** (partial);
  - no facet cell ≥25% *faster* than Meilisearch.

  The residual is full-catalog candidate-bitmap construction (`all_ordinals()`, about 7–9 ms, unchanged retrieval code) plus high-cardinality counting over dense candidate sets (about 12 ms), where ordinal and bitmap counting tie.
- **Planner:** on held-out cells, the rules chose the cheaper path on 6/7 and statistically tied on the 7th. Choosing presorted on narrow sets would have cost up to 25x (SH3), and bitmap counting on narrow facet sets up to 395x (FC7). A single fixed strategy is therefore wrong somewhere, and the rule is load-bearing.
- **Correctness:** there is an exhaustive subset test on a multi-variant fixture, and a 500k oracle gate with 980 candidate checks and 0 failures. Writing the RED test found a real ±0.0 tie-order bug in strategy B before any measurement.
- **Memory:** facet paths add 0 bytes. The S1 columns add 24 B/doc and the S2 presence bitmaps 0.38 B/doc (on-heap estimates, never disk): +10.8% of the index on-heap estimate, +0.29% of measured RSS. The measured RSS delta could not resolve structures this small (decision doc §8).
- **Not solved here, named for #63:**
  - a match-all / full-catalog candidate representation (`all_ordinals()` inserts N elements per request);
  - dense-candidate counting at high cardinality;
  - range-filter candidate construction;
  - whether these rules transfer to non-WANDS catalogs (the constants are WANDS-calibrated).

## Alternatives considered

- **Precomputed per-field *descending* order** (a second copy of `numeric_index`). Rejected: reverse iteration over the existing ascending list with run-wise ordinal merge gives the same order at zero extra bytes.
- **Learned or cost-model planner.** Out of scope (#79 forbids a learned planner). A one-parameter rule of preregistered form was sufficient on the held-out cells.
- **Making bitmap or ordinal the single facet path.** Calibration showed each loses by 7–395x somewhere, so there is a real crossover.
