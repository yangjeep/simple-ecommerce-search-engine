# Issue #55 — does a cross-Brand breadth signal safely extend the residual-lexical fallback to Brand+Color-only catalogs? (append-only log)

Continues `ISSUE55_HYBRID_ZERO_HIT_MECHANISM_DECISION.md`'s revised
next-question 1: `ResidualPolicy::classify`'s cross-`ProductType`-breadth
signal can never fire on a catalog (like all three ESCI verticals) that
registers 0-1 product types, so Issue #42 R2's residual-lexical fallback
is structurally inert there. Does substituting a cross-*Brand*-breadth
axis (the one structural entity ESCI catalogs *do* have real diversity
in) safely recover any of the fallback's benefit?

## Entry 1 — the precedent this module already carries

Before designing anything, re-read `ResidualPolicy::classify`'s own doc
comment and the test it cites
(`treatment_d_does_not_recover_a_compound_constraint_query_whose_wrong_variant_the_residual_word_would_have_excluded`,
`crates/issue42-eval/src/r2_experimental.rs`). This documents a real,
previously-found defect: an earlier version of `classify` took a
`_product_type: ProductTypeId` parameter and checked whether a residual
token was observed *under the query's own named product type*
specifically. A fresh adversarial review (explicitly run "before any
production change was made on the strength of R2's GO verdict") found
this let `"velvet"` (observed among Sofas products generally, via a
purple velvet sofa) incorrectly recover a *blue leather* sofa for the
query `"velvet blue sofas"` -- the real, compound-constrained candidate
set (blue sofas only) never contained a velvet item at all. The fix
removed per-entity scoping entirely, keeping only a purely global,
query-independent cross-*type*-breadth signal (a word observed under
`>=2` distinct product types anywhere in the catalog reads as generic;
this is deliberately blind to which product type or variant the
current query happens to name).

This precedent is the governing constraint for any generalization:
whatever axis substitutes for `ProductType`, it must correlate with
"this word is a broad, generic descriptor" and not merely with
"this word is common across many of whatever the substitute entity is."

## Entry 2 — why Brand is very plausibly the wrong axis, before touching any code

`ProductType` correlates with genericness because it partitions the
catalog by *functional category*, which materially determines a
product's own descriptive vocabulary (a "waterproof" jacket and a
"waterproof" phone case are different functional categories, so a word
appearing across many product types really is topic-general). `Brand`
has no such relationship to vocabulary: competing brands within the
*same* functional category routinely share the exact same descriptive
vocabulary -- colors, materials, sizes -- precisely because they are
selling comparable products. A residual token being observed under many
different brands is therefore only weak evidence it is a broadly generic
word; it is equally consistent with the token being a completely
ordinary, specific, legitimately-disqualifying attribute value that
merely happens to be common across the category (e.g. "black").

## Entry 3 — confirmed directly against real ESCI data, before writing any commerce-core code

Computed, for the real ESCI electronics slice (`dataset_cache/esci_electronics/esci_electronics_products.jsonl`),
how many distinct brands each real `color` value is observed under:

```
color='black'   distinct_brands=259
color='white'   distinct_brands=85
color='blue'    distinct_brands=51
color='silver'  distinct_brands=36
color='red'     distinct_brands=20
color='grey'    distinct_brands=19
color='green'   distinct_brands=10
color='yellow'  distinct_brands=6
color='gold'    distinct_brands=1
```

Every common color value except `"gold"` clears the existing
`CROSS_TYPE_BREADTH_THRESHOLD` (2) by a wide margin -- `"black"` alone
spans 259 distinct brands. Under a naive cross-Brand-breadth
substitution, every one of these would classify `ResidualClass::Preferred`
(safe to bypass a zero-hit delegate result), even though a query like
`"castrol black wiper blades"` (Brand=Castrol, residual="black wiper
blades") using `"black"` as a genuine, disqualifying color filter is
exactly the case this mechanism exists to protect -- a bypassed
delegate-empty result would recover *any* Castrol wiper-blade product
regardless of color, the same class of wrong-variant recovery the
"second correction round" fix (Entry 1) was built to prevent, just via
Brand instead of ProductType.

No commerce-core code was written or modified to reach this
conclusion: the precedent (Entry 1) plus the real-data measurement
(Entry 3) is sufficient to reject the design before implementation,
exactly matching this project's own established practice of adversarial
review "before any production change was made on the strength of" a
favorable-looking design.

See the decision doc for the verdict and what remains open.
