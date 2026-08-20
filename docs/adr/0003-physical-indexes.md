# ADR 0003: Bitmap/range physical indexes replace the linear scan (Gate 3)

## Status

Accepted (Gate 3, Issue #2).

## Context

Gate 0-2 built a correct but `O(products × variants)` linear scan
(`Catalog::search`, `CommerceQuery::execute`). Gate 3 requires specialized
physical primitives: exact entity/id lookup, bitmap structured filters,
numeric/range filtering, minimal lexical postings for unresolved text,
facets, and top-K ranking — and CLAUDE.md's "Architecture bias" explicitly
names bitmaps, typed columns/range structures, minimal postings, and dense
ranking feature arrays as likely (not dogmatic) physical primitives to
benchmark.

## Decision

- **Dense `u32` ordinals, not the logical typed IDs, are the physical
  join key.** `CatalogIndex::build` assigns each `(ProductId, VariantId)`
  a sequential ordinal in catalog encounter order. Every bitmap and range
  structure indexes ordinals, not `ProductId`/`VariantId` directly — this
  keeps bitmaps dense and cheap regardless of what the logical IDs happen
  to look like, and is the "compact IDs" primitive CLAUDE.md names.
- **`roaring::RoaringBitmap` for every structural/attribute filter**
  (brand, product type, category, Enum/MultiEnum attribute values, Boolean
  attribute values): one bitmap per `(attribute, value)` pair, keyed in a
  `HashMap`. Chosen over a hand-rolled bitset because it is a mature,
  widely-benchmarked implementation of exactly this access pattern
  (sparse-or-dense set of small integers, fast intersection) and adding it
  costs one dependency, not meaningfully more code than a naive
  `Vec<bool>`/`HashSet<u32>` alternative would have taken to write
  correctly. A hand-rolled bitset remains an option to benchmark later if
  `roaring`'s overhead (chunking, run-length encoding) turns out not to
  pay off at the catalog sizes this project actually reaches.
- **Numeric attributes and price get a sorted `Vec<(value, ordinal)>` plus
  binary search (`partition_point`)**, not a bitmap per distinct value:
  numeric ranges (`size >= 9`, `price < 15000`) need a contiguous slice of
  a sorted structure, not membership tests against many single-value
  bitmaps. This is the "typed columns/range structures" primitive.
- **`Constraint::Text` (substring containment) is deliberately NOT
  bitmap-indexed.** Substring matching isn't answerable from a
  whole-token inverted index without additional n-gram/suffix structures
  this gate doesn't build. Instead, `CatalogIndex::execute` computes
  `indexed_candidates` from every *other* constraint, then verifies any
  `Text` constraint directly against only that narrowed candidate set
  (never the full catalog) via the same `Constraint::matches` Gate 1
  already proved variant-safe. This is "structural retrieval is primary
  where semantics are known; lexical retrieval handles residual
  uncertainty" applied literally inside one query, not just across
  queries.
- **A whole-token inverted index (`lexical_postings: HashMap<String,
  RoaringBitmap>`) is built from product titles and `Text` attribute
  values**, tokenizing on non-alphanumeric boundaries, lowercased. Gate 2's
  `residual_lexical` terms are exactly what this is for, but Gate 3 does
  not yet wire `residual_lexical` into a query path — building the
  postings without a consumer would be premature, but *not* building them
  at all would leave Gate 4/6 with no lexical fallback to measure against.
  The postings exist and are exercised implicitly (title/material tokens
  are indexed), but no `CommerceQuery` execution path reads them yet; wiring
  `residual_lexical` lookups into `execute` is next-gate work once there is
  a query that actually produces residual terms against a index-backed
  catalog worth measuring.
- **Facets and ranking take an explicit candidate `RoaringBitmap` /
  compiled hits, not a fresh query.** `facet_counts(attribute, &candidates)`
  intersects one bitmap per known value against the caller-supplied
  candidate set (so the caller decides whether facets reflect the
  post-filter or pre-filter state); `execute_ranked` reuses `execute`'s
  correctness-verified hits and only adds a deterministic
  score-desc/id-asc sort on top (Gate 2's `Preference::Boost`, summed).
  Neither is a new matcher — both are read-only views over the same
  `execute`/`indexed_candidates` machinery, so there is exactly one place
  a matching bug could hide.
- **The index is immutable and rebuilt wholesale, matching the
  "immutable/mmap-friendly bundle" bias.** There is no incremental update
  path; `CatalogIndex::build(&catalog)` is the only constructor. mmap
  itself is out of scope for this gate (everything is heap-resident); an
  on-disk immutable bundle format is future work once index *size* (not
  just latency) is under measurement (Gate 7).

## Consequences

- `CatalogIndex::execute` is required to return byte-for-byte the same hit
  set as `CommerceQuery::execute` (the linear scan) for every query;
  `tests/physical_index.rs` asserts this directly on the Gate 1 fixture,
  the Gate 2 representative query, and a Text-only query, rather than
  trusting the two implementations to agree by construction.
- Measured on a 10,000-product / 20,000-variant deterministic synthetic
  catalog (`benches/index_bench.rs`, same generator as Gate 0's bench,
  seed 42), a two-clause structural+numeric query (`color = Black AND size
  >= 9`) is roughly **14x faster indexed than linear-scanned** (see
  `docs/experiments/LOG.md` E003 for raw numbers), at a one-time build
  cost that amortizes after roughly 7-8 queries against that same index.
  This is the first quantitative evidence in this repository for the
  "specialized physical indexes beat generic scanning" half of the
  project's core thesis — at this one scale-ladder tier, for this one
  query shape.
- Memory/RSS of the index itself is not yet measured (Gate 7 metric); this
  gate only measured latency and build time.

## Alternatives considered

- **A single sorted "postings-style" structure covering both structural
  and lexical fields**, mirroring a generic Lucene-style inverted index
  end to end. Rejected for now: it would blur exactly the distinction
  CLAUDE.md asks this project to test (typed structural retrieval vs
  generic document retrieval); keeping bitmap/range structures for typed
  fields and a separate token postings map for genuinely free text makes
  the eventual Gate 7 comparison against "just index everything as text"
  possible to run as a distinct experiment instead of being baked in as
  the only architecture.
- **Approximate facets computed from the pre-filter candidate set
  always**, to avoid recomputing after narrowing. Rejected: `facet_counts`
  takes whatever bitmap the caller passes, so the choice of pre- vs
  post-filter facets is a call-site decision, not hard-coded — correctness
  first, optimize the call site later if profiling shows it matters.
