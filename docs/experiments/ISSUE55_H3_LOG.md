# Issue #55 H3 Experiment Log — variant-scoped conjunction correctness on real Product/Variant data

Protocol: `docs/experiments/ISSUE55_H3_PROTOCOL.md`.

## I55-H3-E00 — Exhaustive real-data variant-conjunction correctness test

**Question**

Does `commerce_core`'s "a product must not match a same-variant
conjunction unless one compatible variant satisfies it" guarantee — so
far proven only on a synthetic 1-product/2-variant fixture
(`crates/commerce-core/tests/variant_safety.rs`) — hold on a real catalog
with genuine Product/Variant structure? Neither WANDS nor ESCI has real
variant structure, so this is untested against real data. (A related,
narrower gap in `docs/decisions/ISSUE47_DECISION.md` — unmerged, PR #53
— named `magento/magento2-sample-data` as a candidate for its own,
different, LLM-control-plane external-validity question; see the
protocol §0 and the decision doc for the full disambiguation.)

**Hypothesis**

H0: the guarantee generalizes (real messy attribute vocabulary does not
break the per-variant `effective_attributes` merge discipline shared by
`Catalog::search`, `CatalogIndex`, and `CommerceQuery::matches_variant`).
H1: real data's larger variant counts and vocabulary diversity exposes a
scoping bug the small synthetic fixture can't reach.

**Workload**

Real Magento configurable-product apparel data (22 parent products,
`men_tops`/`men_bottoms`/`women_tops`/`women_bottoms`), pinned commit
`15d8538019b0c5ddefd349dec18c2b35f384afbb`, checksums verified. A
disclosed, deterministic checkerboard sparsification (protocol §3) turns
the fixture's full 293-combination cartesian product into 155 real kept
variants with genuine per-product holes — every real color/size value
still present somewhere, but not every combination — creating 138 real
cross-variant trap opportunities that the unmodified full-cartesian data
would not have had.

**Metrics / decision rule**

See protocol §5/§6. Binary pass/fail: 0 mismatches across all 293
exhaustive (color, size) queries, on both the naive reference
(`Catalog::search`) and the production route (`plan::execute_planned`
via `CatalogIndex`).

**Implementation**

New eval crate `crates/issue55-eval` (added to the workspace `Cargo.toml`
members list), one binary:
`crates/issue55-eval/src/bin/i55_e00_variant_real_data_correctness.rs`.
New dataset scripts: `scripts/datasets/fetch_magento_configurable.sh`,
`scripts/datasets/prepare_magento_configurable.py`,
`scripts/datasets/magento_configurable_checksums.sha256`. Zero
`commerce_core` (production) code changed.

**Results**

```
loaded 22 parent products, 155 kept (sparsified) variants of 293 full-cartesian combinations
total exhaustive (color,size) queries: 293
  true-positive queries (combo is a real kept variant of that product): 155
  trap queries (color and size each real, but never co-occur on one variant of that product): 138
queries routed to FastPath: 293 / 293
mismatches found: 0
=== VERDICT: CONFIRMED -- every one of 293 exhaustive real-data variant-scoped conjunction queries (138 of them genuine cross-variant traps) matched the independently-computed ground truth exactly, on both Catalog::search and the production execute_planned/CatalogIndex path ===
```

Byte-identical across 2 independent runs (raw output:
`docs/research/artifacts/i55_h3_variant_real_data_correctness/run{1,2}.txt`)
— expected, since this is a pure in-memory computation with no I/O timing
dependency, but confirmed rather than assumed.

**Adversarial review** (self-applied, per protocol §8):

- Cross-product true positives (same color+size combo genuinely present
  on >= 2 different products) were actually exercised, not merely
  theoretically possible: 41 such combos exist in the sparsified data
  (e.g. `("Orange", "XS")` is a real variant on 4 different products:
  WH04, WH01, WH02, MH01), confirmed by an independent script reading the
  same JSONL the Rust binary consumes.
- The sparsification's coverage guarantee (every real color/size value
  present on some kept variant) is enforced by an assertion inside
  `prepare_magento_configurable.py` itself, not just claimed — the script
  crashed during development when an earlier, simpler checkerboard rule
  violated it for single-color products (`WSH08`, 1 color x 5 sizes), which
  is exactly how the "only sparsify when both axes have >= 2 values" rule
  in the final script was arrived at, not an untested edge case.
- `k` (`total_kept_variants + 10`) rules out truncation hiding a false
  negative — a truncated hit would fail the exact-set-equality check
  against ground truth, not be silently absorbed; none did.
- `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets
  --all-features -- -D warnings`, and `cargo test --workspace
  --all-features` all pass clean with the new crate added (workspace
  totals unaffected elsewhere; `cargo build --workspace --release`
  confirmed separately).
- Scope boundary (protocol §7): this experiment exercises `FastPath`
  only, the correct route for pure structural conjunctions with no
  free-text residual and the literal scenario Issue #55's own H3 example
  describes. It does not independently test `Hybrid`'s variant safety,
  which depends on the same shared `matches_variant` re-verification step
  already covered elsewhere (Issue #42 R1/R2/R3) — stated explicitly, not
  glossed into a broader-sounding claim.

**Interpretation**

The variant-scoped conjunction guarantee, previously verified only on a
1-product/2-variant synthetic fixture, holds exactly on real (if
disclosed-sparsified) apparel data with 22 products, 155 real variants,
and 138 genuine cross-variant trap opportunities — through both the naive
reference implementation and the actual production `CatalogIndex`/
`execute_planned` path. This directly answers Issue #55's H3 on real data
for the first time. It does not close Issue #47's own, separate
external-validity gap (that one is about the E2d LLM-consensus
controller, not exercised here — see the decision doc). It also does not
establish variant-safety for `Hybrid`/`Punt` routing, larger/messier real
variant structure (e.g. a
catalog with hundreds of variants per product, or attributes that shift
between product-level and variant-level scope across different SKUs of
the same style), or non-Enum variant attributes (numeric ranges,
multi-value) — those remain open for a future, larger-scope experiment if
warranted.

**Regression check**

`crates/commerce-core/tests/variant_safety.rs` remains the synthetic
regression suite this experiment complements, not replaces; the new
`i55_e00_variant_real_data_correctness` binary itself is a repeatable,
deterministic check (0 external dependencies at run time — the dataset is
already fetched/prepared into `dataset_cache/`) that could be promoted to
a CI-run regression test in a future round if the project wants standing
real-data variant-safety coverage; not done in this checkpoint to keep
the change minimal (not requested, not required to answer the question).

**Next question**

1. If a future round wants Hybrid-path variant-safety evidence on real
   data too, wire a real lexical delegate (Tantivy, reusing
   `phase9_eval::bitmap_delegate`) against this same catalog rather than
   assuming FastPath's result generalizes.
2. Continue the falsification loop: rank the next highest-information
   experiment (see `docs/decisions/ISSUE55_H3_DECISION.md`'s closing
   note).
