# Issue #55 Priority 2 — promotion-gate evidence test on the full live candidate set (preregistration)

Written and committed to the branch **before** the evidence-scoring
script below is run against the full candidate set, per CLAUDE.md's
research discipline ("preregister treatments, metrics, thresholds,
splits and stop conditions before held-out results"). The evidence-
scoring logic itself has already been informed by one prior source
(disclosed under "Prior exposure" below); everything in "Thresholds"
is fixed now, before that script executes on the 149-group set.

## Governing question

`docs/experiments/ISSUE55_SEMANTIC_PROMOTION_LOG.md`'s own preliminary
probe tested exactly one candidate evidence source (top-level category
co-membership) on a hand-picked 2-group subset (`beds`, `recliners`,
8 total pairs) and found it clears the zero-false-promotion safety bar
but recovers only 1/6 (17%) of non-known-bad candidates -- too low to
be a usable promotion rule alone. That probe named the natural next
step: **run on the full currently-live candidate set** (not a hand-
picked subset) and **test whether a deeper evidence source (category-
path overlap at 2 levels, not just top-level) recovers more recall
without reopening the false-promotion risk.**

## Prior exposure (disclosed, not hidden)

Before writing this protocol, `p9_e08_hyponym_group_false_family_audit`
was rerun against current production to confirm the live candidate-set
size (149 groups, 317 total broader/narrower pairs) and to eyeball the
highest-false-positive-risk subset (the 17 single-word-broader-term
groups, e.g. "beds", "desks", "benches", "coolers", "hooks",
"headboards", "sofas", "vanities", "bookcases", "ottomans",
"refrigerators", "sectionals", "slides", "dryers", "gliders",
"nightstands") to judge whether this experiment was tractable at all
before committing to it. None of those visually appeared to contain an
additional wrong-family pair beyond the two already-known-bad ones. This
was a scoping glance, not a full manual re-audit (that re-audit already
exists and is authoritative: `ISSUE55_HYPONYM_LEAF_ONLY_DECISION.md`'s
own re-run of `p9_e08` after the leaf-only fix, which is the ground
truth this experiment uses -- see below). No threshold in this document
was chosen after seeing what value it would produce; they are fixed
before the path-overlap scoring script runs.

## Ground truth (established by prior, already-adversarially-reviewed checkpoints, not invented here)

- `docs/decisions/ISSUE55_HYPONYM_LEAF_ONLY_DECISION.md`'s own disclosed,
  still-unfixed residual risk, confirmed via a real-vocabulary re-audit
  after the leaf-only fix: `"beds" -> "cat beds"` and
  `"beds" -> "dog beds & mats"` are the only two confirmed cross-family
  false positives surviving current production. No other group in the
  current 149-group set has a confirmed false positive.
- `KNOWN_BAD = {("beds", "cat beds"), ("beds", "dog beds & mats")}`.
  This experiment does not attempt to discover new false positives (that
  is `p9_e08`'s job, already done); it tests whether a candidate
  evidence source can distinguish known-bad from everything else,
  using this pre-established ground truth.

## Data source

`dataset_cache/wands/product.csv`'s real `product_class` and
`category hierarchy` fields (tab-delimited), exactly as
`scripts/research/i55_promotion_evidence_probe.py` already loads them.
Candidate set: `docs/research/artifacts/i55_promotion_gate_full_set/candidate_set.json`,
exported by the new `i55_hyponym_candidate_set_export` binary
(`crates/phase9-eval/src/bin/i55_hyponym_candidate_set_export.rs`),
which calls the exact same production `product_type_hyponym_groups`
function `p9_e08` audits -- not a hand-transcribed or re-derived list.

## Method (two evidence sources scored, both preregistered)

For each `(broader B, narrower N)` pair in the full candidate set: look
up every real WANDS product whose raw `product_class` field equals B
(respectively N, case-insensitively), and collect each such product's
own `category hierarchy` field.

- **Evidence source 1 (replicated from the preliminary probe, at full
  scale)**: `top_level(path) = path.split("/")[0].strip().lower()`.
  `PROMOTE` if B's and N's top-level sets overlap and both have >=1 real
  product; `UNRESOLVED` otherwise.
- **Evidence source 2 (new)**: `level_2(path) = the first two "/"-
  delimited segments, each trimmed and lowercased, joined by " / "`.
  `PROMOTE` if B's and N's level-2 sets overlap and both have >=1 real
  product; `UNRESOLVED` otherwise. This is strictly a narrower/harder
  test than evidence source 1 (matching 2 segments is at least as
  specific as matching 1), so it can only ever promote a subset of what
  evidence source 1 promotes for a fixed pair -- any recall change is
  therefore attributable to the added specificity, not an unrelated
  scoring difference.
- A pair with zero raw `product_class` matches for either B or N is
  reported honestly as `UNRESOLVED (no evidence)`, matching the
  preliminary probe's own convention -- several narrower names in
  `product_type_hyponym_groups`' own output are ancestor-breadcrumb
  path-fallback strings that never appear as a literal `product_class`
  value, not a defect in the evidence source.

## Thresholds (fixed now, before the script runs)

1. **Safety gate (must pass for either evidence source to be usable at
   all)**: zero false promotions on `KNOWN_BAD` -- both known-bad pairs
   must stay `UNRESOLVED` under both evidence sources. Any false
   promotion is an automatic REJECT of that evidence source, full stop,
   regardless of recall.
2. **Recall bar**: among all *resolvable* candidates (both B and N have
   >=1 real `product_class` row, i.e. evidence exists at all, excluding
   the 2 known-bad pairs from the denominator), evidence source 2 must
   promote **>=50%** to be judged a usable improvement over evidence
   source 1 alone -- the same bar the preliminary probe's own "next
   question" section named.
3. **Comparative claim**: evidence source 2 (level-2 overlap) is judged
   to have *added value* over evidence source 1 (top-level overlap) only
   if it promotes a **strictly larger** count of non-known-bad resolvable
   candidates at the full-set scale, not just on the original 2-group
   subset.

## Stop condition

Run the scoring script exactly once at both evidence-source
granularities, on the full 149-group / 317-pair set, and record
whatever the resulting counts are as GO / REVISE / REJECT per the
thresholds above -- no threshold changes after seeing the output.

## What this experiment does NOT attempt

- Not a third evidence source (catalog co-occurrence via structural-
  routed candidate sets) or a multi-source-agreement rule -- both remain
  named, unimplemented next steps if this one clears its own bars.
- Not a re-audit for new false positives beyond `KNOWN_BAD` -- that is
  `p9_e08`'s job and is treated as already done and authoritative here.
- Not a production wiring change. `product_type_hyponym_groups` and its
  callers are untouched; this experiment only evaluates a candidate
  *promotion evidence source* for a mechanism (promotion gating) that
  does not exist in production yet.
