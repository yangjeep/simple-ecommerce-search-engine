# Issue #55 — leaf-only restriction for `ProductTypeAny` hyponym expansion

Log: `docs/experiments/ISSUE55_HYPONYM_LEAF_ONLY_LOG.md`. Protocol:
`docs/experiments/ISSUE55_HYPONYM_LEAF_ONLY_PROTOCOL.md`.

## Verdict: KEEP (the mechanism, corrected) — and a major new finding: `structural_routed` NDCG turns positive for the first time, at a disclosed latency cost

`docs/decisions/ISSUE55_PRODUCT_TYPE_HYPONYM_DECISION.md` (checkpoint
11) rejected an unconditional whole-word `ProductTypeAny` mechanism
after a real-vocabulary audit found confirmed cross-family false
positives, all traced to one cause: comparing full ancestor-breadcrumb
paths let a word appearing only in a *parent* category segment
spuriously match an unrelated sibling leaf. This checkpoint implements
and measures the named fix: restrict the comparison to each name's own
trailing path segment (leaf).

**H0 confirmed on every preregistered gate**:

1. **All three named false positives are gone.** `"candles"`, `"hot
   tubs"`, and `"bed accessories"` each now produce **zero** hyponym
   groups (their only prior matches were the ancestor-bleed artifacts).
   `"bath accessories"` keeps exactly one narrower name -- a genuine one
   whose own leaf contains "bath accessories" -- not the false
   shower-curtain match. Re-audited directly via `p9_e08`, not assumed
   from the code change alone.
2. **The flagship win survives.** `"recliners"` still admits `"...gray
   recliners"` (leaf-level match, unaffected by the restriction).
3. **94.1% of the recall win is retained**: mean candidate-set recall
   0.6825 vs. checkpoint 11's 0.6968 peak (both far above the 0.4562
   baseline), comfortably clearing the preregistered >=80% retention
   bar.
4. **H1 (ranking quality) continues to hold**: native NDCG@10 still not
   worse than Solr's on the identical candidate set (+9.21%, was
   +9.84%).
5. **H3 (speed) is borderline, not a clean pass, disclosed as such**:
   three independent runs gave 1.93x, 2.05x, 2.04x (median 2.04x, one
   run individually under the 2x bar). This is a real, modest narrowing
   from checkpoint 11's own 2.54x, most plausibly the project's
   already-documented Solr JVM-latency variance
   (`docs/decisions/ISSUE43_DECISION.md`) rather than a new systematic
   native-side cost (native's own absolute latency did not increase).
   Reported honestly as borderline-but-net-positive, not rounded up to
   an unqualified "CONFIRMED."

**The disclosed residual risk is confirmed present, exactly as
predicted before any code was written**: `"beds"` still admits pet
`"cat beds"`/`"dog beds & mats"` -- both clean (non-path) names, so the
leaf restriction does not and was never claimed to address this class
of genuine cross-vertical lexical polysemy. This remains an open,
quantified limitation, not silently fixed or hidden.

## A major, unplanned finding: `structural_routed` NDCG is positive for the first time this session

Re-running `p9_e02` (not itself one of this checkpoint's preregistered
gates, but run per this session's own standing practice of checking the
full end-to-end picture whenever this code path changes) surfaced
something bigger than this checkpoint set out to test:

| | Baseline | This checkpoint |
|---|---|---|
| `structural_routed` (FastPath+Hybrid) NDCG@10 relative gap | **-25.05%** | **+5.37%** |
| `structural_routed` latency ratio | 1.60x | 1.19x |

This is the first time in this project's entire multi-checkpoint
investigation of `structural_routed` traffic (`PHASE9_DECISION.md`
through the empty-residual, text-token-cache, and product_class
ingestion checkpoints) that native's relevance on this traffic class
has measured *better* than Solr's, not worse. It comes at a real,
disclosed cost: broader `ProductTypeAny` candidate sets are more
expensive to rank, and the latency ratio drops below the `>=2x` bar
this project has used throughout. `p9_e02`'s own tool-computed verdict
is **REVISE**: one axis (relevance) now clears its bar, the other
(latency) does not -- a genuine, disclosed trade-off, not a clean win,
and not claimed as one.

**This is not itself a GO/production-adoption decision for
`structural_routed` traffic** (that remains `PHASE9_DECISION.md`'s and
this session's own standing REVISE), but it is a substantial,
first-ever positive movement on the relevance axis specifically, and
names a sharp new question for a future checkpoint: given relevance now
clears its bar, is recovering `structural_routed`'s latency margin (or
reconsidering whether `>=2x` is still the right bar once relevance has
improved this much) the more valuable next target than further
relevance work on this traffic class?

## Why this checkpoint's own scope is narrower than "solve structural_routed"

This checkpoint's preregistered gates were about the `ProductTypeAny`
mechanism itself (false positives, recall retention, H1/H3) -- not
about `structural_routed`'s own end-to-end latency bar, which was never
named as a gate here (it is a distinct, larger, previously-established
question this project has tracked across many prior checkpoints). The
mechanism is KEPT and wired into production because it does what it
was designed to do, safely, per its own preregistered criteria. The
p9_e02 finding is reported prominently because it is real, important,
and directly downstream of this change -- not because this checkpoint
claims to have resolved it.

## What this does and does not change

- **Reverses** checkpoint 11's REJECT of the unconditional mechanism:
  the *corrected*, leaf-only-restricted version is now wired into
  production (`compile_non_brand_lexicon`), not the rejected full-path
  version.
- **Does not claim** to have fixed the "beds"/pet-beds-style
  clean-vs-clean polysemy risk -- explicitly out of scope, predicted
  unfixed, confirmed unfixed.
- **Does not claim** a GO for `structural_routed` traffic's own
  end-to-end latency bar -- that remains REVISE, now for a different,
  more specific reason (a real latency cost from broader candidate
  sets, not a relevance deficit).
- **Updates** the two regression tests checkpoint 11 added: the
  "boots"/"hiking boots" test now asserts the corrected, expected
  merge behavior (previously asserted the opposite, when the mechanism
  was unwired); a new test reproduces the "candles" ancestor-bleed
  exclusion end-to-end through the public API, guarding specifically
  against a future regression to full-path comparison.

## Adversarial review

- **Checked whether the three named false positives were actually
  fixed, not merely reduced or moved**: re-ran `p9_e08` and confirmed
  each of the three broader terms now produces literally zero or only
  genuine narrower names -- not spot-checked from the code alone.
- **Checked whether the flagship "recliners" win survived by
  coincidence or by the mechanism working as designed**: confirmed via
  a dedicated unit test (`clean_broader_term_still_admits_a_path_names_matching_leaf`)
  and cross-checked against the live `p9_e08` audit output.
- **Checked whether H3's borderline result across 3 runs indicates a
  real regression or measurement noise**: compared native's own
  absolute latency across runs (0.83-1.01ms, stable/no worse than
  checkpoint 11's 0.9091ms) against Solr's (1.70-2.31ms across all
  4 runs including the pre-fix comparison point, clearly the more
  volatile side) -- consistent with this project's own already-disclosed
  Solr JVM-latency variance, not a new native-side cost. Reported as
  "borderline," not smoothed into either "CONFIRMED" or "REGRESSION."
  Did not run a 4th tie-breaking measurement since the direction (net
  positive, 2/3 runs and the mean/median both >=2.0x) was already clear
  enough to not change the KEEP verdict, which does not hinge on H3
  alone.
- **Checked whether the "hardwood beds" empty-native-result example in
  `p9_e02`'s qualitative sample was a new regression**: confirmed
  directly against the pre-fix baseline artifact
  (`docs/research/artifacts/i55_product_type_hyponym/p9_e02_after_revert.txt`)
  -- the same query already returned an empty native result before this
  checkpoint's change.
- **Checked whether reporting the p9_e02 finding prominently risks
  overclaiming a GO this checkpoint did not earn**: the finding is
  stated with both numbers (relevance up, latency down) and the tool's
  own REVISE verdict quoted directly, not summarized as a win.

## Traceability

Source: `crates/commerce-core/src/cold_start/profile.rs`
(`leaf_segment`, `product_type_hyponym_groups`,
`compile_non_brand_lexicon`), `crates/commerce-core/tests/cold_start.rs`.
Raw evidence: `docs/research/artifacts/i55_product_type_hyponym/`
(`p9_e08_after_leaf_fix.txt`, `p9_e04_after_leaf_fix.txt`,
`p9_e04_h3_leaf_fix_3runs.txt`, `p9_e02_after_leaf_fix.txt`).
