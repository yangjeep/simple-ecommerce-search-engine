# Issue #51 Preregistered Protocol — is the Punt-path delegate cost inherent, or a fixable treatment-side inefficiency?

## 0. What this is testing

`docs/decisions/ISSUE51_FULLGATE_SCALE_DECISION.md` found Treatment D/E
still fail R1/Issue #42's preregistered `<=5%` full-gate overhead bar at
realistic (~43,000-product) catalog scale, and precisely localized the
cause: row 1 ("size 22", genuinely ambiguous with no corroborating
entity constraint) costs Treatment A ~0.0002ms (a single near-zero-cost
structural lookup, `candidates=[1]`) but Treatment D/E ~0.0099-0.0104ms
(a real `ExecutionOutcome::Punt` delegate query against the full
42,995-product Tantivy index, `candidates=[42995]`) — a ~50x gap that
accounts for essentially the entire measured full-gate overhead. That
decision named this as a distinct, unaddressed next question rather
than pursuing it.

This checkpoint asks: **is that ~50x gap inherent to correctly
delegating to real lexical search when structural evidence is
genuinely insufficient to disambiguate (meaning no correctness-preserving
treatment could ever clear the `<=5%` bar on a workload containing even
one such row), or does it contain a fixable treatment-side
inefficiency** (e.g. redundant `compile()` calls, unnecessary
`CommerceQuery`/`Resolution` construction, oversampling) **on top of the
delegate's own real, necessary cost?**

Direct code inspection (`crates/commerce-core/src/plan/mod.rs`'s `Punt`
arm) shows the delegate is already **not** oversampled for this exact
case: an existing, disclosed optimization (Issue #6 P1-D / P2-E16)
already sets `limit = k` (not `k * delegate_oversample`) whenever
`query.constraints.is_empty()` — row 1's exact situation after
Treatment C/D/E's demotion. `identifier_hits` (called before the
delegate, an O(matches) `identifier_lookup` dictionary probe) is a
plausible but unconfirmed second contributor. This protocol measures
directly rather than assumes.

## 1. Hypothesis

**H0**: the isolated cost of exactly the two real operations row 1's
`Punt` outcome performs (`identifier_hits` + `BitmapTantivyDelegate::search`
with `terms=["22"], restrict_to=None, limit=10`), called directly with
no surrounding `resolve_e`/`execute_planned`/`plan` machinery, accounts
for **>=80%** of Treatment E's measured row-1 cost (0.01043ms, this
session's own most recent run). If true: the gap is dominated by the
delegate's own inherent, necessary cost of a real lexical fallback, not
by a fixable treatment-side inefficiency — meaning R1's own `<=5%`
overhead bar, applied to a workload mix containing a genuinely
ambiguous, uncorroborated query, is **structurally unclearable by any
correctness-preserving treatment**, independent of Issue #51's own
registry optimization or any other Treatment-D/E-specific mechanism.

**H1 (negation)**: the isolated floor accounts for **<50%** of the
measured cost, meaning a material, currently-unidentified treatment-side
overhead (redundant compilation, allocation, or similar) sits on top of
the delegate's real cost — a genuine, fixable optimization opportunity,
named (and fixed here if the fix is small and low-risk; named as a
follow-up otherwise if it requires a larger change).

**Ambiguous zone (50-80%)**: report both contributions; no clean
KEEP/REJECT of either hypothesis, but still informative (partial
inherent cost, partial fixable overhead).

## 2. Baseline / dataset / treatment

Baseline: current branch HEAD (post checkpoint 11's `ProductTypeAny`
revert). Dataset: the exact same synthetic-scale catalog construction
already used and validated in
`crates/issue42-eval/src/bin/r1_full_gate_scale_rerun.rs` (5 real R1
fixture products + 42,990 inert decoys, approximating real WANDS
scale) — reused verbatim, not rebuilt, so the comparison is apples to
apples against that checkpoint's own numbers. No new dataset
acquisition needed.

Treatment: a new, measurement-only binary
(`crates/issue42-eval/src/bin/r1_punt_cost_floor.rs`) that, using the
same `BitmapTantivyDelegate` and `CatalogIndex` already built for the
full-gate rerun:

1. Re-measures Treatment A's and Treatment E's row-1 latency exactly as
   `r1_full_gate_scale_rerun.rs` does (same `LATENCY_BATCH=200`,
   `LATENCY_TRIALS=7`, median-of-medians methodology), as a same-process
   sanity check that this run reproduces the prior checkpoint's
   magnitude before trusting any new isolated measurement.
2. Measures the **isolated floor**: `index.identifier_lookup("22")`
   (looped, mirroring `identifier_hits`'s own logic, called directly —
   both are public API) followed by
   `delegate.search(&["22".to_string()], None, 10)`, timed with the
   identical batching/trial methodology, with no `CommerceQuery`,
   `compile()`, `resolve_e`, `execute_planned`, or `plan()` call
   anywhere in the timed region.

No `commerce-core` production code is expected to change for the
measurement itself. If H1 is confirmed and the identified overhead is a
small, low-risk, test-covered fix, it may be implemented in this same
checkpoint (per CLAUDE.md's "smallest experiment" discipline) rather
than deferred; if it requires a larger change, it is named only, not
implemented.

## 3. Metrics / gates

- **Reproduction gate (checked first)**: Treatment A and Treatment E's
  freshly re-measured row-1 costs must be within the same order of
  magnitude as `ISSUE51_FULLGATE_SCALE_DECISION.md`'s own recorded
  range (A: ~0.0001-0.0003ms; E: ~0.0097-0.0104ms) — confirms this new
  binary's own measurement methodology is consistent with the prior
  checkpoint's before any new floor number is trusted.
- **Decision gate**: isolated floor / measured Treatment E row-1 cost,
  as a percentage — >=80% confirms H0 (STOP/REFRAME the `<=5%` gate
  question for this workload as inherently unclearable, closing this
  thread), <50% confirms H1 (name or fix the overhead), 50-80% reports
  both.
- **No correctness gate needed**: this checkpoint is measurement-only
  unless H1 is confirmed and a fix is implemented, in which case that
  fix must pass `cargo test --workspace --all-features` with zero new
  failures and, if it touches `commerce-core`, a direct regression test
  proving no behavior change (output-equivalence to the pre-fix path).

Repetitions: the same 3-independent-full-run discipline
`ISSUE51_FULLGATE_SCALE_LOG.md` used is not required here, since this
measurement isolates a single, already-characterized code path rather
than introducing a new one — one run with the established
LATENCY_BATCH/LATENCY_TRIALS median-combining methodology is sufficient,
with a second confirmation run if the first result falls near either
decision threshold (within 10 percentage points of 50% or 80%).
