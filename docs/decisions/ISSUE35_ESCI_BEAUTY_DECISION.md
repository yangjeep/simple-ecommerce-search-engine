# Issue #35 — third unseen-vertical slice: real ESCI beauty/personal-care data

Log: `docs/experiments/ISSUE35_ESCI_BEAUTY_LOG.md`. Protocol:
`docs/experiments/ISSUE35_ESCI_BEAUTY_PROTOCOL.md`.

## Verdict: H0 CONFIRMED — a third independent real vertical replicates the finding; Issue #35's "≥3 materially different verticals" goal is met for this slice of the epic

Two prior checkpoints (`docs/decisions/ISSUE35_ESCI_ELECTRONICS_DECISION.md`,
`docs/decisions/ISSUE35_ESCI_AUTOMOTIVE_DECISION.md`) found this
project's discovery/serving pipeline generalizes safely to real
electronics and automotive-parts verticals with zero `commerce-core`
changes. This checkpoint tests a third, materially different one --
real Amazon beauty/personal-care listings (2,093 products, 600
queries) -- and the same result holds:

1. **Zero `commerce-core` changes** -- the same shared measurement code
   (`issue35_eval::eval::run_vertical_eval`, now exercised on its third
   distinct vertical) ran unmodified.
2. **Zero unsafe/wrong-family matches** across 46 `Brand`-constrained
   queries.
3. **Relevance within bounds again, a third distinct
   direction/magnitude**: native NDCG@10 0.4162 vs. Solr's 0.4220
   (-1.38%). Across the three verticals now measured: electronics
   +8.93%, automotive -2.55%, beauty -1.38% -- clustered near parity
   rather than at one extreme in either direction, which is itself
   evidence against a systematic bias in either the measurement or the
   architecture (a consistently-favorable or consistently-unfavorable
   pattern would be more suspicious than this near-parity scatter).
4. **Brand-collision risk checked a third time, absent again**: 0/1,231
   discovered brands collide with the stopword list (electronics:
   1/1,079; automotive: 0/502) -- three data points now, all consistent
   with the original finding being rare and isolated rather than
   systemic.

**A genuine, disclosed new quirk, found by direct inspection**: the
query `"neutrogena naturals lotion"` resolved to a `Brand(Neutrogena)`
`AND` `color="Lotion"` conjunction (zero hits) because this catalog's
raw `product_color` field is reused for form-factor descriptors
("Lotion", "Cream", "Spray"), not just literal color, and the generic
attribute-indexing mechanism has no way to know that distinction from
the field name alone. This produces a safe null result, not a wrong
one -- consistent with, not a violation of, this project's
Product/Variant correctness discipline -- and is disclosed as a real,
minor coverage limitation on catalogs that overload a generic attribute
name for more than one real-world concept, rather than smoothed over.

## Why three, and why this specific stopping point

Issue #35's own text names "at least three materially different
verticals" as the Workstream D bar. Three real, independently-fetched,
independently-indexed verticals (electronics, automotive parts,
beauty/personal care) -- spanning consumer electronics, replacement
parts with fitment semantics, and cosmetics/skincare with almost purely
free-text distinguishing attributes -- now agree on the same
qualitative safety property while disagreeing on relevance direction
and brand-collision incidence, the signature this session has used
throughout to distinguish a real finding from an overfit one. This is
a natural, disclosed stopping point for *this* slice of Issue #35 (the
"no vertical-specific code" question at small-to-medium real scale) --
not a claim that Issue #35's full epic (methodology freeze, blind
retrospective replay, merchant-level heterogeneity, a cold-start
artifact) is complete.

## What this does and does not establish

- **Completes** the specific, named ">=3 materially different
  verticals" bar for the "does the pipeline need vertical-specific
  code" question -- the narrowest, most central falsifiable claim
  Workstream D poses.
- **Does not** run Workstream E (merchant-level heterogeneity within
  one vertical), Workstream F (a cold-start merchant-profile artifact),
  or Workstream C's formal blind-replay scoring rubric against
  historical WANDS/ESCI conclusions -- all remain open, named, not
  pursued here.
- **No `commerce-core` production code changed** across all three
  vertical checkpoints combined.

## Adversarial review

- **Checked whether three verticals drawn from the same source dataset
  (ESCI) undermine the "materially different" claim**: the three slices
  differ in the concrete way that matters for this architecture --
  structural signal availability and shape (electronics: some
  brand-driven structure; automotive: fitment-adjacent but still
  brand-only structurally; beauty: almost purely free-text, the
  overloaded `color` field being the one exception found) -- not merely
  in surface vocabulary. All three independently lack any
  product-type/category field, so the "no ontology fabrication"
  constraint is exercised identically and rigorously each time, not
  weakened by reusing one source.
- **Checked the qualitative sample for the same kind of surprising
  result the prior two checkpoints found**: found and disclosed the
  `color`-field-overloading quirk directly, rather than only reporting
  favorable examples.
- **Checked whether stopping at three (rather than continuing to a
  fourth) is arbitrary**: no -- three is the literal number Issue #35's
  own text names as the bar ("select at least three materially
  different verticals"), and the near-parity, non-clustering NDCG
  pattern across all three already provides the kind of independent,
  non-overfit confirmation additional verticals would mostly reinforce
  rather than newly reveal at this point.

## Traceability

Source: `crates/issue35-eval/src/bin/esci_beauty_eval.rs` (new, thin
wrapper reusing the shared measurement procedure). Dataset scripts:
`scripts/datasets/{fetch_esci_beauty.sh,filter_esci_beauty.py,solr_index_esci_beauty.py}`.
Raw evidence: `docs/research/artifacts/i35_esci_beauty/run1.txt`.
