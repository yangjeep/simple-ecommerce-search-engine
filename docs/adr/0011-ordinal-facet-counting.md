# ADR 0011: Ordinal/dictionary-based facet counting closes (and reverses) the facet crossover

## Status

Accepted (Issue #21 Phase 6D, `PHASE6D_DECISION.md`).

## Context

Phase 5, 6A, 6B, and P6C-E00 all independently found the same
crossover: `CatalogIndex::facet_counts_by_scan`, a naive per-candidate
scan added in Phase 5 as an `O(|candidates|)` alternative to
`facet_counts`'s `O(global vocabulary)` cost, loses to Solr's own
`facet.field` past a real, repeatedly-measured cardinality threshold.
P6C-E01 (`PHASE6C_DECISION.md`) then found that Lucene's own dedicated,
ordinal-based facet module (`SortedSetDocValuesFacetCounts`) closes most
of that same crossover against Solr, and named the direct, previously-
untested next question: could commerce-native's own architecture adopt
an equivalent technique?

`facet_counts_by_scan`'s per-candidate cost was never really about
*counting* — it was about what the scan did on the way there: for every
surviving ordinal, `effective_attributes(product, variant)` clones the
product's entire `BTreeMap<String, AttributeValue>` and overlays variant
attributes, a fresh heap allocation per candidate, before even looking
up the one attribute the caller asked about.

## Decision

**Add a parallel, ordinal/dictionary-encoded representation for
single-valued `Enum` attributes, alongside (not replacing) the existing
`enum_bitmaps`/`enum_values` structures.** Three new `CatalogIndex`
fields, populated entirely inside the existing `build`/`index_attributes`/
`index_enum_value` build-time pass (no new pass over the catalog):

- `enum_dictionary: HashMap<String, Vec<String>>` — each attribute's
  distinct values in first-encountered order; the `Vec`'s index is the
  value's dense `u32` ordinal.
- `enum_value_ordinal: HashMap<String, HashMap<String, u32>>` — a
  build-time-only reverse lookup (value string → its ordinal), not
  needed at query time.
- `enum_columns: HashMap<String, Vec<u32>>` — one entry per variant
  ordinal, storing that variant's value-ordinal (or `u32::MAX` for "no
  value").

A new method, `facet_counts_ordinal`, counts by reading each candidate's
value-ordinal from the flat column (`O(1)`, no string hash) and
incrementing a flat `Vec<u64>` counter array sized to the dictionary;
`String` cloning is deferred to only the (typically few) non-zero result
buckets. This is the same architectural family as Lucene's own
`SortedSetDocValuesFacetCounts` and Solr's own `facet.field`: build a
dictionary once, count via integer array operations.

**Deliberately scoped to single-valued `Enum` only, matching
`facet_counts_by_scan`'s own existing behavior** (which also silently
skips `MultiEnum` — a pre-existing asymmetry this ADR does not fix, only
replicates exactly, so the new method's correctness test can assert
byte-for-byte equality with the existing scan method rather than with
`facet_counts`, which does handle `MultiEnum`).

## Consequences

- Real-data measurement (P6D-E00, `docs/experiments/PHASE6D_LOG.md`):
  `facet_counts_ordinal` beats Solr's own `facet.field` at all 7 real
  WANDS `category_depth_1` checkpoints by 5.2x-69.8x (no exceptions),
  and beats `facet_counts_by_scan` by 23.5x-89.3x — correctness-gated
  both by a new unit test (exact match against `facet_counts_by_scan`
  across full/filtered/empty/never-indexed inputs) and by 21/21 exact
  top-50-facet matches against Solr's own live response across 3
  repeated runs.
- Confirmed across Phase 6B's own 2x-20x controlled-stress scale ladder
  (P6D-E01): the margin over Solr holds across all 35 checkpoint x tier
  combinations tested (2,002-320,780 candidates), zero exceptions, but
  narrows — not grows — at the largest candidate counts, converging
  toward roughly 2.5x-3x. The margin over `facet_counts_by_scan`, by
  contrast, grows sharply with scale (up to 327x), consistent with the
  scan method's per-candidate `BTreeMap`-clone cost getting relatively
  worse as candidate count grows while both other methods scale closer
  to linearly.
- This is a materially larger, more consistent margin than Lucene's own
  equivalent module achieved over Solr in P6C-E01 (up to 3.0x, still
  trailing at 2 checkpoints) — because `facet_counts_by_scan`'s naive
  baseline was paying a more expensive per-candidate cost (a full
  attribute-map clone) than Lucene's own naive scan ever did, so there
  was more room for the ordinal design to help.
- **This is additive, not a replacement.** `facet_counts`,
  `facet_counts_by_scan`, `enum_bitmaps`, and `enum_values` are
  unchanged; no query-serving/planner code was changed to prefer the new
  method by default (there is no real query-serving path in this
  codebase yet — see `docs/architecture/README.md`'s own "what does not
  exist yet" disclosures). Wiring this in as the default strategy is
  named explicitly as a next step in `PHASE6D_DECISION.md`, not done
  here.
- The generic `Enum`/`MultiEnum` attribute system now has two counting
  strategies with different scope (`facet_counts_by_scan`/
  `facet_counts_ordinal` cover only single-valued `Enum`; `facet_counts`
  alone covers `MultiEnum` too via `enum_bitmaps`). Extending the
  ordinal approach to `MultiEnum` and to the dedicated
  brand/category/product_type facets (currently `HashMap<TypedId,
  RoaringBitmap>`-based) is named as a follow-on in `PHASE6D_DECISION.md`,
  not attempted in this pass.
- No new dependency: built entirely from `std::collections::HashMap`/
  `Vec`, already used throughout `commerce-core`; `roaring` (the crate's
  sole runtime dependency) is unaffected.

## Alternatives considered

- **A `Vec<RoaringBitmap>` indexed by value-ordinal** (one bitmap per
  distinct value, analogous to `enum_bitmaps` but ordinal-keyed instead
  of string-keyed) instead of a flat `Vec<u32>` per-variant column.
  Rejected for this pass: counting via bitmap-intersection-and-length
  per distinct value is `O(distinct values)` per facet request (the same
  complexity class `facet_counts`/`brand_facet_counts` already have and
  that Phase 5 found too slow at scale), whereas the chosen
  per-candidate column scan is `O(|candidates|)` — the same complexity
  class that made `facet_counts_by_scan` worth adding in the first
  place. A `Vec<RoaringBitmap>` variant remains a real, un-benchmarked
  alternative if a future workload has few candidates but very high
  attribute cardinality (the inverse of this phase's tested shape).
- **Extend the ordinal design to `MultiEnum` and the dedicated
  brand/category/product_type facets in this same pass.** Deferred, not
  rejected: scoping to single-valued `Enum` first (matching
  `facet_counts_by_scan`'s own existing scope exactly) kept the
  correctness-gate comparison exact and the change small enough to
  benchmark and land in one checkpoint; both are named as concrete
  next steps in `PHASE6D_DECISION.md`.
- **Wire `facet_counts_ordinal` as the new default immediately**, given
  how decisive the result is. Rejected for this pass: no real
  query-serving/planner code exists yet to wire it into, and the method
  does not yet cover `MultiEnum` or the dedicated typed-ID facets —
  making it a silent default before those gaps close would risk a real
  behavior regression for any future caller needing that coverage.
