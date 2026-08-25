# Issue #51 — R1's full gate at realistic catalog scale

Log: `docs/experiments/ISSUE51_FULLGATE_SCALE_LOG.md`. Protocol:
`docs/experiments/ISSUE51_FULLGATE_SCALE_PROTOCOL.md`.

## Verdict: REVISE (confirmed, not resolved) — this closes the named follow-up with a negative but well-understood answer

`docs/decisions/ISSUE51_DECISION.md` named an explicit next step: rerun
R1's full `<=5%` `execute_planned` overhead gate (not just the isolated
corroboration-decision cost) at realistic catalog scale, since R1's own
5-product fixture could not demonstrate a mechanism whose value
proposition is asymptotic. That rerun is now done, at ~43,000 products
(approximating this project's real WANDS scale), across 3 independent
full runs.

**Result: Treatment E still does not clear the `<=5%` bar** — 14.2%,
14.0%, 16.7% overhead across the three runs, essentially unchanged from
R1's own original N=5 measurement (5.9%-14.5%). Treatment D likewise:
17.4%, 12.9%, 18.9%, vs. R1's original 8.0%-17.5%. Correctness is
intact and matches R1's own original result exactly (NDCG@10=1.0000,
zero wrong-family false positives) once two methodology defects
(described below) were found and fixed.

**This is not what Issue #51's own asymptotic argument predicted**, and
that is the valuable, disclosed negative result here: the isolated
mechanism Issue #51 optimized (`constraint_kind_registered_on_product_type`'s
O(catalog) scan, replaced by an O(1) registry lookup) is real,
independently confirmed, and still cheap at this scale (rows 2/3/6,
the only rows that actually exercise it, cost microseconds regardless
of treatment). But it was never the dominant cost in the full gate, at
any scale tested. A per-row latency breakdown found the actual driver:
row 1 ("size 22", genuinely ambiguous with nothing to corroborate it)
costs ~50x more per call under Treatments D/E (which correctly route it
to `Punt`, the lexical delegate) than under Treatment A (which naively
picks one interpretation as a cheap single-candidate hard constraint).
That delegate-query cost — not the corroboration-decision mechanism —
accounts for essentially the entire measured overhead, and Issue #51's
registry optimization was never going to touch it.

## Why REVISE, not GO or STOP

GO would misrepresent a gate that still fails, at scale, by a wide
margin (12.9%-18.9%, not a marginal miss) as resolved. STOP would
overstate the negative result: the corroboration-decision mechanism
itself (Issue #51's actual subject) is proven correctness-preserving
and cheap at any scale tested (this experiment, `i51_e00`, and R1's own
5-product baseline all agree); nothing here falsifies Treatment D/E's
own core approach or Issue #42's broader typed-ambiguity thesis. What
fails is a *different*, now-precisely-located cost (the `Punt` fallback's
delegate-query overhead for genuinely ambiguous, uncorroborated queries)
that this specific gate's own design (measuring against Treatment A's
naive, cheaper-but-wrong baseline) was always going to expose. REVISE —
the mechanism is sound, the full-system overhead question is answered
(no, scale does not resolve it, for a different reason than expected)
— matches what was actually found.

## Two methodology defects found and fixed before trusting any number

1. Decoy products initially shared product types 1/2/3 with a
   `"size"` enum attribute (matching `i51_e00_catalog_scale_diagnostic.rs`'s
   own shape, which is valid for measuring `resolve_d`/`resolve_e` in
   isolation but not for the full gate): `CatalogProfile::build`
   indexes attribute values from every product regardless of type,
   so decoys polluted the real rows' own lexicon vocabulary, collapsing
   NDCG and changing routing outcomes. Fixed with genuinely inert
   decoys (distinct, unregistered product type/brand/category, zero
   attributes).
2. Decoys were then priced at $10.00, which satisfies "under $34"
   (rows 4/5's regression-guard queries), turning row 4 into an
   accidental 42,994-candidate scan that dominated ~97% of total
   latency identically across every treatment — not biasing the
   direction of the comparison, but drowning out the actual
   treatment-dependent signal in noise. Fixed by pricing decoys at
   exactly the $34.00 threshold, invisible to both strict-inequality
   regression guards.

Both are disclosed in the log with root cause, not silently corrected.
Per this project's own discipline, an experiment that produces a
surprising or exactly-mirror-opposite-of-expected result before
methodology defects are ruled out is not yet trustworthy — both were
caught by directly investigating *why* a result was surprising (D/E
measuring faster than baseline; then D/E's aggregate being dominated by
one shared row) rather than accepting favorable- or unfavorable-looking
numbers at face value.

## What this does and does not change

- **Does not reverse** `ISSUE51_DECISION.md`'s finding that Treatment E
  is correctness-preserving and has a real, large asymptotic advantage
  over Treatment D in the specific mechanism it targets. That claim was
  never about the full-system gate and remains independently supported.
- **Closes** `ISSUE51_DECISION.md`'s own named open item (rerun the full
  gate at realistic scale) with a definitive, reproduced answer: REVISE
  is confirmed, not an artifact of R1's small fixture.
- **Redirects** where future work on this thread should look: the
  delegate-query cost for `Punt`-routed, genuinely-ambiguous queries
  (row 1's own pattern), not the corroboration-decision mechanism, is
  the actual remaining cost standing between Treatment D/E and a GO on
  the full `<=5%` gate. This is a distinct, new, and more precisely
  targeted question than Issue #51 itself asked — named here as a
  candidate follow-up, not pursued in this measurement-only checkpoint.
- **No `commerce-core` production code changed.** All changes are in
  `crates/issue42-eval/src/bin/r1_full_gate_scale_rerun.rs` (new
  diagnostic binary).

## Adversarial review

- **Checked whether the two found methodology defects could still be
  contaminating the final numbers**: no — both are structurally ruled
  out by construction (decoys are provably absent from the lexicon
  profile and from both price regression-guard rows' candidate sets,
  confirmed via the printed per-row `candidates=[...]` diagnostic
  showing exactly 4 candidates for "under $34", not 42,994).
- **Checked whether the row-1 explanation is the true cause or a
  coincidental correlation**: the per-row breakdown isolates row 1's own
  cost directly (not inferred from an aggregate), and its magnitude
  (~0.0097-0.0102ms) closely matches the measured aggregate overhead
  gap in absolute terms, not just in sign.
- **Checked whether 3 runs is sufficient given the two prior corrections
  already found**: given both prior defects were large, qualitative,
  easy-to-spot effects (NDCG collapse; a single row dominating 97% of
  latency) rather than marginal measurement noise, and the post-fix
  results are consistent in magnitude and direction across all 3 runs,
  no further methodology defect is suspected; this matches the
  reproduction-count precedent this project's other Issue #55
  checkpoints have used for less contested results.

## Traceability

Source: `crates/issue42-eval/src/bin/r1_full_gate_scale_rerun.rs` (new,
diagnostic only). Raw evidence:
`docs/research/artifacts/i51_fullgate_scale_rerun/run{1,2,3}.txt`.
