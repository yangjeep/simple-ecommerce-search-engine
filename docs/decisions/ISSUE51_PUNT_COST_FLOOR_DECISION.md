# Issue #51 — is the Punt-path delegate cost inherent, or a fixable treatment-side inefficiency?

Log: `docs/experiments/ISSUE51_PUNT_COST_FLOOR_LOG.md`. Protocol:
`docs/experiments/ISSUE51_PUNT_COST_FLOOR_PROTOCOL.md`.

## Verdict: H0 CONFIRMED — the cost is inherent to correctness, not a fixable inefficiency; this closes the follow-up `ISSUE51_FULLGATE_SCALE_DECISION.md` named

`docs/decisions/ISSUE51_FULLGATE_SCALE_DECISION.md` found Treatment D/E
still fail R1/Issue #42's `<=5%` full-gate overhead bar at realistic
catalog scale, localized to row 1's ("size 22", genuinely ambiguous, no
corroborating entity) `Punt`-path delegate query, and named — but did
not pursue — the question of whether that ~50x cost gap is a fixable
treatment-side inefficiency.

It is not. Isolating exactly the two real operations `execute_planned`'s
`Punt` arm performs for this row (`index.identifier_lookup` +
`BitmapTantivyDelegate::search`), called directly with zero surrounding
`CommerceQuery`/`compile()`/`resolve_e`/`execute_planned`/`plan()`
machinery, reproduces **94.3%-98.8%** of Treatment E's measured row-1
cost across two independent runs — both comfortably clear of the
preregistered >=80% H0 threshold. Essentially the entire gap is the
delegate call's own real cost, not decision-mechanism overhead.

**A supplementary, unplanned finding makes this more decisive, not
less**: the zero-hit `"22"` case *understates* a real match's cost by
45.6x (a genuine large-posting-list match, `"decoy"`, costs 0.427ms vs.
the zero-hit floor's 0.009ms). Row 1's own measured ~50x overhead vs.
Treatment A is therefore a *conservative* example of what correctly
delegating to lexical search costs, not a worst case — real traffic
whose residual term actually matches catalog text would cost
substantially more, not less, than what this checkpoint measured.

## What this means for the R1/Issue #51 overhead-gate line

This is not new evidence that Treatment D/E's mechanism is broken —
`ISSUE51_DECISION.md`'s asymptotic-advantage finding and
`ISSUE51_FULLGATE_SCALE_DECISION.md`'s correctness findings both stand
unchanged. What this closes is a different, more precise question:
**R1's own `<=5%`-overhead-vs-Treatment-A bar cannot be cleared by any
correctness-preserving treatment on a workload that includes even one
genuinely ambiguous, uncorroborated query**, because:

1. Treatment A's cheapness on such a row comes specifically from *not*
   paying for a real search — it keeps a single overconfident hard
   constraint (`candidates=[1]`) without ever consulting the lexical
   delegate.
2. Any treatment that correctly recognizes the ambiguity (nothing in
   the catalog corroborates one interpretation over another) has no
   choice but to fall back to a real lexical query — and a real lexical
   query, even in the best case measured here (zero matches), costs
   ~50x more than a single near-zero-cost structural lookup. In the
   worst case measured here (a genuine match), it costs ~2,300x more.
3. This is not an implementation defect: `BitmapTantivyDelegate` reuses
   a persistent `IndexReader` (no per-query reopen), and
   `execute_planned`'s `Punt` arm already avoids oversampling for this
   exact case (`limit = k`, not `k * delegate_oversample`, whenever
   `query.constraints.is_empty()` — a real, disclosed, pre-existing
   optimization from Issue #6 P1-D / P2-E16). There is no remaining
   slack to optimize away without abandoning correctness itself.

**The `<=5%` bar, as literally defined, was measuring something no
correctness-preserving design can pass whenever the workload contains
this row shape** — not a property of Issue #51's registry mechanism,
Treatment D's corroboration logic, or any other mechanism this session
has touched. Continuing to optimize Treatment D/E's own machinery
cannot close this gap; the gap is the price of correctness itself.

## Why this does not reopen `ISSUE51_DECISION.md`'s "terminal state" note

That note closed the question "does Treatment E's registry mechanism
ever clear the full gate" (REVISE, confirmed, not reopened here). This
checkpoint answers a narrower, distinct question `ISSUE51_FULLGATE_SCALE_DECISION.md`
itself named as still open and unpursued: "is the *specific* cost that
blocks the gate fixable." Answering that with "no, it's inherent" is a
close-out, not a reopening — it forecloses a plausible-sounding future
optimization attempt (chase the Punt-path cost down) with a direct
measurement, rather than leaving it as a dangling, unverified
possibility for a future checkpoint to rediscover from scratch.

## What a real fix would require (named, not pursued)

If a future need arises to bring Treatment D/E's *aggregate* overhead
number closer to a fixed ceiling, the honest options are architectural,
not implementation-level: (a) redefine the gate to measure overhead
only across rows where corroboration is structurally possible (rows
2/3/6 in this fixture, which the earlier per-row breakdown already
showed are cheap for D/E — microseconds regardless of treatment), and
report the `Punt`-path cost for genuinely ambiguous rows as a separate,
expected-to-be-nonzero "cost of correctness" line item rather than
folding it into a single pass/fail percentage; or (b) accept that this
specific `<=5%`-vs-baseline framing is the wrong lens for a mechanism
whose entire value proposition is choosing correctness over a cheap
guess, and evaluate it instead on precision/recall-per-dollar terms.
Neither is implemented here — this checkpoint's job was to determine
whether the cost was fixable, not to redesign the gate.

## Adversarial review

- **Checked whether the isolated-floor comparison is apples-to-apples**:
  yes — the floor calls `index.identifier_lookup` and
  `BitmapTantivyDelegate::search` directly via the same public API
  `execute_planned`'s `Punt` arm calls internally, against the
  identically-constructed catalog/index/delegate instances (not rebuilt
  or reconfigured differently), with the identical arguments
  (`terms=["22"]`, `restrict_to=None`, `limit=10`, matching the Punt
  arm's own non-oversampled-when-`constraints.is_empty()` behavior).
- **Checked whether the zero-hit result for `"22"` could mean the
  isolated call is silently doing less work than the real Punt call
  (e.g., short-circuiting before reaching the delegate)**: no — the
  isolated call *is* the delegate call, not a proxy for it; a
  short-circuit could only happen inside `BitmapTantivyDelegate::search`
  itself, which would affect the production Punt-path call identically,
  since both invoke the exact same method on the exact same object.
  The supplementary `"decoy"` measurement additionally shows the
  delegate does real, variable-cost work depending on match count (not
  a constant-cost short-circuit for every call).
  Confirmed directly: `identifier_lookup("22")` returns 0 (not a
  registered identifier, expected) and `delegate.search(["22"], ...)`
  returns 0 hits because no document's tokenized title/description
  contains the standalone token `"22"` in this synthetic fixture (decoy
  titles embed it only inside a larger numeric token like `"1000022"`,
  which Tantivy's tokenizer does not split) — a real term-dictionary
  miss, not a data-construction bug.
- **Checked whether two runs (94.3%, 98.8%) is sufficient**: both land
  14-19 percentage points clear of the 80% threshold, well outside the
  protocol's own "run a third confirmation only if within 10pp of a
  threshold" trigger — no further reproduction needed for a decision
  this unambiguous.
- **Checked whether the reproduction-gate numbers this run measured
  (A: 0.00018-0.00022ms, E: 0.00901-0.00993ms) are consistent with the
  prior checkpoint's own recorded range** (A: ~0.0001-0.0003ms, E:
  ~0.0097-0.0104ms): yes, same order of magnitude both runs, confirming
  this binary's methodology before any new floor number was trusted.

## What this does and does not change

- **Does not change** any correctness or NDCG finding from
  `ISSUE51_DECISION.md` or `ISSUE51_FULLGATE_SCALE_DECISION.md`.
- **Does not modify `commerce-core` production code** — this is a
  measurement-only checkpoint; no fix was found to implement (H1 was
  falsified, not confirmed).
- **Closes** `ISSUE51_FULLGATE_SCALE_DECISION.md`'s own named next
  step with a definitive negative result: the Punt-path cost is not a
  fixable inefficiency, so no further checkpoint should spend effort
  trying to optimize it down under the current architecture.
- **Names, without implementing**, the one remaining actionable
  direction on this thread: redefining what the `<=5%` gate measures
  (per-row-class overhead, or a precision/cost tradeoff framing) rather
  than continuing to treat it as a single pass/fail bar Treatment D/E
  must clear in aggregate.

## Traceability

Source: `crates/issue42-eval/src/bin/r1_punt_cost_floor.rs` (new,
measurement-only). Raw evidence:
`docs/research/artifacts/i51_punt_cost_floor/run0_initial_no_decoy_comparison.txt`
(98.8%) and `run2_with_decoy_comparison.txt` (94.3%, adds the `"decoy"`
supplementary measurement) — two independent runs, both decisive.
