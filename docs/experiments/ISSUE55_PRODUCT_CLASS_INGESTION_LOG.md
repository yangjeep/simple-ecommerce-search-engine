# Issue #55 Experiment Log — fixing WANDS `product_class` ingestion gaps

Protocol: `docs/experiments/ISSUE55_PRODUCT_CLASS_INGESTION_PROTOCOL.md`.

## I55-PRODCLASS-E00 — fix is real and correct, but does not materially move either preregistered gate; the dominant coverage-gap mechanism is different

**Implementation**

`crates/phase6a-eval/src/catalog.rs`: added `effective_product_class`,
which (1) falls back to the deepest available `category_depth_N`
segment when `product_class` is null/empty (2,852 of 42,994 products,
6.64%, previously mapped to the unmatchable `UNKNOWN_PRODUCT_TYPE`
sentinel), and (2) uses the first `|`-delimited segment instead of the
whole garbled compound string when `product_class` contains multiple
pipe-delimited classes (2,247 products, 5.23%, disclosed as a partial
fix — full multi-type membership would need a `Product`-level schema
change, out of scope here). Wired into `build_catalog`'s existing
`product_type` resolution with no other change. `cargo test --workspace --all-features`:
zero new failures (106+ passing groups, matching pre-fix counts).

**Gate 1 — candidate-set relevant-document recall** (`p9_e04`'s own
diagnostic, n=15 `structural_routed` queries with candidate sets <=5000):

| | Before | After | Change |
|---|---|---|---|
| Mean recall | 0.4460 | 0.4562 | +1.02pp |

Real, non-zero (confirming the fix does add candidates somewhere in
this population), but **below the preregistered >=5pp materiality
threshold**. Per the protocol's own stated fallback: "flag as
insufficient."

**Gate 2 — FastPath/`structural_routed` NDCG@10** (`p9_e02`, real
end-to-end `execute_planned`):

| | Before | After |
|---|---|---|
| FastPath NDCG@10 | 0.1611 | 0.1611 (byte-identical) |
| `structural_routed` NDCG@10 | 0.2953 | 0.2953 (byte-identical) |
| Traffic-weighted overall NDCG@10 | 0.6596 | 0.6596 (byte-identical) |

**Zero measurable movement on any of `p9_e02`'s own headline numbers**
for this specific 480-query judged corpus. The fix produces no
detectable change to which documents end up in the final top-10 for
any of the queries this benchmark actually measures.

**Why the fix is real but doesn't show up here**: `p9_e04`'s own
qualitative low-recall examples (printed both before and after this
fix, unchanged) show the actual missed-relevant products for the
specific measured queries already have valid, non-null,
non-pipe-delimited `product_class` values — e.g. for "twin over full
bunk beds cool desins" (resolved constraint `ProductType("beds")`),
the missed products (#10827, #16846, #16847) all have
`product_class=Some("Kids Beds")` (a real, present value, never
touched by either of this fix's two mechanisms). The query resolves to
the sibling term "beds," a real but *different* lexicon-known product
type from the products' own "Kids Beds" — a category-hierarchy/synonym
mismatch (mechanism 1 from the original investigation), not the
null/pipe-delimited ingestion gap this fix targets (mechanism 3). Every
printed qualitative example shows the same pattern: real, present
`product_class` values that simply don't textually match the query's
own resolved term. This fix's own population (the 11.86% of products
that were null/pipe-delimited) evidently does not substantially overlap
with the specific products these 15-21 measured queries are missing.

**Independent confirmation the fix itself is real, separate from
whether it moves this benchmark**: by direct construction, 2,852
previously-`UNKNOWN_PRODUCT_TYPE` products and 2,247 previously
opaquely-compound-keyed products now resolve to real, potentially
lexicon-matchable product types (verified via the ingestion code path
itself, not inferred) — this is a genuine data-quality improvement,
independent of its lack of visible effect on this specific judged query
set.

## Decision

See `docs/decisions/ISSUE55_PRODUCT_CLASS_INGESTION_DECISION.md`.
