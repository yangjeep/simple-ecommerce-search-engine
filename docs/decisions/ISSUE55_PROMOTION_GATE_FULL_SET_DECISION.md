# Issue #55 Priority 2 — full-set promotion-gate evidence test: decision

Full log: `docs/experiments/ISSUE55_PROMOTION_GATE_FULL_SET_LOG.md`.
Preregistration: `docs/experiments/ISSUE55_PROMOTION_GATE_FULL_SET_PROTOCOL.md`.
Raw output: `docs/research/artifacts/i55_promotion_gate_full_set/run1.txt`.

## Governing question

`ISSUE55_SEMANTIC_PROMOTION_LOG.md`'s preliminary probe tested top-level
category overlap on a hand-picked 2-group subset (8 pairs) and found it
safe (zero false promotions) but low-recall (1/6). It named two next
steps: run on the *full* live candidate set, and test whether a deeper
evidence source (category-path overlap at 2 levels) recovers more
recall.

## Result: REJECT both evidence sources as standalone promotion rules; confirms and quantifies a coverage gap, not just a recall gap

On the full candidate set (149 groups, 317 broader/narrower pairs,
exported directly from the live `product_type_hyponym_groups` function,
not hand-picked):

- **Safety gate holds at full scale**: zero false promotions for either
  evidence source across all 317 pairs, including both known-bad pairs
  (`"beds" -> "cat beds"`, `"beds" -> "dog beds & mats"`). This
  reproduces, and now generalizes past, the preliminary probe's own
  finding.
- **Both evidence sources fail the preregistered >=50% recall bar** by
  a wide margin: top-level overlap 30.6% (15/49), level-2 overlap 22.4%
  (11/49).
- **The comparative sub-question ("does deeper overlap recover more
  recall") was not well-posed** and is withdrawn as a finding, not
  reported as a negative result about category-hierarchy depth: `level_2`
  requires strictly more than `top_level` (both segments must match, not
  just the first), so its promoted set is mathematically a subset of
  `top_level`'s for any fixed data -- it can only tie or lose, never win.
  This should have been caught while designing the protocol, not after
  running it; disclosed as a design flaw in this checkpoint's own
  hypothesis, not smoothed over.
- **The dominant, genuinely new finding**: category-hierarchy overlap,
  at *any* segment depth, can only ever adjudicate 16.1% of the full
  candidate set (51/317 pairs where both the broader and narrower name
  have real `product_class`-matched data at all). The other 83.9% are
  permanently `UNRESOLVED (no evidence)` regardless of how the overlap
  rule itself is tuned -- driven mostly (70.7% of all pairs) by narrower
  names that are ancestor-breadcrumb path-fallback strings rather than
  literal `product_class` values a real product carries (an already-
  disclosed WANDS ingestion fact, `ISSUE55_PRODUCT_CLASS_INGESTION_DECISION.md`,
  now shown to bind hard on this specific downstream use).

## Verdict: REJECT (category-hierarchy overlap alone, at any depth, is not a usable promotion rule) with a concrete, quantified reason why

Per the preregistered thresholds, both evidence sources fail the recall
bar, so neither clears the bar to be adopted alone. This is the safe
failure mode per the governing task's own severity asymmetry (an
`UNRESOLVED` verdict falls back to today's safe non-promotion, not a
wrong hard filter) -- consistent with, not contradicted by, this
session's other findings that graceful fallback (Hybrid routing,
`ISSUE55_PAIRED_COMPARATOR_DECISION.md`) is the architecture's own
recurring strength.

Crucially, this checkpoint reframes *why* recall is low: it is
overwhelmingly a **coverage** problem (no evidence exists for most
pairs under this evidence-source family at all), not a **precision/
threshold** problem (tuning overlap depth). This materially changes
what a productive next evidence source needs to do -- it is not enough
to refine how category-hierarchy overlap is measured; a workable source
needs to produce evidence for narrower names that never appear as a
literal `product_class` value in the first place.

## Real caveats, disclosed rather than smoothed over

- **n=51 resolvable pairs, n=2 known-bad** is still a small evidence
  base for the safety-gate claim, even though it is 6x larger than the
  preliminary probe's. A single-digit false-promotion count would look
  very different in percentage terms; "zero false promotions on 2 known
  cases" is meaningfully short of a statistical guarantee.
- **`KNOWN_BAD` is inherited, not re-derived.** This experiment did not
  re-run a fresh false-family audit against the full 317-pair set; it
  trusts `ISSUE55_HYPONYM_LEAF_ONLY_DECISION.md`'s own re-audit as
  ground truth. If that re-audit missed a case, this experiment would
  not catch it (it tests discrimination given known labels, not
  discovery of new labels).
- **The comparative sub-question's a-priori-answerable nature is a
  genuine process lesson**: a preregistered comparison should have been
  checked for logical coherence (does the null hypothesis structure even
  allow the tested direction to occur) before being written into the
  protocol, not just before being run.

## What this does NOT establish

- Not a claim that no category-hierarchy-based signal could ever work --
  only that the specific overlap-on-`product_class`-matched-rows
  construction used here and in the preliminary probe is
  coverage-limited on this catalog.
- Not a claim about the other three candidate evidence sources the
  preliminary probe named (catalog co-occurrence, multi-source
  agreement) -- untested here.
- Not a production change. `product_type_hyponym_groups` and its
  callers remain untouched; no promotion mechanism exists in production.

## Next question (named, not implemented here)

The coverage finding directly motivates a different kind of evidence
source: instead of matching a narrower name against its own (frequently
absent) literal `product_class` rows, use the narrower name's own
**ancestor-breadcrumb structure** (it was very likely synthesized *from*
a real `category hierarchy` path during ingestion in the first place,
per `ISSUE55_PRODUCT_CLASS_INGESTION_DECISION.md`) directly as the
overlap evidence, rather than re-deriving it through a second
`product_class` lookup that frequently fails. This could plausibly
close most of the 70.7%-narrower-side coverage gap without touching the
broader-side gap (13.2%, a smaller, separate problem). Sized for a
follow-up checkpoint, not implemented here.
