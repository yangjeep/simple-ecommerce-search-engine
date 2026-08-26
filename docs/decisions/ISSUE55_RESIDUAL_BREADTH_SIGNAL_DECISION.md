# Issue #55 — cross-Brand residual breadth signal: decision

Full log: `docs/experiments/ISSUE55_RESIDUAL_BREADTH_SIGNAL_LOG.md`.

## Governing question

`ISSUE55_HYBRID_ZERO_HIT_MECHANISM_DECISION.md`'s revised next-question
1 asked: since Issue #42 R2's residual-lexical fallback is structurally
inert on any product-type-sparse catalog (`ResidualPolicy::classify`'s
cross-`ProductType`-breadth signal can never fire when a catalog
registers 0-1 product types, as all three ESCI verticals do), would
substituting a cross-*Brand*-breadth axis -- the one entity ESCI
catalogs have real diversity in -- safely recover some of the
quantified "recoverable miss" cases (4-12.5% of Hybrid-routed queries
per vertical) without a precision cost?

## Result: REJECT the design, before implementation, on precedent plus real data

This module already documents a directly analogous, previously-found
defect (`ResidualPolicy::classify`'s doc comment, and the regression
test `treatment_d_does_not_recover_a_compound_constraint_query_...`):
an earlier, entity-scoped version of this exact classifier let a
residual token recover a *wrong-variant* product (a blue leather sofa
for a "velvet blue sofa" query) because "observed somewhere under this
product type" is a poor proxy for "generic enough to safely ignore."
The fix was to use a purely global, entity-independent breadth signal
instead.

**Brand is very plausibly the wrong axis to substitute**, for a reason
grounded in what each entity actually correlates with:
`ProductType` partitions the catalog by functional category, which
genuinely determines descriptive vocabulary (generalizable). `Brand`
does not -- competing brands within the same category routinely share
identical descriptive vocabulary (colors, materials, sizes), precisely
*because* they compete on the same attributes.

**Confirmed directly against real ESCI electronics data before writing
any commerce-core code**: common, genuinely specific, legitimately
disqualifying color values have very high cross-brand breadth --
`"black"` spans 259 distinct brands, `"white"` 85, `"blue"` 51, all far
above the existing threshold of 2. A cross-Brand-breadth axis would
classify every one of these `Preferred` (safe to bypass a zero-hit
delegate result), reopening -- via Brand instead of ProductType -- the
exact wrong-variant-recovery bug class the "second correction round"
fix already closed once. A query like `"castrol black wiper blades"`
using `"black"` as a real, disqualifying filter is exactly the case
this mechanism must protect; the naive substitution would silently
recover any-color Castrol wiper blades instead.

**Verdict: REJECT the cross-Brand-breadth substitution. No commerce-core
code was written or changed.** The precedent plus the real-data
measurement are sufficient to close this design before implementation
-- writing and measuring it would have risked shipping (or at minimum
publishing a favorable-looking recall number for) a design with a
foreseeable, precedented correctness defect, exactly the trap this
project's own "second correction round" was created to catch. Per
CLAUDE.md's discipline ("implement the smallest experiment that can
answer the question"), the smallest experiment here was a design
review plus one real-data measurement, not an implementation.

## What this closes, and what remains genuinely open

This closes the specific "swap the breadth axis to Brand" idea named
as checkpoint 20's next question. It does **not** find any safe
generalization of the R2 residual-lexical fallback for Brand+Color-only
catalogs -- neither of the two ideas checkpoint 20 named (relax the
corroboration precondition alone; relax it plus add a Brand-breadth
axis) survives scrutiny for ESCI-shaped data. Any future fix would need
a genuinely different evidence source than "breadth across some other
registered entity," which is not identified here.

## Real caveats, disclosed rather than smoothed over

- **This is a design-and-precedent-based rejection, not an empirical
  A/B measurement on live query traffic.** The color-breadth numbers are
  real and decisive for the failure mode they demonstrate, but this is
  not the same as running the fallback end-to-end and observing a wrong-
  variant recovery in practice (that would require implementing the
  unsafe version first, which this checkpoint deliberately avoided).
- **No dataset in this project currently has both real `ProductType`
  diversity and real `Brand` diversity together** (WANDS has the
  former, no brand data at all; ESCI has the latter, no product-type
  data at all) -- so a direct, controlled, same-catalog comparison of
  the two breadth axes was not possible here, and is disclosed as a
  structural limitation of the available evidence, not assumed away.
- **The "recoverable miss" cases this thread set out to recover
  (`ISSUE55_HYBRID_ZERO_HIT_MECHANISM_DECISION.md`'s automotive
  "castrol 10w30" example) remain unrecovered.** This checkpoint closes
  one candidate fix without providing a replacement; the underlying gap
  it was trying to close is still open.

## What this does NOT establish

- Not a claim that the R2 residual-lexical fallback is broken or wrong
  for WANDS-shaped (real-`ProductType`) catalogs -- its existing,
  measured GO verdict there (`ISSUE42_DECISION.md`) is untouched.
- Not a claim that no safe generalization could ever exist for
  product-type-sparse catalogs -- only that the two specific ideas
  named so far do not survive scrutiny.
