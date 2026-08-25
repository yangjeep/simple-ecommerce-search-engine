# Issue #51 Experiment Log — precomputed typed corroboration (Treatment E)

Protocol: `docs/experiments/ISSUE51_PROTOCOL.md`.

## I51-E00 — Treatment E vs R1's frozen gate, plus a catalog-scale diagnostic

**Question**

R1's Treatment D passes every correctness criterion for corroborated
typed-ambiguity resolution but fails the <=5% latency-overhead bar,
attributable to a per-query O(catalog-size) scan
(`constraint_kind_registered_on_product_type`). Can the same semantics be
preserved while moving that scan to ingestion/compile time?

**Hypothesis**

H0: a precomputed registry reproduces Treatment D's decisions exactly
while eliminating the query-time scan, clearing the overhead bar. H1: the
registry itself, or another cost inside `resolve_d`'s broader logic, still
leaves overhead above the bar.

**Workload**

R1's exact frozen fixture and 9-row workload (`crates/issue42-eval/src/r1_workload.rs::build_typed_ambiguity_catalog`,
5 products), reused unmodified. A second, disclosed, NOT-gate-affecting
diagnostic scales the same fixture with harmless decoy products (see
below).

**Metrics / decision rule**

See protocol §5/§6 — identical to R1's own.

**Implementation**

`crates/issue42-eval/src/r1_experimental.rs`: added `AttrKind` (a plain
5-variant discriminant), `build_attribute_kind_registry` (scans the
catalog once), `registry_has_kind` (query-time lookup), and `resolve_e`
(Treatment D's exact decision logic, `registry_has_kind` in place of the
live scan). Treatments A-D's own functions are **byte-for-byte
unchanged** — verified directly by three new tests
(`treatment_e_matches_treatment_d_exactly_when_jeans_corroborates`,
`..._with_no_corroborating_entity`, `..._on_every_r1_workload_row`), not
merely asserted in prose. `crates/issue42-eval/src/bin/r1_typed_ambiguity_eval.rs`:
added `Treatment::E`, a separately-timed one-time registry-build step,
and threaded the registry through the existing `resolve_for`/
`one_latency_trial` call sites (refactored into a small `EvalContext`
struct to stay under clippy's argument-count lint, not `#[allow]`ed
away). New binary `crates/issue42-eval/src/bin/i51_e00_catalog_scale_diagnostic.rs`
for the scaling diagnostic (below). Zero `commerce_core` code changed.

**An implementation defect found and fixed before any result was trusted**

The first working version keyed the registry as
`HashMap<(ProductTypeId, String), HashSet<AttrKind>>`, which forces
`registry_has_kind` to allocate a fresh `String` (`attribute.to_string()`)
on every query-time lookup just to build the key. On R1's own tiny
5-product fixture, that allocation cost **more** than the linear scan it
replaced: Treatment E measured 18.5% overhead vs. Treatment A in the
first run, *worse* than Treatment D's own 12.6% in the same run — the
opposite of the intended effect. Restructured to a nested
`HashMap<ProductTypeId, HashMap<String, HashSet<AttrKind>>>`, whose inner
lookup uses `HashMap<String, _>::get(&str)` directly (`String:
Borrow<str>`) with zero query-time allocation. All 13 unit tests
(including the three new byte-identical-to-D checks) still pass after the
fix. This is disclosed here rather than only shipping the corrected
version silently, per this project's own evidence-preservation
discipline.

**Results — R1's frozen gate (5 independent runs, post-fix)**

| Run | D overhead vs A | E overhead vs A | D NDCG | E NDCG | D wrong-family FPs | E wrong-family FPs |
|---|---|---|---|---|---|---|
| 1 | 17.5% | 14.5% | 1.0000 | 1.0000 | 0 | 0 |
| 2 | 11.7% | 11.3% | 1.0000 | 1.0000 | 0 | 0 |
| 3 | 8.0% | 10.7% | 1.0000 | 1.0000 | 0 | 0 |
| 4 | 9.7% | 5.9% | 1.0000 | 1.0000 | 0 | 0 |
| 5 | 9.0% | 6.5% | 1.0000 | 1.0000 | 0 | 0 |

Treatment E passes every correctness/wrong-family/row1/negative-row gate
identically to Treatment D in all 5 runs (raw output:
`docs/research/artifacts/i51_e00_precomputed_corroboration/run{1..5}.txt`).
Registry build time itself: 0.008-0.014ms for this 5-product catalog
(disclosed, not counted against the query-time gate per protocol §4).
**Neither treatment clears the <=5% overhead bar at this catalog size.**
E is directionally better than D in 4 of 5 runs (D range 8.0%-17.5%
mean~11.2%; E range 5.9%-14.5%, mean~9.8%) but the gap is small and noisy
at N=5 — not itself a clean win.

**Diagnostic (NOT part of the preregistered gate) — does the mechanism's benefit scale with catalog size?**

R1's fixture has only 5 products, so a linear scan filtered to one
product type is already near-free regardless of mechanism — the worst
possible test bed to observe an O(catalog-size) vs O(1) difference.
`i51_e00_catalog_scale_diagnostic` scales the same fixture with harmless
decoy products (distinct size values, incapable of becoming spurious
hits) and measures `resolve_d`/`resolve_e`'s own cost in isolation
(query compilation + corroboration decision only, not the full
`execute_planned` pipeline):

| Catalog size | D median | E median | D/E ratio |
|---|---|---|---|
| 5 | 0.00174ms | 0.00154ms | 1.13x |
| 20 | 0.00168ms | 0.00117ms | 1.44x |
| 155 | 0.00520ms | 0.00119ms | 4.36x |
| 1,505 | 0.04012ms | 0.00127ms | 31.70x |
| 15,005 | 0.72353ms | 0.00147ms | 491.82x |

Reproduced twice (first pass: 1.49x/1.36x/4.49x/36.01x/453.23x at the
same sizes) — the pattern is stable, not a one-off. Treatment D's cost
grows roughly linearly with catalog size (~490x for a ~3,000x catalog
growth); Treatment E's stays essentially flat (0.0015-0.0017ms across the
same 3,000x range) — exactly the asymptotic behavior the O(catalog-size)
vs. O(1) diagnosis predicts. Raw output:
`docs/research/artifacts/i51_e00_catalog_scale_diagnostic/run1.txt`.

**Adversarial review** (self-applied, per protocol §8):

- Byte-identical-to-D check: confirmed by three dedicated unit tests
  comparing `Resolution`s directly, not just aggregate gate pass/fail —
  holds for every one of R1's 9 rows.
- Registry-rebuilt-per-query check: the registry is constructed once in
  `main`, outside the timed loop, and passed by reference into
  `one_latency_trial`/`resolve_for` — confirmed by reading the call
  sites, not merely asserted; a rebuild-per-query bug would have
  reproduced Treatment D's own overhead almost exactly, which the
  post-fix numbers above do not show.
- Empty-product-type handling: `registry_lookup_for_unknown_product_type_returns_not_registered`
  confirms a `HashMap` miss returns "not registered," matching Treatment
  D's `.any()` over an empty iterator.
- Latency methodology parity: both binaries reuse R1's own median-of-N
  batched-trial discipline (`r1_typed_ambiguity_eval.rs`: 7 trials,
  200-call batches, unchanged; the scale diagnostic: 5 trials, 500-call
  batches, a fair-enough variant for its own much smaller
  per-call-under-test scope) — not a single-batch measurement, avoiding
  the timer-floor artifact R1's own methodology note already disclosed.
- Scope honesty: the scaling diagnostic measures `resolve_d`/`resolve_e`'s
  own cost in isolation, not the full `execute_planned` pipeline's
  overhead-vs-A the R1 gate actually measures — stated explicitly, not
  implied to be the same number.

**Interpretation**

The precomputed-registry mechanism preserves Treatment D's correctness
exactly (verified byte-for-byte, not just via aggregate metrics) and
delivers the intended O(1)-vs-O(catalog-size) asymptotic improvement
decisively at any catalog size beyond a few hundred products — a 492x
speedup at 15,005 products, a small-to-medium real catalog (WANDS alone
has 42,994; ESCI's is far larger). At R1's own frozen 5-product fixture,
neither Treatment D nor E clears the preregistered <=5% bar, because the
absolute cost of *any* catalog-dependent work at that scale is already
negligible relative to other fixed costs (query compilation,
`execute_planned` machinery) — the fixture cannot, by construction,
demonstrate a mechanism whose entire value proposition is asymptotic.
This is not evidence against the mechanism; it is evidence the frozen
fixture is the wrong instrument to settle a production GO/NO-GO decision
for it.

**Regression check**

The three new byte-identical-to-D tests plus
`registry_lookup_for_unknown_product_type_returns_not_registered` and
`registry_lookup_distinguishes_attribute_value_kind_not_just_name` are
the regression surface for Treatment E specifically; R1's own existing
tests (unchanged) remain the regression surface for A-D.

**Next question**

1. If a production decision is actually being made, rerun the full R1
   gate (not just the isolated `resolve_d`/`resolve_e` cost) against a
   catalog of realistic size — the scaling diagnostic strongly predicts
   Treatment E would clear the <=5% bar comfortably while Treatment D
   would not, but this has not been directly measured end-to-end.
2. Continue the falsification loop: select and preregister the next
   highest-information experiment (see `docs/decisions/ISSUE51_DECISION.md`).
