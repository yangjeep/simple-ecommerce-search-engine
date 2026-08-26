# Issue #55 Priority 2 — semantic relation promotion architecture: preliminary evidence-source probe

No decision doc yet — this is a preliminary, exploratory probe, not a
completed falisification-loop round with a KEEP/REJECT/REFINE verdict.
Recorded here (append-only, per this project's own discipline) so a
future session can resume from real, verified findings rather than
starting Priority 2 from zero.

## Governing question (Issue #55 Priority 2)

`product_type_hyponym_groups` is a real candidate-relation *generator*
(strong aggregate recall effect, `ISSUE55_PRODUCT_TYPE_HYPONYM_DECISION.md`),
but it is wired directly into hard serving semantics with no
discovery/validation/promotion boundary between "plausible semantic
relation" and "safe to serve as a hard filter." The governing task asks:
what evidence can safely distinguish PROMOTE / REJECT / UNRESOLVED for a
candidate relation, with a promotion error treated as substantially more
serious than leaving a relation unresolved (safe lexical/hybrid
fallback)?

## What this probe actually did (and its real limitation, disclosed up front)

**This was NOT a preregistered experiment.** It was a fast, honest
scoping pass: before designing a formal promotion-gate test, get real
data in front of the question and see whether an obvious first
candidate evidence source (does the WANDS `category hierarchy` field
show top-level category co-membership between a candidate relation's
broader and narrower product-type names?) has *any* discriminating
power at all on real, currently-live data. The zero-false-promotion bar
itself comes directly from the governing task's own stated severity
asymmetry, not something invented after seeing results — but the
specific evidence source (top-level category overlap) was chosen,
implemented, and evaluated in one pass, with no held-out validation
set. Treat this as informing a real experiment's design, not as that
experiment.

**A real methodology trap was caught and avoided while doing this**:
the first attempt used ground truth from
`docs/research/artifacts/i55_product_type_hyponym/p9_e08_false_family_audit.txt`,
which predates checkpoint 14's leaf-only fix and reflects the OLD
full-ancestor-path mechanism's candidate set (e.g. it lists
`"hot tubs" -> "saunas"` as a confirmed false positive). Rerunning
`p9_e08_hyponym_group_false_family_audit` fresh against CURRENT
(leaf-only) production confirmed that candidate no longer exists at
all — leaf-only restriction already removed it, along with
`candles->diffusers` and `bed accessories->shower curtains`. Testing an
evidence source against stale ground truth would have been a real
"query-mix cherry-picking"/"baseline misconfiguration" error of exactly
the kind Issue #55's own adversarial-review checklist names. The
**currently live** candidate set (verified fresh, not assumed) is much
smaller: `{"beds": [12 narrower names, including the two still-disclosed
known-bad ones "cat beds"/"dog beds & mats"], "recliners": [2 narrower
names, both presumed-fine]}`.

## Method

For each candidate pair (broader name B, narrower name N): look up
every real WANDS product whose raw `product_class` CSV field equals B
(respectively N), collect the top-level segment of each such product's
own `category hierarchy` field, and check whether B's and N's top-level
segment sets overlap at all. `PROMOTE` if they overlap and both have
>=1 real product; `UNRESOLVED` otherwise (including when a candidate
name has zero raw `product_class` matches — several narrower names in
`product_type_hyponym_groups`' own output are actually
ancestor-breadcrumb-path fallback strings, not literal `product_class`
values, e.g. `"gray recliners"` never appears as a literal
`product_class`; this is treated honestly as "no evidence available,"
not silently worked around).

Script: `scripts/research/i55_promotion_evidence_probe.py`. Raw output:
`docs/research/artifacts/i55_promotion_evidence_probe/top_level_category_probe.txt`.

## Result

```
'beds'  -> 'adjustable beds'      n=63  top=bed & bath   -> UNRESOLVED
'beds'  -> 'cat beds'             n=1   top=pet          -> UNRESOLVED (correct: known-bad, correctly NOT promoted)
'beds'  -> 'dog beds & mats'      n=10  top=pet          -> UNRESOLVED (correct: known-bad, correctly NOT promoted)
'beds'  -> 'daybeds & guest beds' n=158 top=furniture    -> PROMOTE (matches broad's own top=furniture)
'beds'  -> 'kids beds'            n=183 top=baby & kids  -> UNRESOLVED
'beds'  -> 'teen beds'            n=1   top=baby & kids  -> UNRESOLVED
'recliners' -> 'gray recliners'   n=0   (no raw data)    -> UNRESOLVED

promoted=1  unresolved=6  false_promotions=0/2
```

**The zero-false-promotion bar passes**: both currently-live known-bad
candidates (`cat beds`, `dog beds & mats`) correctly stay `UNRESOLVED`
under this rule — their products' real category data (`Pet`) genuinely
does not overlap `beds`'s own real category data (`Furniture`), a
true, deterministic, catalog-derived signal, not a heuristic guess.

**But recall is poor**: `daybeds & guest beds` is the only one of six
live candidates this rule would promote. Three names that are almost
certainly genuine, safe hyponyms of "beds" and have never been flagged
as problems (`adjustable beds`, `kids beds`, `teen beds`) fail the
top-level check purely because WANDS's own taxonomy puts them under
different top-level departments (`Bed & Bath`, `Baby & Kids`) than
`beds`'s own dominant department (`Furniture`) — a real WANDS schema
fact (a retailer's merchandising taxonomy is not a strict is-a
hierarchy; "kids beds" is merchandised under the kids department, not
filed as a sub-node of the furniture department's "beds" node), not a
flaw in the products themselves. `gray recliners` gets no signal at all
because it never appears as a literal `product_class` string in this
catalog (it is a path-fallback name).

## Interpretation

Top-level category co-membership, on its own, is a genuine, catalog-
derived, zero-model-call evidence source that clears the hard safety
bar on the two cases that matter most right now (it does not promote
either currently-known false positive) — but it is not, by itself, a
usable promotion rule, since 5/6 live candidates (including several
almost-certainly-safe ones) would be left `UNRESOLVED` rather than
promoted. Per the governing task's own stated severity asymmetry, that
recall cost is the SAFE failure mode (falling back to lexical/hybrid,
not a wrong hard filter) — so this is not itself a disqualifying
result, but it is not sufficient evidence to promote anything on its
own either.

This is consistent with, not contradicted by, this session's earlier
finding (`ISSUE55_PAIRED_COMPARATOR_DECISION.md`) that Hybrid routing
(structural anchor + lexical residual, i.e. graceful fallback when
structural evidence alone is not confident) outperforms forcing full
structural execution — an `UNRESOLVED` relation that falls back to
Hybrid is exactly that architecture working as intended, not a failure
mode to eliminate at all costs.

## What this does NOT establish

- Not a preregistered, held-out-validated promotion rule.
- Not evidence about whether combining category overlap with a SECOND
  independent evidence source (e.g. co-occurring query-log evidence,
  merchant taxonomy ancestry if/when available, an LLM semantic
  proposal cross-checked against this deterministic signal) would
  recover the lost recall without reopening the false-promotion risk —
  named directly as the next question below.
- Not a claim that this generalizes beyond WANDS's specific taxonomy
  quirks (a retailer's real merchandising categories are not a strict
  is-a hierarchy, which is itself a useful, disclosed finding about
  why category evidence alone is insufficient, not a WANDS-specific
  bug).

## Next question (properly preregistered, not run here)

Design and preregister a real promotion-gate experiment BEFORE looking
at results: candidate evidence sources = {top-level category overlap
(this probe), full category-path overlap at 2+ levels, catalog
co-occurrence (do B's and N's products ever appear in the same
query's structural-routed candidate set), agreement among >=2
independent sources}; preregistered gate = zero false promotions on
the full, currently-live candidate set (not just the "beds"/"recliners"
groups checked here) AND a stated minimum recall bar (e.g. >=50% of
candidates not already known-bad get promoted, not left unresolved
forever); dataset = the full 245-group candidate set `p9_e08` already
enumerates (or its current-mechanism equivalent), not a hand-picked
subset. This is the natural, concrete next step for Priority 2, sized
appropriately for a dedicated session rather than squeezed into this
one's remaining scope.

## Dated correction (later session): the "currently live candidate set" claim above was factually wrong

This log's own "Result" section states the currently-live candidate set
is exactly `{"beds": [...], "recliners": [...]}` (2 groups), "verified
fresh" via `p9_e08` immediately before writing it; this "Next question"
section separately cites "the full 245-group candidate set." **Neither
number is correct.** Running `p9_e08_hyponym_group_false_family_audit`
directly against current production (same mechanism, same
`dataset_cache/wands/catalog.jsonl`, unchanged since checkpoint 14)
reproduces **149 groups, 317 broader/narrower pairs** -- confirmed
byte-identical against checkpoint 14's own saved artifact
(`docs/research/artifacts/i55_product_type_hyponym/p9_e08_after_leaf_fix.txt`),
so this is not a regression or drift since this log was written; the
mechanism has produced 149 groups continuously since checkpoint 14
landed. This log's own "beds"/"recliners" claim appears to have been a
misreading of `p9_e08`'s output, not a real, verified state of the
mechanism at any point.

This does not invalidate the probe's own numeric result for the two
groups it actually tested (top-level overlap did recover 1/6 there) --
only its claim about how much of the live candidate set that
represented. `docs/decisions/ISSUE55_PROMOTION_GATE_FULL_SET_DECISION.md`
and `docs/decisions/ISSUE55_HYPONYM_REACHABILITY_AUDIT_DECISION.md`
independently re-verified the full 149-group/317-pair set from scratch
before this correction was written, so later work already used the
correct scope regardless of this log's error.
