# Issue #55 H3 Preregistered Protocol — variant-scoped conjunction correctness on real Product/Variant data

Committed after implementation began (see §4 "disclosed departure from
strict preregistration order" — this is a correctness experiment with an
independently-computed ground truth, not a held-out relevance/economics
measurement, so the stricter "preregister before implementation" bar
Issue #42/#45 impose on those does not transfer unmodified; the departure
is disclosed rather than silently assumed acceptable).

## 0. What this is testing

Issue #55's H3: "measure the correctness and cost of same-variant
constraints... the product must not match unless a compatible variant
satisfies the scoped conjunction." CLAUDE.md's own hard rule: "Product/
Variant correctness is non-negotiable. Cross-variant false matches are
bugs." `crates/commerce-core/tests/variant_safety.rs` already proves this
on one synthetic, 1-product/2-variant fixture. Neither WANDS nor ESCI
(this project's only real datasets to date) has genuine Product/Variant
structure, so H3 has never been tested against real data at all. This
experiment closes that gap directly.

A related, narrower gap exists in `docs/decisions/ISSUE47_DECISION.md`
(note: that file exists only on the *unmerged* `claude/issue-47-e2d-adaptive-consensus`
branch/PR #53, not on `main` — cited here from the PR content, not a
local path): "external validity against a genuine Product/Variant/
relationship dataset remains NOT ESTABLISHED" for Issue #47's own E2d
LLM-consensus *controller* specifically (its `worst_case_robust` proof is
stated sound only where `has_real_variant_grouping=false`). That is a
distinct, LLM-control-plane question this experiment does not touch —
see the decision doc for the full disambiguation. `magento/magento2-sample-data`
was independently named there as the best candidate for that future
work; this experiment integrates it for H3's own, narrower, LLM-free
question, and the resulting dataset/scripts are reusable for that future
Issue #47 work too.

## 1. Hypothesis

**H0 (guarantee holds)**: `commerce_core`'s per-variant `effective_attributes`
merge discipline (`domain::catalog::effective_attributes`, used
identically by `Catalog::search`, `CatalogIndex::build`, and
`CommerceQuery::matches_variant`) generalizes correctly to real, messy
attribute vocabulary — not just the synthetic 2-variant fixture. **H1
(guarantee breaks)**: real data's larger attribute-value diversity and
irregular vocabulary (inconsistent color/size naming, larger variant
counts per product, shared combinations across unrelated products)
exposes a scoping bug the small synthetic fixture cannot reach — e.g. an
index-build shortcut that collapses variant-level attributes into a
per-product structure, or a routing/verification path that skips
`matches_variant`.

## 2. Baseline

Current branch HEAD, zero production code changes. This experiment only
adds new eval-crate code (`crates/issue55-eval`) and a new dataset
adapter — `commerce_core` itself is exercised, not modified.

## 3. Dataset

`magento/magento2-sample-data`, pinned commit
`15d8538019b0c5ddefd349dec18c2b35f384afbb`, the four configurable-product
CSV fixtures (`products_{men_tops,men_bottoms,women_tops,women_bottoms}.csv`
under `app/code/Magento/ConfigurableSampleData/Test/Integration/_files/fixtures/ConfigurableProduct/`).
Real product names, descriptions, categories, materials, and — critically
— real `color`/`size` attribute value vocabulary for 22 configurable
(parent) apparel products. Dual OSL-3.0/AFL-3.0 licensed, public, no
authentication required. Retrieved 2026-08-25; hashes recorded in
`scripts/datasets/magento_configurable_checksums.sha256`.

**Disclosed sparsification** (full rationale/code in
`scripts/datasets/prepare_magento_configurable.py`'s own module
docstring, reproduced in brief here since it materially changes what
"real data" means for this experiment): Magento's fixture format
enumerates the full cartesian product of each parent's color list and
size list as valid variants. That leaves zero genuine within-product
cross-variant trap opportunities — the specific bug class this experiment
targets. A deterministic checkerboard (`keep iff (color_index + size_index)
is even`, applied only when a product has >= 2 colors and >= 2 sizes)
removes roughly half of each product's combinations while a coverage
check guarantees every real color and size value still appears on some
kept variant. This mirrors real retail (most apparel lines do not stock
every color in every size) and is applied deterministically with no
randomness. Every product name, attribute value, and category is
untouched real data; only which color x size *combinations* count as an
in-stock variant is a disclosed, reproducible modification. 22 parent
products -> 155 kept variants (of 293 full-cartesian combinations).

## 4. Treatment

One binary, `crates/issue55-eval/src/bin/i55_e00_variant_real_data_correctness.rs`:

1. Ingests the sparsified real data into `commerce_core::domain::{Product,
   Variant, Catalog}` (color/size as variant-level `AttributeValue::Enum`,
   matching `variant_safety.rs`'s own attribute typing).
2. Builds the real production `CatalogIndex` (`CatalogIndex::build`).
3. For every one of the 22 products, exhaustively generates every (color,
   size) pair drawn from that product's own real color/size vocabulary
   (293 total queries across all products) — both combinations that are a
   real kept variant (155 true-positive queries) and combinations that
   were sparsified away (138 genuine cross-variant trap queries: the
   color is real and present on some variant of that product, the size is
   real and present on some *other* variant of that product, but no
   single variant has both).
4. For each query, computes ground truth **directly from the parsed
   dataset** (independent of any `commerce_core` API) — which
   `(ProductId, VariantId)` pairs, across the *entire* catalog (not just
   the query's own product — a combination can legitimately be a real
   variant of a *different* product too), have exactly that color and
   that size on one variant.
5. Runs each query through both `Catalog::search` (the naive per-variant
   reference oracle) and `plan::execute_planned` (the production
   FastPath/`CatalogIndex` route — see §7 for why this is FastPath, not
   Hybrid/Punt) and compares both against ground truth, exactly (not just
   non-empty/empty).

`k` is set to `total_kept_variants + 10` so no result truncation can hide
a real hit — a false negative from truncation would show up as a mismatch
against the exact-match ground truth, not be silently absorbed.

## 5. Metrics

- Per-query exact-set equality: `Catalog::search` result vs. ground truth;
  `execute_planned` result vs. ground truth.
- Routing outcome per query (expected: 100% `FastPath`, since every query
  is a pure structural conjunction with empty `residual_lexical` —
  `plan::plan`'s own first branch always routes that way regardless of
  constraints).
- Total mismatch count (target: 0).

## 6. Preregistered gates

- **KEEP**: 0 mismatches across all 293 exhaustive queries, on both
  `Catalog::search` and `execute_planned`. The variant-scoped conjunction
  guarantee is confirmed on real, messy attribute data for the first
  time — H3 directly answered (not Issue #47's separate E2d-controller
  external-validity question — see §0).
- **REJECT/CRITICAL**: any mismatch. Given CLAUDE.md's own hard rule
  ("Product/Variant correctness is non-negotiable... cross-variant false
  matches are bugs"), even a single false-positive cross-variant match
  would be a critical, must-fix defect — reported with full detail
  (exact product/color/size/expected/actual), not averaged away.
- **REFINE**: routing outcome is not 100% FastPath for these pure
  structural queries (would indicate `plan::plan`'s documented routing
  contract does not hold as described, a separate finding from H3 itself).

## 7. Disclosed scope boundary

This experiment exercises **`ExecutionOutcome::FastPath` only** — the
correct and expected route for a pure structural `color AND size`
conjunction with no free-text residual (`plan::plan`'s first check routes
any query with empty `residual_lexical` to FastPath unconditionally).
This is the literal scenario Issue #55's own H3 example describes
("query = black AND size 9"). `Hybrid`'s variant-safety depends on the
same shared `CommerceQuery::matches_variant` re-verification step (used
by `verify_and_truncate`/`residual_fallback_hits`, per `plan::mod`'s own
doc comment: "regardless of outcome, commerce_core re-verifies every hard
constraint against every returned hit itself... before returning it") —
already covered by other tests/experiments (Issue #42 R1/R2/R3) and not
duplicated here; exercising it meaningfully would require wiring a real
lexical delegate (Tantivy), which is out of scope for a correctness-only
experiment already answered by FastPath's direct, unmediated bitmap/
ordinal path. This boundary is stated explicitly rather than implied by a
broader-sounding claim.

## 8. Adversarial review checklist (applied before KEEP is recorded)

- Are cross-product true positives (the same color+size combo genuinely
  present on >= 2 different products) actually exercised, not just
  theoretically possible? (Verified: 41 such combos exist and are
  queried.)
- Does the sparsification's coverage guarantee actually hold (every real
  color/size value present on some kept variant)? (Enforced by an
  assertion in `prepare_magento_configurable.py` itself, not just
  claimed.)
- Is `k` genuinely large enough to rule out truncation hiding a false
  negative? (Set to total kept variants + 10; any truncation would appear
  as an exact-match mismatch, not be silently absorbed.)
- Does `cargo clippy --workspace --all-targets --all-features -- -D
  warnings` and the full workspace test suite still pass with the new
  crate added?
