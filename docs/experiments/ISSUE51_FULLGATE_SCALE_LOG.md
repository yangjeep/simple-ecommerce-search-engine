# Issue #51 Experiment Log — R1's full gate at realistic catalog scale

Protocol: `docs/experiments/ISSUE51_FULLGATE_SCALE_PROTOCOL.md`.

## I51-FULLGATE-E00 — E's asymptotic advantage confirmed real, but does not resolve the full-gate failure; a different bottleneck dominates at any scale

**Implementation**

New binary `crates/issue42-eval/src/bin/r1_full_gate_scale_rerun.rs`,
reusing R1's own library functions (`issue42_eval::r1_experimental`,
`issue42_eval::oracle`, `issue42_eval::r1_workload`) and duplicating
only the small, already-tested harness logic
(`r1_typed_ambiguity_eval.rs`'s `Row`/`RowClass`/`Treatment`,
correctness-check helpers, `one_latency_trial`/`resolve_for`, and the
gate-evaluation loop) so this rerun's methodology is provably identical
to R1's own, applied to a scaled-up catalog instead of the frozen
5-product fixture.

### Two methodology defects found and fixed before trusting any number

**Defect 1 — decoy attribute pollution.** The first attempt reused
`i51_e00_catalog_scale_diagnostic.rs`'s own decoy shape (decoys of
product types 1/2/3 with a `"size"` enum attribute) directly. Result:
NDCG collapsed from 1.0 to 0.6667/0.3333 for Treatments A/D/E and
routing outcomes changed (row 3 flipped `fast_path` -> `punt`) — a
correctness-affecting artifact, not the intended latency-only scaling.
Root-caused by reading `CatalogProfile::build`
(`crates/commerce-core/src/cold_start/profile.rs:92-101`): it indexes
attribute values from **every** product regardless of product type, so
42,990 decoy `"size"` enum values polluted the exact lexicon vocabulary
the real rows' resolution depends on. Fixed: decoys now use a distinct,
unregistered product type/brand/category (`9999`, absent from
`fixture.product_types`/`brands`/`categories`) and **zero attributes**
at both product and variant level — inert to the profile/lexicon, while
still inflating `catalog.products.len()`, which is all
`constraint_kind_registered_on_product_type`'s own `.iter()` scan
actually costs against (it filters by product type but still visits
every element to check the predicate).

**Defect 2 — decoy price collision.** With attribute pollution fixed,
a per-row latency breakdown (added specifically to investigate a
surprising early result — Treatments D/E measuring *faster* than
baseline A) revealed row 4 ("under $34") alone consumed ~97% of total
measured latency (~2.13-2.17ms of a ~2.2-2.25ms total), identical
across every treatment. Cause: decoys were priced at $10.00, which
satisfies "under $34" — turning a regression-guard row designed to test
a *narrow* price filter into an accidental near-full-catalog
(42,994-candidate) scan. Because this cost is shared identically across
all five treatments, it did not bias the *direction* of the D-vs-A
comparison, but its magnitude (dwarfing every genuinely
treatment-dependent row by 3+ orders of magnitude) meant the aggregate
number was measuring noise in one irrelevant, shared row rather than
the actual mechanism under test. Fixed: `crates/commerce-core/src/ir/structural.rs:36-37`
confirms `PriceUnderCents`/`PriceOverCents` use strict `<`/`>`; pricing
decoys at exactly $34.00 (3,400 cents) makes them invisible to both
regression-guard rows simultaneously. Both defects are disclosed here,
not silently patched, per this project's own methodology-correction
discipline.

### Result, after both fixes (3 independent full runs)

```
Run 1: Treatment D overhead=17.4%  Treatment E overhead=14.2%
Run 2: Treatment D overhead=12.9%  Treatment E overhead=14.0%
Run 3: Treatment D overhead=18.9%  Treatment E overhead=16.7%
```

Both treatments **consistently fail** the `<=5%` bar at ~43,000-product
scale, in the same order of magnitude as R1's own original N=5
measurement (D: 8.0%-17.5%; E: 5.9%-14.5%). Correctness is unaffected
and improved relative to the earlier, contaminated attempt: Treatments
D and E both reach the required NDCG@10=1.0000 and zero wrong-family
false positives at this scale, matching R1's own original correctness
results (Treatment A itself still fails its own row-1
silent-single-family check and has NDCG 0.6667 — consistent with A
being the naive, uncorroborated baseline this whole mechanism exists to
fix, not a scale-introduced regression).

**Root cause of the persistent overhead, confirmed via a per-row
latency breakdown** (diagnostic only, single trial per treatment, not
part of the preregistered median-of-7 gate):

```
Treatment A row 1 ("size 22"): 0.00020ms  (fast_path, 1 candidate)
Treatment D row 1 ("size 22"): 0.00991ms  (punt)
Treatment E row 1 ("size 22"): 0.01043ms  (punt)
```

Row 1 alone accounts for essentially the entire measured overhead: the
~0.0097-0.0102ms per-call difference is ~14-17% of D/E's own ~0.06-0.07ms
total, matching the measured aggregate overhead almost exactly. This is
**not** the mechanism Issue #51 targeted. Treatment A's naive `compile()`
picks one interpretation of "size 22" as a single hard constraint
(1 candidate, cheap `FastPath` execution); Treatments D/E correctly
recognize the corroboration is absent and route to `Punt` (real, both
interpretations are genuinely ambiguous with nothing to corroborate them
-- the intentional, correct fallback design), which incurs a real,
measurable per-call cost from querying the in-memory Tantivy lexical
delegate, even to return zero hits. This delegate-query cost has nothing
to do with `constraint_kind_registered_on_product_type`'s
catalog-scan-vs-registry-lookup cost (rows 2/3/6, the actually
corroborated rows, cost microseconds regardless of treatment or scale,
confirming Issue #51's own mechanism is and remains cheap). Rows 7/9's
`Punt`-routed regression-guard queries contribute a comparable,
treatment-invariant delegate cost too (~0.02-0.03ms each), further
diluting the corroboration mechanism's own already-tiny share of total
latency.

**This does not contradict `i51_e00_catalog_scale_diagnostic`'s own
finding** (Treatment D's isolated corroboration-decision cost scales
with catalog size while E's stays flat, a 492x advantage at 15,005
products) — that finding is about `resolve_d`/`resolve_e` in isolation,
and remains true and unaffected here (rows 2/3/6's own costs stay
microsecond-scale identically for D and E at this scale, consistent
with E's registry lookup being O(1) and D's scan still being fast enough
at this scale not to show up above other costs). What this experiment
adds is that this isolated mechanism, even fully optimized, is not what
determines whether the *full* `execute_planned` gate passes — a
different part of Treatment D/E's own resolution logic (the row-1
ambiguous-uncorroborated `Punt` fallback) dominates instead, at any
catalog scale tested so far.

## Decision

See `docs/decisions/ISSUE51_FULLGATE_SCALE_DECISION.md`.
