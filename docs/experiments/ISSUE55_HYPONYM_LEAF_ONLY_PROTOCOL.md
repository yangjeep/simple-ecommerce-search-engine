# Issue #55 Preregistered Protocol — leaf-only restriction for `ProductTypeAny` hyponym expansion

## 0. What this is testing

`docs/decisions/ISSUE55_PRODUCT_TYPE_HYPONYM_DECISION.md` (checkpoint
11) rejected the unconditional `ProductTypeAny` hyponym-expansion
mechanism after a real-vocabulary audit (`p9_e08`) found confirmed
cross-family false positives, all traced to one root cause: the
whole-word comparison ran against product-type names' **full
ancestor-breadcrumb paths** (e.g. `"décor & pillows / candles &
holders / scented oils & diffusers"`), so a word appearing only in an
*ancestor* segment (the "candles & holders" parent category) could
spuriously match an unrelated sibling leaf. That checkpoint named, but
did not implement, a hierarchy-aware redesign as the concrete next
step.

This checkpoint tests the smallest fix consistent with that diagnosis:
**restrict the word-set comparison to each name's own trailing path
segment (its "leaf") instead of the full path.** For a name with no
`" / "` separator (a clean `product_class`-derived name like
`"recliners"`), the leaf is the whole name, unchanged. For a
breadcrumb-path-derived name, the leaf is the text after the last
`" / "`.

Manually re-checked against all three concrete false-positive examples
`ISSUE55_PRODUCT_TYPE_HYPONYM_DECISION.md` disclosed, before writing
any code (a prediction made and recorded up front, not fit after
seeing a favorable result):

- `"candles"` (1-word, clean) vs. `"...candles & holders / scented oils
  & diffusers"` (leaf: `"scented oils & diffusers"`, words `{scented,
  oils, &, diffusers}`) -- `{candles}` is **not** a subset -- predicted
  **excluded** (fixed).
- `"hot tubs"` (2-word, clean) vs. `"...hot tubs & saunas / saunas"`
  (leaf: `"saunas"`, words `{saunas}`) -- `{hot, tubs}` is **not** a
  subset of a 1-word set -- predicted **excluded** (fixed).
- `"bed accessories"`/`"bath accessories"` vs. `"...shower curtains &
  accessories / shower curtain hooks"` (leaf: `"shower curtain hooks"`,
  words `{shower, curtain, hooks}`) -- neither `{bed, accessories}` nor
  `{bath, accessories}` is a subset -- predicted **excluded** (fixed).
- `"recliners"` (1-word, clean, the flagship recall-win example) vs.
  `"...chairs & seating / recliners / gray recliners"` (leaf: `"gray
  recliners"`, words `{gray, recliners}`) -- `{recliners}` **is** a
  subset -- predicted **preserved** (the win survives).

**Explicitly not fixed by this change, predicted and disclosed up
front**: `"beds"` (clean, non-path) vs. `"cat beds"`/`"dog beds &
mats"` (also clean, non-path -- both names ARE their own "leaf" already,
so leaf-restriction changes nothing for a clean-vs-clean pair). This is
genuine cross-vertical lexical polysemy, not an ancestor-breadcrumb
artifact, and this checkpoint does not claim to fix it -- it will be
measured and disclosed as a residual, quantified risk either way.

## 1. Hypothesis

**H0**: restricting `product_type_hyponym_groups`'s word-set comparison
to each name's trailing path segment (leaf) eliminates the three
ancestor-breadcrumb-bleed false positives found in checkpoint 11's
audit, while preserving **>=80%** of the original +24.06pp candidate-set
recall improvement (i.e. new recall improvement >= 0.8 x 24.06pp =
19.25pp over the 0.4562 baseline) and continuing to pass the H1/H3
ranking-quality/speed checks. Under H0, this is a genuine, safe,
shippable win -- **KEEP**, superseding checkpoint 11's REJECT with a
corrected, narrower mechanism.

**H1 (negation)**: either (a) the leaf-only restriction does not
actually eliminate the disclosed false positives (the prediction above
is wrong), or (b) it eliminates them but the recall win collapses below
the 80% retention bar (most of the win came from exactly the
ancestor-bleed cases being removed) -- a real, informative negative
result confirming the mechanism cannot be made both safe and valuable
via this specific restriction.

**Explicitly not a gate**: the residual "beds"/pet-beds-style
clean-vs-clean cross-vertical polysemy risk. This will be measured
(re-running `p9_e08`) and disclosed with its quantified rate, but is
predicted and disclosed *not* to be fixed by this change -- finding it
still present is not treated as falsifying H0, since H0 only claims to
fix the ancestor-breadcrumb-bleed class.

## 2. Baseline / dataset / treatment

Baseline: current branch HEAD (post checkpoint 11's revert to plain
`ProductType` matching). Dataset: the same real WANDS catalog + 480
queries + fresh Solr 9.10.1 core used throughout this session.
Treatment: `product_type_hyponym_groups` (`crates/commerce-core/src/cold_start/profile.rs`)
changed to compare each name's leaf segment (text after the last
`" / "`, or the whole name if none) instead of the full name; `compile_non_brand_lexicon`
re-wired to use it exactly as checkpoint 11 originally did (`ProductTypeAny`
for non-empty hyponym groups, `ProductType` otherwise).

## 3. Metrics / gates

- **Correctness (hard gate, checked first)**: existing property tests
  (`hyponym_tests`) updated for leaf-comparison semantics, still
  proving soundness (every produced pair is a genuine leaf-level
  whole-word superset) and completeness against re-derivation from
  names; `cargo test --workspace --all-features` zero new failures.
- **False-family re-audit (the actual falsification test)**: rerun
  `p9_e08_hyponym_group_false_family_audit` against the new mechanism;
  manually re-check the same rows this checkpoint predicted fixed. Any
  *new* false-positive class not predicted above must be treated as a
  fresh finding, not silently accepted.
- **KEEP**: candidate-set recall retains >=80% of checkpoint 11's
  +24.06pp gain (i.e. lands at >=0.6487 mean recall, vs. the 0.4562
  baseline and 0.6968 checkpoint-11 peak) AND `p9_e04`'s H1 (native
  ranking not materially worse than Solr on the identical candidate
  set) and H3 (>=2x speed) continue to hold AND the three named
  ancestor-breadcrumb false positives are confirmed gone from the
  re-audit.
- **REVISE/REJECT**: any of the above fails -- e.g. recall drops below
  the 80%-retention floor, a new false-positive class appears, or a
  ranking/speed gate flips.

Repetitions: deterministic given fixed judgments/lexicon, matching this
session's own established discipline for this exact measurement chain
-- no repetition needed.
