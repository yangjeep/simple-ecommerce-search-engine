# Issue #45 Decision — E2c: deterministic semantic canonicalization

**Decision: REVISE.** Not GO, not STOP. Deterministic canonicalization
substantially and measurably reduces the instability E2b's raw LLM
proposals exhibited (true raw full-descriptor agreement 74.96%, not the
87.60% E2b's own narrower role+primitive-only metric reported) without
introducing any confirmed unsafe accepted classification, under every
measurement design tried, including a stricter one this checkpoint
devised and ran against itself. But the specific preregistered
quantitative bars (≥99% compiled physical-primitive agreement, ≥98%
full canonical descriptor agreement) are met only under an ensemble
(multi-run) reading not met under a stricter, single-proposal-per-field
reading that this checkpoint's own self-skepticism produced — and a
fresh, independent, three-reviewer adversarial review found the
protocol's own explanatory narrative overstated how much of the result
comes from genuinely evidence-based conflict resolution (R3/R4) versus
architectural/structural mechanisms (R1/R6) and real but narrow safety
gates (R5/R7). Both are disclosed fully below, not the basis for a
manufactured GO.

## What this covers

Issue #45's own governance (preregister before held-out runs; preserve
every raw proposal and intermediate decision; independent
evaluator/oracle; no silent replacement of corrected numbers; fresh
adversarial review with regression tests for confirmed defects; report
GO/REVISE/STOP without relaxing thresholds after the fact) — followed
throughout. Full detail: `docs/experiments/ISSUE45_PROTOCOL.md` (the
preregistered protocol, including its own dated Addendum 1 covering the
adversarial review), `docs/experiments/ISSUE45_LOG.md` (the experiment
log).

- **Reused, not reinvented**: `CandidateDescriptor` is
  `e2b_schema::Descriptor` verbatim; the canonical physical-primitive
  vocabulary is E2b's own, audited against real `commerce_core` (no
  `RelationshipIndex` primitive exists anywhere in the engine —
  confirmed by exhaustive grep before any rule was written).
- **11 deterministic canonicalization rules** (R1-R11), each grounded in
  an already-shipped `e2b_validator.rs` threshold, Issue #45's own
  example text, or an audited `commerce_core` capability/dataset-structure
  fact — never fit to specific disagreeing data points (though §1 of the
  protocol discloses honestly that the required "explain the 87.60%"
  analysis means no config/run subset can be called genuinely blind for
  rule design; the real commitment device is that the rules were frozen,
  in code, before any measurement was trusted).
- **Three treatments measured**: B (naive majority vote, kept in its own
  module so it structurally cannot share the canonicalizer's logic), C
  (the canonicalizer), D (C plus a stricter admission bar). D never
  diverges from C on any of the real 20 frozen artifacts — every real
  unstable key has a landslide role split, none falls in D's own
  decision zone. D's distinguishing mechanism is validated only by a
  synthetic unit test in this checkpoint, not real data.
- **A confirmed implementation defect, found by a fresh three-reviewer
  adversarial review, fixed with a regression test**: R3/R9's original
  condition could silently overwrite a real raw-proposal majority when
  an unrelated minority pair happened to conflict. Verified independently
  by all three reviewers, and a fourth time here, not to have affected
  any of this checkpoint's own reported numbers — re-running both eval
  binaries after the fix reproduces every number byte-for-byte.
- **A confirmed documentation defect, corrected in place (rule 9)**: the
  protocol's own §6 table claimed R3 resolves Enum-vs-FreeText
  disagreement; false — R3's own gating condition cannot fire there.
- **A genuinely humbling measured finding**: on the real 20-artifact
  dataset, R3 (the rule most prominently presented as "evidence-based
  resolution") fires on exactly 1 of 125 (config, key) groups and never
  once changes the outcome from what plain majority voting alone already
  gives. Treatment C's real, demonstrated differentiation from naive
  voting comes from R1 (primitive as a deterministic function of role —
  architectural, not evidence-integration), R6 (a disclosed,
  tautological dataset-structural scope default), and R5/R7 (narrow but
  real safety gates that each blocked one actual unsafe-shaped
  promotion naive voting would have made).

## The verdict, stated precisely

**Safety**: zero confirmed unsafe accepted structural classifications
under every treatment and every measurement design tried, including a
worst-case check across every individual single-run canonicalization.
This is the single most load-bearing finding and it held up completely
under adversarial review — real, demonstrated, non-vote-dependent
mechanisms (R5, R7) are the reason, not luck.

**Stability — genuinely mixed, not a clean pass**: full-descriptor
stability rises from a true raw baseline of 74.96% to 100.00% under a
leave-one-out (4-of-5-run ensemble) design — clearing GO-gate criteria
2 (≥99% primitive) and 3 (≥98% full) cleanly. Under a stricter
self-check this checkpoint devised specifically because the 100% number
looked too clean to trust (canonicalizing each raw run individually, no
cross-run voting benefit at all — arguably the more realistic reading
for a production deployment that runs one LLM proposal per field rather
than an ensemble of ~5), full-descriptor and primitive stability are
both **95.20%**, short of both bars. This single-run design was not
part of the preregistered §9 measurement plan — disclosed explicitly as
a post-freeze addition, not a substitute for the preregistered reading,
and not gamed toward either a favorable or unfavorable result (it was
added, then run once, then reported as it came out, both before and
after the R3 fix).

**Recall**: 89.47%, within the preregistered 5-percentage-point band of
E2b's own 86.84% LLM+validator recall (criterion 4, PASS) — but a real,
measured cost relative to naive majority voting's own 97.37%, driven by
Treatment C's higher abstention rate (20.75% vs 13.21%).

**Relevance (criterion 5)**: naive end-to-end NDCG@10 gap of 0.00%
(Treatment C matches the oracle exactly) — PASS, but on the same
near-floor, `check_reliable=false` check E2b's own decision record
already flagged as weak evidence; read as PASS-with-caveat, never as a
strong independent confirmation.

**Serving overhead (criterion 6)**: PASS — P95/P99 of `execute_ranked`
both clear the ≤5% bar on real compiled `CatalogIndex` bundles over the
real 42,994-product WANDS catalog; P50 correctly reported INCONCLUSIVE
(below this measurement's own pre-declared timer floor), not rounded to
PASS.

**Order-independence (criterion 7)**: verified structurally by a unit
test (`canonicalization_output_does_not_depend_on_run_order`); no hidden
last-writer/majority-winner behavior found by three independent
reviewers' own direct code inspection.

## Seven-criterion GO-gate table

| # | Criterion | Leave-one-out reading | Single-run (stricter) reading |
|---|---|---|---|
| 1 | Zero confirmed unsafe accepted | 0 — **PASS** | 0 (worst-case across every single-run canonicalization) — **PASS** |
| 2 | ≥99% compiled primitive agreement | 100.00% — **PASS** | 95.20% — **FAIL** |
| 3 | ≥98% full canonical descriptor agreement | 100.00% — **PASS** | 95.20% — **FAIL** |
| 4 | Recall within 5pp of E2b's 86.84% | 89.47% — **PASS** | not independently recomputed (no single, consistent promoted set to score under N=1 canonicalization) |
| 5 | No material relevance regression | 0.00% relative gap — **PASS**, with the same disclosed near-floor `check_reliable=false` caveat E2b's own decision record already carries | not independently recomputed |
| 6 | Serving overhead within budget | P95/P99 clear ≤5%, P50 INCONCLUSIVE — **PASS** | not applicable (design is orthogonal to which run-count feeds canonicalization) |
| 7 | No hidden last-writer/majority winner | **PASS**, verified structurally by test | **PASS**, same test |

Per Issue #45's own falsification criteria: "post-canonicalization
stability remains close to raw LLM stability" does **not** apply (74.96%
→ 95.20% single-run, → 100.00% ensemble, is a large, real gap in every
reading); "canonicalization merely reproduces majority vote" does
**not** cleanly apply either (R1/R5/R6/R7 are real, demonstrated,
non-vote-derived mechanisms with measured effect — see the log's own
attribution finding), but it is closer to true than the protocol's own
original framing suggested, since R2/R3 specifically — the rules
explicitly billed as "evidence-based conflict resolution" — do
essentially reproduce plain plurality on this dataset. "High stability
achieved only by rejecting most retrieval-significant features" does
**not** apply (recall stayed above the E2b floor). "The internal
semantic vocabulary grows into a generic document/schema system" does
**not** apply (zero new enum variants). "Physical primitive choice
still materially controlled by free-form model wording" does **not**
apply (R1 is a fixed function of role, never of `evidence` text). None
of these trigger a mechanical STOP. But criteria 2 and 3 failing under
the stricter, arguably more production-realistic reading, combined with
the adversarial review's finding that the protocol's own narrative
overstated R3/R4's real contribution, is exactly the kind of genuinely
mixed evidence Issue #45's own governance says should produce REVISE,
not a manufactured GO.

## What this does NOT establish

- That a single-proposal-per-field production deployment (no LLM
  ensemble) would meet the preregistered stability bars today — the
  single-run reading suggests it would not, at 95.20% vs the 98%/99%
  bars, though the gap is far smaller than raw's own 74.96%.
- That R3/R4 (the rules explicitly designed to resolve genuine
  Enum-vs-Numeric and zero-variance disagreement from evidence) add
  real value beyond naive voting — on this specific dataset, they were
  barely exercised (R3: 1 of 125 groups, 0 flips of the plain-plurality
  answer) and cannot be said to have been genuinely tested, let alone
  validated, by this checkpoint's own evidence. Their architectural
  correctness (verified by unit tests and hand-authored adversarial
  fixtures) is not in question; their *empirical* contribution on real
  data is unestablished.
- That Treatment D's stricter admission bar is safer or more useful than
  Treatment C in practice — it never differed from C on any real key in
  this dataset; its own value proposition rests entirely on a synthetic
  test case.
- That this generalizes beyond WANDS/automotive, the same two sources
  E2b itself used — no new dataset was acquired for E2c, per its own
  scope boundary.
- That the `pairwise_stability` metric's Abstain-Abstain-counts-as-
  agreement convention is safe in general — only that it was not the
  mechanism behind this run's own headline numbers (2 of 27
  raw-unstable groups, measured directly).
- Anything about E4/E5/E6, R1b, or production compilation of E2c's
  descriptors into `commerce_core`'s real serving path — none
  authorized or attempted here.

## What would be built next if continuing this thread

1. **A materially larger stability sample for the single-run design
   specifically** — 5 runs/configuration is what E2b's own stability
   rerun already produced; a genuinely larger N (matching E2b's own
   85.60%→87.60% precedent for why small-N estimates deserve a larger
   retest) would narrow the confidence interval on whether 95.20% is a
   stable estimate or noise around a number that might clear 98% with
   more data.
2. **A dataset or fixture that genuinely exercises R3/R4** — a real
   catalog with more Enum-vs-Numeric boundary fields than WANDS's own
   36-key sample happened to contain, so R3's real (not merely
   architectural) contribution can actually be measured rather than
   inferred from a single, vote-concordant case.
3. **A genuine test of Treatment D** — a dataset or fixture where a
   real structural role sits in D's own 34-50% contested zone, so its
   stricter-abstention safety claim can be measured rather than assumed.
4. **Resolve which reading (ensemble vs single-run) should govern
   production deployment cost/architecture decisions** before treating
   either the 100% or the 95.20% number as the "real" answer — this is
   a genuine open architectural question (does a real control plane run
   1 LLM pass per field or an ensemble of ~5?), not merely a measurement
   nuance.

## What should explicitly not be built yet

- Any production compilation of E2c's canonical descriptors into
  `commerce_core`'s actual serving path — E2c's own `CatalogIndex`
  builds remain experimental/evaluation-only, exactly matching E2b's
  own precedent.
- Any claim that Treatment C is "ready" based on the leave-one-out
  reading alone while ignoring the single-run reading this same
  checkpoint measured and disclosed.
- E4/E5/E6, R1b — unauthorized, not started, per Issue #42/#45's own
  scope boundaries.
- Any generic query DSL or document-schema abstraction — CLAUDE.md's
  standing prohibition applies with full force; zero new enum variants
  were added by this checkpoint, and none should be.
