# Issue #55 — first exhaustive, reachability-verified audit of the full hyponym candidate set (append-only log)

Prompted by a real methodology gap found while extending the Priority 2
promotion-gate work: no prior checkpoint had ever actually read through
all 149 live `product_type_hyponym_groups` at once. Every "re-audit"
claim (checkpoint 14's own `ISSUE55_HYPONYM_LEAF_ONLY_DECISION.md`,
and a later session's `ISSUE55_SEMANTIC_PROMOTION_LOG.md`) checked
specific, previously-named groups, not the full set from scratch.

## Entry 1 — a visual scan found a plausible new false positive

While eyeballing `p9_e08_hyponym_group_false_family_audit`'s full
149-group dump (`docs/research/artifacts/i55_promotion_gate_full_set/candidate_set.json`
exported the same set independently, confirming it's not stale) for
the first time, one entry stood out: `"kitchen & tabletop / tableware &
drinkware / dinnerware / plates" -> ["burners & hot plates", ...,
"home improvement / hardware / home hardware / switch plates", ...]`.
At face value this reads as a serious cross-family false positive:
dinnerware "plates" admitting electrical "switch plates" and kitchen
appliance "hot plates" -- comparable in apparent severity to the
already-known "beds"->pet-products case, and never previously
disclosed.

A second, weaker candidate: `"table accessories" -> "table top games &
accessories"` (games surfacing for what reads as a household-accessory
query).

## Entry 2 — mechanical verification found the "plates" concern is not real (a self-caught overclaim)

Before writing this up as a new confirmed defect, compiled the literal
query text against the real, current-default WANDS lexicon directly
(`compile(query_text, &lexicon)`), rather than trusting the visual
read. Result: **`"plates"` produces no constraint at all** --
`CommerceQuery { constraints: [], residual_lexical: ["plates"] }`.
The reason: the *broader term* in that group is not the bare word
"plates" -- it is the full 11-word ancestor-breadcrumb path string
`"kitchen & tabletop / tableware & drinkware / dinnerware / plates"`
(a WANDS ingestion fallback name, `effective_product_class`,
`crates/phase6a-eval/src/catalog.rs`), which no real shopper would ever
type verbatim. Querying that exact path string *also* does not produce
the `ProductTypeAny` -- it resolves to a soft `Preference` (`Boost {
attribute: "category_depth_4", ... }`) instead, from a separate,
category-depth-derived lexicon population path. Same result for
`"table accessories"`'s underlying group. **Both candidate false
positives found by the visual scan are not reachable via `compile()` at
all**, under either the natural short phrase or the exact synthesized
path string.

This directly disproves the assumption a plain visual read of
`p9_e08`'s dump implicitly makes: that every printed "broader term"
string is an equally query-reachable trigger. It is not.

## Entry 3 — a systematic, mechanical reachability sweep of all 149 groups

Wrote `crates/phase9-eval/src/bin/i55_hyponym_reachability_audit.rs`:
for every one of the 149 live groups, compiles the query text equal to
its own literal broader-term name and records whether that produces a
`ProductTypeAny` (reachable) or not (unreachable -- shadowed by a
`Preference` or otherwise unmatched). Raw output:
`docs/research/artifacts/i55_hyponym_reachability_audit/run1.txt`
(deterministic: rerun byte-identical).

**Result: 79 of 149 groups (53%) are reachable via their own literal
broader-term text; 70 (47%) are not.** Reachability does not cleanly
track word count/path-shape as initially assumed -- e.g. `"accent
chests / cabinets"` (containing `"/"`) IS reachable, while the 11-word
`"...dinnerware / plates"` path is not.

Every one of the 79 reachable groups' full admitted-narrower-name list
was then read exhaustively (not sampled) for cross-family concerns.
Result:

- **Exactly one confirmed cross-family false positive**: `"beds"` ->
  `"cat beds"` / `"dog beds & mats"` -- the already-known, already-
  disclosed residual risk from `ISSUE55_HYPONYM_LEAF_ONLY_DECISION.md`.
  No new confirmed violation.
- **One low-practical-risk edge case**: `"accent chests / cabinets"` ->
  `"dartboards and cabinets"` (a WANDS taxonomy leaf that appears to
  bundle a dartboard-cabinet combo product with general cabinets).
  Reachable only via the *exact* literal string `"accent chests /
  cabinets"` (with the slash) -- not a plausible free-text search query,
  though not provably impossible if some future browse/facet mechanism
  ever passed a raw taxonomy label as query text.
- **All other 77 reachable groups' narrower-name lists were read in
  full and found genuinely on-topic** (see the decision doc for the
  verdict; not reproduced line-by-line here since none required a
  judgment call beyond "same product family").

See the decision doc for the verdict on what this does and does not
establish, and a separate, disclosed correction to
`ISSUE55_SEMANTIC_PROMOTION_LOG.md`'s own inaccurate claim about the
live candidate-set size (found while investigating this).
