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

Chosen next: per CLAUDE.md's priority order, "structural coverage"
(priority 2) outranks "physical advantage" (priority 3, already given an
initial answer in E003) and "physical advantage" has no unresolved
correctness question, only unresolved variance/scale — so E004 measures
coverage now and defers repeating the E003 benchmark.

---

## E004 — Versioned semantic context + structural coverage measurement (Gate 4)

**Question**  
What fraction of a representative ecommerce query set resolves
deterministically (zero ambiguity, zero unmodeled/residual terms) against
a small, versioned, hand-curated semantic context — and does adding
alias/canonical-ID resolution (a shopper synonym like "sneakers" or
"trainers" instead of the catalog's own "running shoes") actually land on
the identical typed constraint as the canonical phrase, rather than a
near-miss?

**Hypothesis**  
(a) `ir::SemanticContext` can wrap the Gate 2 lexicon with version +
provenance metadata and support one-or-more phrases per canonical
resolution (aliases) without changing how ambiguity is decided (candidate
count still, not confidence — ADR 0002's rule). (b) On a 20-query
fixture constructed with a known-exact classification per query (12
resolvable, 2 ambiguous, 6 residual — see `fixtures::REPRESENTATIVE_QUERY_SET`'s
doc comment), `measure_coverage` reproduces exactly that classification,
i.e. this is a real measurement of the implemented compiler, not a
hand-picked number.

**Workload**  
`fixtures::shoe_semantic_context()` (the Gate 2 `shoe_lexicon`, now with
two added alias entries — "sneakers"/"trainers" -> the same
`ProductTypeId(1)` "running shoes" resolves to, confidence 0.9 vs. the
canonical phrase's 1.0 — wrapped as `SemanticContext { version: 1, ... }`).
`fixtures::REPRESENTATIVE_QUERY_SET`: 20 hand-authored queries, built and
hand-traced token-by-token against the lexicon before running the test
(see the ADR for the classification and the trace method).

**Metric(s)**  
`CoverageReport { total_queries, fully_resolved, had_ambiguity,
had_residual }` and `fraction_fully_resolved()`; alias equivalence
(`compile(alias) == compile(canonical_phrase)` on `.constraints`).

**Decision rule**  
Advance (the context/coverage mechanism is sound) if `measure_coverage`'s
output matches the fixture's designed-in classification exactly (12/2/6)
and alias resolution matches the canonical phrase's constraints exactly.
A mismatch would mean either the classification was mis-designed or
`compile`/`measure_coverage` has a bug — either way, revise before trusting
the 60% number as evidence of anything.

**Implementation**  
`crates/commerce-core/src/ir/context.rs` (`SemanticContext`),
`crates/commerce-core/src/ir/coverage.rs` (`CoverageReport`,
`measure_coverage`). `fixtures::shoe_lexicon` gained two alias entries;
`fixtures::shoe_semantic_context` and `fixtures::REPRESENTATIVE_QUERY_SET`
are new. Rationale in `docs/adr/0004-semantic-context-and-coverage-metric.md`.
Test-first: `crates/commerce-core/tests/coverage.rs` encodes the exact
expected counts (12/2/6, fraction 0.6) that a hand trace of all 20 queries
through `compile`'s greedy-longest-match algorithm predicted, before
running the test to check the prediction.

**Results**  
```
$ cargo test --workspace --all-features
running 3 tests (tests/coverage.rs)
test semantic_context_carries_version_and_source ... ok
test aliases_resolve_to_the_same_canonical_id_as_the_canonical_phrase ... ok
test measured_structural_coverage_matches_the_constructed_query_set ... ok
test result: ok. 3 passed; 0 failed

# all prior test files unchanged and still green: 7 + 6 + 6 = 19 passed
$ cargo fmt --all -- --check   # exit 0
$ cargo clippy --workspace --all-targets --all-features -- -D warnings   # exit 0, 0 warnings
$ cargo build --workspace --release   # exit 0
```
Measured coverage: `total_queries=20, fully_resolved=12, had_ambiguity=2,
had_residual=6` → **fraction_fully_resolved = 0.60**. Environment: same as
E000-E003 (4 vCPU Intel Xeon @2.80GHz, 15Gi RAM, Linux 6.18.5, rustc/cargo
1.94.1). Commit: see `git log` on `claude/github-issue-2-gates-puv0wb`
immediately following this entry.

**Interpretation**  
The hand trace and the implementation agree exactly (12/2/6, first try, no
correction needed), so `measure_coverage` is measuring what it claims to
measure, and alias resolution genuinely lands on the same canonical
constraint as the phrase it's a synonym for. The 60% figure itself is
**not** evidence about real-world ecommerce query coverage: the query set
was constructed by hand from the same 9-entry lexicon being measured
against, deliberately split roughly 60/10/30 to produce a legible worked
example, not sampled from any real or synthetic shopper query
distribution. What this entry actually establishes is narrower and still
useful: (1) the mechanism (context + alias + coverage measurement) works
as designed with no bugs found on first implementation; (2) a
single-digit-entry hand-curated lexicon predictably leaves a large
residual share (30% here) once queries use *any* vocabulary outside what
was curated (an unmodeled brand, an unmodeled color, informal phrasing
like "wide fit") — which is exactly the gap Gate 6's catalog-profiling
cold start is supposed to close, now with a concrete mechanism
(`measure_coverage`) ready to evaluate it against. This does **not** yet
show: (1) coverage on a query set independent of the lexicon's own
construction (the obvious next-level test); (2) coverage on a
catalog-derived (rather than hand-curated) lexicon (Gate 6); (3) any
promotion/replay evidence for *changing* context versions (Gate 5 —
`SemanticContext.version` exists but nothing yet reads or compares it).

**Regression check**  
`crates/commerce-core/tests/coverage.rs`, run in CI (`rust-ci.yml`) via
`cargo test --workspace --all-features` on every push/PR.

**Next question**  
Gate 5: prototype the offline control-plane flow — observe
`residual_lexical`/`ambiguous` output from queries run through a
`SemanticContext`, propose candidate semantic mappings for the
highest-frequency unresolved terms (deterministic/fixture-driven
proposal, no live model call per CLAUDE.md's hard rule), replay them
against `REPRESENTATIVE_QUERY_SET` (or an expanded version of it), and
only promote a candidate into a new `SemanticContext` version if replay
coverage improves without regressing any previously-resolved query.

---

## E005 — Offline control-plane prototype: observe/propose/replay/promote (Gate 5)

**Question**  
Can an offline flow observe which terms a `SemanticContext` fails to
resolve, request candidate mappings from a model-provider *interface*
(never the query hot path), and promote a new context version only when
replay evidence shows a strict, regression-free coverage improvement —
correctly rejecting both "no proposals" and "a proposal that regresses
even one previously-resolved query," not just rewarding aggregate
coverage gains?

**Hypothesis**  
(a) `observe_residual_terms` run against `REPRESENTATIVE_QUERY_SET` and
the Gate 4 lexicon reproduces exactly the 9 distinct residual terms E004's
hand trace identified (adidas, balance, blue, fit, new, shoes, trail,
vegan, wide), each frequency 1. (b) A `FixtureModelProvider` that proposes
mappings for a subset of observed terms ("adidas", "blue") yields a
candidate lexicon that `try_promote` accepts (coverage 12 -> 14 fully
resolved, zero regressions), producing a version-2 `SemanticContext`. (c)
A provider proposing nothing, and a provider that always declines, both
correctly fail to promote. (d) The promotion gate itself — tested directly
against hand-built `ReplayResult` values, independent of `compile` — must
reject a candidate with a net aggregate gain (+3 fully resolved) if even
one query in it regressed, proving "no regressions" is enforced per-query
and cannot be papered over by aggregate improvement elsewhere.

**Workload**  
`fixtures::shoe_semantic_context()` (Gate 4's version-1 context) and
`fixtures::REPRESENTATIVE_QUERY_SET` (unchanged from E004). Two real
`FixtureModelProvider`s (one mapping adidas/blue, one empty) plus a
hand-written `AlwaysDeclineProvider`. One hand-constructed `ReplayResult`
pair (regression vs. regression-free) for the gate-logic unit test.

**Metric(s)**  
Exact-set equality of observed terms; `SemanticContext.version` after
promotion; per-query `residual_lexical`/`ambiguous` emptiness
before/after for the two newly-mapped terms; `CoverageReport.fully_resolved`
before/after; `ReplayResult::passes_promotion_gate()` boolean outcome for
both the real and the hand-built regression scenario.

**Decision rule**  
Advance (the mechanism is sound) if every case above resolves exactly as
hypothesized with zero test failures: correct observation, correct
promotion on real improvement, correct rejection on no-op and
always-decline providers, and — the highest-value check — correct
rejection of the hand-built net-positive-but-regressing candidate. A
failure on that last case specifically would mean the promotion gate is
reading aggregate coverage instead of per-query regressions, which is
exactly the failure mode CLAUDE.md's replay-evidence rule exists to catch.

**Implementation**  
`crates/commerce-core/src/control_plane/`: `observe.rs`
(`observe_residual_terms`, frequency-then-alphabetical deterministic
ordering), `provider.rs` (`ModelProvider` trait, `FixtureModelProvider`),
`replay.rs` (`ReplayResult`, `replay`, `passes_promotion_gate`), `mod.rs`
(`propose_candidates`, `try_promote`). `SemanticLexicon` gained `Clone` so
a candidate lexicon can be built from a copy of the baseline without
mutating the live context. Rationale (top-level module boundary,
residual-only scope cut, all-or-nothing batch promotion, per-query
regression check) in `docs/adr/0005-control-plane-prototype.md`.
Test-first: `crates/commerce-core/tests/control_plane.rs`, including a
promotion-gate unit test built from hand-authored `CoverageReport`/
`ReplayResult` values (not `compile` output) specifically to isolate the
gate logic from the compiler.

**Results**  
```
$ cargo test --workspace --all-features
running 5 tests (tests/control_plane.rs)
test observes_every_residual_term_from_the_representative_query_set ... ok
test candidate_mappings_are_promoted_when_replay_improves_coverage_without_regressions ... ok
test no_proposals_means_nothing_is_promoted ... ok
test promotion_gate_rejects_any_regression_even_with_a_net_aggregate_gain ... ok
test a_provider_that_always_declines_never_promotes ... ok
test result: ok. 5 passed; 0 failed

# all prior test files unchanged and still green: 3 + 6 + 6 + 7 = 22 passed
$ cargo fmt --all -- --check   # exit 0
$ cargo clippy --workspace --all-targets --all-features -- -D warnings   # exit 0, 0 warnings
$ cargo build --workspace --release   # exit 0
```
Environment: same as E000-E004 (4 vCPU Intel Xeon @2.80GHz, 15Gi RAM,
Linux 6.18.5, rustc/cargo 1.94.1). Commit: see `git log` on
`claude/github-issue-2-gates-puv0wb` immediately following this entry.

**Interpretation**  
Every hypothesized outcome held on first implementation, including the
adversarial gate-logic case (net +3 aggregate, 1 regression -> rejected;
same result with the regression removed -> accepted), which is the
strongest evidence in this entry: the promotion gate demonstrably checks
per-query outcomes, not just an aggregate number, so a candidate cannot
"average out" a real regression. Observed-term extraction reproducing
E004's hand trace exactly (same 9 terms, same frequencies) confirms
`observe_residual_terms` and `compile`'s `residual_lexical` output agree,
which they must since the former is built directly on the latter — this
is a consistency check, not independent evidence. What this entry does
**not** show: (1) whether a *real* model-backed `ModelProvider` would
propose useful mappings at all — `FixtureModelProvider` is a fixed table,
so "the pipeline accepts good proposals and rejects bad ones" is proven,
but "where good proposals come from" is untouched, matching the gate's
intentionally narrow scope; (2) ambiguous-span resolution (narrowing
"leather" to one reading) — out of scope by design (see ADR 0005); (3)
promotion-history/audit trail across multiple attempts — only a single
promote-or-reject call is exercised per test; (4) behavior when a batch
mixes a good and a bad proposal together (the all-or-nothing design means
the good one is discarded too) — not exercised here, flagged as a known
batching cost in the ADR rather than measured.

**Regression check**  
`crates/commerce-core/tests/control_plane.rs`, run in CI (`rust-ci.yml`)
via `cargo test --workspace --all-features` on every push/PR.

**Next question**  
Two candidates remain from E003 (repeat the physical-index benchmark for
variance/scale) and E004 (measure coverage on a query set independent of
the lexicon's own construction). Per CLAUDE.md's priority order, "cold
start" (priority 4) is the next unaddressed thesis question above both:
Gate 6, given a catalog fixture, profile/compress semantic problems,
generate shopper-like query cases from the catalog itself (not hand-typed
by the experimenter), and measure semantic coverage holes — replacing
`REPRESENTATIVE_QUERY_SET`'s hand-authored construction with a
catalog-derived one is the natural way to finally get coverage evidence
independent of the lexicon's own hand-curation, closing both open threads
from E004 and E005 at once.

---

## E006 — Cold-start catalog profiling and shopper-query fuzzing (Gate 6)

**Question**  
Given only a catalog fixture (no hand-typed vocabulary, no per-SKU model
calls), can a deterministic profiler (a) compress raw attribute
occurrences into a small distinct vocabulary, (b) correctly surface a
genuine cross-attribute value collision as ambiguity rather than silently
picking one meaning, (c) generate a reproducible shopper-query set from
that same vocabulary, (d) identify exactly which generated queries the
derived lexicon fails to resolve, and (e) provide coverage evidence on
Gate 4/5's hand-authored query set that is genuinely independent of that
set's own construction (the open thread flagged in both E004 and E005)?

**Hypothesis**  
(a) `CatalogProfile::build` over `fixtures::cold_start_catalog` (4
products, 7 variants, 2 brands x 2 product types, one deliberately
planted "green" collision — a color on one product, a `features` tag
meaning eco-friendly material on another) compresses to exactly 14
distinct values. (b) `compile_lexicon` surfaces "green" as a 2-candidate
ambiguous entry, not a guess. (c) `generate_shopper_queries` is
byte-identical across repeated calls and produces 30 queries (5 templates
x irregular per-type counts, verified as 15 per product type x 2 types).
(d) `coverage_holes` against the self-derived lexicon returns exactly the
2 queries containing "green" (one per product type) and nothing else —
28/30 fully resolved. (e) The catalog-derived lexicon, evaluated against
`REPRESENTATIVE_QUERY_SET` (a set it was never built from), resolves some
non-trivial, non-total fraction of it, differing from the hand-curated
lexicon's known 12/20 (E004) — measured directly rather than predicted,
since predicting 20 queries' resolution against an unfamiliar
14-entry-vocabulary lexicon by hand would be error-prone.

**Workload**  
`fixtures::cold_start_catalog` + `fixtures::cold_start_brands/_product_types/_categories`
(new fixture, hand-authored per `docs/EXPERIMENT_LOOP.md`'s rule against
random relevance fixtures). `fixtures::REPRESENTATIVE_QUERY_SET` (E004/E005,
unchanged) and `fixtures::shoe_semantic_context` (E004, unchanged) for the
cross-check.

**Metric(s)**  
`CatalogProfile::distinct_value_count()`; generated-query-list equality
across repeated calls and exact count; `coverage_holes` exact-set
equality; `CoverageReport` fields for both the self-consistency check and
the cross-check against `REPRESENTATIVE_QUERY_SET`.

**Decision rule**  
Advance (the profiling/generation/hole-finding mechanism is sound) if (a)-(d)
match the hand-predicted values exactly and (e) produces a real,
inspectable number (not a crash, not 0, not 20/20) confirming the two
lexicons are meaningfully different views of overlapping vocabulary. A
mismatch on (b) specifically — the collision resolving to 1 candidate
instead of 2 — would mean the profiler is silently discarding one
attribute's claim on a value, which is exactly the failure CLAUDE.md's
"preserve ambiguity explicitly" rule exists to prevent.

**Implementation**  
`crates/commerce-core/src/cold_start/`: `profile.rs` (`CatalogProfile`,
`compile_lexicon`), `generate.rs` (`generate_shopper_queries`), `mod.rs`
(`coverage_holes`). New fixture `fixtures::cold_start_catalog` plus its
brand/product-type/category registries. Rationale (profiler scope, hard-
constraint-only derivation, template-based generation over random
sampling) in `docs/adr/0006-cold-start-fuzzing.md`. Test-first:
`crates/commerce-core/tests/cold_start.rs`, including one test
(`coverage_holes_are_exactly_the_deliberate_green_collision`) written
against a hand-traced prediction that initially failed only on
list-ordering (product types visit in `BTreeMap` order, "hiking boots"
before "running shoes" — content was correct on first run, order
assumption was not) and was corrected in place; the cross-check test
(`catalog_derived_lexicon_partially_covers_the_hand_authored_query_set`)
was deliberately written with sanity-bound assertions plus targeted spot
checks rather than a hand-predicted exact count, then the real aggregate
number was captured via a temporary probe run (not committed) for this
log entry.

**Results**  
```
$ cargo test --workspace --all-features
running 5 tests (tests/cold_start.rs)
test profile_compresses_ten_variants_into_a_small_distinct_vocabulary ... ok
test generated_queries_are_deterministic_across_runs ... ok
test coverage_holes_are_exactly_the_deliberate_green_collision ... ok
test catalog_derived_lexicon_partially_covers_the_hand_authored_query_set ... ok
test hand_curated_and_catalog_derived_lexicons_are_independently_comparable ... ok
test result: ok. 5 passed; 0 failed

# all prior test files unchanged and still green: 5 + 3 + 6 + 6 + 7 = 27 passed
$ cargo fmt --all -- --check   # exit 0
$ cargo clippy --workspace --all-targets --all-features -- -D warnings   # exit 0, 0 warnings
$ cargo build --workspace --release   # exit 0
```
Measured (via a temporary probe test, not committed — see Implementation):
```
catalog-derived lexicon vs REPRESENTATIVE_QUERY_SET:
  CoverageReport { total_queries: 20, fully_resolved: 11, had_ambiguity: 0, had_residual: 9 }
hand-curated lexicon vs REPRESENTATIVE_QUERY_SET (E004, reconfirmed):
  CoverageReport { total_queries: 20, fully_resolved: 12, had_ambiguity: 0 -> 2, had_residual: 6 }
```
Self-consistency: `CoverageReport { total_queries: 30, fully_resolved: 28,
had_ambiguity: 2, had_residual: 0 }` against the catalog-derived lexicon's
own generated queries. `distinct_value_count() = 14` from 4 products / 7
variants. Environment: same as E000-E005 (4 vCPU Intel Xeon @2.80GHz, 15Gi
RAM, Linux 6.18.5, rustc/cargo 1.94.1). Commit: see `git log` on
`claude/github-issue-2-gates-puv0wb` immediately following this entry.

**Interpretation**  
Every hypothesized mechanism behaved exactly as designed: the planted
collision produced exactly a 2-candidate ambiguous entry (not a silent
pick), the only two coverage holes in the self-consistency check were
that exact collision, and generation was verified byte-identical across
runs. The cross-check is the most informative result: **11/20 (55%) for
the catalog-derived lexicon vs. 12/20 (60%) for the hand-curated one on
the *same* independent query set** — close enough that neither
construction method has an overwhelming advantage on this small fixture,
but they get there differently: the catalog-derived lexicon has zero
ambiguity on this set (0 vs. the hand-curated lexicon's 2, because
`REPRESENTATIVE_QUERY_SET`'s "leather" ambiguity was a hand-curated
construct — nothing in `cold_start_catalog`'s attributes is named
"leather" as a value, only as free `Text`, which the profiler
deliberately does not index) but more residual (9 vs. 6, because it has
no alias/synonym knowledge — "sneakers"/"trainers" are structurally
unrecoverable from catalog data alone, confirmed directly by the
`sneakers_only` spot check). This is genuine, if narrow, evidence that
catalog-profiling and hand-curation are *complementary* rather than one
strictly subsuming the other: profiling correctly recovers what's
actually in the data (including catching value collisions a rushed human
curator might miss) but cannot recover shopper vocabulary that never
appears in the catalog verbatim (synonyms, slang, informal phrasing) —
exactly the gap Gate 5's control-plane loop exists to close from replay
evidence over time, not from catalog data alone. This does **not** yet
show: (1) behavior on a catalog large enough that "one LLM call per SKU"
would actually be tempting/costly to avoid (this fixture is tiny by
design, tens-of-products tier); (2) integration with `control_plane`'s
`ModelProvider` (a profiling-backed provider is a natural next step, not
built here); (3) whether the "hard constraint only" derivation choice
(no auto-detected preferences) costs real coverage — untested because
`REPRESENTATIVE_QUERY_SET`'s preference-only queries (R8, R9) still
resolve fine as hard constraints mechanically, so this fixture can't
distinguish "resolves" from "resolves correctly as a preference."

**Regression check**  
`crates/commerce-core/tests/cold_start.rs`, run in CI (`rust-ci.yml`) via
`cargo test --workspace --all-features` on every push/PR.

**Next question**  
All of Gates 0-6 now have at least initial evidence. Per
`docs/EXPERIMENT_LOOP.md`'s stop conditions and CLAUDE.md's scale-up
decision criteria, the next step is Gate 7: assemble the existing
evidence (variant-safety correctness, ~14.4x indexed-vs-linear-scan
speedup at 10k products, 55-60% structural coverage on two independently-
built lexicons, a working replay-gated promotion loop, a working
cold-start profiler) into a reproducible benchmark/decision package and
determine whether it already meets a PROCEED/REVISE/STOP condition, or
whether closing E003's variance/scale-curve gap is a prerequisite first.
An Elasticsearch baseline and the "medium" (~100k product) scale-ladder
tier are the most likely candidates to require materially larger
infrastructure than this environment has used so far — the first place
this loop may hit its own stop condition.
