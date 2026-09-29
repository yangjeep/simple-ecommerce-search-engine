# ADR 0015: Match-all candidate materialization and logical match-all primitives

## Status

Accepted with caveats (Issue #63 / Infra E3 amended, `docs/decisions/ISSUE63_PRIMITIVE_EFFICIENCY_DECISION.md`). As with ADRs 0011 and 0014, these are `commerce-core` primitives plus an experiment-crate executor mode (`issue79-eval::plp`, `cand_mode`). No product serving path uses them by default yet.

## Context

When a request has no indexable constraint on the faceted domain, `CatalogIndex::indexed_candidates` materializes every ordinal. Examples are a full-catalog PLP, or a disjunctive facet whose only filter is its own and is self-excluded. It does this with `(0..N).collect::<RoaringBitmap>()`, i.e. per-element insertion.

#63 measured that construction at 4.48 ms of hot-loop CPU at N = 516k (106 allocations), and at 12–13 ms per request inside the native server. It was 88% of FH1's in-process cost and 32–36% of FH3/FH4's service CPU.

## Decision

- **P0r, materialize by range.** `CatalogIndex::all_ordinals_bitmap_by_range()` builds the identical bitmap with one `insert_range(0..N)`: 16.6 µs, 10 allocations, 270x cheaper, and no persistent memory. It was adopted by #63's preregistered §3.4 rule (N⁺). With it, native faceting is 0.10–0.43x the fastest equal-work competitor on every FH cell.
- **Logical match-all primitives** (available, not adopted):
  - `CandidateSet { All { count }, Set(bitmap) }` via `CatalogIndex::candidate_set`. An indexable constraint that matches nothing is an empty `Set`, never `All`.
  - `facet_counts_ordinal_all`: a dense column scan without bitmap iteration.
  - `facet_counts_bitmap_all`: value-bitmap `len()`, O(V).
  - `sort::top_k_presorted_all`: a presorted walk without a membership test.

  All are exact-equivalent to their bitmap-candidate counterparts (unit tests plus 3,312 gate checks). They cut FH3/FH4 in-process cost by roughly another 2x. They failed #63's 10% no-regression criterion on cells whose path is unchanged, a criterion that is noise-limited on the measurement host, and so they are **not adopted**.
- **The `indexed_candidates` default is unchanged** in #63. Measured #79/#63 FINAL (`cand_mode=p0`) therefore stays byte-reproducible. P0r is selected explicitly (`cand_mode=p0r`).

## Consequences

- Making P0r the default is a pure implementation change with identical contents. The next experiment that owns the serving path (#64's amendment) should do it, and should record that `cand_mode=p0` then no longer reproduces #79's FINAL cost.
- The dense match-all paths deserve a re-test with enough runs to resolve a 10% effect before adoption.
- The benefit applies only to requests with no indexable constraint on the faceted domain. Category-scoped PLPs already have an explicit candidate set.
