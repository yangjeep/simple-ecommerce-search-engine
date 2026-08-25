# Issue #51 Preregistered Protocol — R1's full `execute_planned` gate at realistic catalog scale

Committed before this round's rerun is executed, per this repository's
governance. This is the explicit next step `docs/decisions/ISSUE51_DECISION.md`
itself named and left undone: "rerun R1's *full* gate (not just the
isolated corroboration-decision cost measured here) against a catalog of
realistic size."

## 0. What this is testing

R1 (Issue #42) and its Treatment E extension (Issue #51) measured
`execute_planned`'s own P50 serving-latency overhead against Treatment A
(baseline `compile()`, no corroboration) on a hand-built, frozen
**5-product** fixture. Both Treatment D (catalog-scan corroboration) and
Treatment E (precomputed-registry corroboration) failed the
preregistered `<=5%` bar there (D: 8.0%-17.5%; E: 5.9%-14.5% across 5
runs) — `ISSUE51_DECISION.md` itself argues this fixture is
structurally incapable of showing the effect, since the scan Treatment
E eliminates was already near-free at N=5. A separate, already-run
diagnostic (`i51_e00_catalog_scale_diagnostic`) confirmed Treatment D's
*isolated* corroboration-decision cost scales with catalog size (0.72ms
at 15,005 products) while Treatment E's stays flat (~0.0016ms) — but
that diagnostic measured `resolve_d`/`resolve_e` in isolation, never the
full `execute_planned` gate the actual GO/REVISE decision depends on.

This experiment reruns R1's own full gate (correctness: wrong-family
false positives, row 1's no-silent-single-family check, negative rows,
corroborated-row NDCG@10; latency: `<=5%` overhead vs. Treatment A) —
identical methodology, identical 9 query rows, identical fixture
products/IDs — against the same fixture scaled up with harmless decoy
products (reusing `i51_e00_catalog_scale_diagnostic.rs`'s own
`scaled_catalog` approach) to approximate WANDS's real, already-used
42,994-product scale.

## 1. Hypothesis

**H0**: at realistic catalog scale, Treatment E clears the `<=5%`
overhead bar (where it failed at N=5) while preserving every
correctness gate unchanged, because the mechanism it isolates
(catalog-scan cost) becomes a larger, now-measurable fraction of total
`execute_planned` cost, and the registry lookup that replaces it stays
O(1). **H1**: even at realistic scale, E still does not clear `<=5%`,
meaning `execute_planned`'s other fixed costs (not the corroboration
scan) dominate the overhead, and Issue #51's asymptotic argument, while
real for the isolated mechanism, does not translate into passing the
full gate.

Correctness gates are expected to be invariant to catalog scale (decoys
are constructed to never collide with any row's real values, matching
`i51_e00`'s own disclosed construction) — a correctness regression at
scale would itself be a surprising, reportable finding, not assumed
away.

## 2. Baseline / dataset / treatment

Baseline: current branch HEAD. Dataset: R1's own frozen 5-product
fixture (`issue42_eval::r1_workload::build_typed_ambiguity_catalog`),
scaled with decoy products of the same 3 corroborating product types
(Jeans/Wiper Blades/Brake Pads), reusing `i51_e00_catalog_scale_diagnostic.rs`'s
exact decoy-construction logic (same distinct-from-any-real-value size
strings). Decoy count chosen to land at ~43,000 total products,
matching this project's own real WANDS catalog scale rather than an
arbitrary round number. Treatment: none — measurement only, via a new
binary (`r1_full_gate_scale_rerun`) that otherwise reuses R1's own
library functions (`issue42_eval::r1_experimental`,
`issue42_eval::oracle`) unchanged.

## 3. Metrics / gates

Identical to R1/#51's own preregistered gate, evaluated at the larger
scale instead of N=5:

- Zero wrong-family false positives per treatment.
- Row 1 does not silently pick one family.
- Negative rows (9, 10) produce zero spurious hard constraints / zero
  hits as applicable.
- Corroborated-row mean NDCG@10 >= 0.95.
- `execute_planned` P50 latency overhead vs. Treatment A <= 5% for
  Treatments B/C/D/E (median of `LATENCY_TRIALS=7` batched trials of
  `LATENCY_BATCH=200`, matching R1's own methodology exactly).

**GO-leaning**: Treatment E clears every gate including `<=5%` overhead.
**REVISE**: E clears correctness but still exceeds `<=5%` (Issue #51's
asymptotic argument does not resolve the full-gate failure).
**Unexpected finding requiring investigation**: any correctness gate
regresses at scale (would mean decoy construction or some other
scale-dependent effect broke an invariant assumed fixed).

Repetitions: matches R1's own methodology exactly (`LATENCY_TRIALS=7`,
median-combined, `LATENCY_BATCH=200` black-box-guarded calls per row per
trial) — no new repetition-count decision to preregister.
