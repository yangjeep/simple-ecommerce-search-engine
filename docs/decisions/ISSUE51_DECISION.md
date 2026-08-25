# Issue #51 Decision — precomputed typed corroboration (Treatment E)

Full protocol: `docs/experiments/ISSUE51_PROTOCOL.md`. Full log/raw
numbers: `docs/experiments/ISSUE51_LOG.md`. Raw artifacts:
`docs/research/artifacts/i51_e00_precomputed_corroboration/`,
`docs/research/artifacts/i51_e00_catalog_scale_diagnostic/`.

## What was tested

Issue #42's R1 found Treatment D (corroboration-aware typed-ambiguity
resolution) passes every correctness gate but fails the <=5%
serving-overhead bar, attributable to a per-query O(catalog-size) scan.
Issue #51 asked whether moving that scan to ingestion/compile time (a
precomputed registry, O(1) query-time lookup) preserves correctness while
clearing the bar. Treatment E implements exactly that, reusing R1's own
decision logic unchanged (verified byte-for-byte against Treatment D, not
just via aggregate gate results) and R1's own frozen 5-product fixture
and measurement methodology.

## Verdict: **REVISE**

Treatment E passes every correctness/wrong-family/fallback gate
identically to Treatment D across 5 independent runs (mean NDCG@10 =
1.0000, 0 wrong-family false positives in every run). **Neither Treatment
D nor Treatment E clears the <=5% latency-overhead bar at R1's own
5-product fixture** (D: 8.0%-17.5% across 5 runs; E: 5.9%-14.5%, directionally
better in 4 of 5 but not decisively). Taken alone, this is REVISE, matching
D's own prior REVISE — the literal preregistered gate is not cleared.

**This is not the full picture, and reporting only the letter of the gate
would be misleading.** A disclosed, NOT-gate-affecting diagnostic
(`i51_e00_catalog_scale_diagnostic`) scaled the same fixture with harmless
decoy products and found Treatment D's own cost grows roughly linearly
with catalog size (0.0017ms at 5 products -> 0.72ms at 15,005, ~490x for
a ~3,000x catalog-size increase) while Treatment E's stays essentially
flat (0.0015-0.0017ms across the same range) — a 492x advantage for E at
15,005 products, reproduced across two independent runs of the
diagnostic. R1's own 5-product fixture is structurally incapable of
demonstrating a mechanism whose entire value proposition is asymptotic:
at N=5, the scan Treatment E eliminates was already near-free, so there
is very little absolute cost left for *any* mechanism change to visibly
remove relative to `execute_planned`'s own other fixed costs.

**Recommendation, stated precisely (Issue #51's own "Done when" requires
a GO/REVISE/STOP statement)**: REVISE, with a specific, actionable path
to resolution rather than an open-ended one. The precomputed-registry
mechanism is correctness-preserving (exactly, not approximately) and
delivers its intended asymptotic improvement decisively once catalog size
moves past a few hundred products — a threshold every real catalog this
project has ever measured (WANDS: 42,994; ESCI: far larger) clears by
orders of magnitude. Before a production GO/NO-GO decision, rerun R1's
*full* gate (not just the isolated corroboration-decision cost measured
here) against a catalog of realistic size. This experiment did not do
that end-to-end rerun — it measured the specific mechanism the
<=5%-overhead failure was attributed to, in isolation, which is the
smallest experiment that could answer Issue #51's own stated research
question, and left the larger end-to-end confirmation as explicit future
work rather than overclaiming it here.

## An implementation defect, found and fixed before any number was trusted

The first working version of the registry's lookup key allocated a fresh
`String` on every query-time call, which cost *more* than the scan it
replaced on this fixture's tiny scale, briefly making Treatment E look
*worse* than Treatment D (18.5% vs. 12.6% overhead in the very first
run). Restructured to a nested map whose inner lookup borrows a `&str`
directly, with zero query-time allocation — fixed before any GO/REVISE
number was drawn from it, and disclosed here rather than only shipping
the corrected version silently.

## Action taken

- `crates/issue42-eval/src/r1_experimental.rs`: added `AttrKind`,
  `build_attribute_kind_registry`, `registry_has_kind`, `resolve_e`, and
  6 new tests. Treatments A-D unchanged (verified, not just claimed).
- `crates/issue42-eval/src/bin/r1_typed_ambiguity_eval.rs`: added
  `Treatment::E` to the existing R1 harness (same process, same fixture,
  same measurement methodology — deliberately not split into a second
  binary, to keep the D-vs-E overhead comparison free of a cross-process
  confound).
- New binary `crates/issue42-eval/src/bin/i51_e00_catalog_scale_diagnostic.rs`
  for the scaling diagnostic.
- Zero `commerce_core` (production) code changed — matches Issue #51's
  own "implemented behind an experimental boundary" requirement and its
  explicit "do not fold into E2d/Issue #47" / "do not add a generic
  dynamic schema/query DSL" boundaries (the registry is a fixed 5-variant
  discriminant lookup, not a schema mechanism).
- Issue #51 is not closed by this checkpoint — REVISE, with a named
  concrete next step (full-gate rerun at realistic scale), left open per
  Issue #51's own "Done when" checklist until that rerun (or an explicit
  decision not to pursue it) happens.

## Architecture delta

Positive evidence, with a caveat. This strengthens confidence that H5's
"deterministic profiling/compiler logic should choose physical
representation from measurable properties" thesis extends cleanly to
this specific serving-contract gap: moving catalog-dependent work to
compile time is not just correctness-preserving here, it is decisively
faster at any realistic scale. It does not itself authorize production
adoption — Issue #51's own <=5% bar, applied to R1's specific frozen
fixture, is not cleared, and this checkpoint's own diagnostic is
explicitly scoped to the isolated mechanism, not the full end-to-end
pipeline a production decision would need measured.

### Update (2026-08-25) — the named next step is done: REVISE confirmed, not resolved, at realistic scale

The follow-up this decision named (`docs/decisions/ISSUE51_FULLGATE_SCALE_DECISION.md`)
reran R1's full `<=5%` gate at ~43,000-product scale. Treatment E still
does not clear it (14.0%-16.7% across 3 runs, essentially unchanged from
R1's own N=5 measurement) — the asymptotic advantage above is real and
confirmed again at this scale, but it was never the dominant cost in the
full gate. A per-row breakdown found the actual driver: the `Punt`
fallback's lexical-delegate query cost for genuinely ambiguous,
uncorroborated queries (row 1's own case), which Issue #51's registry
optimization does not touch. REVISE is confirmed as this thread's
terminal state, not an artifact of R1's small fixture — closing this
open item without re-opening a new investigation into the same
mechanism.
