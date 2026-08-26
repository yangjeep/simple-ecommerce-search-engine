# Issue #55 Priority 2 — ancestor-breadcrumb-structure promotion evidence (preregistration)

Written and committed before the evidence-scoring script below is run.
Continues `ISSUE55_PROMOTION_GATE_FULL_SET_DECISION.md`'s own named next
step.

## Governing question

The full-set promotion-gate test found category-hierarchy overlap can
only ever adjudicate 16.1% of the candidate set (51/317 pairs), because
70.7% of pairs have no `product_class`-matched real product data for the
*narrower* name. But `ISSUE55_PRODUCT_CLASS_INGESTION_DECISION.md`
(`crates/phase6a-eval/src/catalog.rs`'s `effective_product_class`)
already establishes exactly why: when a real product's own
`product_class` was null/empty, ingestion fell back to that product's
own deepest `category_depth_N` value -- a **full slash-delimited prefix
path** (e.g. `"furniture / bedroom furniture / beds & headboards / beds /
full & double beds"`), used verbatim as that product's registered
`ProductType` name. A narrower name that is itself such a path IS
already a real category-hierarchy fact, sourced from a real product,
with no further lookup needed -- confirmed directly: 210 of 317 pairs
(66.2%) have a narrower name containing `"/"`, and 138 of 317 (43.5%)
have a broader name containing `"/"` too.

**Hypothesis**: treating a `"/"`-containing name as direct evidence of
its own ancestor path (in addition to, not instead of, the existing
`product_class`-lookup evidence) will raise the resolvable-pair count
and, if the earlier evidence sources' precision holds, the recall
among resolvable non-known-bad pairs -- without reopening the
zero-false-promotion safety gate.

## Method

For a name `X` (broader or narrower), its set of real category-hierarchy
paths is:

```
direct   = {X}                      if "/" in X, else {}
looked_up = product_class-matched category-hierarchy rows for X (unchanged from the prior protocol)
paths(X) = direct | looked_up
```

`top_level`/`level_2` extraction and the `PROMOTE`/`UNRESOLVED` rule are
otherwise **exactly** as in `ISSUE55_PROMOTION_GATE_FULL_SET_PROTOCOL.md`
-- this experiment changes only how evidence is *gathered* per name, not
how overlap is scored once gathered.

## Ground truth and thresholds (unchanged, reused verbatim from the prior protocol)

- `KNOWN_BAD = {("beds", "cat beds"), ("beds", "dog beds & mats")}`.
  Neither known-bad name contains `"/"` (verified directly against the
  candidate-set JSON before writing this document), so this new evidence
  source cannot itself add or remove evidence for either known-bad pair
  -- the safety-gate check is exercised exactly as before, not weakened
  by construction.
- Safety gate: zero false promotions on `KNOWN_BAD`, still an automatic
  REJECT if violated.
- Recall bar: `>=50%` among resolvable non-known-bad candidates (the
  resolvable set is expected to grow; the same 50% bar is reused, not
  loosened for the larger denominator).

## What a promoted pair does and does not mean here

A pair promoted under this evidence source is not thereby individually
certified correct -- exactly as the prior protocol disclosed. The only
ground-truth-anchored claim this experiment can make is about the two
`KNOWN_BAD` pairs specifically; newly-promoted pairs are evaluated in
aggregate (does recall clear the preregistered bar) without a fresh
manual re-audit of each one (that remains `p9_e08`'s job).

## Stop condition

Run the scoring script exactly once, report the safety gate and recall
bar outcome for both `top_level` and `level_2`, plus the size of the
newly-resolvable set attributable specifically to the ancestor-structure
addition -- no threshold changes after seeing the output.
