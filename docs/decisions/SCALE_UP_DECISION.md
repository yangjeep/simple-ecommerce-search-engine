# Scale-Up Decision

**Decision: PROCEED** — to the next round of infrastructure-heavier
experiments (larger scale tiers, a real/public catalog and query log, an
external baseline), not to a production rollout. See "What this decision
does and does not claim" below.

This closes the Issue #2 epic's autonomous experiment loop through Gates
0-7. Every phase gate named in the issue has at least one falsifiable
hypothesis, a real measurement, and a passing regression test. No
hypothesis in this loop was falsified; the unresolved risks section below
is deliberately as prominent as the results, because "nothing failed yet"
is not the same claim as "nothing can fail."

## Architecture tested

A Rust workspace (`crates/commerce-core`) implementing the semantic
forwarding plane / learned control plane split from `CLAUDE.md`:

- **Domain** (`domain/`): typed `Product`/`Variant`/`ProductType`/`Brand`/
  `Category`/`Price`/`Inventory`, typed attributes
  (`Enum`/`MultiEnum`/`Boolean`/`Numeric`/`Text`) over a per-variant
  merged attribute map (`effective_attributes`), with a linear-scan
  reference matcher (`Catalog::search`) kept throughout as ground truth.
- **Commerce IR** (`ir/`): a compiler (`compile`) turning free text into
  typed `ResolvedConstraint`s (attribute or structural) plus soft
  `Preference`s, with explicit `AmbiguousSpan`s and `residual_lexical`
  terms instead of silent flattening; a versioned `SemanticContext`
  wrapper with alias/canonical-ID support; a `measure_coverage` metric.
- **Physical indexes** (`index/`): `CatalogIndex` — dense `u32` ordinals,
  `RoaringBitmap` per structural id and per `(attribute, value)` pair,
  sorted-vector + binary search for numeric/price ranges, exact O(1)
  entity lookup, facet counts, top-K preference ranking, and a
  narrow-then-verify path for the one constraint kind (`Text` substring)
  that isn't bitmap-indexable.
- **Control plane** (`control_plane/`): a `ModelProvider` interface (never
  referenced by the hot query path), observe -> propose -> replay ->
  promote/reject, gated on per-query regression evidence, not aggregate
  coverage alone.
- **Cold start** (`cold_start/`): catalog profiling that compresses raw
  attribute occurrences into a deduplicated vocabulary, derives a lexicon
  with zero model calls, generates shopper-like queries from that
  vocabulary, and surfaces coverage holes — including a genuinely
  ambiguous cross-attribute value collision, caught rather than guessed.

33 tests, 7 ADRs (`docs/adr/0001`-`0007`), 8 experiment log entries
(`docs/experiments/LOG.md` E000-E007), all reproducible via `cargo fmt
--check`, `cargo clippy -- -D warnings`, `cargo test`, `cargo build
--release`, and CI-verified on every commit
(`.github/workflows/rust-ci.yml`).

## Datasets / workloads

Everything is a small, hand-authored, deterministic fixture — there is no
real or public ecommerce dataset anywhere in this evidence base:

- `variant_safety_catalog` / `representative_query_catalog`: 1-2 products,
  the adversarial variant-safety case (black/size-8, red/size-9).
- `REPRESENTATIVE_QUERY_SET`: 20 hand-authored queries with a known-exact
  classification (12 resolvable, 2 ambiguous, 6 residual).
- `cold_start_catalog`: 4 products / 7 variants, 2 brands x 2 product
  types, one deliberately planted attribute-value collision.
- `benches/common::synthetic_catalog`: deterministic (seed 42), used only
  for scale/performance measurement, explicitly not for relevance claims
  — 1,000 / 10,000 / 100,000 products (2,000 / 20,000 / 200,000 variants).

## Measured results

| Question (CLAUDE.md priority) | Evidence | Result |
|---|---|---|
| Semantic correctness (1) | E001 | Cross-variant false matches structurally unrepresentable, not just untested (7 tests, incl. the exact Gate 1 adversarial case) |
| Structural coverage (2) | E004, E006 | 12/20 (60%) hand-curated lexicon; 11/20 (55%) independently-derived catalog-profiled lexicon on the *same* held-out query set; both nonzero, neither total |
| Physical advantage (3) | E003, E007 | Indexed queries **6x / 16x / 57x** faster than linear scan at 1k / 10k / 100k products respectively — the gap *widens* with scale, not a fixed-overhead artifact |
| Cold start (4) | E006 | 28/30 (93%) self-consistency on catalog-derived queries; the only 2 holes are the one deliberately planted ambiguous collision, correctly caught rather than guessed |
| Learning loop (5) | E005 | Promotion gate proven to reject a net-positive-aggregate candidate that regresses even one query, and to accept a real regression-free improvement (12 -> 14 fully resolved) |
| Scaling curve (6) | E007 | Reproducible (identical index size/RSS across repeated runs) through 100k products; not yet measured past that tier |

Index size at 100k products / 200k variants: ~11.4 MB (approximate,
bitmap-dominated). RSS delta around index build at the same tier: ~50 MB
(see Unresolved risks — this gap between the two numbers is itself a
finding). P99 indexed-query latency at 100k: ~1.5 ms; P50 linear-scan at
the same tier: ~57 ms.

## Failed experiments

None outright falsified. Two things were wrong on first attempt and
corrected in place, recorded here rather than smoothed over per
`docs/experiments/LOG.md`'s "do not rewrite failed experiments into
success stories" rule:

- E006's `coverage_holes_are_exactly_the_deliberate_green_collision` test
  failed on its first run — not on content (the predicted 2 holes were
  exactly right) but on list order (product types are visited in
  `BTreeMap` key order, "hiking boots" before "running shoes"; the test's
  hand-predicted order was reversed). Corrected in place; kept as a
  reminder that even a fully-traced-by-hand prediction can get
  incidental details wrong.
- An unused `Product` import in `cold_start/profile.rs` (clippy caught
  it before commit, not a test failure, but a real first-draft mistake).

No hypothesis about correctness, coverage direction, or physical
advantage direction was wrong. Whether that reflects a genuinely robust
architecture or an evidence base too narrow to find the failure modes is
exactly the open question the next round of experiments (below) needs to
answer.

## Unresolved risks

These are the reasons this decision is **PROCEED-to-more-experiments**,
not **PROCEED-to-production**:

1. **No external baseline anywhere.** Every latency/throughput number is
   the Rust engine measured against itself. "57x faster than a linear
   scan we wrote ourselves" is necessary but not sufficient evidence
   against a real alternative (Elasticsearch/Lucene, a vector index, or a
   better-optimized generic document store). The Elasticsearch baseline
   Gate 7 asked for was blocked in this environment (no reachable Docker
   daemon) and recorded, not obtained by another means.
2. **All fixtures are small and hand-authored** (tens of products, a
   20-query hand-built set, a 4-product cold-start catalog). The 55-60%
   coverage and "93% self-consistency" numbers describe these specific
   fixtures' vocabulary, not a real shopper query distribution. A
   production catalog with thousands of distinct attribute values would
   plausibly change the coverage number substantially in either
   direction.
3. **Single environment, no cross-hardware validation.** Every
   measurement in E000-E007 ran on the same 4 vCPU container. Variance
   was checked (3 repeated runs in E007) but only on one machine.
4. **The approximate index size (~11.4 MB at 100k) accounts for only
   about a fifth of the measured RSS delta (~50 MB) at the same tier**
   (E007). The gap is most plausibly `HashMap` bucket overhead and
   `String` heap allocations for attribute/value names, neither itemized
   by `approximate_size_bytes`. Whether this ratio holds, worsens, or
   improves at larger scale is unmeasured — a real risk to any memory
   budget claim beyond "roughly the right order of magnitude."
5. **No update path.** `CatalogIndex` is immutable and rebuilt wholesale
   (280 ms at 100k products). Real catalogs mutate constantly
   (inventory, price); whether periodic full-rebuild is acceptable or an
   incremental update path is required depends on freshness requirements
   never specified or tested here.
6. **The IR compiler is intentionally minimal** — no boolean logic
   (OR/NOT), no numeric words ("nine" vs. "9"), no multi-clause ranges
   ("between $50 and $100"). Real query coverage would likely be lower
   than the measured 55-60% until the compiler grows past this gate's
   scope.
7. **The control-plane loop has never seen a real `ModelProvider`.**
   `FixtureModelProvider` proves the propose/replay/promote *mechanism*
   works; it says nothing about whether any real model or heuristic
   proposes mappings worth promoting.
8. **QPS/core is a single-threaded bound, not a measured concurrent
   number.** `CatalogIndex` is read-only after `build` (thread-safe by
   construction), so concurrent read throughput is plausible but
   unverified.
9. **The `Text` narrow-then-verify path's worst case is untested** — its
   cost when the indexed-candidate set stays large (e.g., a query with no
   selective structural/attribute constraint at all, only a `Text`
   clause) was checked for correctness, never benchmarked separately.

## What would be built next if scaling up

In priority order, matched to the unresolved risks above:

1. **An external baseline**, in an environment with container/JVM access:
   Elasticsearch or OpenSearch on the same hardware and the same
   synthetic catalog, to finally answer whether the measured advantage
   holds against a real alternative, not just a hand-written linear scan.
2. **A real or realistically-large public ecommerce dataset** (product
   catalog + query log, or a well-constructed synthetic one an order of
   magnitude more diverse than the current fixtures) to re-run the
   coverage and cold-start measurements against something closer to
   production vocabulary.
3. **Extend the scale ladder to 500k and 1M products** (the "target
   proof" and "stretch" tiers `docs/EXPERIMENT_LOOP.md` names) to check
   whether the widening speedup trend continues, plateaus, or reverses,
   and to get real numbers on the size/RSS-accounting gap at larger
   scale.
4. **Grow the IR compiler**: boolean logic, negation, numeric-word
   parsing, multi-clause ranges — direct upstream investment in the
   structural-coverage number every measurement here depends on.
5. **Wire a profiling-backed `ModelProvider`** (cold-start's
   `CatalogProfile` feeding `control_plane`'s propose/replay/promote loop
   instead of a fixed test table) and run multiple promotion rounds
   against a growing unresolved-term backlog, not just one round.
6. **Concurrent-load benchmarking** now that read-only thread-safety is
   architecturally true but unverified under real concurrent load.
7. **An incremental index update path** if the next round of catalog
   testing reveals full-rebuild latency is unacceptable for real
   inventory/price freshness requirements — not built speculatively now.

## What should explicitly not be built yet

- **Distributed/sharded serving, cluster coordination, multi-tenancy,
  HA.** No evidence at any tested tier (100k products / 200k variants /
  ~11 MB approximate index / ~50 MB RSS) that single-node capacity is
  close to exhausted. Building distributed infrastructure before hitting
  a real single-node ceiling would be exactly the premature complexity
  CLAUDE.md and `docs/EXPERIMENT_LOOP.md` both warn against.
- **A generic Elasticsearch-compatible query DSL.** The typed Commerce IR
  is the thing this whole epic exists to test; building generic document-
  search compatibility on top would blur the comparison this project is
  supposed to make, not sharpen it.
- **A production LLM-backed `ModelProvider`.** The interface and gating
  mechanism are proven with a deterministic mock (Gate 5's actual job);
  wiring a real model in belongs with item 5 above (feed it something
  concrete to propose against first, from item 2's real vocabulary),
  not before.
- **Any UI beyond the CLI report tools already built.** Every measurement
  in this decision came from `cargo test`, `cargo bench`, and
  `examples/decision_bench.rs`; nothing so far has needed more.

## What this decision does and does not claim

**Claims:** the commerce-native architecture — typed variant-safe
domain model, a compiler that preserves ambiguity instead of guessing,
bitmap/range physical indexes, a replay-gated learning loop, and a
zero-model-call cold-start profiler — is coherent, internally consistent
across 8 independent experiments, and shows a real, scale-*growing*
physical advantage over the naive alternative this same codebase also
implements as ground truth. That is enough evidence to justify the next,
more expensive round of experiments (external baseline, larger scale,
real data).

**Does not claim:** that this number would hold against Elasticsearch,
that 55-60% coverage generalizes to real shopper queries, that memory
behavior is characterized past 100k products, or that this is ready for
any production traffic. Those are exactly the gaps the next round is for.
