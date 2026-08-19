# Experiment Log

Append-only research log for the current architecture epic.

Do not rewrite failed experiments into success stories. If an experiment is superseded, keep the original entry and add the follow-up.

---

## E000 — Baseline / repository reset

**Question**  
Can the repository be converted into a clean Rust experimental baseline with reproducible correctness and performance harnesses before implementing the commerce thesis?

**Hypothesis**  
A minimal Rust workspace, deterministic fixtures, CI quality gates, and benchmark harness can replace the legacy C active path without carrying forward irrelevant architecture.

**Workload**  
To be established by Gate 0.

**Metrics / decision rule**  
Gate 0 passes only when formatting, clippy, unit/integration tests, release build, and at least one reproducible benchmark/replay command pass from a clean checkout.

**Implementation**  
Pending.

**Results**  
Pending.

**Interpretation**  
Pending.

**Regression check**  
Pending.

**Next question**  
After Gate 0, prove variant-safe commerce semantics before optimizing retrieval structures.

**Results (2026-08-19)**

- Deleted the C/GTrie implementation, CMake/Docker build, and the old
  `cmake-multi-platform.yml` CI workflow (`git rm`, history preserved).
- Added a Cargo workspace (`Cargo.toml`) with one member crate,
  `crates/commerce-core`. Rationale recorded in `docs/adr/0001-rust-workspace-baseline.md`.
- Added `.github/workflows/rust-ci.yml` running, in order: `cargo fmt --all
  -- --check`, `cargo clippy --workspace --all-targets --all-features -- -D
  warnings`, `cargo test --workspace --all-features`, `cargo build
  --workspace --release` — identical to the CLAUDE.md quality gate.
- Deterministic fixtures added as typed Rust builder functions in
  `crates/commerce-core/src/fixtures.rs` (no JSON/YAML, no randomness in
  the fixture itself).
- Benchmark harness added: `crates/commerce-core/benches/catalog_bench.rs`
  (Criterion, `criterion_group!`/`criterion_main!`), seeded with
  `ChaCha8Rng::seed_from_u64(42)` for reproducibility.

Commands run from a clean checkout, environment: 4 vCPU Intel(R) Xeon(R)
Processor @ 2.80GHz, 15Gi RAM, Linux 6.18.5, rustc/cargo 1.94.1.

```
$ cargo fmt --all -- --check          # exit 0
$ cargo clippy --workspace --all-targets --all-features -- -D warnings   # exit 0, 0 warnings
$ cargo test --workspace --all-features
running 7 tests (tests/variant_safety.rs) ... 7 passed; 0 failed
$ cargo build --workspace --release   # exit 0
$ cargo bench --package commerce-core
catalog_search_5k_products_2_variants
                        time:   [1.4999 ms 1.5191 ms 1.5436 ms]
```

Commit: see `git log` on `claude/github-issue-2-gates-puv0wb` immediately
following this entry.

**Interpretation**  
Gate 0's decision rule (fmt + clippy + unit/integration tests + release
build + at least one reproducible bench, all green from a clean checkout)
is satisfied. This is scaffold-only evidence: it says nothing yet about the
commerce thesis, only that the experiment loop has a working harness to
produce that evidence in.

**Regression check**  
`.github/workflows/rust-ci.yml` runs the same four commands on every push
and PR. `cargo test --workspace --all-features` (7 tests in
`tests/variant_safety.rs`, exercised here ahead of Gate 0 formally closing
because the fixture/harness work overlapped with E001 below).

**Next question**  
Answered by E001: is the typed domain model variant-safe under the exact
adversarial case named in Issue #2 Gate 1?

---

## E001 — Variant-safe structural matching (Gate 1)

**Question**  
Does a typed Product/Variant domain model with per-variant attribute
matching avoid the classic "flattened document" bug where a product
satisfies a query composed from attribute values that actually belong to
two different variants?

**Hypothesis**  
If constraints are evaluated against one variant's *combined* (product +
variant) attribute map at a time — rather than against attribute values
pooled across all of a product's variants — then a product with a black
size-8 variant and a red size-9 variant will not satisfy the query "black
size 9", while it will correctly satisfy "black size 8" and "red size 9".

**Workload**  
`commerce_core::fixtures::variant_safety_catalog()`: one product ("Nike Air
Zoom Runner") with product-level typed attributes (`waterproof: Boolean`,
`material: Text`, `features: MultiEnum`) and two variants — Black/size 8
and Red/size 9 — each with variant-level typed attributes (`color: Enum`,
`size: Numeric`). Deterministic, tens-of-products tier per the
`docs/EXPERIMENT_LOOP.md` scale ladder (exhaustive-correctness fixture,
not a performance fixture).

**Metric(s)**  
Boolean pass/fail per test case; no cross-variant false positive is the
load-bearing metric (CLAUDE.md: "Cross-variant false matches are bugs").

**Decision rule**  
Advance if: (a) `black size 9` and `red size 8` both return zero matches,
(b) `black size 8` returns exactly the black variant and `red size 9`
returns exactly the red variant, (c) product-level attributes (waterproof,
material, features) correctly apply to every variant. Reject/revise the
representation if any adversarial case produces a cross-variant match.

**Implementation**  
`crates/commerce-core/src/domain/`: `ids.rs` (typed newtype IDs),
`attribute.rs` (`AttributeValue::{Enum,MultiEnum,Boolean,Numeric,Text}` over
a `BTreeMap<String, AttributeValue>` for deterministic iteration),
`price.rs` (integer cents, no float currency math), `inventory.rs`
(`Availability` + `Inventory`), `product.rs`/`variant.rs` (`Product` holds
shared attributes + `Vec<Variant>`; `Variant` holds its own attributes),
`constraint.rs` (`Constraint` enum + `Constraint::matches`), `catalog.rs`
(`effective_attributes(product, variant)` merges product attrs with
variant-level overrides into one map *per variant*; `Catalog::search`
evaluates every constraint against that single merged map — this is the
one place scopes are combined, and it never sees more than one variant at
a time). Test-first: `crates/commerce-core/tests/variant_safety.rs` was
written against this design before the matching logic was finalized.

**Results**  
```
$ cargo test --workspace --all-features
test black_size_8_matches_only_the_black_variant ... ok
test red_size_9_matches_only_the_red_variant ... ok
test black_size_9_matches_nothing ... ok
test red_size_8_matches_nothing ... ok
test product_level_attributes_apply_to_every_variant ... ok
test multi_enum_and_text_constraints_match_shared_attributes ... ok
test numeric_range_constraint_narrows_by_variant ... ok
test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```
Commit: see `git log` on `claude/github-issue-2-gates-puv0wb` immediately
following this entry. rustc/cargo 1.94.1, same environment as E000.

**Interpretation**  
Supports the hypothesis for this fixture and this constraint language
(Enum/MultiEnum/Boolean/Numeric/Text over a per-variant merged attribute
map). The safety property is structural, not incidental: `Catalog::search`
has no code path that evaluates a constraint set against more than one
variant's attributes at once, so a cross-variant false match is not merely
untested but unrepresentable in the current representation for these five
attribute kinds. This does **not** yet demonstrate: (1) behavior at
`Constraint` counts or catalog sizes beyond this fixture and the 5k-product
synthetic bench (no correctness claim attached to the bench data), (2)
resolving free-text query strings into `Constraint`s (Gate 2's job), (3)
performance of the naive `O(products × variants)` linear scan at the
"medium" (~100k product) or larger scale-ladder tiers — no index exists
yet (Gate 3).

**Regression check**  
`crates/commerce-core/tests/variant_safety.rs`, run in CI
(`rust-ci.yml`) on every push/PR via `cargo test --workspace
--all-features`.

**Next question**  
Gate 2: compile a representative query ("black Nike waterproof running
shoes size 9 under $150") into a typed `Constraint` list with explicit
ambiguity/confidence, rather than hand-constructing `Constraint`s as the
tests currently do.

---

## E002 — Commerce IR compiler with explicit ambiguity (Gate 2)

**Question**  
Can a deterministic compiler turn the Issue #2 representative query into a
typed Commerce IR (structural + attribute constraints, plus preferences)
without collapsing genuinely ambiguous terms into a guessed resolution or
silently discarding unrecognized terms — and does the compiled query
remain variant-safe when executed against Gate 1's fixture?

**Hypothesis**  
A phrase-lookup compiler backed by a fixed lexicon
(phrase -> `Vec<Candidate>`, each candidate carrying a confidence) can: (a)
compile `"black Nike waterproof running shoes size 9 under $150"` into
exactly the six typed constraints a human would expect (color, brand,
waterproof, product type, size, price-under), with zero ambiguity/residual
terms; (b) when a lexicon phrase has more than one plausible candidate,
emit an explicit `AmbiguousSpan` rather than picking one; (c) keep
unrecognized tokens as `residual_lexical` rather than dropping them; and
(d) executing the compiled representative query against the Gate 1
variant-safety fixture must still return zero matches (that fixture has no
variant that is both black and size 9), proving the IR layer does not
reopen the cross-variant bug Gate 1 closed.

**Workload**  
`fixtures::shoe_lexicon()` (9 entries: brand, 2 colors, waterproof, a
2-word product-type phrase, 2 preference terms, and one deliberately
double-mapped ambiguous term, "leather"). `fixtures::REPRESENTATIVE_QUERY`
= the Issue #2 query verbatim. Two catalogs: the existing
`fixtures::variant_safety_catalog()` (negative control — no variant should
match) and a new `fixtures::representative_query_catalog()` built to
actually contain a black/size-9/waterproof/Nike/running-shoe/$139.99
variant (positive control).

**Metric(s)**  
Structural equality of the compiled `CommerceQuery` against a
hand-written expected value; hit-set equality after `execute()` against
each catalog; boolean pass/fail per adversarial case (ambiguous term,
unrecognized brand, preference-only terms).

**Decision rule**  
Advance if all of (a)-(d) above hold with zero test failures and zero
clippy/fmt warnings. Revise the lexicon/compiler design if any expected
constraint is missing/extra, if the ambiguous case gets silently resolved,
or if the residual/positive/negative execution checks fail.

**Implementation**  
`crates/commerce-core/src/ir/`: `structural.rs` (`StructuralConstraint` for
brand/product-type/category/price, `ResolvedConstraint` wrapping it with
`domain::Constraint`), `lexicon.rs` (`SemanticLexicon`, `Candidate`,
`ResolvedTerm`), `query.rs` (`Preference`, `AmbiguousSpan`,
`CommerceQuery`, `compile()`, `CommerceQuery::execute()`). Rationale for
the two-constraint-kind split and the "no auto-resolve" ambiguity rule is
in `docs/adr/0002-commerce-ir-compiler.md`. Test-first:
`crates/commerce-core/tests/ir_compiler.rs` was written against this
design (expected constraint list, ambiguous-term shape, residual-lexical
shape) before the greedy longest-match tokenizer was finalized.

**Results**  
```
$ cargo test --workspace --all-features
running 6 tests (tests/ir_compiler.rs)
test compiles_representative_query_into_expected_typed_constraints ... ok
test representative_query_does_not_cross_variant_match ... ok
test representative_query_matches_a_catalog_that_actually_has_it ... ok
test ambiguous_term_is_preserved_not_silently_resolved ... ok
test unrecognized_brand_becomes_residual_lexical_not_dropped ... ok
test descriptive_terms_compile_as_preferences_not_hard_constraints ... ok
test result: ok. 6 passed; 0 failed

running 7 tests (tests/variant_safety.rs) ... 7 passed; 0 failed  # unchanged, still green
$ cargo fmt --all -- --check   # exit 0
$ cargo clippy --workspace --all-targets --all-features -- -D warnings   # exit 0, 0 warnings
$ cargo build --workspace --release   # exit 0
```
Environment: same as E000/E001 (4 vCPU Intel Xeon @2.80GHz, 15Gi RAM,
Linux 6.18.5, rustc/cargo 1.94.1). Commit: see `git log` on
`claude/github-issue-2-gates-puv0wb` immediately following this entry.

**Interpretation**  
Supports the hypothesis for this lexicon and this one representative
query plus five adversarial variants of it (ambiguous term, unrecognized
brand, preference-only terms, negative-control catalog, positive-control
catalog). The variant-safety property survives composition with the IR
layer because `CommerceQuery::execute` reuses the same
"merge-then-evaluate-per-variant" shape `Catalog::search` uses; it is not
a second, independently-written matcher that could drift out of sync. This
does **not** yet demonstrate: (1) structural query coverage over a
realistic query distribution — the lexicon has 9 hand-picked entries, not
a catalog-derived vocabulary (that is Gate 4/6's job); (2) what fraction of
real shopper queries resolve deterministically (Gate 4's explicit metric);
(3) any ranking use of `preferences` (Gate 3); (4) performance of
`execute()` at scale-ladder tiers beyond the Gate 0 5k-product bench — no
index exists yet (Gate 3).

**Regression check**  
`crates/commerce-core/tests/ir_compiler.rs` and
`crates/commerce-core/tests/variant_safety.rs`, both run in CI
(`rust-ci.yml`) via `cargo test --workspace --all-features` on every
push/PR.

**Next question**  
Gate 3: replace the `O(products × variants)` linear scan in both
`Catalog::search` and `CommerceQuery::execute` with specialized physical
indexes (bitmap structural filters, numeric/range structures, minimal
lexical postings for `residual_lexical`) and measure whether that changes
latency/memory at the "small" (~10k product) scale-ladder tier — the
current implementation has never been benchmarked against an index-backed
alternative, so there is no evidence yet that specialization beats the
linear scan at any scale.

---

## E003 — Physical indexes vs. linear scan (Gate 3)

**Question**  
Does replacing the `O(products × variants)` linear scan with bitmap
structural filters, numeric/range structures, exact-id hash lookup, and
narrow-then-verify text handling measurably reduce query latency at the
"small" (~10k product) scale-ladder tier, without changing which
documents match (i.e. without any correctness regression versus the
linear-scan ground truth)?

**Hypothesis**  
A `CatalogIndex` built once from a `Catalog` — dense `u32` ordinals,
`roaring::RoaringBitmap` per `(attribute, value)` and per structural id,
sorted `(value, ordinal)` vectors with binary search for numeric/price
ranges — will (a) return byte-identical hit sets to
`CommerceQuery::execute`/`Catalog::search` for every query in the existing
Gate 1/2 fixture set, including a `Constraint::Text` clause that isn't
bitmap-indexable and must be verified against the narrowed candidate set
rather than dropped or approximated, and (b) answer a representative
two-clause structural+numeric query meaningfully faster than the linear
scan on a 10k-product synthetic catalog, at some index-build cost that is
worth stating explicitly rather than hidden.

**Workload**  
Correctness: existing fixtures (`variant_safety_catalog`,
`representative_query_catalog`, their union) plus every constraint
combination already exercised in `tests/variant_safety.rs` /
`tests/ir_compiler.rs`, re-run through both `CatalogIndex::execute` and
`CommerceQuery::execute` for equality. Performance:
`benches/common/synthetic_catalog(10_000)` — 10,000 products, 2 variants
each (20,000 variants total), deterministic (`ChaCha8Rng::seed_from_u64(42)`,
same generator Gate 0 used at 5k), queried with `color = "Black" AND size
>= 9` (the same clause shape as the Gate 0 baseline bench, now run through
both execution paths for direct comparison).

**Metric(s)**  
Hit-set equality (correctness, boolean pass/fail); Criterion wall-clock
latency distribution (P50-ish "time" estimate Criterion reports) for
`CatalogIndex::build`, linear-scan query, and indexed query; ratio of
linear-scan latency to indexed latency as the physical-advantage signal.

**Decision rule**  
Advance (physical indexing is worth keeping and extending in Gate 7) if
every equality check passes AND the indexed query is meaningfully faster
(not noise-level) than the linear scan at 10k products. Revise if
correctness diverges on any case (index is wrong, not just slow) or if
the speedup is negligible relative to build cost at this scale (would
suggest the linear scan is fine until a much larger tier).

**Implementation**  
`crates/commerce-core/src/index/`: `mod.rs` (`CatalogIndex`, `build`,
`indexed_candidates`, `execute`, `facet_counts`, exact `lookup_variant`/
`lookup_product`), `rank.rs` (`execute_ranked`, preference scoring). Design
rationale (ordinals as the physical join key, RoaringBitmap choice,
narrow-then-verify for `Text`, facets/ranking as read-only views over the
same `execute` machinery) in `docs/adr/0003-physical-indexes.md`. Test-first:
`crates/commerce-core/tests/physical_index.rs` asserts index/linear-scan
equivalence, exact-id lookup, facet counts against a known catalog, and
deterministic top-K ranking, before the benchmark was written.
`benches/common/mod.rs` factors the Gate 0 synthetic-catalog generator out
so `benches/index_bench.rs` and `benches/catalog_bench.rs` share it
without duplicating the generator (the `tests/common/` idiom applied to
`benches/`, not auto-discovered as its own bench target).

**Results**  
```
$ cargo test --workspace --all-features
running 6 tests (tests/physical_index.rs) ... 6 passed; 0 failed
running 6 tests (tests/ir_compiler.rs) ... 6 passed; 0 failed   # unchanged
running 7 tests (tests/variant_safety.rs) ... 7 passed; 0 failed # unchanged
$ cargo fmt --all -- --check   # exit 0
$ cargo clippy --workspace --all-targets --all-features -- -D warnings   # exit 0, 0 warnings
$ cargo build --workspace --release   # exit 0

$ cargo bench --package commerce-core --bench index_bench
index_build_10k_products_2_variants     time: [23.247 ms 23.547 ms 23.930 ms]
query_linear_scan_10k_products_2_variants   time: [3.4540 ms 3.5066 ms 3.5623 ms]
query_indexed_10k_products_2_variants       time: [242.21 µs 243.13 µs 244.14 µs]

$ cargo bench --package commerce-core --bench catalog_bench   # unchanged shape, still passes
catalog_search_5k_products_2_variants   time: [1.4332 ms 1.4415 ms 1.4542 ms]
```
Environment: same as E000-E002 (4 vCPU Intel Xeon @2.80GHz, 15Gi RAM,
Linux 6.18.5, rustc/cargo 1.94.1). Single run, not yet repeated for
variance (see Limitations below). Commit: see `git log` on
`claude/github-issue-2-gates-puv0wb` immediately following this entry.

**Interpretation**  
At 10k products / 20k variants, the indexed query (≈243µs) is roughly
**14.4x faster** than the linear scan (≈3.51ms) for a two-clause
structural+numeric query. The one-time index build (≈23.5ms) amortizes
after roughly `23.5ms / (3.51ms - 0.24ms) ≈ 7.2` queries against the same
index — i.e. any workload issuing more than a handful of queries against
one catalog snapshot comes out ahead. This is the first quantitative
support in this repository for "specialized physical structures reduce
cost at useful scale" (CLAUDE.md's Physical advantage priority), not just
"the code compiles and is variant-safe." It does **not** yet show: (1)
memory/RSS of the index vs. the un-indexed catalog (Gate 7 metric, not
measured here); (2) behavior at the "medium" (~100k) or "target proof"
(~500k) scale-ladder tiers — 14.4x at 10k is not a claim about the curve's
shape at 100x that size, especially since `RoaringBitmap` intersection
cost and hash-map lookup cost both scale differently than a flat scan
does; (3) build time or query latency for queries dominated by the
narrow-then-verify `Text` path (not benchmarked separately — only
correctness-checked); (4) variance — each Criterion number above is one
run's estimate, not repeated across multiple process invocations to
separate signal from machine noise, which `docs/EXPERIMENT_LOOP.md`'s
benchmark rules ask for and this entry does not yet satisfy.

**Regression check**  
`crates/commerce-core/tests/physical_index.rs`, run in CI (`rust-ci.yml`)
via `cargo test --workspace --all-features` on every push/PR. Benchmarks
are not run in CI (Criterion needs a stable machine for meaningful
numbers); they are a manual/scheduled experiment-loop step per
`docs/EXPERIMENT_LOOP.md`.

**Next question**  
Two candidates, both smaller than a new gate: (a) repeat this benchmark 3+
times and at an additional size point (e.g. 1k and 50k) to establish
whether the ~14x ratio holds or drifts with scale, addressing the
variance/scale-curve limitation above; (b) wire `residual_lexical` into
`CatalogIndex::execute` so a query with unresolved text actually
intersects `lexical_postings`, which today are built but never read by
any query path. Gate 4 proper (versioned/compiled semantic FIB with a
promotion workflow) is the next full gate once one of these is resolved.
