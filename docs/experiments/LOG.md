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
