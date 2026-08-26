# Issue #55 Priority 2 — full-set promotion-gate evidence test (append-only log)

Preregistration: `docs/experiments/ISSUE55_PROMOTION_GATE_FULL_SET_PROTOCOL.md`
(committed before this script ran). Raw output:
`docs/research/artifacts/i55_promotion_gate_full_set/run1.txt`. Candidate
set: `docs/research/artifacts/i55_promotion_gate_full_set/candidate_set.json`
(149 groups, 317 broader/narrower pairs, exported directly from the live
`product_type_hyponym_groups` production function).

## Entry 1 — ran the preregistered script once

`scripts/research/i55_promotion_gate_full_set_probe.py`, run exactly
once against the full candidate set:

```
--- top_level ---
  promoted=15 unresolved_no_overlap=36 unresolved_no_evidence=266
  false_promotions=[]  (safety gate: PASS)
  recall among resolvable non-known-bad candidates: 15/49 = 30.6%  (>=50% bar: FAIL)

--- level_2 ---
  promoted=11 unresolved_no_overlap=40 unresolved_no_evidence=266
  false_promotions=[]  (safety gate: PASS)
  recall among resolvable non-known-bad candidates: 11/49 = 22.4%  (>=50% bar: FAIL)

comparative claim: top_level=15 level_2=11 -- level_2 does NOT promote
strictly more (no added value over top_level at this scale)
```

## Entry 2 — a design flaw in the experiment's own hypothesis, caught after running, disclosed rather than hidden

The preregistered "does level-2 overlap recover *more* recall than
top-level overlap" comparison could only ever go one way, and this
should have been caught before running, not after: `level_2` requires
matching on 2 specific segments, a **strictly harder** condition than
`top_level`'s 1-segment match. For any fixed pair, a level-2 match
implies a top-level match (the first segment is shared whenever the
first two are), but not the reverse. This makes `level_2`'s promoted
set a subset of `top_level`'s promoted set by construction -- its
recall can only be lower or equal, never higher. The preregistered
protocol's own text even names this monotonicity ("this is strictly a
narrower/harder test... any recall change is attributable to the added
specificity") without drawing the obvious conclusion it implies. The
result (`level_2` recall 22.4% < `top_level` 30.6%) is therefore not a
new empirical finding about category-hierarchy depth -- it is a
restatement of set-subset arithmetic. Recorded honestly: this specific
comparative sub-question was not well-posed, not merely "answered
negatively."

## Entry 3 — the real, informative finding: coverage, not depth, is the binding constraint

A breakdown of the 266 "no evidence" pairs (script:
`python3 -c "..."`, not saved as a separate artifact since it is a
direct, reproducible recomputation from the same two input files):

```
both_present (resolvable):    51 (2 of which are the known-bad pair)
narrow-side missing only:    140
broad-side missing only:      42
both sides missing:           84
```

**70.7% of all 317 pairs (224/317) have no real `category hierarchy`
data available for the *narrower* name at all** -- consistent with, and
now quantified at full scale, this project's own already-disclosed fact
that many narrower names in `product_type_hyponym_groups`' own output
are ancestor-breadcrumb path-fallback strings (synthesized during
ingestion when a product's own `product_class` field was null or
pipe-delimited, per `ISSUE55_PRODUCT_CLASS_INGESTION_DECISION.md`), not
literal `product_class` values any real product carries. A smaller but
non-trivial 13.2% of pairs (42/317) are missing *broader*-side evidence
too, for the same underlying reason. **Category-hierarchy overlap, at
any segment depth, can only ever adjudicate the 16.1% of pairs (51/317)
where both sides have real product data at all** -- the remaining 83.9%
are permanently `UNRESOLVED (no evidence)` under this entire evidence-
source family, regardless of overlap-depth tuning.

See the decision doc for the verdict.
