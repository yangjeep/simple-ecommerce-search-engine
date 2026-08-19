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
